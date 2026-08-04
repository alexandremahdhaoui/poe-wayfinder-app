//! The global hotkey.
//!
//! Registers the price check hotkey with Windows and reports when it fires.
//!
//! # Why RegisterHotKey and not a keyboard hook
//!
//! A low level keyboard hook sees every keystroke in the system, including
//! passwords typed into other applications. That is a lot of trust to ask for
//! when all the tool needs is one key combination.
//!
//! `RegisterHotKey` asks Windows for exactly one combination and sees nothing
//! else. It also cannot be blamed for input lag, which a slow hook can.
//!
//! The cost is that a combination already claimed by another application
//! cannot be registered. That is reported at startup rather than silently
//! never firing.

use thiserror::Error;

use crate::types::hotkey::{Hotkey, Key, Modifier};

/// Why a hotkey could not be registered.
#[derive(Debug, Error)]
pub enum HotkeyDriverError {
    /// Windows refused the registration.
    ///
    /// Almost always means another application already owns the combination.
    #[error("registering {hotkey}. Another application may already use it.")]
    AlreadyTaken { hotkey: String },

    /// The key has no Windows virtual key code.
    #[error("{key} cannot be registered as a global hotkey")]
    Unsupported { key: String },
}

/// The Windows virtual key code for a key.
///
/// Returns None for anything Windows has no code for, so the caller reports it
/// at startup rather than registering something that can never fire.
pub fn virtual_key_code(key: &Key) -> Option<u16> {
    let code = match key {
        // Letters and digits use their ASCII value as their virtual key code.
        // This is a documented quirk of the Windows API and not a coincidence.
        Key::Char(c) if c.is_ascii_uppercase() || c.is_ascii_digit() => *c as u16,
        Key::Char(_) => return None,

        // VK_F1 is 0x70 and they run consecutively to F24.
        Key::Function(n) if (1..=24).contains(n) => 0x6F + u16::from(*n),
        Key::Function(_) => return None,

        Key::Escape => 0x1B,
        Key::Space => 0x20,
        Key::Tab => 0x09,
        Key::Enter => 0x0D,
        Key::Backspace => 0x08,
        Key::Delete => 0x2E,
        Key::Insert => 0x2D,
        Key::Home => 0x24,
        Key::End => 0x23,
        Key::PageUp => 0x21,
        Key::PageDown => 0x22,
        Key::Left => 0x25,
        Key::Up => 0x26,
        Key::Right => 0x27,
        Key::Down => 0x28,
    };

    Some(code)
}

/// The Windows modifier bitmask for a hotkey.
///
/// `MOD_NOREPEAT` is always set. Without it, holding the key fires a price
/// check every few milliseconds, which would hit the rate limiter instantly
/// and could get the account banned.
pub fn modifier_mask(modifiers: &[Modifier]) -> u32 {
    const MOD_ALT: u32 = 0x0001;
    const MOD_CONTROL: u32 = 0x0002;
    const MOD_SHIFT: u32 = 0x0004;
    const MOD_WIN: u32 = 0x0008;
    const MOD_NOREPEAT: u32 = 0x4000;

    let mut mask = MOD_NOREPEAT;

    for modifier in modifiers {
        mask |= match modifier {
            Modifier::Alt => MOD_ALT,
            Modifier::Ctrl => MOD_CONTROL,
            Modifier::Shift => MOD_SHIFT,
            Modifier::Meta => MOD_WIN,
        };
    }

    mask
}

