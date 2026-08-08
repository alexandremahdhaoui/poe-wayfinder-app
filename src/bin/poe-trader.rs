use std::process::ExitCode;

use poe_trader_app::adapter::game_data_adapter::GameTables;
#[cfg(windows)]
use poe_trader_app::adapter::http_adapter::HttpAdapter;
use poe_trader_app::config::PoeTraderConfig;
use poe_trader_app::controller::startup_controller;
use poe_trader_app::driver::cli_driver;
use poe_trader_app::logging::{Logger, Value};
#[cfg(windows)]
use poe_trader_core::types::GameVersion;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Some(code) = cli_driver::run_subcommand(&args) {
        return code;
    }

    let check_clipboard = args.iter().any(|a| a == "--check-clipboard");

    let press = args.iter().any(|a| a == "--press-hotkey");

    let args: Vec<String> = args
        .into_iter()
        .filter(|a| a != "--check-clipboard" && a != "--press-hotkey")
        .collect();

    let cfg = match PoeTraderConfig::load(&args) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("poe-trader: loading config: {err}");

            return ExitCode::FAILURE;
        }
    };

    let log = Logger::new(&cfg.log_level, "poe-trader");

    let http = match cli_driver::build_http(&cfg, &log) {
        Some(http) => http,
        None => return ExitCode::FAILURE,
    };

    let validated = match startup_controller::validate(
        &cfg.game,
        &cfg.price_check_hotkey,
        http.policy().check(&cfg.trade_base_url),
    ) {
        Ok(validated) => validated,
        Err(err) => {
            log.error(
                "the configuration cannot start the overlay",
                &[(
                    "error",
                    Value::Str(poe_trader_app::util::error_chain::render(&err)),
                )],
            );

            return ExitCode::FAILURE;
        }
    };

    if validated.network_disabled {
        log.warn("network is disabled. Pricing will not work.", &[]);
    }

    let hotkey = validated.hotkey;
    let game = validated.game;

    if press {
        return cli_driver::press_hotkey(&hotkey);
    }

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

    cli_driver::report_startup(&log, game, &cfg.window_title, &hotkey.to_string(), &data);

    #[cfg(windows)]
    if check_clipboard {
        return cli_driver::check_clipboard_now(game, &data, &log);
    }

    #[cfg(not(windows))]
    let _ = check_clipboard;

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

#[cfg(windows)]
fn run_overlay(
    cfg: &PoeTraderConfig,
    game: GameVersion,
    data: GameTables,
    hotkey: poe_trader_app::types::Hotkey,
    http: HttpAdapter,
    log: Logger,
) -> ExitCode {
    use poe_trader_app::adapter::clipboard_adapter::{CopyTiming, SystemClipboard};
    use poe_trader_app::adapter::clock_adapter::SystemClock;
    use poe_trader_app::adapter::game_window_adapter::{GameWindowAdapter, KeyboardCopyTrigger};
    use poe_trader_app::adapter::window_probe_adapter::SystemWindowProbe;
    use poe_trader_app::controller::copy_controller::CopyController;
    use poe_trader_app::controller::game_state_controller::GameStateController;
    use poe_trader_app::controller::panel_health_controller::PanelHealthController;
    use poe_trader_app::controller::price_check_controller::PriceCheckController;
    use poe_trader_app::driver::overlay_loop::{OverlayLoopDriver, OverlaySettings};

    let settings = OverlaySettings {
        window_title: cfg.window_title.clone(),
        league: cfg.league.clone(),
        session: cfg.poesessid.clone(),
        site_url: cfg.trade_base_url.clone(),
        data_dir: cfg.data_dir.clone(),
        log_level: cfg.log_level.clone(),
        latency: cfg.api_latency_seconds.max(0) as u32,
        restore_clipboard: cfg.restore_clipboard,
    };

    let window = GameStateController::new(GameWindowAdapter::new(&cfg.window_title));

    let clipboard = match SystemClipboard::new() {
        Ok(clipboard) => clipboard,
        Err(err) => {
            log.error(
                "opening the clipboard",
                &[("error", Value::Str(err.to_string()))],
            );

            return ExitCode::FAILURE;
        }
    };

    let copier = CopyController::new(
        clipboard,
        KeyboardCopyTrigger::new(),
        CopyTiming::default(),
        cfg.restore_clipboard,
    );

    let prices = PriceCheckController::new(
        http,
        SystemClock::new(),
        &cfg.trade_base_url,
        game,
        &cfg.league,
    )
    .with_session(&cfg.poesessid)
    .with_latency(settings.latency);

    let health = PanelHealthController::new(SystemWindowProbe::new());
    let stats = data.stat_count();

    let driver = match OverlayLoopDriver::new(
        settings,
        game,
        data,
        stats,
        &hotkey,
        window,
        copier,
        prices,
        health,
        Logger::new(&cfg.log_level, "poe-trader"),
    ) {
        Ok(driver) => driver,
        Err(err) => {
            log.error(
                "starting the overlay",
                &[(
                    "error",
                    Value::Str(poe_trader_app::util::error_chain::render(&err)),
                )],
            );

            return ExitCode::FAILURE;
        }
    };

    if let Err(err) = driver.run() {
        log.error(
            "running the overlay",
            &[(
                "error",
                Value::Str(poe_trader_app::util::error_chain::render(&err)),
            )],
        );

        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
