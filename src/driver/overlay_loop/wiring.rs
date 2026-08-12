use crate::adapter::config_store_adapter::ConfigStore;
use crate::adapter::game_data_adapter::{cache_dir, resolve_both, GameTables};
use crate::adapter::game_log_adapter::{GameLogWatcher, NoLog};
use crate::adapter::http_adapter::{HttpAdapter, HttpClient, NetworkPolicy};
use crate::config::PoeWayfinderConfig;
use crate::controller::data_refresh_controller;
use crate::controller::log_watch_controller::{LogSource, LogWatchController};
use crate::controller::settings_controller::{RememberedSettings, SettingsController};
use crate::logging::{Logger, Value};

use poe_wayfinder_core::controller::game_detect;
use poe_wayfinder_core::controller::trade_api::TradeUrls;
use poe_wayfinder_core::types::{GamePair, GameVersion};

use std::time::Duration;

pub fn build_http(cfg: &PoeWayfinderConfig, log: &Logger) -> Option<HttpAdapter> {
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

            return None;
        }
    };

    Some(http)
}

pub fn build_geometry() -> crate::types::overlay::OverlayGeometry {
    use crate::types::overlay::{Anchor, OverlayGeometry};

    OverlayGeometry {
        anchor: Anchor::Cursor,
        offset_x: 24.0,
        offset_y: 24.0,
        ..OverlayGeometry::default()
    }
}

#[cfg(windows)]
pub fn build_window(
    window_title: &str,
    log: &Logger,
) -> crate::adapter::game_window_adapter::GameWindowAdapter {
    let window = crate::adapter::game_window_adapter::GameWindowAdapter::new(window_title);

    crate::driver::cli_driver::report_input(log, &window);

    window
}

const DIAGNOSTIC_FLAGS: &[&str] = &["--check-clipboard", "--press-hotkey", "--panel-hold"];

pub fn strip_diagnostic_flags(args: Vec<String>) -> Vec<String> {
    args.into_iter()
        .filter(|a| !DIAGNOSTIC_FLAGS.contains(&a.as_str()))
        .collect()
}

pub fn panel_hold() -> bool {
    std::env::args().any(|a| a == "--panel-hold")
}

pub fn league_is_unknown(config_dir: &std::path::Path) -> bool {
    build_settings(config_dir).last_league().is_none()
}

pub fn window_title(configured: &str, game: GameVersion) -> String {
    match configured.trim().is_empty() {
        true => game_detect::title_for(game).to_string(),
        false => configured.to_string(),
    }
}

pub fn hotkeys_from(
    cfg: &PoeWayfinderConfig,
    log: &Logger,
) -> Result<
    crate::controller::startup_controller::Validated,
    crate::controller::startup_controller::StartupError,
> {
    let validated = crate::controller::startup_controller::validate(
        crate::controller::startup_controller::Declared {
            game: &cfg.game,
            hotkey: &cfg.price_check_hotkey,
            locked: &[
                &cfg.price_check_locked_hotkey,
                &cfg.price_check_locked_alt_hotkey,
            ],
            overlay: &cfg.overlay_hotkey,
            commands: &cfg.chat_commands,
            searches: &cfg.stash_searches,
            links: &cfg.item_link_hotkeys,
        },
        NetworkPolicy::new(
            cfg.network_enabled,
            cfg.block_unlisted_hosts,
            &cfg.allowed_hosts,
        )
        .check(&cfg.trade_base_url),
    )?;

    log.info(
        "hotkeys",
        &[
            ("price_check", Value::Str(validated.hotkey.to_string())),
            ("locked", Value::Int(validated.locked.len() as i64)),
            ("commands", Value::Int(validated.commands.len() as i64)),
            (
                "overlay",
                Value::Str(
                    validated
                        .overlay
                        .as_ref()
                        .map(|h| h.to_string())
                        .unwrap_or_else(|| "off".to_string()),
                ),
            ),
        ],
    );

    Ok(validated)
}

