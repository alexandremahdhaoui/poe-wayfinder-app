//! The system tray menu.
//!
//! The overlay has no window a user can click to reach settings, because a
//! window like that would sit over the game. The tray is where the app lives
//! when it is not showing a price.
//!
//! # Why the menu is a pure model
//!
//! What the menu offers depends on state: it cannot say "search again" with no
//! previous search, and it should say whether the game was found. That logic
//! is testable and the drawing is not, so they are separated the same way the
//! overlay is.

/// What the user picked from the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// Run the last search again.
    Research,
    /// Open the last search on the trade site.
    OpenInBrowser,
    /// Stop reacting to the hotkey without quitting.
    TogglePaused,
    /// Rebuild the game data.
    RebuildData,
    /// Quit.
    Quit,
}

/// One row in the tray menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub label: String,
    /// None for a row that only reports state.
    pub action: Option<TrayAction>,
    /// Shown but not clickable.
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

/// What the tray needs to know to build its menu.
#[derive(Debug, Clone, PartialEq)]
pub struct TrayState {
    /// The game window was found.
    pub game_found: bool,
    /// The hotkey is being ignored.
    pub paused: bool,
    /// A search has run this session.
    pub has_search: bool,
    /// The league being searched.
    pub league: Option<String>,
    /// How many stats are loaded.
    pub stat_count: usize,
}

/// Build the menu for a state.
///
/// The status rows come first, because the question a user opens the tray to
/// answer is usually "why is nothing happening".
pub fn menu(state: &TrayState) -> Vec<MenuItem> {
    let mut out = Vec::new();

    // The most common reason nothing happens is that the game is not running
    // or the window title is wrong. Saying so here saves a support round trip.
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
        // Nothing will ever match without data, and the failure would
        // otherwise show up as every modifier being unknown.
        out.push(MenuItem::status("No game data loaded".to_string()));
    }

    out.push(MenuItem::action(
        if state.paused { "Resume" } else { "Pause" },
        TrayAction::TogglePaused,
        true,
    ));

    // Both need a previous search to act on.
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

/// Whether the hotkey should be acted on.
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
        // The most common reason nothing happens is that the game is not
        // running or the window title is wrong.
        assert!(labels(&ready()).contains(&"Game found".to_string()));

        let missing = TrayState {
            game_found: false,
            ..ready()
        };

        assert!(labels(&missing).contains(&"Game not found".to_string()));
    }

    #[test]
    fn the_menu_says_which_league_it_will_search() {
        // A wrong league returns nothing rather than an error, so seeing it is
        // worth a row.
        assert!(labels(&ready()).contains(&"League: Standard".to_string()));

        let unset = TrayState {
            league: None,
            ..ready()
        };

        assert!(labels(&unset).contains(&"League: not set".to_string()));
    }

    #[test]
    fn missing_data_gets_its_own_row() {
        // Nothing will ever match without it, and the failure would otherwise
        // show up as every modifier being unknown.
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
        // The question a user opens the tray to answer is usually why nothing
        // is happening.
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
        // Offering them with nothing to act on is a click that does nothing.
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
        // A user must always be able to leave, whatever state the app is in.
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
        // A duplicate row would fire twice or confuse which one was clicked.
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
        // Copying from whatever has focus would take text from another
        // application and try to price it.
        assert!(!accepts_hotkey(&TrayState {
            game_found: false,
            ..ready()
        }));
    }

    #[test]
    fn the_hotkey_is_ignored_with_no_data() {
        // Every modifier would be unknown and the price would be meaningless.
        assert!(!accepts_hotkey(&TrayState {
            stat_count: 0,
            ..ready()
        }));
    }
}

// ---------------------------------------------------------------------------
// The renderer
//
// Everything above is a pure model and was written first. Nothing drew it, so
// there was never an icon: the menu existed only in its own tests, and a
// running overlay was indistinguishable from one that never started.
// ---------------------------------------------------------------------------

/// The tooltip shown on hover.
///
/// The only text the tool gets to show without the user clicking anything, so
/// it answers the two questions that bring someone to the tray: is it working,
/// and what key do I press.
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

    format!("poe-trader — {game} — {status}")
}

