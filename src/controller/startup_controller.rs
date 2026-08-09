use thiserror::Error;

use crate::adapter::http_adapter::PolicyError;
use crate::types::Hotkey;

use poe_trader_core::types::GameVersion;

#[derive(Debug, Error)]
pub enum StartupError {
    #[error("reading the price check hotkey {hotkey:?}")]
    Hotkey {
        hotkey: String,
        #[source]
        source: crate::types::hotkey::HotkeyError,
    },

    #[error("unknown game {game:?}")]
    Game { game: String },

    #[error("trade_base_url {url:?} is refused by the network policy")]
    Url {
        url: String,
        #[source]
        source: PolicyError,
    },
}

pub const AUTO_GAME: &str = "auto";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validated {
    pub game: Option<GameVersion>,
    pub hotkey: Hotkey,
    pub network_disabled: bool,
}

impl Validated {
    pub fn starting_game(&self) -> GameVersion {
        self.game.unwrap_or(GameVersion::Poe2)
    }
}

pub fn validate(
    game: &str,
    hotkey: &str,
    url_check: Result<(), PolicyError>,
) -> Result<Validated, StartupError> {
    let parsed_hotkey = Hotkey::parse(hotkey).map_err(|source| StartupError::Hotkey {
        hotkey: hotkey.to_string(),
        source,
    })?;

    let parsed_game = match game.trim() {
        AUTO_GAME | "" => None,
        named => Some(GameVersion::parse(named).ok_or_else(|| StartupError::Game {
            game: game.to_string(),
        })?),
    };

    let network_disabled = match url_check {
        Ok(()) => false,
        Err(PolicyError::NetworkDisabled) => true,
        Err(source) => {
            return Err(StartupError::Url {
                url: String::new(),
                source,
            })
        }
    };

    Ok(Validated {
        game: parsed_game,
        hotkey: parsed_hotkey,
        network_disabled,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_good_config_validates() {
        let got = validate("poe2", "Ctrl+D", Ok(())).expect("valid");

        assert_eq!(got.game, Some(GameVersion::Poe2));
        assert_eq!(got.hotkey.to_string(), "Ctrl+D");
        assert!(!got.network_disabled);
    }

    #[test]
    fn a_bad_hotkey_is_reported_before_anything_starts() {
        let err = validate("poe2", "NotAKey+++", Ok(())).expect_err("a failure");

        assert!(err.to_string().contains("NotAKey"), "{err}");
    }

    #[test]
    fn auto_means_nothing_is_pinned_and_poe2_is_where_it_starts() {
        for spelling in ["auto", " auto ", ""] {
            let got = validate(spelling, "Ctrl+D", Ok(())).expect("valid");

            assert_eq!(got.game, None, "{spelling:?}");
            assert_eq!(got.starting_game(), GameVersion::Poe2, "{spelling:?}");
        }
    }

    #[test]
    fn a_named_game_stays_pinned_to_itself() {
        let got = validate("poe1", "Ctrl+D", Ok(())).expect("valid");

        assert_eq!(got.game, Some(GameVersion::Poe1));
        assert_eq!(got.starting_game(), GameVersion::Poe1);
    }

    #[test]
    fn an_unknown_game_is_reported() {
        let err = validate("poe3", "Ctrl+D", Ok(())).expect_err("a failure");

        assert!(err.to_string().contains("poe3"), "{err}");
    }

    #[test]
    fn a_disabled_network_is_allowed_and_flagged() {
        let got = validate("poe1", "Ctrl+D", Err(PolicyError::NetworkDisabled)).expect("valid");

        assert!(got.network_disabled);
    }

    #[test]
    fn a_refused_url_stops_startup() {
        let refused = Err(PolicyError::HostNotAllowed {
            host: "evil.example".to_string(),
        });

        assert!(validate("poe1", "Ctrl+D", refused).is_err());
    }

    #[test]
    fn the_hotkey_is_checked_before_the_game() {
        let err = validate("poe3", "NotAKey+++", Ok(())).expect_err("a failure");

        assert!(
            err.to_string().contains("NotAKey"),
            "the first thing a user typed wrong should be the first thing reported: {err}"
        );
    }
}
