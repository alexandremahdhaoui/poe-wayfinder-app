//! What the app knows about the current play session.
//!
//! Which league, which character, which area. All three come from the game's
//! own log, so the user never has to type them.
//!
//! # Why this matters more than it looks
//!
//! The trade site keeps a separate index per league. Searching the wrong one
//! returns nothing rather than an error, which reads as "this item is
//! worthless" when it means "you searched the wrong index". That is the worst
//! kind of failure and it is entirely avoidable, because the game says which
//! league it is in every trade whisper.

use crate::adapter::game_log_adapter::{league_from_whisper, LogEvent};

/// Where a piece of session state came from.
///
/// Configuration wins over anything learned, because a user who set a value
/// meant it. A log line only fills in what was left blank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The user set it.
    Config,
    /// Learned from the game log.
    Log,
}

/// The current session.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Session {
    league: Option<(String, Source)>,
    character: Option<String>,
    character_level: Option<u32>,
    area: Option<String>,
}

impl Session {
    /// A session seeded from config.
    ///
    /// A blank league is treated as unset rather than as a league called "",
    /// which would search an index that does not exist.
    pub fn from_config(league: &str) -> Self {
        let league = if league.trim().is_empty() {
            None
        } else {
            Some((league.trim().to_string(), Source::Config))
        };

        Self {
            league,
            ..Self::default()
        }
    }

    /// The league to search.
    pub fn league(&self) -> Option<&str> {
        self.league.as_ref().map(|(name, _)| name.as_str())
    }

    /// Where the league came from.
    pub fn league_source(&self) -> Option<Source> {
        self.league.as_ref().map(|(_, source)| *source)
    }

    /// The character being played.
    pub fn character(&self) -> Option<&str> {
        self.character.as_deref()
    }

    /// The character's level.
    pub fn character_level(&self) -> Option<u32> {
        self.character_level
    }

    /// The area the character is in.
    pub fn area(&self) -> Option<&str> {
        self.area.as_deref()
    }

    /// Take in one log event.
    ///
    /// Returns whether anything changed, so a caller can log only the
    /// transitions rather than every poll.
    pub fn apply(&mut self, event: &LogEvent) -> bool {
        match event {
            LogEvent::EnteredArea { name } => {
                if self.area.as_deref() == Some(name.as_str()) {
                    return false;
                }

                self.area = Some(name.clone());

                true
            }

            LogEvent::LevelUp {
                character, level, ..
            } => {
                let changed = self.character.as_deref() != Some(character.as_str())
                    || self.character_level != Some(*level);

                self.character = Some(character.clone());
                self.character_level = Some(*level);

                changed
            }

            LogEvent::Whisper { body, .. } => {
                let Some(league) = league_from_whisper(body) else {
                    return false;
                };

                self.learn_league(&league)
            }
        }
    }

    /// Record a league learned from the log.
    ///
    /// A configured league is never overwritten. The user set it and meant it,
    /// and a stray whisper from a friend in another league must not silently
    /// redirect every search.
    fn learn_league(&mut self, league: &str) -> bool {
        if self.league_source() == Some(Source::Config) {
            return false;
        }

        if self.league() == Some(league) {
            return false;
        }

        self.league = Some((league.to_string(), Source::Log));

        true
    }

