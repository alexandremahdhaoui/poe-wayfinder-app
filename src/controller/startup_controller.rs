use thiserror::Error;

use crate::adapter::http_adapter::PolicyError;
use crate::types::Hotkey;

use poe_wayfinder_core::controller::commands::{self, Command};
use poe_wayfinder_core::controller::item_links::Site;
use poe_wayfinder_core::types::GameVersion;

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
    pub locked: Vec<Hotkey>,
    pub overlay: Option<Hotkey>,
    pub commands: Vec<(Hotkey, Command)>,
    pub searches: Vec<(Hotkey, Command)>,
    pub links: Vec<(Hotkey, Site)>,
    pub network_disabled: bool,
}

impl Validated {
    pub fn starting_game(&self) -> GameVersion {
        self.game.unwrap_or(GameVersion::Poe2)
    }

    pub fn every_hotkey(&self) -> Vec<Hotkey> {
        let mut out = vec![self.hotkey.clone()];

        out.extend(self.locked.iter().cloned());
        out.extend(self.overlay.iter().cloned());
        out.extend(self.commands.iter().map(|(key, _)| key.clone()));
        out.extend(self.searches.iter().map(|(key, _)| key.clone()));
        out.extend(self.links.iter().map(|(key, _)| key.clone()));
        out
    }

    pub fn role_of(&self, binding: usize) -> Press {
        if binding == 0 {
            return Press::Check;
        }

        if binding <= self.locked.len() {
            return Press::Locked;
        }

        let after_locked = binding - self.locked.len() - 1;

        if self.overlay.is_some() && after_locked == 0 {
            return Press::ToggleOverlay;
        }

        let command = match self.overlay.is_some() {
            true => after_locked - 1,
            false => after_locked,
        };

        if command < self.commands.len() {
            return Press::Command { index: command };
        }

        let search = command - self.commands.len();

        if search < self.searches.len() {
            return Press::StashSearch { index: search };
        }

        match self.links.get(search - self.searches.len()) {
            Some((_, site)) => Press::OpenLink { site: *site },
            None => Press::Check,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    Check,
    Locked,
    ToggleOverlay,
    Command { index: usize },
    StashSearch { index: usize },
    OpenLink { site: Site },
}

pub fn site_named(name: &str) -> Option<Site> {
    match name.trim().to_ascii_lowercase().as_str() {
        "wiki" => Some(Site::Wiki),
        "poedb" | "poe2db" => Some(Site::Poedb),
        "craft of exile" | "craftofexile" | "coe" => Some(Site::CraftOfExile),
        _ => None,
    }
}

pub fn parse_links(declared: &str) -> Vec<(Hotkey, Site)> {
    commands::parse(declared)
        .into_iter()
        .filter_map(|entry| {
            let key = Hotkey::parse(&entry.hotkey).ok()?;
            let site = site_named(&entry.text)?;

            Some((key, site))
        })
        .collect()
}

pub fn parse_commands(declared: &str) -> Vec<(Hotkey, Command)> {
    commands::parse(declared)
        .into_iter()
        .filter_map(|command| {
            Hotkey::parse(&command.hotkey)
                .ok()
                .map(|key| (key, command))
        })
        .collect()
}

pub fn optional_hotkeys(spellings: &[&str]) -> Vec<Hotkey> {
    spellings
        .iter()
        .filter(|s| !s.trim().is_empty())
        .filter_map(|s| Hotkey::parse(s).ok())
        .collect()
}

#[allow(clippy::too_many_arguments)]
#[derive(Debug, Default, Clone)]
pub struct Declared<'a> {
    pub game: &'a str,
    pub hotkey: &'a str,
    pub locked: &'a [&'a str],
    pub overlay: &'a str,
    pub commands: &'a str,
    pub searches: &'a str,
    pub links: &'a str,
}