/// Check a hotkey can be registered before trying.
///
/// Called at startup so an impossible hotkey is reported with a message rather
/// than silently never firing.
pub fn check_registrable(hotkey: &Hotkey) -> Result<(), HotkeyDriverError> {
    if virtual_key_code(hotkey.key()).is_none() {
        return Err(HotkeyDriverError::Unsupported {
            key: hotkey.key().as_string(),
        });
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// The real registration
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod win {
    use super::{check_registrable, modifier_mask, virtual_key_code, HotkeyDriverError};
    use crate::types::hotkey::Hotkey;

    use std::sync::mpsc::{self, Receiver};

    use windows::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};

    /// Our id for the price check hotkey.
    const PRICE_CHECK_ID: i32 = 1;

    /// Listens for the price check hotkey.
    ///
    /// Registration and the message pump both happen on a dedicated thread,
    /// because Windows delivers `WM_HOTKEY` to the thread that registered it
    /// and the UI thread is busy drawing.
    pub struct HotkeyDriver {
        presses: Receiver<()>,
    }

    impl HotkeyDriver {
        /// Register the hotkey and start listening.
        pub fn start(hotkey: &Hotkey) -> Result<Self, HotkeyDriverError> {
            check_registrable(hotkey)?;

            let mask = modifier_mask(hotkey.modifiers());
            let code = virtual_key_code(hotkey.key()).expect("checked above");
            let label = hotkey.to_string();

            let (ready_tx, ready_rx) = mpsc::channel();
            let (press_tx, press_rx) = mpsc::channel();

            std::thread::spawn(move || {
                // SAFETY: a null window handle registers the hotkey against
                // this thread, which is exactly what the message loop below
                // reads from.
                let registered = unsafe {
                    RegisterHotKey(
                        None,
                        PRICE_CHECK_ID,
                        HOT_KEY_MODIFIERS(mask),
                        u32::from(code),
                    )
                };

                if registered.is_err() {
                    let _ = ready_tx.send(false);

                    return;
                }

                let _ = ready_tx.send(true);

                let mut message = MSG::default();

                // SAFETY: `message` is a live, correctly sized MSG. A null
                // window handle reads messages for the whole thread, which is
                // where WM_HOTKEY lands.
                while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
                    if message.message == WM_HOTKEY && message.wParam.0 as i32 == PRICE_CHECK_ID {
                        // A closed channel means the app is shutting down.
                        if press_tx.send(()).is_err() {
                            break;
                        }
                    }
                }

                // SAFETY: unregistering the id this thread registered.
                let _ = unsafe { UnregisterHotKey(None, PRICE_CHECK_ID) };
            });

            match ready_rx.recv() {
                Ok(true) => Ok(Self { presses: press_rx }),
                _ => Err(HotkeyDriverError::AlreadyTaken { hotkey: label }),
            }
        }

        /// Whether the hotkey fired since the last check.
        ///
        /// Drains every pending press and reports whether there was at least
        /// one. Queuing them would run a price check per press after a
        /// stutter, which is exactly what the rate limiter exists to prevent.
        pub fn fired(&self) -> bool {
            let mut any = false;

            while self.presses.try_recv().is_ok() {
                any = true;
            }

            any
        }
    }
}

#[cfg(windows)]
pub use win::HotkeyDriver;

#[cfg(test)]
mod tests {
    use super::*;

    const MOD_ALT: u32 = 0x0001;
    const MOD_CONTROL: u32 = 0x0002;
    const MOD_SHIFT: u32 = 0x0004;
    const MOD_WIN: u32 = 0x0008;
    const MOD_NOREPEAT: u32 = 0x4000;

    #[test]
    fn a_letter_uses_its_ascii_value() {
        // A documented quirk of the Windows API and not a coincidence.
        assert_eq!(virtual_key_code(&Key::Char('D')), Some(0x44));
        assert_eq!(virtual_key_code(&Key::Char('A')), Some(0x41));
        assert_eq!(virtual_key_code(&Key::Char('Z')), Some(0x5A));
    }

    #[test]
    fn a_digit_uses_its_ascii_value() {
        assert_eq!(virtual_key_code(&Key::Char('0')), Some(0x30));
        assert_eq!(virtual_key_code(&Key::Char('9')), Some(0x39));
    }

    #[test]
    fn a_lower_case_letter_has_no_code() {
        // The parser upper cases every letter, so a lower case one here means
        // something built a Key by hand and got it wrong.
        assert_eq!(virtual_key_code(&Key::Char('d')), None);
    }

    #[test]
    fn the_function_keys_run_consecutively_from_f1() {
        assert_eq!(virtual_key_code(&Key::Function(1)), Some(0x70));
        assert_eq!(virtual_key_code(&Key::Function(12)), Some(0x7B));
        assert_eq!(virtual_key_code(&Key::Function(24)), Some(0x87));
    }

    #[test]
    fn a_function_key_outside_the_real_range_has_no_code() {
        assert_eq!(virtual_key_code(&Key::Function(0)), None);
        assert_eq!(virtual_key_code(&Key::Function(25)), None);
    }

    #[test]
    fn every_named_key_has_a_code() {
        for key in [
            Key::Escape,
            Key::Space,
            Key::Tab,
            Key::Enter,
            Key::Backspace,
            Key::Delete,
            Key::Insert,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
            Key::Left,
            Key::Up,
            Key::Right,
            Key::Down,
        ] {
            assert!(virtual_key_code(&key).is_some(), "{key:?}");
        }
    }

