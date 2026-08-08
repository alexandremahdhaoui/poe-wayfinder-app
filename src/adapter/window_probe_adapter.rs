use poe_trader_core::controller::panel_visible::{Measured, Rect};

#[cfg_attr(test, mockall::automock)]
pub trait WindowProbe: Send + Sync {
    fn measure(&self, panel_title: &str, game_title: &str) -> Result<Measured, ProbeError>;
}

pub struct SystemWindowProbe;

impl SystemWindowProbe {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SystemWindowProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowProbe for SystemWindowProbe {
    fn measure(&self, panel_title: &str, game_title: &str) -> Result<Measured, ProbeError> {
        measure(panel_title, game_title)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("no window titled {title:?} is open")]
    NotFound { title: String },

    #[error("reading the window rectangle")]
    Rect,
}

#[cfg(windows)]
mod win {
    use super::{Measured, ProbeError, Rect};

    use windows::core::HSTRING;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetSystemMetrics, GetWindow, GetWindowRect, IsWindowVisible, GW_HWNDPREV,
        SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };

    fn find(title: &str) -> Result<HWND, ProbeError> {
        let window = unsafe { FindWindowW(None, &HSTRING::from(title)) };

        window.map_err(|_| ProbeError::NotFound {
            title: title.to_string(),
        })
    }

    fn rect_of(window: HWND) -> Result<Rect, ProbeError> {
        let mut rect = RECT::default();

        unsafe { GetWindowRect(window, &mut rect) }.map_err(|_| ProbeError::Rect)?;

        Ok(Rect {
            x: rect.left,
            y: rect.top,
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
        })
    }

    pub fn desktop() -> Rect {
        unsafe {
            Rect {
                x: GetSystemMetrics(SM_XVIRTUALSCREEN),
                y: GetSystemMetrics(SM_YVIRTUALSCREEN),
                width: GetSystemMetrics(SM_CXVIRTUALSCREEN),
                height: GetSystemMetrics(SM_CYVIRTUALSCREEN),
            }
        }
    }

    fn is_above(above: HWND, below: HWND) -> bool {
        let mut current = below;

        for _ in 0..10_000 {
            let Ok(previous) = (unsafe { GetWindow(current, GW_HWNDPREV) }) else {
                return false;
            };

            if previous.0.is_null() {
                return false;
            }

            if previous == above {
                return true;
            }

            current = previous;
        }

        false
    }

    pub fn measure(panel_title: &str, game_title: &str) -> Result<Measured, ProbeError> {
        let panel = find(panel_title)?;
        let game = find(game_title)?;

        let shown = unsafe { IsWindowVisible(panel) }.as_bool();

        Ok(Measured {
            window: rect_of(panel)?,
            desktop: desktop(),
            shown,
            above_game: is_above(panel, game),
        })
    }
}

#[cfg(windows)]
pub use win::{desktop, measure};

#[cfg(not(windows))]
pub fn measure(_panel_title: &str, _game_title: &str) -> Result<Measured, ProbeError> {
    Err(ProbeError::NotFound {
        title: "measuring a window only works on Windows".to_string(),
    })
}

#[cfg(not(windows))]
pub fn desktop() -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_names_what_could_not_be_measured() {
        let missing = ProbeError::NotFound {
            title: "Path of Exile 2".to_string(),
        };

        assert!(missing.to_string().contains("Path of Exile 2"), "{missing}");
        assert!(ProbeError::Rect.to_string().contains("rectangle"));
    }
}