pub fn validate(
    declared: Declared<'_>,
    url_check: Result<(), PolicyError>,
) -> Result<Validated, StartupError> {
    let parsed_hotkey = Hotkey::parse(declared.hotkey).map_err(|source| StartupError::Hotkey {
        hotkey: declared.hotkey.to_string(),
        source,
    })?;

    let parsed_game = match declared.game.trim() {
        AUTO_GAME | "" => None,
        named => Some(GameVersion::parse(named).ok_or_else(|| StartupError::Game {
            game: declared.game.to_string(),
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
        locked: optional_hotkeys(declared.locked),
        overlay: optional_hotkeys(&[declared.overlay]).into_iter().next(),
        commands: parse_commands(declared.commands),
        searches: parse_commands(declared.searches),
        links: parse_links(declared.links),
        network_disabled,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(
        game: &str,
        hotkey: &str,
        url_check: Result<(), PolicyError>,
    ) -> Result<Validated, StartupError> {
        super::validate(
            Declared {
                game,
                hotkey,
                ..Declared::default()
            },
            url_check,
        )
    }

    fn validate_with_locked(
        game: &str,
        hotkey: &str,
        locked: &[&str],
        url_check: Result<(), PolicyError>,
    ) -> Result<Validated, StartupError> {
        super::validate(
            Declared {
                game,
                hotkey,
                locked,
                ..Declared::default()
            },
            url_check,
        )
    }

    fn validate_every(
        game: &str,
        hotkey: &str,
        locked: &[&str],
        overlay: &str,
        url_check: Result<(), PolicyError>,
    ) -> Result<Validated, StartupError> {
        super::validate(
            Declared {
                game,
                hotkey,
                locked,
                overlay,
                ..Declared::default()
            },
            url_check,
        )
    }

    fn validate_all(
        game: &str,
        hotkey: &str,
        locked: &[&str],
        overlay: &str,
        commands: &str,
        url_check: Result<(), PolicyError>,
    ) -> Result<Validated, StartupError> {
        super::validate(
            Declared {
                game,
                hotkey,
                locked,
                overlay,
                commands,
                ..Declared::default()
            },
            url_check,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_everything(
        game: &str,
        hotkey: &str,
        locked: &[&str],
        overlay: &str,
        commands: &str,
        searches: &str,
        url_check: Result<(), PolicyError>,
    ) -> Result<Validated, StartupError> {
        super::validate(
            Declared {
                game,
                hotkey,
                locked,
                overlay,
                commands,
                searches,
                ..Declared::default()
            },
            url_check,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_with_links(
        game: &str,
        hotkey: &str,
        locked: &[&str],
        overlay: &str,
        commands: &str,
        searches: &str,
        links: &str,
        url_check: Result<(), PolicyError>,
    ) -> Result<Validated, StartupError> {
        super::validate(
            Declared {
                game,
                hotkey,
                locked,
                overlay,
                commands,
                searches,
                links,
            },
            url_check,
        )
    }

    #[test]
    fn each_reference_site_gets_its_own_binding_and_role() {
        let got = validate_with_links(
            "poe2",
            "Ctrl+D",
            &[],
            "",
            "",
            "",
            "Alt+W=wiki;Alt+B=poedb;Alt+C=craft of exile",
            Ok(()),
        )
        .expect("valid");

        assert_eq!(got.links.len(), 3);
        assert_eq!(got.role_of(1), Press::OpenLink { site: Site::Wiki });
        assert_eq!(got.role_of(2), Press::OpenLink { site: Site::Poedb });
        assert_eq!(
            got.role_of(3),
            Press::OpenLink {
                site: Site::CraftOfExile
            }
        );
    }

    #[test]
    fn a_site_nobody_recognises_is_dropped() {
        let got = validate_with_links("poe2", "Ctrl+D", &[], "", "", "", "Alt+W=nowhere", Ok(()))
            .unwrap();

        assert!(got.links.is_empty());
    }

    #[test]
    fn the_site_name_is_read_however_it_is_spelled() {
        assert_eq!(site_named("Wiki"), Some(Site::Wiki));
        assert_eq!(site_named("poe2db"), Some(Site::Poedb));
        assert_eq!(site_named("CoE"), Some(Site::CraftOfExile));
        assert_eq!(site_named("craftofexile"), Some(Site::CraftOfExile));
    }

    #[test]
    fn a_stash_search_gets_its_own_binding_and_role() {
        let got = validate_everything(
            "poe2",
            "Ctrl+D",
            &[],
            "",
            "F5=/hideout",
            "Ctrl+F1=chaos;Ctrl+F2=exalted",
            Ok(()),
        )
        .expect("valid");

        assert_eq!(got.searches.len(), 2);
        assert_eq!(got.role_of(1), Press::Command { index: 0 });
        assert_eq!(got.role_of(2), Press::StashSearch { index: 0 });
        assert_eq!(got.role_of(3), Press::StashSearch { index: 1 });
        assert_eq!(got.searches[1].1.text, "exalted");
    }

    #[test]
    fn a_chat_command_gets_its_own_binding_and_role() {
        let got = validate_all(
            "poe2",
            "Ctrl+D",
            &["Ctrl+Alt+D"],
            "Shift+Space",
            "F5=/hideout;F9=/exit",
            Ok(()),
        )
        .expect("valid");

        assert_eq!(got.commands.len(), 2);
        assert_eq!(got.every_hotkey().len(), 5);
        assert_eq!(got.role_of(0), Press::Check);
        assert_eq!(got.role_of(1), Press::Locked);
        assert_eq!(got.role_of(2), Press::ToggleOverlay);
        assert_eq!(got.role_of(3), Press::Command { index: 0 });
        assert_eq!(got.role_of(4), Press::Command { index: 1 });
    }

    #[test]
    fn commands_still_line_up_when_there_is_no_overlay_key() {
        let got = validate_all("poe2", "Ctrl+D", &[], "", "F5=/hideout", Ok(())).expect("valid");

        assert_eq!(got.role_of(1), Press::Command { index: 0 });
    }

    #[test]
    fn a_command_whose_key_does_not_parse_is_dropped_rather_than_shifting_the_rest() {
        let got = validate_all("poe2", "Ctrl+D", &[], "", "NotAKey+++=/one;F9=/two", Ok(()))
            .expect("valid");

        assert_eq!(got.commands.len(), 1);
        assert_eq!(got.commands[0].1.text, "/two");
    }

    #[test]
    fn every_binding_knows_which_job_it_does() {
        let got = validate_every(
            "poe2",
            "Ctrl+D",
            &["Ctrl+Alt+D", "Ctrl+Shift+D"],
            "Shift+Space",
            Ok(()),
        )
        .expect("valid");

        assert_eq!(got.every_hotkey().len(), 4);
        assert_eq!(got.role_of(0), Press::Check);
        assert_eq!(got.role_of(1), Press::Locked);
        assert_eq!(got.role_of(2), Press::Locked);
        assert_eq!(got.role_of(3), Press::ToggleOverlay);
    }

    #[test]
    fn with_no_locked_bindings_the_overlay_key_is_still_found() {
        let got = validate_every("poe2", "Ctrl+D", &[], "Shift+Space", Ok(())).expect("valid");

        assert_eq!(got.every_hotkey().len(), 2);
        assert_eq!(got.role_of(1), Press::ToggleOverlay);
    }

    #[test]
    fn an_empty_overlay_key_is_simply_off() {
        let got = validate_every("poe2", "Ctrl+D", &[], "", Ok(())).expect("valid");

        assert!(got.overlay.is_none());
        assert_eq!(got.every_hotkey().len(), 1);
    }

    #[test]
    fn the_locked_bindings_come_after_the_plain_one() {
        let got = validate_with_locked("poe2", "Ctrl+D", &["Ctrl+Alt+D", "Ctrl+Shift+D"], Ok(()))
            .expect("valid");

        let every = got.every_hotkey();

        assert_eq!(every.len(), 3);
        assert_eq!(every[0].to_string(), "Ctrl+D");
        assert!(every[1].to_string().contains("Alt"));
        assert!(every[2].to_string().contains("Shift"));
    }

    #[test]
    fn an_empty_locked_binding_is_simply_off() {
        let got = validate_with_locked("poe2", "Ctrl+D", &["", "  "], Ok(())).expect("valid");

        assert!(got.locked.is_empty());
        assert_eq!(got.every_hotkey().len(), 1);
    }

    #[test]
    fn a_locked_binding_that_does_not_parse_is_skipped_rather_than_fatal() {
        let got = validate_with_locked("poe2", "Ctrl+D", &["NotAKey+++", "Ctrl+Alt+D"], Ok(()))
            .expect("the plain hotkey still works");

        assert_eq!(got.locked.len(), 1, "the good one survives");
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