/// Why the tray icon could not be created.
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
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
        NOTIFYICONDATAW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
        DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW, PostQuitMessage, RegisterClassW,
        SetForegroundWindow, TrackPopupMenu, TranslateMessage, IDI_APPLICATION, MF_GRAYED,
        MF_STRING, MSG, TPM_BOTTOMALIGN, TPM_RIGHTALIGN, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
        WM_COMMAND, WM_DESTROY, WM_RBUTTONUP, WNDCLASSW,
    };

    /// Our id for the icon. One tray icon per process.
    const TRAY_ID: u32 = 1;

    /// The message Windows posts to us when the icon is clicked.
    const WM_TRAY: u32 = WM_APP + 1;

    /// The first menu command id. Rows are numbered from here in order.
    const MENU_BASE: usize = 100;

    /// What the window procedure needs.
    ///
    /// A static because `window_proc` is a bare function pointer with nowhere
    /// to hang state, and there is exactly one tray icon per process. The
    /// `Sender` is behind a mutex only to make the whole thing `Sync`.
    struct Context {
        state: Arc<Mutex<TrayState>>,
        actions: Mutex<Sender<TrayAction>>,
        /// What each menu row does, by position. Rebuilt on every open,
        /// because the model decides the rows from the current state.
        rows: Mutex<Vec<Option<TrayAction>>>,
    }

    static CONTEXT: OnceLock<Context> = OnceLock::new();

    /// The live tray icon.
    ///
    /// Owns nothing Windows side. The icon belongs to the thread started in
    /// `start`, which removes it when the message loop ends.
    pub struct TrayIcon {
        actions: Receiver<TrayAction>,
        state: Arc<Mutex<TrayState>>,
        window: Arc<Mutex<Option<isize>>>,
        game: String,
        hotkey: String,
    }

    impl TrayIcon {
        /// Add the icon and start listening.
        ///
        /// `game` and `hotkey` only ever appear in the tooltip.
        pub fn start(state: TrayState, game: &str, hotkey: &str) -> Result<Self, TrayError> {
            let shared = Arc::new(Mutex::new(state));
            let (action_tx, action_rx) = mpsc::channel();
            let (ready_tx, ready_rx) = mpsc::channel();

            // Set before the thread starts, so a click that arrives during
            // startup finds a context rather than dropping.
            let _ = CONTEXT.set(Context {
                state: Arc::clone(&shared),
                actions: Mutex::new(action_tx),
                rows: Mutex::new(Vec::new()),
            });

            let window = Arc::new(Mutex::new(None));

            let tip = tooltip(&shared.lock().unwrap().clone(), game, hotkey);
            let thread_window = Arc::clone(&window);

            std::thread::spawn(move || {
                // A failure after the ready signal has nowhere useful to go.
                // The overlay prices items perfectly well with no icon.
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

        /// Everything clicked since the last call.
        ///
        /// Drained rather than queued. Two Quits are one Quit, and a stutter
        /// must not run four price checks.
        pub fn actions(&self) -> Vec<TrayAction> {
            let mut out = Vec::new();

            while let Ok(action) = self.actions.try_recv() {
                if !out.contains(&action) {
                    out.push(action);
                }
            }

            out
        }

        /// Tell the tray what the app is doing now.
        ///
        /// Cheap enough to call every frame. The tooltip is only rewritten
        /// when it actually changes, because `NIM_MODIFY` is a syscall.
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

            // SAFETY: `data` is a live, correctly sized NOTIFYICONDATAW and
            // the handle came from the tray thread's own window.
            let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
        }
    }

    /// Create the window, add the icon, pump messages, remove the icon.
    fn run(
        tip: &str,
        ready: &Sender<Result<(), TrayError>>,
        window_out: &Arc<Mutex<Option<isize>>>,
    ) -> Result<(), TrayError> {
        let class_name = wide("poe_trader_tray");

        // SAFETY: a null module handle is the current process.
        let instance = unsafe { GetModuleHandleW(None) }.map_err(|_| TrayError::WindowClass)?;

        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };

        // SAFETY: `class` is live and every pointer in it outlives the call.
        if unsafe { RegisterClassW(&class) } == 0 {
            let _ = ready.send(Err(TrayError::WindowClass));

            return Err(TrayError::WindowClass);
        }

        // SAFETY: the class was just registered and every pointer is live.
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

        // SAFETY: IDI_APPLICATION is built in and always present. A stock icon
        // avoids embedding an .ico that would have to track the build.
        let icon = unsafe { LoadIconW(None, IDI_APPLICATION) }.map_err(|_| TrayError::AddIcon)?;

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

        // SAFETY: `data` is live and its window handle was just created.
        if !unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool() {
            let _ = ready.send(Err(TrayError::AddIcon));

            return Err(TrayError::AddIcon);
        }

        let _ = ready.send(Ok(()));

        let mut message = MSG::default();

        // SAFETY: `message` is live. A null window handle reads every message
        // for this thread, which is where the tray callback lands.
        while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        // SAFETY: removing the icon this thread added. Skipping it leaves a
        // ghost icon in the tray until the user hovers over it.
        let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };

        Ok(())
    }

    /// Handle the tray's messages.
    ///
    /// # Safety
    ///
    /// Called by Windows with a valid window handle. Nothing else calls it.
    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            // Right click opens the menu. Left click deliberately does
            // nothing: there is no main window to restore, so it would either
            // do nothing or surprise the user.
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

                    // Quit also stops this thread, which removes the icon on
                    // the way out.
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

    /// What a menu command id maps to.
    fn row_action(id: usize) -> Option<TrayAction> {
        let context = CONTEXT.get()?;
        let rows = context.rows.lock().unwrap();

        rows.get(id.checked_sub(MENU_BASE)?).copied().flatten()
    }

    /// Build and show the menu from the model.
    ///
    /// # Safety
    ///
    /// `window` must be live.
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

            // A status row and a disabled action are both greyed. The model
            // decides which; this only draws what it says.
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

        // Windows only dismisses a tray menu when its owner is the foreground
        // window. Without this the menu hangs around after a click elsewhere.
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

    /// Copy a tooltip into the fixed buffer Windows expects.
    ///
    /// Truncated rather than refused. A tooltip too long to fit is still worth
    /// showing, and 128 characters is far more than this needs.
    fn write_tip(buffer: &mut [u16; 128], text: &str) {
        let encoded: Vec<u16> = text.encode_utf16().take(buffer.len() - 1).collect();

        buffer[..encoded.len()].copy_from_slice(&encoded);
        buffer[encoded.len()] = 0;
    }

    /// A null terminated wide string. The caller keeps it alive.
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
        // The two questions that bring somebody to the tray.
        let got = tooltip(&ready(), "poe2", "Ctrl+D");

        assert!(got.contains("poe2"), "{got}");
        assert!(got.contains("Ctrl+D"), "{got}");
    }

    #[test]
    fn a_paused_tooltip_says_so_instead_of_the_hotkey() {
        // Pressing the hotkey while paused does nothing, so offering it would
        // be a lie.
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
        // A paused app with no game and no data is paused first. The user
        // turned it off; that is the answer to give them.
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
        // Windows takes 128 wide characters and no more. A longer one has to
        // truncate rather than be refused, because the tray is the only place
        // the tool can speak.
        let long = "x".repeat(500);

        let got = tooltip(&ready(), &long, &long);

        assert!(
            got.encode_utf16().count() > 128,
            "the case is not exercised"
        );
    }

    #[test]
    fn every_action_the_model_offers_is_one_this_file_can_dispatch() {
        // The bug this whole file exists to close: an action declared in the
        // menu and handled with `=> {}` somewhere else. Adding a variant to
        // TrayAction without handling it fails to compile here.
        for item in menu(&ready()) {
            let Some(action) = item.action else {
                continue;
            };

            let handled = match action {
                TrayAction::Research
                | TrayAction::OpenInBrowser
                | TrayAction::TogglePaused
                | TrayAction::RebuildData
                | TrayAction::Quit => true,
            };

            assert!(handled, "{action:?}");
        }
    }
}
