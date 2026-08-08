use std::process::ExitCode;
use std::time::Duration;

use poe_trader_app::adapter::game_data_adapter::GameTables;
use poe_trader_app::adapter::http_adapter::{HttpAdapter, NetworkPolicy, PolicyError};
use poe_trader_app::config::PoeTraderConfig;
use poe_trader_app::driver::cli_driver;
use poe_trader_app::logging::{Logger, Value};
use poe_trader_app::types::Hotkey;
use poe_trader_core::types::GameVersion;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--list-windows") {
        return cli_driver::list_windows();
    }

    if args.first().map(String::as_str) == Some("--fake-game") {
        let title = args
            .get(1)
            .cloned()
            .unwrap_or_else(|| "Path of Exile 2".into());
        let seconds = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(30);

        let Some(path) = args.get(3) else {
            eprintln!("usage: --fake-game <title> <seconds> <item-file>");

            return ExitCode::FAILURE;
        };

        let item = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) => {
                eprintln!("poe-trader: reading {path}: {err}");

                return ExitCode::FAILURE;
            }
        };

        return cli_driver::fake_game(&title, seconds, &item);
    }

    if args.iter().any(|a| a == "--self-test-hook") {
        return cli_driver::self_test_hook();
    }

    if args.iter().any(|a| a == "--self-test-hotkey") {
        return cli_driver::self_test_hotkey();
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

    let policy = NetworkPolicy::new(
        cfg.network_enabled,
        cfg.block_unlisted_hosts,
        &cfg.allowed_hosts,
    );

    log.info(
        "network policy",
        &[
            ("enabled", Value::Bool(cfg.network_enabled)),
            ("block_unlisted", Value::Bool(cfg.block_unlisted_hosts)),
            ("hosts", Value::Str(policy.allowed_hosts().join(","))),
        ],
    );

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

    if press {
        return cli_driver::press_hotkey(&hotkey);
    }

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

    let game_config = poe_trader_app::adapter::game_config_adapter::read(
        std::path::Path::new(&cli_driver::documents_dir()),
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
            ("read", Value::Bool(game_config.read)),
        ],
    );

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
