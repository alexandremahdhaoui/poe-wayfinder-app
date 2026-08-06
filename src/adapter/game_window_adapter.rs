//! Finding and watching the game window.
//!
//! The overlay has to know where the game is, whether it is in front, and
//! where the cursor sits. All three come from the Windows API.
//!
//! # Why the title is config and not a constant
//!
//! The window title changes between game versions and between regional
//! clients. A hardcoded title is the single most likely reason a user reports
//! that the overlay never appears.

use thiserror::Error;

use crate::types::overlay::WindowRect;

/// Why a window operation failed.
#[derive(Debug, Error)]
pub enum WindowError {
    /// No window with that title exists.
    ///
    /// Usually means the game is not running. It is not fatal: the overlay
    /// waits and tries again, because starting the tool before the game is the
    /// normal order.
    #[error("no window titled {title:?} is open")]
    NotFound { title: String },

    #[error("reading the window rectangle for {title:?}")]
    Rect { title: String },

    #[error("sending input to the game window")]
    SendInput,
}

/// What the overlay needs to know about the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameWindow {
    pub rect: WindowRect,
    /// Whether the game is the window with focus.
    ///
    /// The overlay hides when it is not. Drawing over another application is
    /// the fastest way to make a tool feel broken.
    pub is_foreground: bool,
}

/// Looking at the game window.
///
/// Declared here because this module implements it. A test supplies one with
/// fixed answers, so the overlay controller is testable with no window.
pub trait GameWindowSource: Send + Sync {
    /// Find the game window.
    fn find(&self) -> Result<GameWindow, WindowError>;

    /// The cursor position, in physical screen pixels.
    fn cursor(&self) -> (i32, i32);

    /// The display scale factor where the game is.
    fn scale(&self) -> f32;
}

/// Whether the overlay should be drawn for this window.
///
/// Three conditions, all required. Any one failing means the overlay would be
/// drawn somewhere the user is not looking.
pub fn should_draw(window: &GameWindow) -> bool {
    window.is_foreground && window.rect.is_visible()
}

// ---------------------------------------------------------------------------
// The real implementation
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod win {
    use super::{GameWindow, GameWindowSource, WindowError};
    use crate::types::overlay::WindowRect;
    use poe_trader_core::controller::overlay::copy_key_sequence;

    use windows::core::HSTRING;
    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_C,
        VK_CONTROL,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetCursorPos, GetForegroundWindow, GetWindowRect,
    };

    /// The game window, found by title.
    pub struct GameWindowAdapter {
        title: String,
    }

    impl GameWindowAdapter {
        /// Watch for a window with this exact title.
        pub fn new(title: &str) -> Self {
            Self {
                title: title.to_string(),
            }
        }

        fn handle(&self) -> Result<HWND, WindowError> {
            let title = HSTRING::from(self.title.as_str());

            // SAFETY: FindWindowW takes two optional wide strings and returns
            // a handle or an error. Both arguments are valid for the call.
            let handle = unsafe { FindWindowW(None, &title) };

            match handle {
                Ok(h) if !h.is_invalid() => Ok(h),
                _ => Err(WindowError::NotFound {
                    title: self.title.clone(),
                }),
            }
        }
    }

    impl GameWindowSource for GameWindowAdapter {
        fn find(&self) -> Result<GameWindow, WindowError> {
            let handle = self.handle()?;

            let mut rect = RECT::default();

            // SAFETY: `rect` is a live, correctly sized RECT and `handle` came
            // from FindWindowW and was checked for validity.
            unsafe { GetWindowRect(handle, &mut rect) }.map_err(|_| WindowError::Rect {
                title: self.title.clone(),
            })?;

            // SAFETY: no arguments and no failure mode.
            let foreground = unsafe { GetForegroundWindow() };

            Ok(GameWindow {
                rect: WindowRect::new(
                    rect.left,
                    rect.top,
                    (rect.right - rect.left).max(0) as u32,
                    (rect.bottom - rect.top).max(0) as u32,
                ),
                is_foreground: foreground == handle,
            })
        }

        fn cursor(&self) -> (i32, i32) {
            let mut point = POINT::default();

            // SAFETY: `point` is a live, correctly sized POINT.
            if unsafe { GetCursorPos(&mut point) }.is_err() {
                // The cursor is always somewhere. A failure here means the
                // desktop is locked, and the origin is as good an answer as
                // any for a frame nobody can see.
                return (0, 0);
            }

            (point.x, point.y)
        }

        fn scale(&self) -> f32 {
            let Ok(handle) = self.handle() else {
                return 1.0;
            };

            // SAFETY: `handle` was checked for validity.
            let dpi = unsafe { GetDpiForWindow(handle) };

            if dpi == 0 {
                return 1.0;
            }

            // 96 is one hundred percent scaling on Windows.
            dpi as f32 / 96.0
        }
    }

    /// Sends Ctrl+C to whatever has focus.
    ///
    /// The game has no API. Sending the keystroke is the only way to make it
    /// write the item under the cursor to the clipboard.
    pub struct KeyboardCopyTrigger;

    impl KeyboardCopyTrigger {
        pub fn new() -> Self {
            Self
        }
    }

    impl Default for KeyboardCopyTrigger {
        fn default() -> Self {
            Self::new()
        }
    }

    impl crate::adapter::clipboard_adapter::CopyTrigger for KeyboardCopyTrigger {
        fn trigger_copy(&self) -> Result<(), crate::adapter::clipboard_adapter::ClipboardError> {
            // The sequence itself is decided in the domain crate and tested
            // there, because the order is the whole of the correctness and it
            // cannot be checked from here. This turns it into Windows events
            // and nothing else.
            let events: Vec<INPUT> = copy_key_sequence()
                .iter()
                .filter_map(|stroke| {
                    let key = match stroke.key.as_str() {
                        "Ctrl" => VK_CONTROL,
                        "C" => VK_C,
                        // A key this adapter cannot send is dropped rather
                        // than guessed. Sending the wrong scan code types a
                        // character into the game.
                        _ => return None,
                    };

                    Some(key_event(key, !stroke.down))
                })
                .collect();

            // SAFETY: `events` is a live, correctly sized array of INPUT and
            // the size argument matches INPUT exactly.
            let sent = unsafe { SendInput(&events, std::mem::size_of::<INPUT>() as i32) };

            if sent as usize != events.len() {
                return Err(crate::adapter::clipboard_adapter::ClipboardError::Open(
                    Box::new(WindowError::SendInput),
                ));
            }

            Ok(())
        }
    }

    /// Prove `SendInput` works, without touching anything the user owns.
    ///
    /// # Why this exists
    ///
    /// `SendInput` was the one Windows call in this build that no check
    /// reached. It fires only on a hotkey press and types into whatever has
    /// focus, so exercising it the ordinary way means pressing Ctrl+C on the
    /// user's desktop and overwriting their clipboard. That is their state.
    ///
    /// So this sends `VK_NONAME` instead. Windows documents it as reserved
    /// and it carries no character, no command and no binding. Every part of
    /// the call is the same as a real copy: the same struct, the same size
    /// argument, the same up and down pair, the same return check. Only the
    /// key differs, and that key does nothing anywhere.
    ///
    /// What it proves is the call itself. Whether Windows then delivers a
    /// Ctrl+C to the game is Windows' job, and the key order it would deliver
    /// is decided and tested in `poe_trader_core::controller::overlay`.
    ///
    /// Returns how many events Windows accepted. Two is success.
    pub fn self_test_send_input() -> u32 {
        use windows::Win32::UI::Input::KeyboardAndMouse::VK_NONAME;

        let events = [key_event(VK_NONAME, false), key_event(VK_NONAME, true)];

        // SAFETY: the same contract as trigger_copy. A live, correctly sized
        // array of INPUT and a size argument that matches INPUT exactly.
        unsafe { SendInput(&events, std::mem::size_of::<INPUT>() as i32) }
    }

    fn key_event(key: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    dwFlags: if up {
                        KEYEVENTF_KEYUP
                    } else {
                        Default::default()
                    },
                    ..Default::default()
                },
            },
        }
    }
}