#[cfg(windows)]
pub fn build_settings_for(
    cfg: &PoeWayfinderConfig,
    game: GameVersion,
    pinned: Option<GameVersion>,
    origin: &GamePair<crate::adapter::game_data_adapter::Origin>,
) -> super::OverlaySettings {
    super::OverlaySettings {
        window_title: window_title(&cfg.window_title, game),
        pinned_title: !cfg.window_title.trim().is_empty(),
        pinned_game: pinned,
        league: cfg.league.clone(),
        session: cfg.poesessid.clone(),
        site_url: cfg.trade_base_url.clone(),
        data_dir: cfg.data_dir.clone(),
        log_level: cfg.log_level.clone(),
        latency: cfg.api_latency_seconds.max(0) as u32,
        restore_clipboard: cfg.restore_clipboard,
        data_origin: origin.get(game).as_str().to_string(),
        network: cfg.network_enabled,
    }
}

#[cfg(windows)]
pub fn build_game_state(
    cfg: &PoeWayfinderConfig,
    pinned: Option<GameVersion>,
    log: &Logger,
) -> (
    crate::controller::game_state_controller::GameStateController<
        crate::adapter::game_window_adapter::GameWindowAdapter,
    >,
    GameVersion,
) {
    use crate::controller::game_state_controller::{GameState, GameStateController};

    let provisional = pinned.unwrap_or(GameVersion::Poe2);
    let window = GameStateController::new(build_window(
        &window_title(&cfg.window_title, provisional),
        log,
    ));

    let game = match pinned {
        Some(named) => named,
        None => window.detect_game().unwrap_or(provisional),
    };

    if game != provisional {
        window.retarget(game);
    }

    log.info(
        "the overlay is watching",
        &[
            ("game", Value::Str(game.as_str().to_string())),
            (
                "window_title",
                Value::Str(window_title(&cfg.window_title, game)),
            ),
            ("detected", Value::Bool(pinned.is_none())),
        ],
    );

    (window, game)
}

pub fn build_data(
    cfg: &PoeWayfinderConfig,
    config_dir: &std::path::Path,
    pinned: Option<GameVersion>,
    log: &Logger,
) -> Option<(
    GamePair<GameTables>,
    GamePair<crate::adapter::game_data_adapter::Origin>,
)> {
    let (tables, origin) = match resolve_both(&cfg.data_dir, config_dir, pinned) {
        Ok(found) => found,
        Err(err) => {
            log.error(
                "loading game data",
                &[
                    ("data_dir", Value::Str(cfg.data_dir.clone())),
                    ("error", Value::Str(err.to_string())),
                ],
            );

            return None;
        }
    };

    for game in [GameVersion::Poe1, GameVersion::Poe2] {
        let one = tables.get(game);

        log.info(
            "loaded the game data",
            &[
                ("origin", Value::Str(origin.get(game).as_str().to_string())),
                ("game", Value::Str(game.as_str().to_string())),
                ("stats", Value::Int(one.stat_count() as i64)),
                ("items", Value::Int(one.item_name_count() as i64)),
                ("augments", Value::Int(one.augment_count() as i64)),
            ],
        );
    }

    Some((tables, origin))
}

const LOG_ROOTS: &[&str] = &[
    r"C:\Program Files (x86)\Grinding Gear Games",
    r"C:\Program Files\Grinding Gear Games",
    r"C:\Program Files (x86)\Steam\steamapps\common",
    r"C:\Program Files\Steam\steamapps\common",
    r"D:\SteamLibrary\steamapps\common",
];

pub fn client_log_candidates(game: GameVersion) -> Vec<std::path::PathBuf> {
    let folder = game_detect::title_for(game);
    let mut out = Vec::new();

    if let Some(profile) = std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()) {
        out.push(
            std::path::PathBuf::from(profile)
                .join("Documents")
                .join("My Games")
                .join(folder)
                .join("logs")
                .join("Client.txt"),
        );
    }

    for root in LOG_ROOTS {
        out.push(
            std::path::PathBuf::from(root)
                .join(folder)
                .join("logs")
                .join("Client.txt"),
        );
    }

    out
}

