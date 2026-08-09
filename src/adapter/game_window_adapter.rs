use thiserror::Error;

use crate::types::overlay::WindowRect;

#[derive(Debug, Error)]
pub enum WindowError {
    #[error("no window titled {title:?} is open")]
    NotFound { title: String },

    #[error("reading the window rectangle for {title:?}")]
    Rect { title: String },

    #[error("sending input to the game window")]
    SendInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameWindow {
    pub rect: WindowRect,
    pub is_foreground: bool,
}

pub trait GameWindowSource: Send + Sync {
    fn find(&self) -> Result<GameWindow, WindowError>;

    fn cursor(&self) -> (i32, i32);

    fn scale(&self) -> f32;

    fn retarget(&self, _title: &str) {}

    fn open_titles(&self) -> Vec<String> {
        Vec::new()
    }

    fn foreground(&self) -> Option<String> {
        None
    }
}

pub fn should_draw(window: &GameWindow) -> bool {
    window.is_foreground && window.rect.is_visible()
}

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

    pub struct GameWindowAdapter {
        title: std::sync::RwLock<String>,
    }

    impl GameWindowAdapter {
        pub fn new(title: &str) -> Self {
            Self {
                title: std::sync::RwLock::new(title.to_string()),
            }
        }

        pub fn raw_handle(&self) -> Option<isize> {
            self.handle().ok().map(|h| h.0 as isize)
        }

        pub fn title(&self) -> String {
            match self.title.read() {
                Ok(title) => title.clone(),
                Err(poisoned) => poisoned.into_inner().clone(),
            }
        }

        fn handle(&self) -> Result<HWND, WindowError> {
            let wanted = self.title();
            let title = HSTRING::from(wanted.as_str());

            let handle = unsafe { FindWindowW(None, &title) };

            match handle {
                Ok(h) if !h.is_invalid() => Ok(h),
                _ => Err(WindowError::NotFound { title: wanted }),
            }
        }
    }

    impl GameWindowSource for GameWindowAdapter {
        fn find(&self) -> Result<GameWindow, WindowError> {
            let handle = self.handle()?;

            let mut rect = RECT::default();

            unsafe { GetWindowRect(handle, &mut rect) }.map_err(|_| WindowError::Rect {
                title: self.title(),
            })?;

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

            if unsafe { GetCursorPos(&mut point) }.is_err() {
                return (0, 0);
            }

            (point.x, point.y)
        }

        fn retarget(&self, title: &str) {
            let mut held = match self.title.write() {
                Ok(held) => held,
                Err(poisoned) => poisoned.into_inner(),
            };

            *held = title.to_string();
        }

        fn open_titles(&self) -> Vec<String> {
            visible_window_titles()
        }

        fn foreground(&self) -> Option<String> {
            foreground_title()
        }

        fn scale(&self) -> f32 {
            let Ok(handle) = self.handle() else {
                return 1.0;
            };

            let dpi = unsafe { GetDpiForWindow(handle) };

            if dpi == 0 {
                return 1.0;
            }

            dpi as f32 / 96.0
        }
    }

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
            let events: Vec<INPUT> = copy_key_sequence()
                .iter()
                .filter_map(|stroke| {
                    let key = match stroke.key.as_str() {
                        "Ctrl" => VK_CONTROL,
                        "C" => VK_C,
                        _ => return None,
                    };

                    Some(key_event(key, !stroke.down))
                })
                .collect();

            let sent = unsafe { SendInput(&events, std::mem::size_of::<INPUT>() as i32) };

            if sent as usize != events.len() {
                return Err(crate::adapter::clipboard_adapter::ClipboardError::Open(
                    Box::new(WindowError::SendInput),
                ));
            }

            Ok(())
        }
    }

    pub fn foreground_title() -> Option<String> {
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};

        let handle = unsafe { GetForegroundWindow() };

        if handle.is_invalid() {
            return None;
        }

        let length = unsafe { GetWindowTextLengthW(handle) };

        if length <= 0 {
            return None;
        }

        let mut buffer = vec![0u16; length as usize + 1];
        let written = unsafe { GetWindowTextW(handle, &mut buffer) };

        if written <= 0 {
            return None;
        }

        Some(String::from_utf16_lossy(&buffer[..written as usize]))
    }

    pub fn visible_window_titles() -> Vec<String> {
        use windows::Win32::Foundation::LPARAM;
        use windows::Win32::UI::WindowsAndMessaging::EnumWindows;

        let mut out: Vec<String> = Vec::new();

        unsafe {
            let _ = EnumWindows(Some(collect), LPARAM(&mut out as *mut Vec<String> as isize));
        }

        out.sort();
        out.dedup();

        out
    }

    unsafe extern "system" fn collect(
        handle: HWND,
        lparam: windows::Win32::Foundation::LPARAM,
    ) -> windows::core::BOOL {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
        };

        let keep_going = windows::core::BOOL(1);

        if !IsWindowVisible(handle).as_bool() {
            return keep_going;
        }

        let length = GetWindowTextLengthW(handle);

        if length <= 0 {
            return keep_going;
        }

        let mut buffer = vec![0u16; length as usize + 1];
        let written = GetWindowTextW(handle, &mut buffer);

        if written > 0 {
            let title = String::from_utf16_lossy(&buffer[..written as usize]);

            let out = &mut *(lparam.0 as *mut Vec<String>);
            out.push(title);
        }

        keep_going
    }

    pub fn press_combination(modifiers: &[u16], key: u16) -> u32 {
        let mut events: Vec<INPUT> = Vec::new();

        for code in modifiers {
            events.push(key_event(VIRTUAL_KEY(*code), false));
        }

        events.push(key_event(VIRTUAL_KEY(key), false));
        events.push(key_event(VIRTUAL_KEY(key), true));

        for code in modifiers.iter().rev() {
            events.push(key_event(VIRTUAL_KEY(*code), true));
        }

        unsafe { SendInput(&events, std::mem::size_of::<INPUT>() as i32) }
    }

    pub fn self_test_send_input() -> u32 {
        use windows::Win32::UI::Input::KeyboardAndMouse::VK_NONAME;

        let events = [key_event(VK_NONAME, false), key_event(VK_NONAME, true)];

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
pub use win::{
    foreground_title, press_combination, self_test_send_input, visible_window_titles,
    GameWindowAdapter, KeyboardCopyTrigger,
};

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
        assert!(!should_draw(&window(0, 0, 1920, 1080, false)));
    }

    #[test]
    fn a_minimised_window_is_not_drawn_over() {
        assert!(!should_draw(&window(0, 0, 0, 0, true)));
        assert!(!should_draw(&window(0, 0, 1920, 0, true)));
    }

    #[test]
    fn a_window_at_a_negative_origin_is_still_drawn_over() {
        assert!(should_draw(&window(-1920, 0, 1920, 1080, true)));
    }

    #[test]
    fn a_missing_window_names_the_title_it_looked_for() {
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