#[cfg(windows)]
pub use win::{self_test_send_input, GameWindowAdapter, KeyboardCopyTrigger};

#[cfg(test)]
mod tests {
    use super::*;

    fn window(x: i32, y: i32, w: u32, h: u32, foreground: bool) -> GameWindow {
        GameWindow {
            rect: WindowRect::new(x, y, w, h),
            is_foreground: foreground,
        }
    }

    #[test]
    fn a_visible_foreground_window_is_drawn_over() {
        assert!(should_draw(&window(0, 0, 1920, 1080, true)));
    }

    #[test]
    fn a_background_window_is_not_drawn_over() {
        // Drawing over another application is the fastest way to make a tool
        // feel broken.
        assert!(!should_draw(&window(0, 0, 1920, 1080, false)));
    }

    #[test]
    fn a_minimised_window_is_not_drawn_over() {
        // Windows reports a zero size for one, and drawing into it wastes a
        // frame every tick.
        assert!(!should_draw(&window(0, 0, 0, 0, true)));
        assert!(!should_draw(&window(0, 0, 1920, 0, true)));
    }

    #[test]
    fn a_window_at_a_negative_origin_is_still_drawn_over() {
        // A second monitor to the left of the primary one has negative
        // coordinates. Treating that as invalid would break a dual monitor
        // setup, which is most of the audience.
        assert!(should_draw(&window(-1920, 0, 1920, 1080, true)));
    }

    #[test]
    fn a_missing_window_names_the_title_it_looked_for() {
        // The title is config precisely because it changes between versions,
        // so the message has to say which one was tried.
        let err = WindowError::NotFound {
            title: "Path of Exile 2".into(),
        };

        assert!(err.to_string().contains("Path of Exile 2"));
    }

    #[test]
    fn every_window_error_prints_a_distinct_message() {
        let messages: Vec<String> = [
            WindowError::NotFound { title: "a".into() },
            WindowError::Rect { title: "a".into() },
            WindowError::SendInput,
        ]
        .iter()
        .map(std::string::ToString::to_string)
        .collect();

        let mut seen = messages.clone();
        seen.sort();
        seen.dedup();

        assert_eq!(seen.len(), messages.len());
    }
}