    #[test]
    fn every_named_key_has_a_distinct_code() {
        // Two keys sharing a code would register the wrong one silently.
        let keys = [
            Key::Escape,
            Key::Space,
            Key::Tab,
            Key::Enter,
            Key::Backspace,
            Key::Delete,
            Key::Insert,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
            Key::Left,
            Key::Up,
            Key::Right,
            Key::Down,
        ];

        let mut codes: Vec<u16> = keys.iter().filter_map(virtual_key_code).collect();
        let before = codes.len();
        codes.sort_unstable();
        codes.dedup();

        assert_eq!(codes.len(), before);
    }

    #[test]
    fn the_arrow_keys_are_in_the_windows_order() {
        // Left, up, right, down and not the reading order. Getting this wrong
        // registers a different arrow than the user asked for.
        assert_eq!(virtual_key_code(&Key::Left), Some(0x25));
        assert_eq!(virtual_key_code(&Key::Up), Some(0x26));
        assert_eq!(virtual_key_code(&Key::Right), Some(0x27));
        assert_eq!(virtual_key_code(&Key::Down), Some(0x28));
    }

    #[test]
    fn no_repeat_is_always_set() {
        // Without it, holding the key fires a price check every few
        // milliseconds, which hits the rate limiter instantly and could get
        // the account banned.
        assert_eq!(modifier_mask(&[]) & MOD_NOREPEAT, MOD_NOREPEAT);
        assert_eq!(
            modifier_mask(&[Modifier::Ctrl]) & MOD_NOREPEAT,
            MOD_NOREPEAT
        );
    }

    #[test]
    fn each_modifier_sets_its_own_bit() {
        for (modifier, bit) in [
            (Modifier::Alt, MOD_ALT),
            (Modifier::Ctrl, MOD_CONTROL),
            (Modifier::Shift, MOD_SHIFT),
            (Modifier::Meta, MOD_WIN),
        ] {
            assert_eq!(modifier_mask(&[modifier]) & bit, bit, "{modifier:?}");
        }
    }

    #[test]
    fn several_modifiers_combine() {
        let mask = modifier_mask(&[Modifier::Ctrl, Modifier::Alt, Modifier::Shift]);

        assert_eq!(mask & MOD_CONTROL, MOD_CONTROL);
        assert_eq!(mask & MOD_ALT, MOD_ALT);
        assert_eq!(mask & MOD_SHIFT, MOD_SHIFT);
        assert_eq!(mask & MOD_WIN, 0);
    }

    #[test]
    fn a_bare_hotkey_sets_only_the_no_repeat_bit() {
        assert_eq!(modifier_mask(&[]), MOD_NOREPEAT);
    }

    #[test]
    fn a_registrable_hotkey_passes_the_startup_check() {
        assert!(check_registrable(&Hotkey::parse("Ctrl+D").unwrap()).is_ok());
        assert!(check_registrable(&Hotkey::parse("F5").unwrap()).is_ok());
        assert!(check_registrable(&Hotkey::parse("Ctrl+Alt+Home").unwrap()).is_ok());
    }

    #[test]
    fn the_startup_check_names_the_key_it_cannot_register() {
        // Reported at startup rather than silently never firing.
        let err = check_registrable(&Hotkey::parse("Ctrl+D").unwrap());
        assert!(err.is_ok());

        let unsupported = HotkeyDriverError::Unsupported {
            key: "F99".to_string(),
        };

        assert!(unsupported.to_string().contains("F99"));
    }

    #[test]
    fn a_taken_hotkey_says_what_probably_took_it() {
        // A bare "registration failed" leaves the user with nothing to try.
        let err = HotkeyDriverError::AlreadyTaken {
            hotkey: "Ctrl+D".to_string(),
        };

        let message = err.to_string();

        assert!(message.contains("Ctrl+D"));
        assert!(message.contains("Another application"));
    }

    #[test]
    fn every_hotkey_the_parser_accepts_can_be_registered() {
        // A hotkey that parses and then cannot register is a gap the user
        // falls into. There is no such gap.
        for text in [
            "Ctrl+D",
            "Alt+1",
            "Shift+F12",
            "Meta+Up",
            "Escape",
            "Ctrl+Alt+Shift+PageDown",
        ] {
            let hotkey = Hotkey::parse(text).unwrap();

            assert!(check_registrable(&hotkey).is_ok(), "{text}");
        }
    }
}
