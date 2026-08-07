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

    // Answered before config, because the whole point is to find out what to
    // put in the config. An overlay pointed at a title that does not exist
    // starts perfectly, logs nothing wrong and never draws, and there is no
    // way to guess the right string from outside the machine.
    if args.iter().any(|a| a == "--list-windows") {
        return list_windows();
    }

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

    // The game's own config. Read for a diagnostic and not for the copy: since
    // 3.29 a copy produces the detailed format on its own. Saying at startup
    // whether the install was found turns a silent setup problem into a line
    // the user can act on.
    let game_config = poe_trader_app::adapter::game_config_adapter::read(
        std::path::Path::new(&documents_dir()),
        game,
        poe_trader_app::adapter::game_config_adapter::load_from_disk,
    );

    log.info(
        "game configuration",
        &[
            (
                "path",
                Value::Str(
                    game_config
                        .path
                        .as_ref()
                        .map_or_else(|| "not found".to_string(), |p| p.display().to_string()),
                ),
            ),
            (
                "show_mods_key",
                Value::Str(game_config.show_mods_key.clone()),
            ),
            // The difference between reading Alt and assuming it. Both are the
            // same key and very different confidence.
            ("read", Value::Bool(game_config.read)),
        ],
    );

    #[cfg(windows)]
    return run_overlay(&cfg, game, data, hotkey, http, log);

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

