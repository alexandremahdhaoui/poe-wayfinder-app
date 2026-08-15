use std::time::{Duration, SystemTime};

use poe_wayfinder_core::controller::league_list::LeagueFrom;
use poe_wayfinder_core::controller::switching::{GameOption, LeagueMenu};
use poe_wayfinder_core::types::GameVersion;

use crate::adapter::rate_limit_adapter::LimiterLine;
use crate::controller::data_refresh_controller::MAX_AGE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Ready,
    Waiting,
    Paused,
    Degraded,
}

impl Health {
    pub fn as_str(self) -> &'static str {
        match self {
            Health::Ready => "ready",
            Health::Waiting => "waiting",
            Health::Paused => "paused",
            Health::Degraded => "degraded",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Status {
    pub game: Option<GameVersion>,
    pub pinned: bool,
    pub window_title: String,
    pub hotkey: String,
    pub league: String,
    pub league_source: LeagueFrom,
    pub league_menu: LeagueMenu,
    pub game_menu: Vec<GameOption>,
    pub origin: String,
    pub stats: usize,
    pub items: usize,
    pub augments: usize,
    pub last_refresh: Option<SystemTime>,
    pub paused: bool,
    pub network: bool,
    pub limits: Vec<LimiterLine>,
    pub note: Option<String>,
}

pub fn health(status: &Status) -> Health {
    if status.paused {
        return Health::Paused;
    }

    if status.stats == 0 {
        return Health::Degraded;
    }

    if !status.network {
        return Health::Degraded;
    }

    match status.game {
        Some(_) => Health::Ready,
        None => Health::Waiting,
    }
}

pub fn headline(status: &Status) -> String {
    match health(status) {
        Health::Paused => "Paused. The hotkey is ignored.".to_string(),
        Health::Degraded if status.stats == 0 => "No game data loaded.".to_string(),
        Health::Degraded => "Network is off. Pricing will not work.".to_string(),
        Health::Waiting => "Waiting for Path of Exile to start.".to_string(),
        Health::Ready => format!(
            "Watching {}. Press {} over an item.",
            status.window_title, status.hotkey
        ),
    }
}

pub fn game_caption(status: &Status) -> String {
    let name = match status.game {
        Some(GameVersion::Poe1) => "Path of Exile",
        Some(GameVersion::Poe2) => "Path of Exile 2",
        None => return "no game running".to_string(),
    };

    match status.pinned {
        true => format!("{name}, pinned by hand"),
        false => format!("{name}, detected"),
    }
}

pub fn data_caption(status: &Status) -> String {
    let where_from = match status.origin.as_str() {
        "embedded" => "built into this app",
        "cache" => "refreshed from the trade site",
        "directory" => "from the folder you named",
        other => other,
    };

    format!(
        "{} stats, {} items, {} runes, {where_from}",
        status.stats, status.items, status.augments
    )
}

pub fn league_caption(status: &Status) -> String {
    let league = match status.league.trim().is_empty() {
        true => "not set",
        false => status.league.as_str(),
    };

    format!("{league}, {}", source_caption(status.league_source))
}

fn source_caption(from: LeagueFrom) -> &'static str {
    match from {
        LeagueFrom::Configured => "as configured",
        LeagueFrom::Chosen => "chosen by hand",
        LeagueFrom::TradeApi => "the trade site's current league",
        LeagueFrom::LastRun => "from the last run",
        LeagueFrom::GameLog => "read from the game",
        LeagueFrom::Fallback => "a fallback, nothing named one",
    }
}

pub fn refresh_caption(last: Option<SystemTime>, now: SystemTime) -> String {
    let Some(last) = last else {
        return "never, it runs on the next start".to_string();
    };

    let Ok(age) = now.duration_since(last) else {
        return "just now".to_string();
    };

    let due = MAX_AGE.saturating_sub(age);

    format!("{}, next in {}", ago(age), remaining(due))
}

fn ago(age: Duration) -> String {
    let days = age.as_secs() / 86_400;
    let hours = age.as_secs() / 3_600;

    match (days, hours) {
        (0, 0) => "less than an hour ago".to_string(),
        (0, 1) => "an hour ago".to_string(),
        (0, h) => format!("{h} hours ago"),
        (1, _) => "a day ago".to_string(),
        (d, _) => format!("{d} days ago"),
    }
}

fn remaining(due: Duration) -> String {
    let days = due.as_secs() / 86_400;

    match days {
        0 if due.is_zero() => "any moment".to_string(),
        0 => "under a day".to_string(),
        1 => "a day".to_string(),
        d => format!("{d} days"),
    }
}

pub fn rows(status: &Status, now: SystemTime) -> Vec<(&'static str, String)> {
    vec![
        ("Game", game_caption(status)),
        ("Hotkey", status.hotkey.clone()),
        ("League", league_caption(status)),
        ("Data", data_caption(status)),
        ("Refreshed", refresh_caption(status.last_refresh, now)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> Status {
        Status {
            game: Some(GameVersion::Poe2),
            window_title: "Path of Exile 2".to_string(),
            hotkey: "Ctrl+D".to_string(),
            league: "Standard".to_string(),
            origin: "embedded".to_string(),
            stats: 3787,
            items: 3573,
            augments: 253,
            network: true,
            ..Status::default()
        }
    }

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
    }

    #[test]
    fn a_running_game_and_loaded_data_is_ready() {
        assert_eq!(health(&ready()), Health::Ready);
    }

    #[test]
    fn no_game_yet_is_waiting_rather_than_broken() {
        let status = Status {
            game: None,
            ..ready()
        };

        assert_eq!(health(&status), Health::Waiting);
        assert!(
            headline(&status).contains("Waiting"),
            "{}",
            headline(&status)
        );
    }

    #[test]
    fn no_data_is_degraded_even_with_a_game_running() {
        let status = Status {
            stats: 0,
            ..ready()
        };

        assert_eq!(health(&status), Health::Degraded);
        assert!(headline(&status).contains("No game data"));
    }

    #[test]
    fn a_disabled_network_is_degraded_and_says_so() {
        let status = Status {
            network: false,
            ..ready()
        };

        assert_eq!(health(&status), Health::Degraded);
        assert!(headline(&status).contains("Network"));
    }

    #[test]
    fn paused_beats_every_other_state_because_the_user_asked_for_it() {
        let status = Status {
            paused: true,
            stats: 0,
            game: None,
            ..ready()
        };

        assert_eq!(health(&status), Health::Paused);
    }

    #[test]
    fn the_headline_names_the_window_and_the_key_to_press() {
        let text = headline(&ready());

        assert!(text.contains("Path of Exile 2"), "{text}");
        assert!(text.contains("Ctrl+D"), "{text}");
    }

    #[test]
    fn a_detected_game_is_told_apart_from_one_pinned_by_hand() {
        assert!(game_caption(&ready()).contains("detected"));

        let pinned = Status {
            pinned: true,
            ..ready()
        };

        assert!(game_caption(&pinned).contains("pinned"));
    }

    #[test]
    fn every_data_origin_reads_as_plain_english() {
        for (origin, want) in [
            ("embedded", "built into this app"),
            ("cache", "refreshed"),
            ("directory", "folder you named"),
        ] {
            let status = Status {
                origin: origin.to_string(),
                ..ready()
            };

            assert!(
                data_caption(&status).contains(want),
                "{origin}: {}",
                data_caption(&status)
            );
        }
    }

    #[test]
    fn the_data_caption_counts_everything_that_was_loaded() {
        let text = data_caption(&ready());

        assert!(text.contains("3787"), "{text}");
        assert!(text.contains("3573"), "{text}");
        assert!(text.contains("253"), "{text}");
    }

    #[test]
    fn the_league_caption_says_where_the_league_came_from() {
        for source in [
            LeagueFrom::Configured,
            LeagueFrom::Chosen,
            LeagueFrom::TradeApi,
            LeagueFrom::LastRun,
            LeagueFrom::GameLog,
            LeagueFrom::Fallback,
        ] {
            let status = Status {
                league_source: source,
                ..ready()
            };

            assert!(league_caption(&status).contains(source_caption(source)));
        }
    }

    #[test]
    fn a_league_the_user_chose_by_hand_does_not_read_as_one_the_app_worked_out() {
        let chosen = Status {
            league_source: LeagueFrom::Chosen,
            ..ready()
        };

        let automatic = Status {
            league_source: LeagueFrom::TradeApi,
            ..ready()
        };

        assert_ne!(league_caption(&chosen), league_caption(&automatic));
    }

    #[test]
    fn an_empty_league_says_not_set_rather_than_nothing() {
        let status = Status {
            league: String::new(),
            ..ready()
        };

        assert!(league_caption(&status).contains("not set"));
    }

    #[test]
    fn a_cache_that_never_refreshed_says_when_it_will() {
        let text = refresh_caption(None, at(0));

        assert!(text.contains("never"), "{text}");
        assert!(text.contains("next start"), "{text}");
    }

    #[test]
    fn a_fresh_refresh_reads_in_hours_and_an_old_one_in_days() {
        let now = at(MAX_AGE.as_secs() * 2);

        assert!(refresh_caption(Some(now - Duration::from_secs(60)), now).contains("less than"));
        assert!(refresh_caption(Some(now - Duration::from_secs(3_600)), now).contains("an hour"));
        assert!(refresh_caption(Some(now - Duration::from_secs(7_200)), now).contains("2 hours"));
        assert!(
            refresh_caption(Some(now - Duration::from_secs(86_400)), now).contains("a day ago")
        );
        assert!(
            refresh_caption(Some(now - Duration::from_secs(3 * 86_400)), now)
                .contains("3 days ago")
        );
    }

    #[test]
    fn a_refresh_that_is_overdue_says_it_happens_any_moment() {
        let now = at(MAX_AGE.as_secs() * 2);

        assert!(refresh_caption(Some(now - MAX_AGE), now).contains("any moment"));
    }

    #[test]
    fn a_stamp_from_the_future_does_not_panic_on_the_subtraction() {
        let now = at(100);

        assert_eq!(
            refresh_caption(Some(now + Duration::from_secs(60)), now),
            "just now"
        );
    }

    #[test]
    fn every_row_carries_a_label_and_something_to_show() {
        for (label, value) in rows(&ready(), at(0)) {
            assert!(!label.is_empty());
            assert!(!value.trim().is_empty(), "{label} has nothing to show");
        }
    }

    #[test]
    fn the_rows_cover_everything_a_user_would_check_first() {
        let labels: Vec<&str> = rows(&ready(), at(0)).into_iter().map(|(l, _)| l).collect();

        for wanted in ["Game", "Hotkey", "League", "Data", "Refreshed"] {
            assert!(labels.contains(&wanted), "{wanted} is missing");
        }
    }

    #[test]
    fn every_health_state_names_itself_for_the_log() {
        for state in [
            Health::Ready,
            Health::Waiting,
            Health::Paused,
            Health::Degraded,
        ] {
            assert!(!state.as_str().is_empty());
        }
    }
}
