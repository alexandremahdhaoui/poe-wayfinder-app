//! The overlay.
//!
//! main reads config, builds adapters, injects them into controllers, injects
//! controllers into drivers, then starts the drivers. It calls nothing else.
//!
//! Controllers and drivers are not written yet. What runs today is the part
//! that must be right before any of them exist: config resolution and the
//! network policy.

use std::process::ExitCode;
use std::time::Duration;

use poe_trader_app::adapter::game_data_adapter::GameTables;
use poe_trader_app::adapter::http_adapter::{HttpAdapter, NetworkPolicy, PolicyError};
use poe_trader_app::config::PoeTraderConfig;
use poe_trader_app::logging::{Logger, Value};
use poe_trader_app::types::Hotkey;
use poe_trader_core::types::GameVersion;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let cfg = match PoeTraderConfig::load(&args) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("poe-trader: loading config: {err}");

            return ExitCode::FAILURE;
        }
    };

    let log = Logger::new(&cfg.log_level, "poe-trader");

    let policy = NetworkPolicy::new(
        cfg.network_enabled,
        cfg.block_unlisted_hosts,
        &cfg.allowed_hosts,
    );

    // Log the policy at startup. An operator must never have to read source to
    // answer "what can this thing reach".
    log.info(
        "network policy",
        &[
            ("enabled", Value::Bool(cfg.network_enabled)),
            ("block_unlisted", Value::Bool(cfg.block_unlisted_hosts)),
            ("hosts", Value::Str(policy.allowed_hosts().join(","))),
        ],
    );

    // poesessid is never logged. Only whether one was supplied.
    log.info(
        "session",
        &[("poesessid_present", Value::Bool(!cfg.poesessid.is_empty()))],
    );

    let http = match HttpAdapter::new(policy, Duration::from_secs(30)) {
        Ok(http) => http,
        Err(err) => {
            log.error(
                "building http client",
                &[("error", Value::Str(err.to_string()))],
            );

            return ExitCode::FAILURE;
        }
    };

    // Prove the policy is live before anything else starts. A trade_base_url
    // the allowlist refuses is a config mistake, and failing here beats
    // failing on the first hotkey press.
    //
    // NetworkDisabled is not a mistake. It is the documented way to run the
    // parser offline, so it warns and keeps going.
    match http.policy().check(&cfg.trade_base_url) {
        Ok(()) => {}
        Err(PolicyError::NetworkDisabled) => {
            log.warn("network is disabled. Pricing will not work.", &[]);
        }
        Err(err) => {
            log.error(
                "trade_base_url is refused by the network policy",
                &[
                    ("url", Value::Str(cfg.trade_base_url.clone())),
                    ("error", Value::Str(err.to_string())),
                ],
            );

            return ExitCode::FAILURE;
        }
    }

    // Config is validated before anything starts. A bad hotkey that fails at
    // the first keypress is the hardest kind of bug to report, because there
    // is nothing to see.
    let hotkey = match Hotkey::parse(&cfg.price_check_hotkey) {
        Ok(hotkey) => hotkey,
        Err(err) => {
            log.error(
                "reading the price check hotkey",
                &[
                    ("hotkey", Value::Str(cfg.price_check_hotkey.clone())),
                    ("error", Value::Str(err.to_string())),
                ],
            );

            return ExitCode::FAILURE;
        }
    };

    let Some(game) = GameVersion::parse(&cfg.game) else {
        log.error("unknown game", &[("game", Value::Str(cfg.game.clone()))]);

        return ExitCode::FAILURE;
    };

    let data = match GameTables::load(std::path::Path::new(&cfg.data_dir)) {
        Ok(data) => data,
        Err(err) => {
            log.error(
                "loading game data",
                &[
                    ("data_dir", Value::Str(cfg.data_dir.clone())),
                    ("error", Value::Str(err.to_string())),
                ],
            );

            return ExitCode::FAILURE;
        }
    };

    log.info(
        "startup",
        &[
            ("game", Value::Str(game.as_str().to_string())),
            ("window_title", Value::Str(cfg.window_title.clone())),
            ("hotkey", Value::Str(hotkey.to_string())),
            ("stats", Value::Int(data.stat_count() as i64)),
            ("item_names", Value::Int(data.item_name_count() as i64)),
        ],
    );

    #[cfg(windows)]
    return run_overlay(&cfg, game, data, hotkey, log);

    #[cfg(not(windows))]
    {
        log.warn(
            "the overlay only runs on Windows. Use poe-trader-cli here.",
            &[],
        );

        let _ = (http, hotkey, game, data);

        ExitCode::SUCCESS
    }
}

