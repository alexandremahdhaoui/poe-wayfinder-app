pub mod search;
pub mod wiring;

use thiserror::Error;

pub use search::{urlencode, SearchOutcome};

#[derive(Debug, Error)]
pub enum OverlayLoopError {
    #[error("starting the search runtime")]
    Runtime {
        #[source]
        source: std::io::Error,
    },

    #[error("running the overlay window: {message}")]
    Window { message: String },
}

#[derive(Debug, Clone)]
pub struct OverlaySettings {
    pub window_title: String,
    pub pinned_title: bool,
    pub pinned_game: Option<poe_wayfinder_core::types::GameVersion>,
    pub league: String,
    pub league_from: poe_wayfinder_core::controller::league_list::LeagueFrom,
    pub known_leagues: poe_wayfinder_core::types::GamePair<Vec<String>>,
    pub league_choice: poe_wayfinder_core::types::GamePair<
        poe_wayfinder_core::controller::switching::LeagueChoice,
    >,
    pub session: String,
    pub site_url: String,
    pub data_dir: String,
    pub log_level: String,
    pub latency: u32,
    pub restore_clipboard: bool,
    pub gamepad_chord: String,
    pub data_origin: String,
    pub network: bool,
}

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::OverlayLoopDriver;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_says_what_failed() {
        let window = OverlayLoopError::Window {
            message: "no display".to_string(),
        };

        assert!(window.to_string().contains("overlay window"), "{window}");
        assert!(window.to_string().contains("no display"), "{window}");
    }
}