    /// Take in a batch of events.
    ///
    /// Returns whether anything changed.
    pub fn apply_all(&mut self, events: &[LogEvent]) -> bool {
        let mut changed = false;

        for event in events {
            changed |= self.apply(event);
        }

        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn whisper(body: &str) -> LogEvent {
        LogEvent::Whisper {
            from: "Kaom".into(),
            body: body.into(),
        }
    }

    fn trade_whisper(league: &str) -> LogEvent {
        whisper(&format!("Hi, I would like to buy your item listed for 5 divine in {league}"))
    }

    #[test]
    fn a_configured_league_is_used() {
        let s = Session::from_config("Standard");

        assert_eq!(s.league(), Some("Standard"));
        assert_eq!(s.league_source(), Some(Source::Config));
    }

    #[test]
    fn a_blank_configured_league_is_treated_as_unset() {
        // A league called "" searches an index that does not exist and returns
        // nothing, which reads as the item being worthless.
        for text in ["", "   "] {
            assert_eq!(Session::from_config(text).league(), None, "{text:?}");
        }
    }

    #[test]
    fn a_configured_league_is_trimmed() {
        assert_eq!(Session::from_config("  Standard  ").league(), Some("Standard"));
    }

    #[test]
    fn the_league_is_learned_from_a_trade_whisper() {
        // The game says which league it is in every trade whisper, so the user
        // never has to type it.
        let mut s = Session::from_config("");

        assert!(s.apply(&trade_whisper("Hardcore Ruthless")));
        assert_eq!(s.league(), Some("Hardcore Ruthless"));
        assert_eq!(s.league_source(), Some(Source::Log));
    }

    #[test]
    fn a_configured_league_is_never_overwritten_by_the_log() {
        // The user set it and meant it. A stray whisper from a friend in
        // another league must not silently redirect every search.
        let mut s = Session::from_config("Standard");

        assert!(!s.apply(&trade_whisper("Hardcore")));
        assert_eq!(s.league(), Some("Standard"));
    }

    #[test]
    fn a_learned_league_can_be_replaced_by_a_later_one() {
        // The user changed league mid session. The most recent whisper is the
        // better guess.
        let mut s = Session::from_config("");
        s.apply(&trade_whisper("Standard"));

        assert!(s.apply(&trade_whisper("Hardcore")));
        assert_eq!(s.league(), Some("Hardcore"));
    }

    #[test]
    fn the_same_league_twice_is_not_a_change() {
        // A caller logs only transitions, so repeating one would spam the log
        // on every whisper.
        let mut s = Session::from_config("");
        s.apply(&trade_whisper("Standard"));

        assert!(!s.apply(&trade_whisper("Standard")));
    }

    #[test]
    fn a_whisper_that_names_no_league_changes_nothing() {
        let mut s = Session::from_config("");

        assert!(!s.apply(&whisper("hello there")));
        assert_eq!(s.league(), None);
    }

    #[test]
    fn the_character_is_learned_from_a_level_up() {
        let mut s = Session::default();

        assert!(s.apply(&LogEvent::LevelUp {
            character: "Kaom".into(),
            class: "Marauder".into(),
            level: 42
        }));

        assert_eq!(s.character(), Some("Kaom"));
        assert_eq!(s.character_level(), Some(42));
    }

    #[test]
    fn a_further_level_up_updates_the_level() {
        let mut s = Session::default();
        s.apply(&LogEvent::LevelUp {
            character: "Kaom".into(),
            class: "Marauder".into(),
            level: 42,
        });

        assert!(s.apply(&LogEvent::LevelUp {
            character: "Kaom".into(),
            class: "Marauder".into(),
            level: 43
        }));

        assert_eq!(s.character_level(), Some(43));
    }

    #[test]
    fn switching_character_is_a_change() {
        let mut s = Session::default();
        s.apply(&LogEvent::LevelUp {
            character: "Kaom".into(),
            class: "Marauder".into(),
            level: 42,
        });

        assert!(s.apply(&LogEvent::LevelUp {
            character: "Izaro".into(),
            class: "Templar".into(),
            level: 42
        }));

        assert_eq!(s.character(), Some("Izaro"));
    }

    #[test]
    fn the_same_level_up_twice_is_not_a_change() {
        let mut s = Session::default();
        let event = LogEvent::LevelUp {
            character: "Kaom".into(),
            class: "Marauder".into(),
            level: 42,
        };

        s.apply(&event);

        assert!(!s.apply(&event));
    }

    #[test]
    fn the_area_is_learned() {
        let mut s = Session::default();

        assert!(s.apply(&LogEvent::EnteredArea {
            name: "Sarn Encampment".into()
        }));

        assert_eq!(s.area(), Some("Sarn Encampment"));
    }

    #[test]
    fn re_entering_the_same_area_is_not_a_change() {
        let mut s = Session::default();
        let event = LogEvent::EnteredArea {
            name: "Sarn Encampment".into(),
        };

        s.apply(&event);

        assert!(!s.apply(&event));
    }

    #[test]
    fn a_batch_reports_whether_anything_changed() {
        let mut s = Session::from_config("");

        let changed = s.apply_all(&[
            LogEvent::EnteredArea {
                name: "Sarn Encampment".into(),
            },
            trade_whisper("Standard"),
        ]);

        assert!(changed);
        assert_eq!(s.area(), Some("Sarn Encampment"));
        assert_eq!(s.league(), Some("Standard"));
    }

    #[test]
    fn a_batch_of_repeats_reports_no_change() {
        let mut s = Session::from_config("");
        let events = [trade_whisper("Standard")];

        s.apply_all(&events);

        assert!(!s.apply_all(&events));
    }

    #[test]
    fn an_empty_batch_reports_no_change() {
        let mut s = Session::default();

        assert!(!s.apply_all(&[]));
    }
}
