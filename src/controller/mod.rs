//! Business logic that needs the outside world.
//!
//! Pure logic belongs in `poe-trader-core`. A controller lives here only when
//! it orchestrates adapters.

pub mod datagen_controller;
pub mod overlay_controller;
pub mod price_check_controller;

pub use price_check_controller::{PriceCheckController, PriceCheckError, SearchResult};
