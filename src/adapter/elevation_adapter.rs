//! Reading whether a process runs as administrator.
//!
//! The decision about what that means lives in
//! `poe_trader_core::controller::elevation`. This only answers the two
//! factual questions: are we elevated, and is the process that owns a window
//! elevated.
//!
//! # Why the game's answer is a probe and not a read
//!
//! Reading another process's token needs `TOKEN_QUERY` on it, and an ordinary
//! process cannot open a token belonging to an elevated one. The failure is
//! the answer: if the handle opens but the token does not, the other process
//! is above us, which is exactly the case that breaks the hotkey.
//!
//! That inference is only sound when we are not elevated ourselves, so an
//! elevated overlay reports the game as unknown rather than guessing.

use poe_trader_core::controller::elevation::Elevation;

#[cfg(windows)]
mod win {
    use super::Elevation;

    use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    /// Whether this process runs as administrator.
    pub fn own_elevation() -> Elevation {
        // SAFETY: a pseudo handle to this process, which never fails and never
        // needs closing.
        let process = unsafe { GetCurrentProcess() };

        elevation_of_process(process)
    }

    /// Whether the process owning a window runs as administrator.
    ///
    /// `window` is the raw handle value, so the domain never sees a Windows
    /// type.
    pub fn window_elevation(window: isize, we_are_elevated: bool) -> Elevation {
        let handle = HWND(window as *mut std::ffi::c_void);

        let mut pid = 0u32;

        // SAFETY: `pid` is a live u32 and `handle` came from FindWindowW.
        let thread = unsafe { GetWindowThreadProcessId(handle, Some(&mut pid)) };

        if thread == 0 || pid == 0 {
            return Elevation::Unknown;
        }

        // SAFETY: querying limited information is allowed across privilege
        // levels, which is why it is asked for rather than full access.
        let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
        else {
            // Cannot even open it. That is itself a sign of a process above
            // us, but not a certain one, so it is not claimed as fact.
            return Elevation::Unknown;
        };

        let answer = elevation_of_process(process);

        // SAFETY: `process` came from OpenProcess and is not used again.
        let _ = unsafe { CloseHandle(process) };

        match answer {
            // A refused token read from an ordinary process means the other
            // side is above us. From an elevated one it means something else,
            // so nothing is claimed.
            Elevation::Unknown if !we_are_elevated => Elevation::Elevated,
            other => other,
        }
    }

    /// Read the elevation flag off a process token.
    fn elevation_of_process(process: HANDLE) -> Elevation {
        let mut token = HANDLE::default();

        // SAFETY: `token` is a live handle slot. A failure leaves it untouched
        // and is reported rather than used.
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.is_err() {
            return Elevation::Unknown;
        }

        let mut elevation = TOKEN_ELEVATION::default();
        let mut size = 0u32;

        // SAFETY: `elevation` is a live, correctly sized TOKEN_ELEVATION and
        // the size argument matches it.
        let read = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elevation as *mut _ as *mut std::ffi::c_void),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut size,
            )
        };

        // SAFETY: `token` came from OpenProcessToken and is not used again.
        let _ = unsafe { CloseHandle(token) };

        if read.is_err() {
            return Elevation::Unknown;
        }

        match elevation.TokenIsElevated {
            0 => Elevation::Normal,
            _ => Elevation::Elevated,
        }
    }
}

#[cfg(windows)]
pub use win::{own_elevation, window_elevation};

/// Off Windows there is no such thing to read.
#[cfg(not(windows))]
pub fn own_elevation() -> Elevation {
    Elevation::Unknown
}

#[cfg(not(windows))]
pub fn window_elevation(_window: isize, _we_are_elevated: bool) -> Elevation {
    Elevation::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use poe_trader_core::controller::elevation::{hotkey_outlook, HotkeyOutlook};

    #[test]
    fn reading_our_own_elevation_never_panics() {
        // It runs at startup on every machine. A panic here takes the overlay
        // down before it does anything.
        let _ = own_elevation();
    }

    #[test]
    fn an_invalid_window_reports_unknown_rather_than_guessing() {
        // A zero handle owns no process. Claiming a level for it would put a
        // confident wrong message in front of the user.
        assert_eq!(window_elevation(0, false), Elevation::Unknown);
    }

    #[test]
    fn an_unknown_game_never_produces_a_blocking_verdict() {
        // The pair of this adapter and the model: whatever this cannot read,
        // the model must not turn into an accusation.
        let outlook = hotkey_outlook(own_elevation(), Elevation::Unknown);

        assert_ne!(outlook, HotkeyOutlook::BlockedByGame);
    }
}
