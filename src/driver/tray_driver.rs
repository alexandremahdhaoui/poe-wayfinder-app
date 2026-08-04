//! The system tray menu.
//!
//! The overlay has no window a user can click to reach settings, because a
//! window like that would sit over the game. The tray is where the app lives
//! when it is not showing a price.
//!
//! # Why the menu is a pure model
//!
//! What the menu offers depends on state: it cannot say "search again" with no
//! previous search, and it should say whether the game was found. That logic
//! is testable and the drawing is not, so they are separated the same way the
//! overlay is.

/// What the user picked from the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// Run the last search again.
    Research,
    /// Open the last search on the trade site.
    OpenInBrowser,
    /// Stop reacting to the hotkey without quitting.
    TogglePaused,
    /// Rebuild the game data.
    RebuildData,
    /// Quit.
    Quit,
}

/// One row in the tray menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub label: String,
    /// None for a row that only reports state.
    pub action: Option<TrayAction>,
    /// Shown but not clickable.
    pub enabled: bool,
}

impl MenuItem {
    fn action(label: &str, action: TrayAction, enabled: bool) -> Self {
        Self {
            label: label.to_string(),
            action: Some(action),
            enabled,
        }
    }

    fn status(label: String) -> Self {
        Self {
            label,
            action: None,
            enabled: false,
        }
    }
}

/// What the tray needs to know to build its menu.
#[derive(Debug, Clone, PartialEq)]
pub struct TrayState {
    /// The game window was found.
    pub game_found: bool,
    /// The hotkey is being ignored.
    pub paused: bool,
    /// A search has run this session.
    pub has_search: bool,
    /// The league being searched.
    pub league: Option<String>,
    /// How many stats are loaded.
    pub stat_count: usize,
}

/// Build the menu for a state.
///
/// The status rows come first, because the question a user opens the tray to
/// answer is usually "why is nothing happening".
pub fn menu(state: &TrayState) -> Vec<MenuItem> {
    let mut out = Vec::new();

    // The most common reason nothing happens is that the game is not running
    // or the window title is wrong. Saying so here saves a support round trip.
    out.push(MenuItem::status(if state.game_found {
        "Game found".to_string()
    } else {
        "Game not found".to_string()
    }));

    out.push(MenuItem::status(match &state.league {
        Some(league) => format!("League: {league}"),
        None => "League: not set".to_string(),
    }));

    if state.stat_count == 0 {
        // Nothing will ever match without data, and the failure would
        // otherwise show up as every modifier being unknown.
        out.push(MenuItem::status("No game data loaded".to_string()));
    }

    out.push(MenuItem::action(
        if state.paused { "Resume" } else { "Pause" },
        TrayAction::TogglePaused,
        true,
    ));

    // Both need a previous search to act on.
    out.push(MenuItem::action(
        "Search again",
        TrayAction::Research,
        state.has_search,
    ));

    out.push(MenuItem::action(
        "Open in browser",
        TrayAction::OpenInBrowser,
        state.has_search,
    ));

    out.push(MenuItem::action(
        "Rebuild game data",
        TrayAction::RebuildData,
        true,
    ));

    out.push(MenuItem::action("Quit", TrayAction::Quit, true));

    out
}

