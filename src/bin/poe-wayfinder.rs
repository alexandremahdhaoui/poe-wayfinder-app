#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;

use poe_wayfinder_app::adapter::config_store_adapter;
#[cfg(windows)]
use poe_wayfinder_app::adapter::game_data_adapter::GameTables;
#[cfg(windows)]
use poe_wayfinder_app::adapter::http_adapter::HttpAdapter;
use poe_wayfinder_app::config::PoeWayfinderConfig;
use poe_wayfinder_app::driver::cli_driver;
use poe_wayfinder_app::driver::overlay_loop::wiring;
use poe_wayfinder_app::logging::{Logger, Value};
#[cfg(windows)]
use poe_wayfinder_core::types::{GamePair, GameVersion};

fn main() -> ExitCode {
    cli_driver::attach_console();

    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Some(code) = cli_driver::run_subcommand(&args) {
        return code;
    }

    let check_clipboard = args.iter().any(|a| a == "--check-clipboard");

    let press = args.iter().any(|a| a == "--press-hotkey");

    let args = wiring::strip_diagnostic_flags(args);

    let cfg = match PoeWayfinderConfig::load(&args) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("poe-wayfinder: loading config: {err}");

            return ExitCode::FAILURE;
        }
    };

    let log = Logger::new(&cfg.log_level, "poe-wayfinder");

    let http = match wiring::build_http(&cfg, &log) {
        Some(http) => http,
        None => return ExitCode::FAILURE,
    };

    let validated = match wiring::hotkeys_from(&cfg, &log) {
        Ok(validated) => validated,
        Err(err) => {
            log.error(
                "the configuration cannot start the overlay",
                &[(
                    "error",
                    Value::Str(poe_wayfinder_app::util::error_chain::render(&err)),
                )],
            );

            return ExitCode::FAILURE;
        }
    };

    if validated.network_disabled {
        log.warn("network is disabled. Pricing will not work.", &[]);
    }

    let game = validated.starting_game();
    let hotkey = validated.hotkey.clone();
    let roles = validated.clone();
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
    return run_overlay(
        &cfg,
        &config_dir,
        pinned,
        data,
        origin,
        hotkey,
        roles,
        http,
        log,
    );

    #[cfg(not(windows))]
    {
        log.warn(
            "the overlay only runs on Windows. Use poe-wayfinder-cli here.",
            &[],
        );

        let _ = (http, hotkey, roles, game, data, origin);

        ExitCode::SUCCESS
    }
}

#[cfg(windows)]
fn run_overlay(
    cfg: &PoeWayfinderConfig,
    config_dir: &std::path::Path,
    pinned: Option<GameVersion>,
    data: GamePair<GameTables>,
    origin: GamePair<poe_wayfinder_app::adapter::game_data_adapter::Origin>,
    hotkey: poe_wayfinder_app::types::Hotkey,
    roles: poe_wayfinder_app::controller::startup_controller::Validated,
    http: HttpAdapter,
    log: Logger,
) -> ExitCode {
    use poe_wayfinder_app::adapter::clock_adapter::SystemClock;
    use poe_wayfinder_app::adapter::input_state_adapter::{hold_key_for, SystemInput};
    use poe_wayfinder_app::adapter::window_probe_adapter::SystemWindowProbe;
    use poe_wayfinder_app::controller::input_controller::InputController;

    use poe_wayfinder_app::controller::panel_health_controller::PanelHealthController;
    use poe_wayfinder_app::controller::price_check_controller::PriceCheckController;
    use poe_wayfinder_app::driver::overlay_loop::{wiring, OverlayLoopDriver};

    let (window, game) = wiring::build_game_state(cfg, pinned, &log);

    let mut settings = wiring::build_settings_for(cfg, game, pinned, &origin);

    let Some(copier) = wiring::build_copier(cfg.restore_clipboard, &log) else {
        return ExitCode::FAILURE;
    };

    let league = wiring::resolve_league(
        &cfg.league,
        wiring::remembered_league(config_dir),
        match cfg.league.trim().is_empty() {
            true => wiring::fetch_current_league(&http, &cfg.trade_base_url, game, &log),
            false => None,
        },
    );

    log.info(
        "searching this league",
        &[("league", Value::Str(league.clone()))],
    );

    settings.league.clone_from(&league);

    let prices = PriceCheckController::new(
        http,
        SystemClock::new(),
        &cfg.trade_base_url,
        game,
        &league,
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
        roles,
        window,
        copier,
        prices,
        health,
        input,
        logs,
        remembered,
        hold,
        refresh,
        Logger::new(&cfg.log_level, "poe-wayfinder"),
    ) {
        Ok(driver) => driver,
        Err(err) => {
            log.error(
                "starting the overlay",
                &[(
                    "error",
                    Value::Str(poe_wayfinder_app::util::error_chain::render(&err)),
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
                Value::Str(poe_wayfinder_app::util::error_chain::render(&err)),
            )],
        );

        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