/// Run the overlay window.
///
/// The window is created hidden and only shown once a price check produces
/// something. An overlay that appears at startup covers the game before the
/// user has asked for anything.
#[cfg(windows)]
fn run_overlay(
    cfg: &PoeTraderConfig,
    game: GameVersion,
    data: GameTables,
    hotkey: poe_trader_app::types::Hotkey,
    log: Logger,
) -> ExitCode {
    use poe_trader_app::adapter::clipboard_adapter::{copy_item, CopyTiming, SystemClipboard};
    use poe_trader_app::adapter::game_window_adapter::{
        GameWindowAdapter, GameWindowSource, KeyboardCopyTrigger,
    };
    use poe_trader_app::controller::overlay_controller::OverlayModel;
    use poe_trader_app::driver::hotkey_driver::HotkeyDriver;
    use poe_trader_app::driver::overlay_ui_driver::{overlay_viewport, paint, UiEvent};
    use poe_trader_app::types::overlay::OverlayGeometry;
    use poe_trader_core::controller::price_check::{price_check, PriceCheckOptions};

    let window = GameWindowAdapter::new(&cfg.window_title);

    match window.find() {
        Ok(found) => log.info(
            "found the game window",
            &[
                ("width", Value::Int(i64::from(found.rect.width))),
                ("height", Value::Int(i64::from(found.rect.height))),
                ("foreground", Value::Bool(found.is_foreground)),
                ("scale", Value::Str(format!("{:.2}", window.scale()))),
            ],
        ),
        Err(err) => log.warn(
            "the game window is not open yet",
            &[("error", Value::Str(err.to_string()))],
        ),
    }

    // Registered before the window opens. A hotkey another application owns
    // has to be reported now, not on the first press that does nothing.
    let hotkeys = match HotkeyDriver::start(&hotkey) {
        Ok(hotkeys) => {
            log.info(
                "registered the price check hotkey",
                &[("hotkey", Value::Str(hotkey.to_string()))],
            );

            hotkeys
        }
        Err(err) => {
            log.error(
                "registering the price check hotkey",
                &[("error", Value::Str(err.to_string()))],
            );

            return ExitCode::FAILURE;
        }
    };

    let mut clipboard = match SystemClipboard::new() {
        Ok(clipboard) => clipboard,
        Err(err) => {
            log.error(
                "opening the clipboard",
                &[("error", Value::Str(err.to_string()))],
            );

            return ExitCode::FAILURE;
        }
    };

    let trigger = KeyboardCopyTrigger::new();
    let timing = CopyTiming::default();
    let options = PriceCheckOptions::new(game);

    let mut model = OverlayModel::new(OverlayGeometry::default());

    // Shown once so the user can see the tool started and which key to press.
    // The first price check replaces it.
    model.start(window.cursor());
    model.fail(&format!(
        "Ready. Press {hotkey} with the cursor over an item.",
    ));

    let first = model.frame_scaled(window.find().ok(), window.scale());

    let native_options = eframe::NativeOptions {
        viewport: overlay_viewport(&first),
        ..eframe::NativeOptions::default()
    };

    let cfg_restore = cfg.restore_clipboard;

    let result = eframe::run_simple_native("poe-trader", native_options, move |ctx, _frame| {
        // A press drains the whole queue. Queuing them would run one price
        // check per press after a stutter, which is what the rate limiter
        // exists to prevent.
        if hotkeys.fired() {
            model.start(window.cursor());

            match copy_item(
                &mut clipboard,
                &trigger,
                timing,
                cfg_restore,
                std::thread::sleep,
            ) {
                Ok(text) => match price_check(&text, &data, options) {
                    // The search itself is not wired into the loop yet, so the
                    // panel shows what was parsed and how many filters it
                    // built. Nothing here is fabricated.
                    Ok(checked) => model.finish(checked, 0),
                    Err(err) => model.fail(&format!("Could not read the item: {err}")),
                },
                Err(err) => model.fail(&format!("Could not copy the item: {err}")),
            }
        }

        let found = window.find().ok();
        let frame = model.frame_scaled(found, window.scale());

        // The window follows the game every frame. The game can be moved,
        // resized or alt tabbed at any moment.
        if let Some(rect) = frame.rect {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                rect.x as f32,
                rect.y as f32,
            )));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                rect.width as f32,
                rect.height as f32,
            )));
        }

        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(frame.rect.is_some()));
        ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(!frame.takes_input));

        for event in paint(ctx, &model) {
            match event {
                UiEvent::Dismiss => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                UiEvent::OpenInBrowser | UiEvent::Research | UiEvent::ToggleFilter(_) => {}
            }
        }

        // Repaint continuously. The game window can move at any time and there
        // is no event that tells us.
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    });

    if let Err(err) = result {
        log.error(
            "running the overlay window",
            &[("error", Value::Str(err.to_string()))],
        );

        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