pub fn default_client_log(game: GameVersion) -> Option<std::path::PathBuf> {
    client_log_candidates(game)
        .into_iter()
        .find(|p| p.is_file())
}

pub fn resolve_client_log(configured: &str, game: GameVersion, log: &Logger) -> String {
    if !configured.trim().is_empty() {
        return configured.to_string();
    }

    match default_client_log(game) {
        Some(found) => {
            log.info(
                "found the client log, so the league is read from the game",
                &[("path", Value::Str(found.display().to_string()))],
            );

            found.display().to_string()
        }
        None => {
            log.info(
                "no client log found, so the league stays as configured",
                &[("game", Value::Str(game.as_str().to_string()))],
            );

            String::new()
        }
    }
}

pub fn build_logs(client_log_path: &str, read_history: bool) -> Box<dyn LogSource> {
    if client_log_path.is_empty() {
        return Box::new(LogWatchController::new(NoLog));
    }

    let path = std::path::Path::new(client_log_path);

    let watcher = match read_history {
        true => GameLogWatcher::from_start(path),
        false => GameLogWatcher::new(path),
    };

    Box::new(LogWatchController::new(watcher))
}

#[cfg(windows)]
pub fn build_copier(
    restore: bool,
    log: &Logger,
) -> Option<
    crate::controller::copy_controller::CopyController<
        crate::adapter::clipboard_adapter::SystemClipboard,
        crate::adapter::game_window_adapter::KeyboardCopyTrigger,
    >,
> {
    use crate::adapter::clipboard_adapter::{CopyTiming, SystemClipboard};
    use crate::adapter::game_window_adapter::KeyboardCopyTrigger;
    use crate::controller::copy_controller::CopyController;

    let clipboard = match SystemClipboard::new() {
        Ok(clipboard) => clipboard,
        Err(err) => {
            log.error(
                "opening the clipboard",
                &[("error", Value::Str(err.to_string()))],
            );

            return None;
        }
    };

    Some(CopyController::new(
        clipboard,
        KeyboardCopyTrigger::new(),
        CopyTiming::default(),
        restore,
    ))
}

#[cfg(windows)]
pub fn send_chat<P>(action: &poe_wayfinder_core::controller::chat::ChatAction, put: P) -> bool
where
    P: FnOnce(&str) -> bool,
{
    use crate::adapter::game_window_adapter::press_combination;
    use crate::driver::chat_driver::{key_code, needs_control};

    const CTRL: u16 = 0x11;

    if !put(&action.text) {
        return false;
    }

    for key in &action.keys {
        let modifiers: &[u16] = match needs_control(*key) {
            true => &[CTRL],
            false => &[],
        };

        if press_combination(modifiers, key_code(*key)) == 0 {
            return false;
        }

        std::thread::sleep(Duration::from_millis(20));
    }

    true
}

#[cfg(not(windows))]
pub fn send_chat<P>(_action: &poe_wayfinder_core::controller::chat::ChatAction, _put: P) -> bool
where
    P: FnOnce(&str) -> bool,
{
    false
}

pub fn as_happening(
    event: &crate::adapter::game_log_adapter::LogEvent,
) -> Option<poe_wayfinder_core::controller::background::Happening> {
    use crate::adapter::game_log_adapter::LogEvent;
    use poe_wayfinder_core::controller::background::Happening;

    match event {
        LogEvent::EnteredArea { name } => Some(Happening::EnteredArea { name: name.clone() }),
        LogEvent::LevelUp {
            character, level, ..
        } => Some(Happening::LevelledUp {
            character: character.clone(),
            level: *level,
        }),
        LogEvent::Whisper { from, .. } => Some(Happening::Whisper { from: from.clone() }),
    }
}

pub fn build_settings(dir: &std::path::Path) -> SettingsController<ConfigStore> {
    SettingsController::new(ConfigStore::new(dir))
}

