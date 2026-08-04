//! User settings that outlive one run.
//!
//! Config is what the user sets before starting. This is what the app itself
//! learns and has to remember: where they dragged the panel, which filters
//! they usually want on, which league they last played.
//!
//! # Why this is separate from config
//!
//! Config comes from flags and environment and a file the user owns. Writing
//! back to it would fight whoever wrote it. This file belongs to the app and
//! nobody else edits it.
//!
//! # Why a corrupt file is not fatal
//!
//! It holds preferences. Losing them is annoying. Refusing to start because a
//! preference file was truncated by a power cut is worse.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Why settings could not be saved.
///
/// There is no load error. A load that fails returns the defaults, because a
/// preference file is never worth refusing to start over.
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

/// What the app remembers between runs.
///
/// Every field has a default, so an older file missing a newer field loads
/// cleanly rather than being rejected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Where the user dragged the panel, in logical pixels from the anchor.
    pub panel_offset_x: f32,
    pub panel_offset_y: f32,
    pub panel_width: f32,
    pub panel_height: f32,
    /// The league last searched.
    ///
    /// Remembered so a user who plays one league does not set it every run.
    pub last_league: String,
    /// Include offline sellers.
    pub include_offline: bool,
    /// How much room to leave around a roll.
    pub roll_tolerance: f64,
    /// Filter on item level.
    pub filter_item_level: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            panel_offset_x: 16.0,
            panel_offset_y: 16.0,
            panel_width: 400.0,
            panel_height: 600.0,
            last_league: String::new(),
            include_offline: false,
            roll_tolerance: 0.1,
            filter_item_level: false,
        }
    }
}

impl Settings {
    /// Clamp anything a hand edit could have made nonsensical.
    ///
    /// The file is plain text and a user will edit it. A panel of zero size
    /// fails to create a surface on Windows, and a negative tolerance inverts
    /// every filter.
    pub fn sanitised(mut self) -> Self {
        self.panel_width = self.panel_width.clamp(120.0, 4000.0);
        self.panel_height = self.panel_height.clamp(80.0, 4000.0);
        self.panel_offset_x = self.panel_offset_x.clamp(-4000.0, 4000.0);
        self.panel_offset_y = self.panel_offset_y.clamp(-4000.0, 4000.0);
        self.roll_tolerance = self.roll_tolerance.clamp(0.0, 1.0);

        // A league name from a hand edit or a hostile whisper can be anything.
        if self.last_league.len() > 40 {
            self.last_league = String::new();
        }

        self
    }
}

/// Reads and writes the settings file.
#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    /// Store settings in this directory.
    pub fn new(dir: &Path) -> Self {
        Self {
            path: dir.join("settings.json"),
        }
    }

    /// The file it reads and writes.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load the settings.
    ///
    /// Returns the defaults for a missing file, an unreadable one or a corrupt
    /// one. Losing preferences is annoying. Refusing to start because a
    /// preference file was truncated by a power cut is worse.
    pub fn load(&self) -> Settings {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return Settings::default();
        };

        serde_json::from_str::<Settings>(&text)
            .unwrap_or_default()
            .sanitised()
    }

    /// Save the settings.
    ///
    /// Written to a temporary file and renamed, so a crash mid write leaves
    /// the old file intact rather than a half one. A rename is atomic on both
    /// Windows and Linux.
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

    fn tempdir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("poe-trader-store-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        dir
    }

    #[test]
    fn a_missing_file_loads_the_defaults() {
        // Starting for the first time is the normal case.
        let store = ConfigStore::new(Path::new("/nonexistent/poe-trader"));

        assert_eq!(store.load(), Settings::default());
    }

    #[test]
    fn settings_survive_a_round_trip() {
        let store = ConfigStore::new(&tempdir("round-trip"));

        let settings = Settings {
            panel_offset_x: 100.0,
            last_league: "Standard".into(),
            include_offline: true,
            ..Settings::default()
        };

        store.save(&settings).unwrap();

        assert_eq!(store.load(), settings);
    }

    #[test]
    fn a_corrupt_file_loads_the_defaults() {
        // Refusing to start because a preference file was truncated by a power
        // cut is worse than losing the preferences.
        let dir = tempdir("corrupt");
        let store = ConfigStore::new(&dir);

        std::fs::write(store.path(), "{not json").unwrap();

        assert_eq!(store.load(), Settings::default());
    }

    #[test]
    fn an_older_file_missing_a_newer_field_still_loads() {
        // Every field has a default precisely so this works.
        let dir = tempdir("partial");
        let store = ConfigStore::new(&dir);

        std::fs::write(store.path(), r#"{"last_league":"Hardcore"}"#).unwrap();

        let got = store.load();

        assert_eq!(got.last_league, "Hardcore");
        assert_eq!(got.panel_width, Settings::default().panel_width);
    }

    #[test]
    fn a_zero_sized_panel_is_clamped() {
        // A zero sized surface fails to create on Windows.
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
        // It would invert every filter, asking for rolls worse than the item.
        let got = Settings {
            roll_tolerance: -0.5,
            ..Settings::default()
        }
        .sanitised();

        assert_eq!(got.roll_tolerance, 0.0);
    }

    #[test]
    fn a_tolerance_above_one_is_clamped() {
        // Above one the floor goes negative and the filter stops narrowing.
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
            last_league: "x".repeat(100),
            ..Settings::default()
        }
        .sanitised();

        assert!(got.last_league.is_empty());
    }

    #[test]
    fn a_hand_edited_file_is_sanitised_on_load() {
        // The file is plain text and a user will edit it.
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
                last_league: "Hardcore".into(),
                ..Settings::default()
            })
            .unwrap();

        assert_eq!(store.load().last_league, "Hardcore");
    }

    #[test]
    fn saving_creates_the_directory_it_needs() {
        // The config directory may not exist on a first run.
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
    fn the_defaults_are_sane_without_sanitising() {
        // A default that needed clamping would be a bug that only showed up
        // on a first run.
        assert_eq!(Settings::default(), Settings::default().sanitised());
    }
}
