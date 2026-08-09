use poe_wayfinder_core::controller::elevation::Elevation;

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

    pub fn own_elevation() -> Elevation {
        let process = unsafe { GetCurrentProcess() };

        elevation_of_process(process)
    }

    pub fn window_elevation(window: isize, we_are_elevated: bool) -> Elevation {
        let handle = HWND(window as *mut std::ffi::c_void);

        let mut pid = 0u32;

        let thread = unsafe { GetWindowThreadProcessId(handle, Some(&mut pid)) };

        if thread == 0 || pid == 0 {
            return Elevation::Unknown;
        }

        let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
        else {
            return Elevation::Unknown;
        };

        let answer = elevation_of_process(process);

        let _ = unsafe { CloseHandle(process) };

        match answer {
            Elevation::Unknown if !we_are_elevated => Elevation::Elevated,
            other => other,
        }
    }

    fn elevation_of_process(process: HANDLE) -> Elevation {
        let mut token = HANDLE::default();

        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.is_err() {
            return Elevation::Unknown;
        }

        let mut elevation = TOKEN_ELEVATION::default();
        let mut size = 0u32;

        let read = unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elevation as *mut _ as *mut std::ffi::c_void),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut size,
            )
        };

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
    use poe_wayfinder_core::controller::elevation::{hotkey_outlook, HotkeyOutlook};

    #[test]
    fn reading_our_own_elevation_never_panics() {
        let _ = own_elevation();
    }

    #[test]
    fn an_invalid_window_reports_unknown_rather_than_guessing() {
        assert_eq!(window_elevation(0, false), Elevation::Unknown);
    }

    #[test]
    fn an_unknown_game_never_produces_a_blocking_verdict() {
        let outlook = hotkey_outlook(own_elevation(), Elevation::Unknown);

        assert_ne!(outlook, HotkeyOutlook::BlockedByGame);
    }
}