#[derive(Debug, Clone)]
pub struct RefreshPlan {
    network_enabled: bool,
    block_unlisted_hosts: bool,
    allowed_hosts: String,
    trade_base_url: String,
    user_agent: String,
    log_level: String,
    config_dir: std::path::PathBuf,
}

impl RefreshPlan {
    pub fn new(cfg: &PoeWayfinderConfig, config_dir: &std::path::Path) -> Self {
        Self {
            network_enabled: cfg.network_enabled,
            block_unlisted_hosts: cfg.block_unlisted_hosts,
            allowed_hosts: cfg.allowed_hosts.clone(),
            trade_base_url: cfg.trade_base_url.clone(),
            user_agent: cfg.user_agent.clone(),
            log_level: cfg.log_level.clone(),
            config_dir: config_dir.to_path_buf(),
        }
    }

    pub fn due_now(&self) -> Vec<GameVersion> {
        [GameVersion::Poe1, GameVersion::Poe2]
            .into_iter()
            .filter(|game| refresh_is_due(&self.config_dir, *game))
            .collect()
    }

    pub fn last_refresh(&self, game: GameVersion) -> Option<std::time::SystemTime> {
        data_refresh_controller::last_refresh(&cache_dir(&self.config_dir, game))
    }

    pub fn forget(&self, game: GameVersion) {
        let cache = cache_dir(&self.config_dir, game);

        let _ = std::fs::remove_file(data_refresh_controller::stamp_path(&cache));
    }

    pub fn start(&self, games: Vec<GameVersion>, log: &Logger) {
        if !self.network_enabled || games.is_empty() {
            log.debug("nothing to refresh", &[]);

            return;
        }

        let policy = NetworkPolicy::new(
            self.network_enabled,
            self.block_unlisted_hosts,
            &self.allowed_hosts,
        );

        let base_url = self.trade_base_url.clone();
        let user_agent = self.user_agent.clone();
        let config_dir = self.config_dir.clone();
        let log_level = self.log_level.clone();

        std::thread::spawn(move || {
            let log = Logger::new(&log_level, "poe-wayfinder-refresh");

            let Ok(http) =
                HttpAdapter::with_user_agent(policy, Duration::from_secs(60), &user_agent)
            else {
                return;
            };

            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };

            for game in games {
                runtime.block_on(refresh_one(&http, &base_url, &config_dir, game, &log));
            }
        });
    }
}

pub fn refresh_is_due(config_dir: &std::path::Path, game: GameVersion) -> bool {
    let cache = cache_dir(config_dir, game);

    data_refresh_controller::refresh_due(
        data_refresh_controller::last_refresh(&cache),
        std::time::SystemTime::now(),
    )
}

async fn refresh_one(
    http: &HttpAdapter,
    base_url: &str,
    config_dir: &std::path::Path,
    game: GameVersion,
    log: &Logger,
) {
    let urls = TradeUrls::new(base_url, game);

    let mut bodies = Vec::new();

    for table in ["stats", "items", "static"] {
        let url = urls.data(table);

        log.info(
            "fetching a data table",
            &[
                ("url", Value::Str(url.clone())),
                ("game", Value::Str(game.as_str().to_string())),
            ],
        );

        match http.get(&url, &[("accept", "application/json")]).await {
            Ok(response) if response.status == 200 => bodies.push(response.body),
            Ok(response) => {
                log.warn(
                    "the trade api refused the data table, so the cache is left alone",
                    &[
                        ("table", Value::Str(table.to_string())),
                        ("status", Value::Int(i64::from(response.status))),
                    ],
                );

                return;
            }
            Err(err) => {
                log.warn(
                    "could not reach the trade api, so the cache is left alone",
                    &[
                        ("table", Value::Str(table.to_string())),
                        ("error", Value::Str(crate::util::error_chain::render(&err))),
                    ],
                );

                return;
            }
        }
    }

    let built = match data_refresh_controller::build(&bodies[0], &bodies[1], &bodies[2]) {
        Ok(built) => built,
        Err(err) => {
            log.warn(
                "the trade api answered with something unusable, so the cache is left alone",
                &[("error", Value::Str(crate::util::error_chain::render(&err)))],
            );

            return;
        }
    };

    let cache = cache_dir(config_dir, game);

    if let Err(err) = write_cache(&cache, &built) {
        log.warn(
            "could not write the refreshed data",
            &[
                ("path", Value::Str(cache.display().to_string())),
                ("error", Value::Str(err.to_string())),
            ],
        );

        return;
    }

    log.info(
        "refreshed the game data. It is used from the next launch.",
        &[
            ("game", Value::Str(game.as_str().to_string())),
            ("path", Value::Str(cache.display().to_string())),
            ("stats", Value::Int(built.stat_count as i64)),
            ("items", Value::Int(built.item_count as i64)),
        ],
    );
}

