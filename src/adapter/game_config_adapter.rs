//! Finding and reading the game's own configuration file.
//!
//! The parsing lives in `poe_trader_core::controller::game_config`. This finds
//! the file and hands its text over, which is the only part that touches a
//! disk.
//!
//! # Why the overlay reads it at all
//!
//! To say, at startup, whether it can see the user's game installation. A user
//! whose overlay cannot find the game config has a setup problem, and finding
//! that out from a startup line beats finding it out from prices that are
//! quietly wrong.

use std::path::{Path, PathBuf};

use poe_trader_core::controller::game_config::{show_mods_key, show_mods_key_was_read};
use poe_trader_core::types::GameVersion;

/// What reading the game config produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameConfigInfo {
    /// Where it was found. None when no candidate path existed.
    pub path: Option<PathBuf>,
    /// The key that shows the detailed item text.
    pub show_mods_key: String,
    /// Whether that key was read rather than assumed.
    pub read: bool,
}

impl GameConfigInfo {
    /// What to report when nothing could be found.
    ///
    /// Not an error. The overlay works without it, because a copy produces the
    /// detailed format on its own since 3.29. Saying so is the point.
    fn unknown() -> Self {
        Self {
            path: None,
            show_mods_key: poe_trader_core::controller::game_config::DEFAULT_SHOW_MODS_KEY
                .to_string(),
            read: false,
        }
    }
}

/// The file name each game writes.
///
/// PoE2 is not `production_Config.ini`. The reference looks for that name and
/// finds nothing on a PoE2 install, which is the kind of thing that only shows
/// up against a real installation.
fn config_file_name(game: GameVersion) -> &'static str {
    match game {
        GameVersion::Poe1 => "production_Config.ini",
        GameVersion::Poe2 => "poe2_production_Config.ini",
    }
}

/// The directory each game keeps its config in.
fn config_dir_name(game: GameVersion) -> &'static str {
    match game {
        GameVersion::Poe1 => "Path of Exile",
        GameVersion::Poe2 => "Path of Exile 2",
    }
}

/// Where to look, in order.
///
/// The documents folder is where the game writes on Windows. The list is
/// ordered because a user can have both games installed and the first hit for
/// the game being priced is the right one.
pub fn candidate_paths(documents: &Path, game: GameVersion) -> Vec<PathBuf> {
    vec![documents
        .join("My Games")
        .join(config_dir_name(game))
        .join(config_file_name(game))]
}

/// Read the game config, however far it gets.
///
/// Never fails. A missing or unreadable config is reported as not read rather
/// than as an error, because the overlay works without it and refusing to
/// start over a file it only wants for a diagnostic would be worse than the
/// diagnostic is worth.
pub fn read(
    documents: &Path,
    game: GameVersion,
    load: impl Fn(&Path) -> Option<String>,
) -> GameConfigInfo {
    for path in candidate_paths(documents, game) {
        let Some(text) = load(&path) else {
            continue;
        };

        let parsed = poe_trader_core::controller::game_config::parse_ini(&text);

        return GameConfigInfo {
            path: Some(path),
            show_mods_key: show_mods_key(&parsed),
            read: show_mods_key_was_read(&parsed),
        };
    }

    GameConfigInfo::unknown()
}

/// Read a file from disk, or nothing.
///
/// The default loader. Separate so tests drive `read` without a filesystem.
pub fn load_from_disk(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = "[ACTION_KEYS]\nshow_advanced_item_descriptions=18\n";

    fn documents() -> PathBuf {
        PathBuf::from("/docs")
    }

    #[test]
    fn poe2_looks_for_its_own_file_name() {
        // The reference looks for production_Config.ini and finds nothing on a
        // PoE2 install.
        let got = candidate_paths(&documents(), GameVersion::Poe2);

        assert!(got[0].ends_with("poe2_production_Config.ini"), "{got:?}");
    }

    #[test]
    fn poe1_looks_for_the_original_name() {
        let got = candidate_paths(&documents(), GameVersion::Poe1);

        assert!(got[0].ends_with("production_Config.ini"), "{got:?}");
        assert!(!got[0].to_string_lossy().contains("poe2"));
    }

    #[test]
    fn each_game_looks_in_its_own_directory() {
        // Both games write under My Games and a shared directory would read
        // the wrong game's bindings.
        let one = candidate_paths(&documents(), GameVersion::Poe1);
        let two = candidate_paths(&documents(), GameVersion::Poe2);

        assert_ne!(one, two);
    }

    #[test]
    fn a_config_that_is_there_is_read() {
        let got = read(&documents(), GameVersion::Poe2, |_| Some(REAL.to_string()));

        assert!(got.read);
        assert_eq!(got.show_mods_key, "Alt");
        assert!(got.path.is_some());
    }

    #[test]
    fn a_missing_config_is_not_an_error() {
        // The overlay works without it. Refusing to start over a file it wants
        // for a diagnostic is worse than the diagnostic is worth.
        let got = read(&documents(), GameVersion::Poe2, |_| None);

        assert!(!got.read);
        assert_eq!(got.path, None);
        assert_eq!(got.show_mods_key, "Alt");
    }

    #[test]
    fn an_unreadable_setting_falls_back_and_says_so() {
        // The fallback and a real Alt are the same key and very different
        // confidence.
        let got = read(&documents(), GameVersion::Poe2, |_| {
            Some("[ACTION_KEYS]\nshow_advanced_item_descriptions=0\n".to_string())
        });

        assert_eq!(got.show_mods_key, "Alt");
        assert!(!got.read, "an unbound setting was reported as read");
    }

    #[test]
    fn the_path_reported_is_the_one_that_was_read() {
        // A reported path that was not the one opened sends a user to fix the
        // wrong file.
        let seen = std::cell::RefCell::new(None);

        let got = read(&documents(), GameVersion::Poe2, |p| {
            *seen.borrow_mut() = Some(p.to_path_buf());

            Some(REAL.to_string())
        });

        assert_eq!(got.path, seen.into_inner());
    }

    #[test]
    fn a_rebound_key_is_reported_rather_than_the_default() {
        // The whole reason to read the file.
        let got = read(&documents(), GameVersion::Poe2, |_| {
            Some("[ACTION_KEYS]\nshow_advanced_item_descriptions=67 2\n".to_string())
        });

        assert_eq!(got.show_mods_key, "Ctrl + C");
        assert!(got.read);
    }
}
