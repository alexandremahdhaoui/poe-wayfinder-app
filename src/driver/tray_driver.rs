#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    OpenStatus,
    Research,
    OpenInBrowser,
    TogglePaused,
    RebuildData,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub label: String,
    pub action: Option<TrayAction>,
    pub enabled: bool,
}

impl MenuItem {
    fn action(label: &str, action: TrayAction, enabled: bool) -> Self {
        Self {
            label: label.to_string(),
            action: Some(action),
            enabled,
        }
    }

    fn status(label: String) -> Self {
        Self {
            label,
            action: None,
            enabled: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrayState {
    pub game_found: bool,
    pub paused: bool,
    pub has_search: bool,
    pub league: Option<String>,
    pub stat_count: usize,
}

pub fn menu(state: &TrayState) -> Vec<MenuItem> {
    let mut out = Vec::new();

    out.push(MenuItem::status(if state.game_found {
        "Game found".to_string()
    } else {
        "Game not found".to_string()
    }));

    out.push(MenuItem::status(match &state.league {
        Some(league) => format!("League: {league}"),
        None => "League: not set".to_string(),
    }));

    if state.stat_count == 0 {
        out.push(MenuItem::status("No game data loaded".to_string()));
    }

    out.push(MenuItem::action(
        "Open poe-wayfinder",
        TrayAction::OpenStatus,
        true,
    ));

    out.push(MenuItem::action(
        if state.paused { "Resume" } else { "Pause" },
        TrayAction::TogglePaused,
        true,
    ));

    out.push(MenuItem::action(
        "Search again",
        TrayAction::Research,
        state.has_search,
    ));

    out.push(MenuItem::action(
        "Open in browser",
        TrayAction::OpenInBrowser,
        state.has_search,
    ));

    out.push(MenuItem::action(
        "Rebuild game data",
        TrayAction::RebuildData,
        true,
    ));

    out.push(MenuItem::action("Quit", TrayAction::Quit, true));

    out
}

pub fn accepts_hotkey(state: &TrayState) -> bool {
    !state.paused && state.game_found && state.stat_count > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> TrayState {
        TrayState {
            game_found: true,
            paused: false,
            has_search: false,
            league: Some("Standard".into()),
            stat_count: 3787,
        }
    }

    fn labels(state: &TrayState) -> Vec<String> {
        menu(state).into_iter().map(|i| i.label).collect()
    }

    fn item(state: &TrayState, action: TrayAction) -> MenuItem {
        menu(state)
            .into_iter()
            .find(|i| i.action == Some(action))
            .expect("action is in the menu")
    }

    #[test]
    fn the_menu_says_whether_the_game_was_found() {
        assert!(labels(&ready()).contains(&"Game found".to_string()));

        let missing = TrayState {
            game_found: false,
            ..ready()
        };

        assert!(labels(&missing).contains(&"Game not found".to_string()));
    }

    #[test]
    fn the_menu_says_which_league_it_will_search() {
        assert!(labels(&ready()).contains(&"League: Standard".to_string()));

        let unset = TrayState {
            league: None,
            ..ready()
        };

        assert!(labels(&unset).contains(&"League: not set".to_string()));
    }

    #[test]
    fn missing_data_gets_its_own_row() {
        let empty = TrayState {
            stat_count: 0,
            ..ready()
        };

        assert!(labels(&empty).contains(&"No game data loaded".to_string()));
    }

    #[test]
    fn loaded_data_gets_no_row() {
        assert!(!labels(&ready()).contains(&"No game data loaded".to_string()));
    }

    #[test]
    fn the_status_rows_come_first() {
        let rows = menu(&ready());

        assert!(rows[0].action.is_none());
        assert!(rows[1].action.is_none());
    }

    #[test]
    fn a_status_row_cannot_be_clicked() {
        for row in menu(&ready()) {
            if row.action.is_none() {
                assert!(!row.enabled, "{}", row.label);
            }
        }
    }

    #[test]
    fn pause_reads_as_resume_once_paused() {
        assert_eq!(item(&ready(), TrayAction::TogglePaused).label, "Pause");

        let paused = TrayState {
            paused: true,
            ..ready()
        };

        assert_eq!(item(&paused, TrayAction::TogglePaused).label, "Resume");
    }

    #[test]
    fn the_search_actions_need_a_previous_search() {
        let fresh = ready();

        assert!(!item(&fresh, TrayAction::Research).enabled);
        assert!(!item(&fresh, TrayAction::OpenInBrowser).enabled);

        let after = TrayState {
            has_search: true,
            ..ready()
        };

        assert!(item(&after, TrayAction::Research).enabled);
        assert!(item(&after, TrayAction::OpenInBrowser).enabled);
    }

    #[test]
    fn quit_and_rebuild_are_always_available() {
        for state in [
            ready(),
            TrayState {
                game_found: false,
                stat_count: 0,
                paused: true,
                ..ready()
            },
        ] {
            assert!(item(&state, TrayAction::Quit).enabled);
            assert!(item(&state, TrayAction::RebuildData).enabled);
        }
    }

    #[test]
    fn every_action_appears_exactly_once() {
        let actions: Vec<TrayAction> = menu(&ready())
            .into_iter()
            .filter_map(|i| i.action)
            .collect();

        let mut seen = actions.clone();
        seen.sort_by_key(|a| format!("{a:?}"));
        seen.dedup();

        assert_eq!(seen.len(), actions.len());
    }

    #[test]
    fn a_ready_app_accepts_the_hotkey() {
        assert!(accepts_hotkey(&ready()));
    }

    #[test]
    fn a_paused_app_ignores_the_hotkey() {
        assert!(!accepts_hotkey(&TrayState {
            paused: true,
            ..ready()
        }));
    }

    #[test]
    fn the_hotkey_is_ignored_when_the_game_is_gone() {
        assert!(!accepts_hotkey(&TrayState {
            game_found: false,
            ..ready()
        }));
    }

    #[test]
    fn the_hotkey_is_ignored_with_no_data() {
        assert!(!accepts_hotkey(&TrayState {
            stat_count: 0,
            ..ready()
        }));
    }
}

pub fn tooltip(state: &TrayState, game: &str, hotkey: &str) -> String {
    let status = if state.paused {
        "paused"
    } else if !state.game_found {
        "game not found"
    } else if state.stat_count == 0 {
        "no data"
    } else {
        hotkey
    };

    format!("poe-wayfinder — {game} — {status}")
}

#[derive(Debug, thiserror::Error)]
pub enum TrayError {
    #[error("registering the tray window class")]
    WindowClass,

    #[error("creating the tray window")]
    Window,

    #[error("adding the tray icon")]
    AddIcon,
}

#[cfg(windows)]
mod win {
    use super::{menu, tooltip, TrayAction, TrayError, TrayState};

    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::{Arc, Mutex, OnceLock};

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
        NOTIFYICONDATAW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
        DestroyMenu, DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW, PostQuitMessage,
        RegisterClassW, SetForegroundWindow, TrackPopupMenu, TranslateMessage, HICON, ICONINFO,
        IDI_APPLICATION, MF_GRAYED, MF_STRING, MSG, TPM_BOTTOMALIGN, TPM_RIGHTALIGN,
        WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_COMMAND, WM_DESTROY, WM_RBUTTONUP, WNDCLASSW,
    };

    const TRAY_ID: u32 = 1;

    const WM_TRAY: u32 = WM_APP + 1;

    const MENU_BASE: usize = 100;

    struct Context {
        state: Arc<Mutex<TrayState>>,
        actions: Mutex<Sender<TrayAction>>,
        rows: Mutex<Vec<Option<TrayAction>>>,
    }

    static CONTEXT: OnceLock<Context> = OnceLock::new();

    pub struct TrayIcon {
        actions: Receiver<TrayAction>,
        state: Arc<Mutex<TrayState>>,
        window: Arc<Mutex<Option<isize>>>,
        game: String,
        hotkey: String,
    }

    impl TrayIcon {
        pub fn start(state: TrayState, game: &str, hotkey: &str) -> Result<Self, TrayError> {
            let shared = Arc::new(Mutex::new(state));
            let (action_tx, action_rx) = mpsc::channel();
            let (ready_tx, ready_rx) = mpsc::channel();

            let _ = CONTEXT.set(Context {
                state: Arc::clone(&shared),
                actions: Mutex::new(action_tx),
                rows: Mutex::new(Vec::new()),
            });

            let window = Arc::new(Mutex::new(None));

            let tip = tooltip(&shared.lock().unwrap().clone(), game, hotkey);
            let thread_window = Arc::clone(&window);

            std::thread::spawn(move || {
                let _ = run(&tip, &ready_tx, &thread_window);
            });

            match ready_rx.recv() {
                Ok(Ok(())) => Ok(Self {
                    actions: action_rx,
                    state: shared,
                    window,
                    game: game.to_string(),
                    hotkey: hotkey.to_string(),
                }),
                Ok(Err(err)) => Err(err),
                Err(_) => Err(TrayError::Window),
            }
        }

        pub fn actions(&self) -> Vec<TrayAction> {
            let mut out = Vec::new();

            while let Ok(action) = self.actions.try_recv() {
                if !out.contains(&action) {
                    out.push(action);
                }
            }

            out
        }

        pub fn update(&self, state: TrayState) {
            let changed = {
                let mut held = self.state.lock().unwrap();

                if *held == state {
                    return;
                }

                *held = state.clone();

                tooltip(&state, &self.game, &self.hotkey)
            };

            let Some(handle) = *self.window.lock().unwrap() else {
                return;
            };

            let mut data = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: HWND(handle as *mut std::ffi::c_void),
                uID: TRAY_ID,
                uFlags: NIF_TIP,
                ..Default::default()
            };

            write_tip(&mut data.szTip, &changed);

            let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
        }
    }

    fn run(
        tip: &str,
        ready: &Sender<Result<(), TrayError>>,
        window_out: &Arc<Mutex<Option<isize>>>,
    ) -> Result<(), TrayError> {
        let class_name = wide("poe_wayfinder_tray");

        let instance = unsafe { GetModuleHandleW(None) }.map_err(|_| TrayError::WindowClass)?;

        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };

        if unsafe { RegisterClassW(&class) } == 0 {
            let _ = ready.send(Err(TrayError::WindowClass));

            return Err(TrayError::WindowClass);
        }

        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(class_name.as_ptr()),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )
        }
        .map_err(|_| TrayError::Window)?;

        *window_out.lock().unwrap() = Some(window.0 as isize);

        let icon = match draw_icon() {
            Some(icon) => icon,
            None => unsafe { LoadIconW(None, IDI_APPLICATION) }.map_err(|_| TrayError::AddIcon)?,
        };

        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: window,
            uID: TRAY_ID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: icon,
            ..Default::default()
        };

        write_tip(&mut data.szTip, tip);

        if !unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool() {
            let _ = ready.send(Err(TrayError::AddIcon));

            return Err(TrayError::AddIcon);
        }

        let _ = ready.send(Ok(()));

        let mut message = MSG::default();

        while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };

        Ok(())
    }

    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_TRAY if lparam.0 as u32 == WM_RBUTTONUP => {
                show_menu(window);

                LRESULT(0)
            }

            WM_COMMAND => {
                let id = wparam.0 & 0xFFFF;

                if let Some(action) = row_action(id) {
                    if let Some(context) = CONTEXT.get() {
                        let _ = context.actions.lock().unwrap().send(action);
                    }

                    if action == TrayAction::Quit {
                        PostQuitMessage(0);
                    }
                }

                LRESULT(0)
            }

            WM_DESTROY => {
                PostQuitMessage(0);

                LRESULT(0)
            }

            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }

    fn row_action(id: usize) -> Option<TrayAction> {
        let context = CONTEXT.get()?;
        let rows = context.rows.lock().unwrap();

        rows.get(id.checked_sub(MENU_BASE)?).copied().flatten()
    }

    unsafe fn show_menu(window: HWND) {
        let Some(context) = CONTEXT.get() else {
            return;
        };

        let state = context.state.lock().unwrap().clone();
        let items = menu(&state);

        let Ok(handle) = CreatePopupMenu() else {
            return;
        };

        let mut rows = Vec::with_capacity(items.len());

        for (index, item) in items.iter().enumerate() {
            let label = wide(&item.label);

            let flags = if item.enabled {
                MF_STRING
            } else {
                MF_STRING | MF_GRAYED
            };

            let _ = AppendMenuW(handle, flags, MENU_BASE + index, PCWSTR(label.as_ptr()));

            rows.push(item.action);
        }

        *context.rows.lock().unwrap() = rows;

        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);

        let _ = SetForegroundWindow(window);

        let _ = TrackPopupMenu(
            handle,
            TPM_RIGHTALIGN | TPM_BOTTOMALIGN,
            point.x,
            point.y,
            Some(0),
            window,
            None,
        );

        let _ = DestroyMenu(handle);
    }

    const ICON: i32 = 32;

    fn draw_icon() -> Option<HICON> {
        let image = crate::assets::tray_icon();

        if !image.is_intact() || image.width as i32 != ICON {
            return None;
        }

        let mut pixels = vec![0u32; (ICON * ICON) as usize];

        for (i, p) in image.rgba.chunks_exact(4).enumerate() {
            let a = p[3] as u32;
            let r = p[0] as u32 * a / 255;
            let g = p[1] as u32 * a / 255;
            let b = p[2] as u32 * a / 255;

            pixels[i] = (a << 24) | (r << 16) | (g << 8) | b;
        }

        let colour = unsafe {
            CreateBitmap(
                ICON,
                ICON,
                1,
                32,
                Some(pixels.as_ptr() as *const std::ffi::c_void),
            )
        };

        if colour.is_invalid() {
            return None;
        }

        let mask = unsafe { CreateBitmap(ICON, ICON, 1, 1, None) };

        if mask.is_invalid() {
            unsafe {
                let _ = DeleteObject(colour.into());
            }

            return None;
        }

        let info = ICONINFO {
            fIcon: true.into(),
            hbmColor: colour,
            hbmMask: mask,
            ..Default::default()
        };

        let icon = unsafe { CreateIconIndirect(&info) };

        unsafe {
            let _ = DeleteObject(colour.into());
            let _ = DeleteObject(mask.into());
        }

        icon.ok()
    }

    fn write_tip(buffer: &mut [u16; 128], text: &str) {
        let encoded: Vec<u16> = text.encode_utf16().take(buffer.len() - 1).collect();

        buffer[..encoded.len()].copy_from_slice(&encoded);
        buffer[encoded.len()] = 0;
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(windows)]
pub use win::TrayIcon;