fn write_cache(
    cache: &std::path::Path,
    built: &data_refresh_controller::Built,
) -> std::io::Result<()> {
    std::fs::create_dir_all(cache)?;

    for (name, body) in [
        ("stats.ndjson", &built.stats),
        ("items.ndjson", &built.items),
    ] {
        let temporary = cache.join(format!("{name}.tmp"));

        std::fs::write(&temporary, body)?;
        std::fs::rename(&temporary, cache.join(name))?;
    }

    std::fs::write(data_refresh_controller::stamp_path(cache), b"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unset_window_title_is_derived_from_the_game() {
        assert_eq!(window_title("", GameVersion::Poe1), "Path of Exile");
        assert_eq!(window_title("  ", GameVersion::Poe2), "Path of Exile 2");
    }

    #[test]
    fn a_window_title_given_by_hand_pins_the_window() {
        assert_eq!(
            window_title("Some Other Window", GameVersion::Poe2),
            "Some Other Window"
        );
    }

    #[test]
    fn with_no_flags_at_all_the_data_comes_from_inside_the_binary() {
        let cfg = PoeWayfinderConfig::load(&[]).expect("defaults load with no arguments");

        assert_eq!(cfg.data_dir, "", "--data-dir must not be required");
        assert_eq!(cfg.game, "auto", "--game must default to detection");
        assert_eq!(cfg.window_title, "", "the title is derived, not configured");

        let log = Logger::new("error", "test");
        let dir = std::env::temp_dir().join("poe-wayfinder-wiring-nodata");

        let (tables, _) = build_data(&cfg, &dir, None, &log).expect("the built in data loads");

        assert!(tables.get(GameVersion::Poe1).stat_count() > 1000);
        assert!(tables.get(GameVersion::Poe2).stat_count() > 1000);
    }

    fn plan_in(name: &str) -> (RefreshPlan, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "poe-wayfinder-refresh-{}-{name}",
            std::process::id()
        ));

        let _ = std::fs::remove_dir_all(&dir);

        std::fs::create_dir_all(&dir).unwrap();

        let mut cfg = PoeWayfinderConfig::load(&[]).expect("defaults");

        cfg.network_enabled = false;

        (RefreshPlan::new(&cfg, &dir), dir)
    }

    fn stamp(dir: &std::path::Path, game: GameVersion) {
        let cache = cache_dir(dir, game);

        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(data_refresh_controller::stamp_path(&cache), b"").unwrap();
    }

    #[test]
    fn a_first_run_wants_both_games_refreshed() {
        let (plan, _) = plan_in("first");

        assert_eq!(plan.due_now().len(), 2);
    }

    #[test]
    fn a_game_refreshed_just_now_is_not_asked_for_again() {
        let (plan, dir) = plan_in("throttle");

        stamp(&dir, GameVersion::Poe2);

        assert_eq!(plan.due_now(), vec![GameVersion::Poe1]);
    }

    #[test]
    fn forgetting_a_game_puts_it_back_on_the_list() {
        let (plan, dir) = plan_in("forget");

        stamp(&dir, GameVersion::Poe1);
        stamp(&dir, GameVersion::Poe2);

        assert!(plan.due_now().is_empty());

        plan.forget(GameVersion::Poe1);

        assert_eq!(plan.due_now(), vec![GameVersion::Poe1]);
    }

    #[test]
    fn forgetting_a_game_that_was_never_refreshed_is_not_an_error() {
        let (plan, _) = plan_in("forget-missing");

        plan.forget(GameVersion::Poe2);
    }

    #[test]
    fn a_disabled_network_starts_nothing() {
        let (plan, dir) = plan_in("offline");
        let log = Logger::new("error", "test");

        plan.start(plan.due_now(), &log);

        std::thread::sleep(std::time::Duration::from_millis(100));

        assert!(
            !cache_dir(&dir, GameVersion::Poe2)
                .join("stats.ndjson")
                .exists(),
            "network_enabled false must mean no socket and no write"
        );
    }

    #[test]
    fn a_client_log_path_given_by_hand_is_used_as_given() {
        let log = Logger::new("error", "test");

        assert_eq!(
            resolve_client_log("/somewhere/Client.txt", GameVersion::Poe2, &log),
            "/somewhere/Client.txt"
        );
    }

    #[test]
    fn each_game_is_looked_for_in_its_own_install_folder() {
        let one = client_log_candidates(GameVersion::Poe1);
        let two = client_log_candidates(GameVersion::Poe2);

        assert!(!one.is_empty());
        assert_eq!(one.len(), two.len());

        for (a, b) in one.iter().zip(two.iter()) {
            assert_ne!(a, b, "PoE1 and PoE2 must not share a Client.txt");
        }
    }

    #[test]
    fn every_candidate_ends_at_a_client_log() {
        for game in [GameVersion::Poe1, GameVersion::Poe2] {
            for path in client_log_candidates(game) {
                assert_eq!(
                    path.file_name().and_then(|n| n.to_str()),
                    Some("Client.txt"),
                    "{}",
                    path.display()
                );
                assert!(
                    path.to_string_lossy()
                        .contains(game_detect::title_for(game)),
                    "{}",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn nothing_installed_leaves_the_league_alone_rather_than_failing() {
        let log = Logger::new("error", "test");

        let got = resolve_client_log("", GameVersion::Poe2, &log);

        assert!(got.is_empty() || std::path::Path::new(&got).is_file());
    }

    #[test]
    fn an_empty_log_path_selects_a_source_that_reads_nothing() {
        let mut logs = build_logs("", false);

        assert!(logs.poll().expect("no error").is_empty());
    }

    #[test]
    fn a_log_path_that_does_not_exist_does_not_panic() {
        let mut logs = build_logs("/nowhere/Client.txt", true);

        assert!(logs.poll().is_err() || logs.poll().expect("no error").is_empty());
    }

    #[test]
    fn settings_from_a_missing_directory_fall_back_to_defaults() {
        use crate::controller::settings_controller::RememberedSettings;

        let settings = build_settings(std::path::Path::new("/nowhere/at/all"));

        assert_eq!(settings.last_league(), None);
    }
    #[test]
    fn a_diagnostic_flag_never_reaches_the_config_parser() {
        let got = strip_diagnostic_flags(vec![
            "--game".to_string(),
            "poe2".to_string(),
            "--panel-hold".to_string(),
            "--press-hotkey".to_string(),
        ]);

        assert_eq!(got, vec!["--game".to_string(), "poe2".to_string()]);
    }

    #[test]
    fn every_diagnostic_flag_is_stripped() {
        for flag in DIAGNOSTIC_FLAGS {
            assert!(
                strip_diagnostic_flags(vec![flag.to_string()]).is_empty(),
                "{flag}"
            );
        }
    }

    #[test]
    fn a_real_argument_survives() {
        let got = strip_diagnostic_flags(vec!["--data-dir".to_string(), "data-poe2".to_string()]);

        assert_eq!(got.len(), 2);
    }
}
