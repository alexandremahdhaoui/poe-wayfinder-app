//! Plain data used across this crate's layers.

pub mod hotkey;
pub mod overlay;

pub use hotkey::{Hotkey, HotkeyError, Key, Modifier};
pub use overlay::{Anchor, OverlayGeometry, WindowRect};
