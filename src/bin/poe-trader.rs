#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;

use poe_trader_app::adapter::config_store_adapter;
#[cfg(windows)]
use poe_trader_app::adapter::game_data_adapter::GameTables;
#[cfg(windows)]
use poe_trader_app::adapter::http_adapter::HttpAdapter;
use poe_trader_app::config::PoeTraderConfig;
use poe_trader_app::controller::startup_controller;
use poe_trader_app::driver::cli_driver;
use poe_trader_app::driver::overlay_loop::wiring;
use poe_trader_app::logging::{Logger, Value};
#[cfg(windows)]
use poe_trader_core::types::{GamePair, GameVersion};

fn main() -> ExitCode {
    cli_driver::attach_console();

    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Some(code) = cli_driver::run_subcommand(&args) {
        return code;
    }

    let check_clipboard = args.iter().any(|a| a == "--check-clipboard");

    let press = args.iter().any(|a| a == "--press-hotkey");

    let args = wiring::strip_diagnostic_flags(args);

    let cfg = match PoeTraderConfig::load(&args) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("poe-trader: loading config: {err}");

            return ExitCode::FAILURE;
        }
    };

    let log = Logger::new(&cfg.log_level, "poe-trader");

    let http = match wiring::build_http(&cfg, &log) {
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

    let game = validated.starting_game();
    let hotkey = validated.hotkey;
    let pinned = validated.game;

    if press {
        return cli_driver::press_hotkey(&hotkey);
    }

    let config_dir = config_store_adapter::resolve_dir(&cfg.config_dir);

    let Some((data, origin)) = wiring::build_data(&cfg, &config_dir, pinned, &log) else {
        return ExitCode::FAILURE;
    };

    let title = wiring::window_title(&cfg.window_title, game);

    cli_driver::report_startup(&log, game, &title, &hotkey.to_string(), data.get(game));

    #[cfg(windows)]
    if check_clipboard {
        return cli_driver::check_clipboard_now(game, data.get(game), &log);
    }

    #[cfg(not(windows))]
    let _ = check_clipboard;

    #[cfg(windows)]
    return run_overlay(&cfg, &config_dir, pinned, data, origin, hotkey, http, log);

    #[cfg(not(windows))]
    {
        log.warn(
            "the overlay only runs on Windows. Use poe-trader-cli here.",
            &[],
        );

        let _ = (http, hotkey, game, data, origin);

        ExitCode::SUCCESS
    }
}

#[cfg(windows)]
fn run_overlay(
    cfg: &PoeTraderConfig,
    config_dir: &std::path::Path,
    pinned: Option<GameVersion>,
    data: GamePair<GameTables>,
    origin: GamePair<poe_trader_app::adapter::game_data_adapter::Origin>,
    hotkey: poe_trader_app::types::Hotkey,
    http: HttpAdapter,
    log: Logger,
) -> ExitCode {
    use poe_trader_app::adapter::clock_adapter::SystemClock;
    use poe_trader_app::adapter::input_state_adapter::{hold_key_for, SystemInput};
    use poe_trader_app::adapter::window_probe_adapter::SystemWindowProbe;
    use poe_trader_app::controller::input_controller::InputController;

    use poe_trader_app::controller::panel_health_controller::PanelHealthController;
    use poe_trader_app::controller::price_check_controller::PriceCheckController;
    use poe_trader_app::driver::overlay_loop::{wiring, OverlayLoopDriver};

    let (window, game) = wiring::build_game_state(cfg, pinned, &log);

    let settings = wiring::build_settings_for(cfg, game, pinned, &origin);

    let Some(copier) = wiring::build_copier(cfg.restore_clipboard, &log) else {
        return ExitCode::FAILURE;
    };

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
    let hold = hold_key_for(hotkey.modifiers());
    let input = InputController::new(SystemInput::new(), hold);

    let client_log = wiring::resolve_client_log(&cfg.client_log_path, game, &log);
    let refresh = wiring::RefreshPlan::new(cfg, config_dir);

    refresh.start(refresh.due_now(), &log);

    let logs = wiring::build_logs(&client_log, wiring::league_is_unknown(config_dir));
    let remembered = wiring::build_settings(config_dir);
    let stats = data.get(game).stat_count();

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
        input,
        logs,
        remembered,
        hold,
        refresh,
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
