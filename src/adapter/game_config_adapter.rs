use std::path::{Path, PathBuf};

use poe_trader_core::controller::game_config::{show_mods_key, show_mods_key_was_read};
use poe_trader_core::types::GameVersion;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameConfigInfo {
    pub path: Option<PathBuf>,
    pub show_mods_key: String,
    pub read: bool,
}

impl GameConfigInfo {
    fn unknown() -> Self {
        Self {
            path: None,
            show_mods_key: poe_trader_core::controller::game_config::DEFAULT_SHOW_MODS_KEY
                .to_string(),
            read: false,
        }
    }
}

fn config_file_name(game: GameVersion) -> &'static str {
    match game {
        GameVersion::Poe1 => "production_Config.ini",
        GameVersion::Poe2 => "poe2_production_Config.ini",
    }
}

fn config_dir_name(game: GameVersion) -> &'static str {
    match game {
        GameVersion::Poe1 => "Path of Exile",
        GameVersion::Poe2 => "Path of Exile 2",
    }
}

pub fn candidate_paths(documents: &Path, game: GameVersion) -> Vec<PathBuf> {
    vec![documents
        .join("My Games")
        .join(config_dir_name(game))
        .join(config_file_name(game))]
}

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
        let got = read(&documents(), GameVersion::Poe2, |_| None);

        assert!(!got.read);
        assert_eq!(got.path, None);
        assert_eq!(got.show_mods_key, "Alt");
    }

    #[test]
    fn an_unreadable_setting_falls_back_and_says_so() {
        let got = read(&documents(), GameVersion::Poe2, |_| {
            Some("[ACTION_KEYS]\nshow_advanced_item_descriptions=0\n".to_string())
        });

        assert_eq!(got.show_mods_key, "Alt");
        assert!(!got.read, "an unbound setting was reported as read");
    }

    #[test]
    fn the_path_reported_is_the_one_that_was_read() {
        let seen = std::cell::RefCell::new(None);

        let got = read(&documents(), GameVersion::Poe2, |p| {
            *seen.borrow_mut() = Some(p.to_path_buf());

            Some(REAL.to_string())
        });

        assert_eq!(got.path, seen.into_inner());
    }

    #[test]
    fn a_rebound_key_is_reported_rather_than_the_default() {
        let got = read(&documents(), GameVersion::Poe2, |_| {
            Some("[ACTION_KEYS]\nshow_advanced_item_descriptions=67 2\n".to_string())
        });

        assert_eq!(got.show_mods_key, "Ctrl + C");
        assert!(got.read);
    }
}
