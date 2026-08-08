use crate::adapter::game_log_adapter::{league_from_whisper, LogEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Config,
    Log,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Session {
    league: Option<(String, Source)>,
    character: Option<String>,
    character_level: Option<u32>,
    area: Option<String>,
}

impl Session {
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

    pub fn league(&self) -> Option<&str> {
        self.league.as_ref().map(|(name, _)| name.as_str())
    }

    pub fn league_source(&self) -> Option<Source> {
        self.league.as_ref().map(|(_, source)| *source)
    }

    pub fn character(&self) -> Option<&str> {
        self.character.as_deref()
    }

    pub fn character_level(&self) -> Option<u32> {
        self.character_level
    }

    pub fn area(&self) -> Option<&str> {
        self.area.as_deref()
    }

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
        let mut s = Session::from_config("");

        assert!(s.apply(&trade_whisper("Hardcore Ruthless")));
        assert_eq!(s.league(), Some("Hardcore Ruthless"));
        assert_eq!(s.league_source(), Some(Source::Log));
    }

    #[test]
    fn a_configured_league_is_never_overwritten_by_the_log() {
        let mut s = Session::from_config("Standard");

        assert!(!s.apply(&trade_whisper("Hardcore")));
        assert_eq!(s.league(), Some("Standard"));
    }

    #[test]
    fn a_learned_league_can_be_replaced_by_a_later_one() {
        let mut s = Session::from_config("");
        s.apply(&trade_whisper("Standard"));

        assert!(s.apply(&trade_whisper("Hardcore")));
        assert_eq!(s.league(), Some("Hardcore"));
    }

    #[test]
    fn the_same_league_twice_is_not_a_change() {
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
