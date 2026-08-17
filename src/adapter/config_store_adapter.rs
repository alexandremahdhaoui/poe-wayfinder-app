use std::path::{Path, PathBuf};

use poe_wayfinder_core::types::{GamePair, GameVersion};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("creating {path}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("writing {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("encoding the settings")]
    Encode(#[source] serde_json::Error),
}

pub const LEAGUE_NAME_LIMIT: usize = 40;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LastLeague {
    Shared(String),
    PerGame(GamePair<String>),
}

impl Default for LastLeague {
    fn default() -> Self {
        LastLeague::Shared(String::new())
    }
}

impl LastLeague {
    pub fn get(&self, game: GameVersion) -> &str {
        match self {
            LastLeague::Shared(shared) => shared,
            LastLeague::PerGame(pair) => pair.get(game),
        }
    }

    pub fn set(&mut self, game: GameVersion, league: &str) {
        let mut pair = match self {
            LastLeague::Shared(shared) => GamePair::new(shared.clone(), shared.clone()),
            LastLeague::PerGame(pair) => pair.clone(),
        };

        *pair.get_mut(game) = league.to_string();

        *self = LastLeague::PerGame(pair);
    }

    fn dropping_absurd_names(self) -> Self {
        let sane = |name: &str| match name.len() > LEAGUE_NAME_LIMIT {
            true => String::new(),
            false => name.to_string(),
        };

        match self {
            LastLeague::Shared(shared) => LastLeague::Shared(sane(&shared)),
            LastLeague::PerGame(pair) => LastLeague::PerGame(GamePair::new(
                sane(pair.get(GameVersion::Poe1)),
                sane(pair.get(GameVersion::Poe2)),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub panel_offset_x: f32,
    pub panel_offset_y: f32,
    pub panel_width: f32,
    pub panel_height: f32,
    pub last_league: LastLeague,
    pub pinned_leagues: GamePair<bool>,
    pub include_offline: bool,
    pub roll_tolerance: f64,
    pub filter_item_level: bool,
    pub notes: String,
    pub map_verdicts: Vec<(String, String)>,
    pub bound_hotkey: String,
    pub bound_chord: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            panel_offset_x: 16.0,
            panel_offset_y: 16.0,
            panel_width: 400.0,
            panel_height: 600.0,
            last_league: LastLeague::default(),
            pinned_leagues: GamePair::default(),
            include_offline: false,
            roll_tolerance: 0.1,
            filter_item_level: false,
            notes: String::new(),
            map_verdicts: Vec::new(),
            bound_hotkey: String::new(),
            bound_chord: String::new(),
        }
    }
}

impl Settings {
    pub fn sanitised(mut self) -> Self {
        self.panel_width = self.panel_width.clamp(120.0, 4000.0);
        self.panel_height = self.panel_height.clamp(80.0, 4000.0);
        self.panel_offset_x = self.panel_offset_x.clamp(-4000.0, 4000.0);
        self.panel_offset_y = self.panel_offset_y.clamp(-4000.0, 4000.0);
        self.roll_tolerance = self.roll_tolerance.clamp(0.0, 1.0);

        self.last_league = self.last_league.dropping_absurd_names();

        self
    }
}

#[cfg_attr(test, mockall::automock)]
pub trait SettingsStore: Send + Sync {
    fn load(&self) -> Settings;

    fn save(&self, settings: &Settings) -> Result<(), StoreError>;
}

impl SettingsStore for ConfigStore {
    fn load(&self) -> Settings {
        ConfigStore::load(self)
    }

    fn save(&self, settings: &Settings) -> Result<(), StoreError> {
        ConfigStore::save(self, settings)
    }
}

pub const APP_DIR: &str = "poe-wayfinder";

pub fn resolve_dir(configured: &str) -> PathBuf {
    if !configured.trim().is_empty() {
        return PathBuf::from(configured);
    }

    default_dir()
}

pub fn default_dir() -> PathBuf {
    for key in ["APPDATA", "XDG_CONFIG_HOME"] {
        if let Some(base) = std::env::var_os(key).filter(|v| !v.is_empty()) {
            return PathBuf::from(base).join(APP_DIR);
        }
    }

    if let Some(home) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(home).join(".config").join(APP_DIR);
    }

    PathBuf::from(".").join(APP_DIR)
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn new(dir: &Path) -> Self {
        Self {
            path: dir.join("settings.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Settings {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return Settings::default();
        };

        serde_json::from_str::<Settings>(&text)
            .unwrap_or_default()
            .sanitised()
    }

    pub fn save(&self, settings: &Settings) -> Result<(), StoreError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| StoreError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let text = serde_json::to_string_pretty(settings).map_err(StoreError::Encode)?;

        let temporary = self.path.with_extension("json.tmp");

        std::fs::write(&temporary, text).map_err(|source| StoreError::Write {
            path: temporary.clone(),
            source,
        })?;

        std::fs::rename(&temporary, &self.path).map_err(|source| StoreError::Write {
            path: self.path.clone(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_configured_directory_is_used_as_given() {
        assert_eq!(resolve_dir("/somewhere"), PathBuf::from("/somewhere"));
        assert_eq!(resolve_dir("  "), default_dir());
        assert_eq!(resolve_dir(""), default_dir());
    }

    #[test]
    fn the_default_directory_is_named_after_the_app_and_is_not_the_working_directory() {
        let dir = default_dir();

        assert_eq!(dir.file_name().and_then(|n| n.to_str()), Some(APP_DIR));
        assert!(dir.parent().is_some(), "{}", dir.display());
    }

    fn tempdir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("poe-wayfinder-store-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        dir
    }

    #[test]
    fn a_missing_file_loads_the_defaults() {
        let store = ConfigStore::new(Path::new("/nonexistent/poe-wayfinder"));

        assert_eq!(store.load(), Settings::default());
    }

    #[test]
    fn settings_survive_a_round_trip() {
        let store = ConfigStore::new(&tempdir("round-trip"));

        let settings = Settings {
            panel_offset_x: 100.0,
            last_league: LastLeague::Shared("Standard".into()),
            include_offline: true,
            ..Settings::default()
        };

        store.save(&settings).unwrap();

        assert_eq!(store.load(), settings);
    }

    #[test]
    fn a_corrupt_file_loads_the_defaults() {
        let dir = tempdir("corrupt");
        let store = ConfigStore::new(&dir);

        std::fs::write(store.path(), "{not json").unwrap();

        assert_eq!(store.load(), Settings::default());
    }

    #[test]
    fn an_older_file_missing_a_newer_field_still_loads() {
        let dir = tempdir("partial");
        let store = ConfigStore::new(&dir);

        std::fs::write(store.path(), r#"{"last_league":"Hardcore"}"#).unwrap();

        let got = store.load();

        assert_eq!(got.last_league.get(GameVersion::Poe1), "Hardcore");
        assert_eq!(got.last_league.get(GameVersion::Poe2), "Hardcore");
        assert_eq!(got.panel_width, Settings::default().panel_width);
    }

    #[test]
    fn a_zero_sized_panel_is_clamped() {
        let got = Settings {
            panel_width: 0.0,
            panel_height: 0.0,
            ..Settings::default()
        }
        .sanitised();

        assert!(got.panel_width >= 120.0);
        assert!(got.panel_height >= 80.0);
    }

    #[test]
    fn an_absurd_panel_size_is_clamped() {
        let got = Settings {
            panel_width: 1.0e9,
            ..Settings::default()
        }
        .sanitised();

        assert!(got.panel_width <= 4000.0);
    }

    #[test]
    fn a_negative_tolerance_is_clamped_to_zero() {
        let got = Settings {
            roll_tolerance: -0.5,
            ..Settings::default()
        }
        .sanitised();

        assert_eq!(got.roll_tolerance, 0.0);
    }

    #[test]
    fn a_tolerance_above_one_is_clamped() {
        let got = Settings {
            roll_tolerance: 5.0,
            ..Settings::default()
        }
        .sanitised();

        assert_eq!(got.roll_tolerance, 1.0);
    }

    #[test]
    fn an_absurd_league_name_is_dropped() {
        let got = Settings {
            last_league: LastLeague::Shared("x".repeat(100)),
            ..Settings::default()
        }
        .sanitised();

        assert!(got.last_league.get(GameVersion::Poe1).is_empty());

        let per_game = Settings {
            last_league: LastLeague::PerGame(GamePair::new(
                "x".repeat(100),
                "Rise of the Abyssal".to_string(),
            )),
            ..Settings::default()
        }
        .sanitised();

        assert!(per_game.last_league.get(GameVersion::Poe1).is_empty());
        assert_eq!(
            per_game.last_league.get(GameVersion::Poe2),
            "Rise of the Abyssal"
        );
    }

    #[test]
    fn a_hand_edited_file_is_sanitised_on_load() {
        let dir = tempdir("sanitise");
        let store = ConfigStore::new(&dir);

        std::fs::write(store.path(), r#"{"panel_width":0,"roll_tolerance":-1}"#).unwrap();

        let got = store.load();

        assert!(got.panel_width >= 120.0);
        assert_eq!(got.roll_tolerance, 0.0);
    }

    #[test]
    fn saving_leaves_no_temporary_file_behind() {
        let dir = tempdir("no-temp");
        let store = ConfigStore::new(&dir);

        store.save(&Settings::default()).unwrap();

        assert!(!store.path().with_extension("json.tmp").exists());
        assert!(store.path().exists());
    }

    #[test]
    fn saving_twice_replaces_rather_than_appends() {
        let dir = tempdir("replace");
        let store = ConfigStore::new(&dir);

        store.save(&Settings::default()).unwrap();
        store
            .save(&Settings {
                last_league: LastLeague::Shared("Hardcore".into()),
                ..Settings::default()
            })
            .unwrap();

        assert_eq!(store.load().last_league.get(GameVersion::Poe1), "Hardcore");
    }

    #[test]
    fn saving_creates_the_directory_it_needs() {
        let dir = tempdir("nested").join("deeper").join("still");
        let store = ConfigStore::new(&dir);

        store.save(&Settings::default()).unwrap();

        assert!(store.path().exists());
    }

    #[test]
    fn an_unwritable_path_reports_the_path_it_tried() {
        let store = ConfigStore::new(Path::new("/proc/definitely/not/writable"));

        let err = store.save(&Settings::default()).unwrap_err();

        assert!(err.to_string().contains("/proc/definitely"));
    }

    #[test]
    fn a_settings_file_written_by_the_old_build_still_loads_its_single_league() {
        let got: Settings =
            serde_json::from_str(r#"{"last_league":"Rise of the Abyssal"}"#).unwrap();

        assert_eq!(
            got.last_league,
            LastLeague::Shared("Rise of the Abyssal".into())
        );
    }

    #[test]
    fn a_league_remembered_for_one_game_is_not_read_back_for_the_other() {
        let mut league = LastLeague::default();

        league.set(GameVersion::Poe1, "Mercenaries");
        league.set(GameVersion::Poe2, "Rise of the Abyssal");

        assert_eq!(league.get(GameVersion::Poe1), "Mercenaries");
        assert_eq!(league.get(GameVersion::Poe2), "Rise of the Abyssal");
    }

    #[test]
    fn upgrading_a_single_league_keeps_it_for_the_game_that_was_not_named() {
        let mut league = LastLeague::Shared("Standard".into());

        league.set(GameVersion::Poe2, "Rise of the Abyssal");

        assert_eq!(league.get(GameVersion::Poe1), "Standard");
        assert_eq!(league.get(GameVersion::Poe2), "Rise of the Abyssal");
    }

    #[test]
    fn a_per_game_league_survives_a_round_trip_through_the_file() {
        let store = ConfigStore::new(&tempdir("per-game"));

        let mut settings = Settings::default();
        settings.last_league.set(GameVersion::Poe1, "Mercenaries");
        settings
            .last_league
            .set(GameVersion::Poe2, "Rise of the Abyssal");

        store.save(&settings).unwrap();

        let got = store.load();

        assert_eq!(got.last_league.get(GameVersion::Poe1), "Mercenaries");
        assert_eq!(
            got.last_league.get(GameVersion::Poe2),
            "Rise of the Abyssal"
        );
    }

    #[test]
    fn a_league_pinned_by_hand_survives_a_round_trip_through_the_file() {
        let store = ConfigStore::new(&tempdir("pinned"));

        let mut settings = Settings::default();
        *settings.pinned_leagues.get_mut(GameVersion::Poe2) = true;

        store.save(&settings).unwrap();

        let got = store.load();

        assert!(*got.pinned_leagues.get(GameVersion::Poe2));
        assert!(!*got.pinned_leagues.get(GameVersion::Poe1));
    }

    #[test]
    fn a_file_written_before_pinning_existed_reads_as_nothing_pinned() {
        let got: Settings =
            serde_json::from_str(r#"{"last_league":"Rise of the Abyssal"}"#).unwrap();

        assert!(!*got.pinned_leagues.get(GameVersion::Poe1));
        assert!(!*got.pinned_leagues.get(GameVersion::Poe2));
    }

    #[test]
    fn the_defaults_are_sane_without_sanitising() {
        assert_eq!(Settings::default(), Settings::default().sanitised());
    }
}