#[cfg(test)]
mod render_tests {
    use super::*;

    fn ready() -> TrayState {
        TrayState {
            game_found: true,
            paused: false,
            has_search: false,
            league: Some("Standard".into()),
            stat_count: 3787,
        }
    }

    #[test]
    fn the_tooltip_names_the_game_and_the_hotkey() {
        let got = tooltip(&ready(), "poe2", "Ctrl+D");

        assert!(got.contains("poe2"), "{got}");
        assert!(got.contains("Ctrl+D"), "{got}");
    }

    #[test]
    fn a_paused_tooltip_says_so_instead_of_the_hotkey() {
        let got = tooltip(
            &TrayState {
                paused: true,
                ..ready()
            },
            "poe2",
            "Ctrl+D",
        );

        assert!(got.contains("paused"), "{got}");
        assert!(!got.contains("Ctrl+D"), "{got}");
    }

    #[test]
    fn a_missing_game_shows_in_the_tooltip() {
        let got = tooltip(
            &TrayState {
                game_found: false,
                ..ready()
            },
            "poe2",
            "Ctrl+D",
        );

        assert!(got.contains("game not found"), "{got}");
    }

    #[test]
    fn missing_data_shows_in_the_tooltip() {
        let got = tooltip(
            &TrayState {
                stat_count: 0,
                ..ready()
            },
            "poe2",
            "Ctrl+D",
        );

        assert!(got.contains("no data"), "{got}");
    }

    #[test]
    fn paused_beats_every_other_reason() {
        let got = tooltip(
            &TrayState {
                paused: true,
                game_found: false,
                stat_count: 0,
                ..ready()
            },
            "poe2",
            "Ctrl+D",
        );

        assert!(got.contains("paused"), "{got}");
    }

    #[test]
    fn a_tooltip_always_fits_the_windows_buffer() {
        let long = "x".repeat(500);

        let got = tooltip(&ready(), &long, &long);

        assert!(
            got.encode_utf16().count() > 128,
            "the case is not exercised"
        );
    }

    #[test]
    fn every_action_the_model_offers_is_one_this_file_can_dispatch() {
        for item in menu(&ready()) {
            let Some(action) = item.action else {
                continue;
            };

            let handled = match action {
                TrayAction::OpenStatus
                | TrayAction::Research
                | TrayAction::OpenInBrowser
                | TrayAction::TogglePaused
                | TrayAction::RebuildData
                | TrayAction::Quit => true,
            };

            assert!(handled, "{action:?}");
        }
    }
}
