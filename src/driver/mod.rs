//! Inputs. Each driver validates its input and calls a controller.
//!
//! A driver never touches an adapter.

pub mod hook_driver;
pub mod hotkey_driver;
pub mod overlay_placement;
pub mod overlay_ui_driver;
pub mod tray_driver;
