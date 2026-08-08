pub mod search;
pub mod wiring;

use thiserror::Error;

pub use search::{urlencode, SearchOutcome};

#[derive(Debug, Error)]
pub enum OverlayLoopError {
    #[error("opening the clipboard")]
    Clipboard {
        #[source]
        source: crate::adapter::clipboard_adapter::ClipboardError,
    },

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
    pub league: String,
    pub session: String,
    pub site_url: String,
    pub data_dir: String,
    pub log_level: String,
    pub latency: u32,
    pub restore_clipboard: bool,
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