/// Print every visible window title and exit.
///
/// Plain text rather than the JSON the rest of the tool logs, because this is
/// read by a person who is about to copy one of these lines into `--window-
/// title`.
fn list_windows() -> ExitCode {
    #[cfg(windows)]
    {
        let titles = poe_trader_app::adapter::game_window_adapter::visible_window_titles();

        println!("Visible windows, one per line.");
        println!("Copy the game's line into --window-title, quotes included.\n");

        for title in &titles {
            println!("  {title:?}");
        }

        if titles.is_empty() {
            println!("  (none, which should be impossible on a running desktop)");
        }

        ExitCode::SUCCESS
    }

    #[cfg(not(windows))]
    {
        eprintln!("poe-trader: --list-windows only works on Windows.");

        ExitCode::FAILURE
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
    http: HttpAdapter,
    log: Logger,
) -> ExitCode {
    use poe_trader_app::adapter::clipboard_adapter::{copy_item, CopyTiming, SystemClipboard};
    use poe_trader_app::adapter::game_window_adapter::{
        GameWindowAdapter, GameWindowSource, KeyboardCopyTrigger,
    };
    use poe_trader_app::adapter::rate_limit_adapter::LimiterSet;
    use poe_trader_app::adapter::trade_api_adapter::TradeUrls;
    use poe_trader_app::controller::overlay_controller::OverlayModel;
    use poe_trader_app::controller::price_check_loop;
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

    // The one Windows call nothing else reaches. It fires only on a hotkey
    // press and types into whatever has focus, so the ordinary path cannot be
    // exercised without overwriting the user's clipboard. This sends a key
    // that does nothing anywhere and reports whether Windows took it.
    //
    // Logged at startup rather than hidden behind a flag, because a build
    // whose SendInput does not work cannot copy an item and should say so
    // before the user presses the hotkey and sees nothing happen.
    let sent = poe_trader_app::adapter::game_window_adapter::self_test_send_input();

    if sent == 2 {
        log.info(
            "keyboard input works",
            &[("events_accepted", Value::Int(2))],
        );
    } else {
        log.error(
            "keyboard input is not working, a price check will not be able to copy the item",
            &[("events_accepted", Value::Int(i64::from(sent)))],
        );
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

    // The search runs on the UI thread and blocks it for the length of one
    // request. That is deliberate for now: a background task would need the
    // model behind a lock, and a price check the user asked for is the only
    // thing they are waiting on.
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            log.error(
                "starting the search runtime",
                &[("error", Value::Str(err.to_string()))],
            );

            return ExitCode::FAILURE;
        }
    };

    let urls = TradeUrls::new(&cfg.trade_base_url, game);
    let league = cfg.league.clone();
    let session = cfg.poesessid.clone();
    let latency = cfg.api_latency_seconds.max(0) as u32;
    let mut limits = LimiterSet::conservative();
    let search_log = Logger::new(&cfg.log_level, "poe-trader");
    let title = cfg.window_title.clone();

    // Starts as whatever the startup check found, so the first frame does not
    // repeat a line already logged.
    let mut window_was_found = window.find().is_ok();

    let result = eframe::run_simple_native("poe-trader", native_options, move |ctx, _frame| {
        // A press drains the whole queue. Queuing them would run one price
        // check per press after a stutter, which is what the rate limiter
        // exists to prevent.
        if hotkeys.fired() {
            // Logged on every press. Without this line a user whose hotkey
            // never reaches us and a user whose price check silently worked
            // see exactly the same empty log, and there is no way to tell
            // which half of the chain to look at.
            search_log.info("price check hotkey pressed", &[]);

            // The whole chain lives in a controller so it can be tested
            // without a game, a clipboard or a network. This is the only
            // place that supplies the real three.
            let outcome = price_check_loop::run(
                &mut model,
                window.cursor(),
                || {
                    copy_item(
                        &mut clipboard,
                        &trigger,
                        timing,
                        cfg_restore,
                        std::thread::sleep,
                    )
                    .map_err(|e| format!("{e}"))
                },
                |text| price_check(text, &data, options).map_err(|e| format!("{e}")),
                |checked| {
                    runtime.block_on(search(
                        &http,
                        &urls,
                        &league,
                        &session,
                        latency,
                        &mut limits,
                        checked,
                    ))
                },
            );

            match outcome {
                price_check_loop::Outcome::Priced { total } => search_log.info(
                    "price check finished",
                    &[("listings", Value::Int(total as i64))],
                ),
                other => search_log.warn(
                    "price check did not produce a price",
                    &[("outcome", Value::Str(format!("{other:?}")))],
                ),
            }
        }

        let found = window.find().ok();

        // Said once each time the answer changes. The overlay draws nothing at
        // all while the window is missing, and without this the user sees an
        // overlay that started cleanly and then does nothing, with no line
        // anywhere saying the title never matched.
        let visible = found.is_some();

        if visible != window_was_found {
            window_was_found = visible;

            match found {
                Some(rect) => search_log.info(
                    "the game window appeared",
                    &[
                        ("width", Value::Int(i64::from(rect.rect.width))),
                        ("height", Value::Int(i64::from(rect.rect.height))),
                    ],
                ),
                None => search_log.warn(
                    "the game window is gone. The overlay stays hidden until it is back.",
                    &[("window_title", Value::Str(title.clone()))],
                ),
            }
        }

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

/// Run one search, respecting the rate limits.
///
/// Returns how many listings matched. The order is wait, send, adjust, and
/// none of the three can move: adjusting before sending applies the previous
/// response's limits to this one, and sending before waiting is what gets an
/// account banned.
#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
async fn search(
    http: &poe_trader_app::adapter::http_adapter::HttpAdapter,
    urls: &poe_trader_app::adapter::trade_api_adapter::TradeUrls,
    league: &str,
    session: &str,
    latency: u32,
    limits: &mut poe_trader_app::adapter::rate_limit_adapter::LimiterSet,
    checked: &poe_trader_core::controller::price_check::PriceCheck,
) -> Result<u64, String> {
    use poe_trader_app::adapter::http_adapter::HttpClient;
    use poe_trader_app::adapter::query_json_adapter::{to_exchange_json, to_json};
    use poe_trader_app::controller::price_check_controller::read_search_result;
    use poe_trader_core::controller::bulk::Endpoint;

    if session.is_empty() {
        return Err("No POESESSID. Set it to search the trade site.".to_string());
    }

    // A query that narrows nothing matches the entire trade site, and the
    // price it comes back with is the market's median rather than this item's.
    // It happens when the base type is missing from the data file, which is a
    // fixable problem worth naming rather than pricing through.
    if !checked.constrains_something() {
        return Err(
            "Nothing to search on. The base type is missing from the data file. Rebuild it from the tray."
                .to_string(),
        );
    }

    // A currency goes to the exchange endpoint. Pricing one on the search
    // endpoint returns the handful of people who listed one individually
    // rather than the market rate.
    let exchange = match (checked.endpoint, &checked.trade_tag) {
        (Endpoint::Exchange, Some(tag)) => Some(tag.clone()),
        _ => None,
    };

    let body = match &exchange {
        Some(tag) => serde_json::to_string(&to_exchange_json(tag, &[], checked.query.status)),
        None => serde_json::to_string(&to_json(&checked.query)),
    }
    .map_err(|e| format!("building the search body: {e}"))?;

    let now = now_millis();
    let wait = limits.wait_for(now);

    if wait > 0 {
        std::thread::sleep(std::time::Duration::from_millis(wait));
    }

    let now = now_millis();
    limits.borrow(now);

    let cookie = format!("POESESSID={session}");
    let headers = [("accept", "application/json"), ("cookie", cookie.as_str())];

    let url = match &exchange {
        Some(_) => urls.exchange(league),
        None => urls.search(league),
    };

    let response = http
        .post_json(&url, &headers, &body)
        .await
        .map_err(|e| format!("{e}"))?;

    limits.adjust(&response.headers, latency, now_millis());

    let result = read_search_result(&response).map_err(|e| format!("{e}"))?;

    Ok(result.total)
}

/// Milliseconds since the process started.
///
/// Monotonic, so a clock change cannot make the limiter think a window has
/// passed when it has not.
#[cfg(windows)]
fn now_millis() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;

    static START: OnceLock<Instant> = OnceLock::new();

    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// The user's documents folder.
///
/// The game writes its config under here. Read from the environment rather
/// than guessed, because a user who moved their documents folder would
/// otherwise get "not found" for a file that is plainly there.
fn documents_dir() -> String {
    if let Ok(profile) = std::env::var("USERPROFILE") {
        return format!("{profile}\\Documents");
    }

    // Not Windows, or a stripped environment. The read reports not found,
    // which is the honest answer rather than a guess at a path.
    std::env::var("HOME").map_or_else(|_| String::new(), |home| format!("{home}/Documents"))
}