/// Whether the hotkey should be acted on.
pub fn accepts_hotkey(state: &TrayState) -> bool {
    !state.paused && state.game_found && state.stat_count > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> TrayState {
        TrayState {
            game_found: true,
            paused: false,
            has_search: false,
            league: Some("Standard".into()),
            stat_count: 3787,
        }
    }

    fn labels(state: &TrayState) -> Vec<String> {
        menu(state).into_iter().map(|i| i.label).collect()
    }

    fn item(state: &TrayState, action: TrayAction) -> MenuItem {
        menu(state)
            .into_iter()
            .find(|i| i.action == Some(action))
            .expect("action is in the menu")
    }

    #[test]
    fn the_menu_says_whether_the_game_was_found() {
        // The most common reason nothing happens is that the game is not
        // running or the window title is wrong.
        assert!(labels(&ready()).contains(&"Game found".to_string()));

        let missing = TrayState {
            game_found: false,
            ..ready()
        };

        assert!(labels(&missing).contains(&"Game not found".to_string()));
    }

    #[test]
    fn the_menu_says_which_league_it_will_search() {
        // A wrong league returns nothing rather than an error, so seeing it is
        // worth a row.
        assert!(labels(&ready()).contains(&"League: Standard".to_string()));

        let unset = TrayState {
            league: None,
            ..ready()
        };

        assert!(labels(&unset).contains(&"League: not set".to_string()));
    }

    #[test]
    fn missing_data_gets_its_own_row() {
        // Nothing will ever match without it, and the failure would otherwise
        // show up as every modifier being unknown.
        let empty = TrayState {
            stat_count: 0,
            ..ready()
        };

        assert!(labels(&empty).contains(&"No game data loaded".to_string()));
    }

    #[test]
    fn loaded_data_gets_no_row() {
        assert!(!labels(&ready()).contains(&"No game data loaded".to_string()));
    }

    #[test]
    fn the_status_rows_come_first() {
        // The question a user opens the tray to answer is usually why nothing
        // is happening.
        let rows = menu(&ready());

        assert!(rows[0].action.is_none());
        assert!(rows[1].action.is_none());
    }

    #[test]
    fn a_status_row_cannot_be_clicked() {
        for row in menu(&ready()) {
            if row.action.is_none() {
                assert!(!row.enabled, "{}", row.label);
            }
        }
    }

    #[test]
    fn pause_reads_as_resume_once_paused() {
        assert_eq!(item(&ready(), TrayAction::TogglePaused).label, "Pause");

        let paused = TrayState {
            paused: true,
            ..ready()
        };

        assert_eq!(item(&paused, TrayAction::TogglePaused).label, "Resume");
    }

    #[test]
    fn the_search_actions_need_a_previous_search() {
        // Offering them with nothing to act on is a click that does nothing.
        let fresh = ready();

        assert!(!item(&fresh, TrayAction::Research).enabled);
        assert!(!item(&fresh, TrayAction::OpenInBrowser).enabled);

        let after = TrayState {
            has_search: true,
            ..ready()
        };

        assert!(item(&after, TrayAction::Research).enabled);
        assert!(item(&after, TrayAction::OpenInBrowser).enabled);
    }

    #[test]
    fn quit_and_rebuild_are_always_available() {
        // A user must always be able to leave, whatever state the app is in.
        for state in [
            ready(),
            TrayState {
                game_found: false,
                stat_count: 0,
                paused: true,
                ..ready()
            },
        ] {
            assert!(item(&state, TrayAction::Quit).enabled);
            assert!(item(&state, TrayAction::RebuildData).enabled);
        }
    }

    #[test]
    fn every_action_appears_exactly_once() {
        // A duplicate row would fire twice or confuse which one was clicked.
        let actions: Vec<TrayAction> = menu(&ready())
            .into_iter()
            .filter_map(|i| i.action)
            .collect();

        let mut seen = actions.clone();
        seen.sort_by_key(|a| format!("{a:?}"));
        seen.dedup();

        assert_eq!(seen.len(), actions.len());
    }

    #[test]
    fn a_ready_app_accepts_the_hotkey() {
        assert!(accepts_hotkey(&ready()));
    }

    #[test]
    fn a_paused_app_ignores_the_hotkey() {
        assert!(!accepts_hotkey(&TrayState {
            paused: true,
            ..ready()
        }));
    }

    #[test]
    fn the_hotkey_is_ignored_when_the_game_is_gone() {
        // Copying from whatever has focus would take text from another
        // application and try to price it.
        assert!(!accepts_hotkey(&TrayState {
            game_found: false,
            ..ready()
        }));
    }

    #[test]
    fn the_hotkey_is_ignored_with_no_data() {
        // Every modifier would be unknown and the price would be meaningless.
        assert!(!accepts_hotkey(&TrayState {
            stat_count: 0,
            ..ready()
        }));
    }
}
