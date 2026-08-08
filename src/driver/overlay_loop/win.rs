use std::time::{Duration, Instant};

use eframe::egui;

use super::search::SearchOutcome;
use super::{OverlayLoopError, OverlaySettings};

use crate::adapter::clipboard_adapter::{copy_item, CopyTiming, SystemClipboard};
use crate::adapter::clock_adapter::SystemClock;
use crate::adapter::game_data_adapter::GameTables;
use crate::adapter::game_window_adapter::{
    self, GameWindowAdapter, GameWindowSource, KeyboardCopyTrigger,
};
use crate::adapter::http_adapter::HttpAdapter;
use crate::adapter::window_probe_adapter;
use crate::controller::overlay_controller::{Frame, OverlayModel};
use crate::controller::price_check_controller::PriceCheckController;
use crate::controller::price_check_loop;
use crate::driver::hook_driver::HookDriver;
use crate::driver::hotkey_driver::HotkeyDriver;
use crate::driver::overlay_placement;
use crate::driver::overlay_ui_driver::{overlay_viewport, paint, should_paint, UiEvent};
use crate::driver::tray_driver::{accepts_hotkey, TrayAction, TrayIcon, TrayState};
use crate::logging::{Logger, Value};
use crate::types::overlay::OverlayGeometry;
use crate::types::Hotkey;
use crate::util::error_chain::render;

use poe_trader_core::controller::panel_visible::{explain, visibility};
use poe_trader_core::controller::press_coalesce;
use poe_trader_core::controller::price_check::{price_check, PriceCheckOptions};
use poe_trader_core::types::GameVersion;

const PANEL_WINDOW_TITLE: &str = "poe-trader";
const FRAME_INTERVAL: Duration = Duration::from_millis(100);
const HEARTBEAT_FRAMES: i64 = 100;

pub struct OverlayLoopDriver {
    window: GameWindowAdapter,
    clipboard: SystemClipboard,
    trigger: KeyboardCopyTrigger,
    hotkeys: Option<HotkeyDriver>,
    hook: Option<HookDriver>,
    tray: Option<TrayIcon>,
    tray_state: TrayState,
    model: OverlayModel,
    runtime: tokio::runtime::Runtime,
    prices: PriceCheckController<HttpAdapter, SystemClock>,
    log: Logger,
    settings: OverlaySettings,
    game: GameVersion,
    data: GameTables,
    options: PriceCheckOptions,
    timing: CopyTiming,
    frames: i64,
    started: bool,
    last_press: Option<Instant>,
    window_was_found: bool,
    was_showing: bool,
    probe_due: bool,
    last_search: Option<SearchOutcome>,
    pending: Vec<UiEvent>,
}

impl OverlayLoopDriver {
    pub fn new(
        settings: OverlaySettings,
        game: GameVersion,
        data: GameTables,
        hotkey: &Hotkey,
        http: HttpAdapter,
        log: Logger,
    ) -> Result<Self, OverlayLoopError> {
        let window = GameWindowAdapter::new(&settings.window_title);

        report_window(&window, &log);
        crate::driver::cli_driver::report_hotkey_outlook(&window, &log);
        report_input(&log);

        let hotkeys = start_registration(hotkey, &log);
        let hook = start_hook(hotkey, &log);

        let tray_state = TrayState {
            game_found: window.find().is_ok(),
            paused: false,
            has_search: false,
            league: Some(settings.league.clone()),
            stat_count: data.stat_count(),
        };

        let tray = start_tray(&tray_state, game, hotkey, &log);

        let clipboard =
            SystemClipboard::new().map_err(|source| OverlayLoopError::Clipboard { source })?;

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|source| OverlayLoopError::Runtime { source })?;

        let mut model = OverlayModel::new(OverlayGeometry::default());

        model.start(window.cursor());
        model.fail(&format!(
            "Ready. Press {hotkey} with the cursor over an item."
        ));

        let prices = PriceCheckController::new(
            http,
            SystemClock::new(),
            &settings.site_url,
            game,
            &settings.league,
        )
        .with_session(&settings.session)
        .with_latency(settings.latency);

        let window_was_found = window.find().is_ok();

        Ok(Self {
            window,
            clipboard,
            trigger: KeyboardCopyTrigger::new(),
            hotkeys,
            hook,
            tray,
            tray_state,
            model,
            runtime,
            prices,
            log,
            settings,
            game,
            data,
            options: PriceCheckOptions::new(game),
            timing: CopyTiming::default(),
            frames: 0,
            started: false,
            last_press: None,
            window_was_found,
            was_showing: false,
            probe_due: false,
            last_search: None,
            pending: Vec::new(),
        })
    }

    pub fn run(mut self) -> Result<(), OverlayLoopError> {
        let first = self
            .model
            .frame_scaled(self.window.find().ok(), self.window.scale());

        let native_options = eframe::NativeOptions {
            viewport: overlay_viewport(&first),
            ..eframe::NativeOptions::default()
        };

        let result = eframe::run_simple_native(PANEL_WINDOW_TITLE, native_options, {
            move |ctx, _frame| self.frame(ctx)
        });

        result.map_err(|err| OverlayLoopError::Window {
            message: err.to_string(),
        })
    }

    fn frame(&mut self, ctx: &egui::Context) {
        self.heartbeat();

        if self.probe_due {
            self.probe_due = false;
            self.probe_panel();
        }

        let asked = self.collect_actions();
        self.dispatch(asked, ctx);

        if self.read_hotkey() {
            self.run_price_check();
        }

        let found = self.window.find().ok();

        self.refresh_tray(found.is_some());
        self.report_window_change(found.is_some(), found);

        let frame = self.model.frame_scaled(found, self.window.scale());

        self.report_panel_change(&frame, found);
        self.apply_placement(ctx, &frame);

        self.pending.extend(paint(ctx, &self.model));

        ctx.request_repaint_after(FRAME_INTERVAL);
    }

    fn heartbeat(&mut self) {
        if !self.started {
            self.started = true;

            self.log
                .info("the frame loop is running. The hotkey is being read.", &[]);
        }

        self.frames += 1;

        if self.frames % HEARTBEAT_FRAMES == 0 {
            self.log
                .debug("frame loop alive", &[("frames", Value::Int(self.frames))]);
        }
    }

    fn collect_actions(&mut self) -> Vec<TrayAction> {
        let mut asked: Vec<TrayAction> =
            self.tray.as_ref().map(|t| t.actions()).unwrap_or_default();

        for event in std::mem::take(&mut self.pending) {
            match event {
                UiEvent::OpenInBrowser => asked.push(TrayAction::OpenInBrowser),
                UiEvent::Research => asked.push(TrayAction::Research),
                UiEvent::Dismiss => self.model.hide(),
                UiEvent::ToggleFilter(_) => {}
            }
        }

        asked
    }

    fn dispatch(&mut self, asked: Vec<TrayAction>, ctx: &egui::Context) {
        for action in asked {
            match action {
                TrayAction::Quit => {
                    self.log.info("quit chosen from the tray", &[]);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }

                TrayAction::TogglePaused => {
                    self.tray_state.paused = !self.tray_state.paused;

                    self.log.info(
                        "pause toggled",
                        &[("paused", Value::Bool(self.tray_state.paused))],
                    );
                }

                TrayAction::OpenInBrowser => self.open_last_search(),

                TrayAction::Research => self.research(),

                TrayAction::RebuildData => self.rebuild_data(),
            }
        }
    }

    fn open_last_search(&mut self) {
        let Some(outcome) = &self.last_search else {
            self.model.warn("Nothing searched yet.");

            return;
        };

        match outcome.browser_url(&self.settings.site_url, self.game, &self.settings.league) {
            Some(url) => {
                self.log
                    .info("opening the search", &[("url", Value::Str(url.clone()))]);

                open_in_browser(&url);
            }
            None => self.log.warn("the last search has no id to open", &[]),
        }
    }

    fn research(&mut self) {
        let Some(checked) = self.model.result().cloned() else {
            self.model.warn("Nothing to search again yet.");

            return;
        };

        self.log.info("searching again", &[]);

        match self.search_for(&checked) {
            Ok(total) => self.model.finish(checked, total),
            Err(message) => self.model.warn(&message),
        }
    }

    fn rebuild_data(&mut self) {
        self.log.info("rebuilding the game data", &[]);

        match rebuild_data(self.game, &self.settings.data_dir) {
            Ok(()) => self
                .model
                .warn("Rebuilding the data. Restart when it finishes."),
            Err(message) => {
                self.log.error(
                    "rebuilding the game data",
                    &[("error", Value::Str(message.clone()))],
                );

                self.model
                    .warn(&format!("Could not rebuild the data: {message}"));
            }
        }
    }

    fn search_for(
        &mut self,
        checked: &poe_trader_core::controller::price_check::PriceCheck,
    ) -> Result<u64, String> {
        let (result, exchange) = self
            .runtime
            .block_on(self.prices.search_checked(checked))
            .map_err(|e| render(&e))
            .inspect_err(|message| {
                self.log.error(
                    "searching the trade site",
                    &[("error", Value::Str(message.clone()))],
                );
            })?;

        let total = result.total;

        self.last_search = Some(SearchOutcome {
            total,
            id: result.id,
            exchange,
        });

        Ok(total)
    }

    fn read_hotkey(&mut self) -> bool {
        let by_registration = self.hotkeys.as_mut().is_some_and(|h| h.fired());
        let by_hook = self.hook.as_mut().is_some_and(|h| h.fired());

        if !(by_registration || by_hook) {
            return false;
        }

        if !press_coalesce::accept(self.last_press.map(|at| at.elapsed())) {
            self.log
                .debug("the same press reported twice. Ignored.", &[]);

            return false;
        }

        if !accepts_hotkey(&self.tray_state) {
            self.last_press = Some(Instant::now());

            self.log.info(
                "hotkey ignored",
                &[
                    ("paused", Value::Bool(self.tray_state.paused)),
                    ("game_found", Value::Bool(self.tray_state.game_found)),
                    ("stats", Value::Int(self.tray_state.stat_count as i64)),
                ],
            );

            return false;
        }

        true
    }

    fn drain_hotkeys(&mut self) {
        if let Some(h) = self.hotkeys.as_mut() {
            h.fired();
        }

        if let Some(h) = self.hook.as_mut() {
            h.fired();
        }

        self.last_press = Some(Instant::now());
    }

    fn run_price_check(&mut self) {
        self.log.info("price check hotkey pressed", &[]);

        let cursor = self.window.cursor();

        let Self {
            model,
            clipboard,
            trigger,
            timing,
            settings,
            data,
            options,
            runtime,
            prices,
            log,
            last_search,
            ..
        } = self;

        let timing = *timing;
        let options = *options;
        let restore = settings.restore_clipboard;

        let outcome = price_check_loop::run(
            model,
            cursor,
            || {
                copy_item(clipboard, trigger, timing, restore, std::thread::sleep)
                    .map_err(|e| render(&e))
            },
            |text| price_check(text, data, options).map_err(|e| render(&e)),
            |checked| {
                let (result, exchange) = runtime
                    .block_on(prices.search_checked(checked))
                    .map_err(|e| render(&e))
                    .inspect_err(|message| {
                        log.error(
                            "searching the trade site",
                            &[("error", Value::Str(message.clone()))],
                        );
                    })?;

                let total = result.total;

                *last_search = Some(SearchOutcome {
                    total,
                    id: result.id,
                    exchange,
                });

                Ok(total)
            },
        );

        match outcome {
            price_check_loop::Outcome::Priced { total } => self.log.info(
                "price check finished",
                &[("listings", Value::Int(total as i64))],
            ),
            other => self.log.warn(
                "price check did not produce a price",
                &[("outcome", Value::Str(format!("{other:?}")))],
            ),
        }

        self.drain_hotkeys();
    }

    fn refresh_tray(&mut self, found: bool) {
        self.tray_state.game_found = found;
        self.tray_state.has_search = self.model.result().is_some();

        if let Some(tray) = &self.tray {
            tray.update(self.tray_state.clone());
        }
    }

    fn report_window_change(
        &mut self,
        visible: bool,
        found: Option<crate::adapter::game_window_adapter::GameWindow>,
    ) {
        if visible == self.window_was_found {
            return;
        }

        self.window_was_found = visible;

        match found {
            Some(game) => self.log.info(
                "the game window appeared",
                &[
                    ("width", Value::Int(i64::from(game.rect.width))),
                    ("height", Value::Int(i64::from(game.rect.height))),
                ],
            ),
            None => self.log.warn(
                "the game window is gone. The overlay stays hidden until it is back.",
                &[(
                    "window_title",
                    Value::Str(self.settings.window_title.clone()),
                )],
            ),
        }
    }

    fn report_panel_change(
        &mut self,
        frame: &Frame,
        found: Option<crate::adapter::game_window_adapter::GameWindow>,
    ) {
        let showing = should_paint(frame);

        if showing == self.was_showing {
            return;
        }

        self.was_showing = showing;

        self.log.info(
            if showing {
                "the panel is on screen"
            } else {
                "the panel is not being drawn"
            },
            &[
                ("state", Value::Str(format!("{:?}", frame.state))),
                (
                    "game_foreground",
                    Value::Bool(found.is_some_and(|w| w.is_foreground)),
                ),
                ("game_found", Value::Bool(found.is_some())),
                ("takes_input", Value::Bool(frame.takes_input)),
                (
                    "rect",
                    Value::Str(frame.rect.map_or_else(
                        || "none".to_string(),
                        |r| format!("{}x{} at {},{}", r.width, r.height, r.x, r.y),
                    )),
                ),
                ("scale", Value::Str(format!("{:.2}", self.window.scale()))),
            ],
        );

        self.probe_due = showing;
    }

    fn probe_panel(&self) {
        let measured =
            match window_probe_adapter::measure(PANEL_WINDOW_TITLE, &self.settings.window_title) {
                Ok(measured) => measured,
                Err(err) => {
                    self.log.debug(
                        "could not measure the panel window",
                        &[("error", Value::Str(err.to_string()))],
                    );

                    return;
                }
            };

        let verdict = visibility(measured);

        let fields = [
            ("verdict", Value::Str(format!("{verdict:?}"))),
            ("landed", Value::Str(rect_text(measured.window))),
            ("desktop", Value::Str(rect_text(measured.desktop))),
            ("above_game", Value::Bool(measured.above_game)),
        ];

        match explain(verdict) {
            None => self.log.info("the panel is where it should be", &fields),
            Some(why) => self.log.error(why, &fields),
        }
    }

    fn apply_placement(&self, ctx: &egui::Context, frame: &Frame) {
        let placement = overlay_placement::placement(
            frame.rect.map(|r| overlay_placement::Rect {
                x: r.x,
                y: r.y,
                width: r.width,
                height: r.height,
            }),
            frame.takes_input,
            self.window.scale(),
        );

        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            placement.x,
            placement.y,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            placement.width,
            placement.height,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(placement.visible));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            egui::WindowLevel::AlwaysOnTop,
        ));
        ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(
            placement.passthrough,
        ));
    }
}

impl Drop for OverlayLoopDriver {
    fn drop(&mut self) {
        if let Some(hook) = self.hook.take() {
            hook.stop();
        }
    }
}

fn rect_text(rect: poe_trader_core::controller::panel_visible::Rect) -> String {
    format!("{}x{} at {},{}", rect.width, rect.height, rect.x, rect.y)
}

fn report_window(window: &GameWindowAdapter, log: &Logger) {
    match window.find() {
        Ok(found) => log.info(
            "found the game window",
            &[
                ("width", Value::Int(i64::from(found.rect.width))),
                ("height", Value::Int(i64::from(found.rect.height))),
                ("foreground", Value::Bool(found.is_foreground)),
                ("scale", Value::Str(format!("{:.2}", window.scale()))),
            ],
        ),
        Err(err) => log.warn(
            "the game window is not open yet",
            &[("error", Value::Str(err.to_string()))],
        ),
    }
}

fn report_input(log: &Logger) {
    let sent = game_window_adapter::self_test_send_input();

    if sent == 2 {
        log.info(
            "keyboard input works",
            &[("events_accepted", Value::Int(2))],
        );
    } else {
        log.error(
            "keyboard input is not working, a price check will not be able to copy the item",
            &[("events_accepted", Value::Int(i64::from(sent)))],
        );
    }
}

fn start_registration(hotkey: &Hotkey, log: &Logger) -> Option<HotkeyDriver> {
    match HotkeyDriver::start(hotkey) {
        Ok(hotkeys) => {
            log.info(
                "registered the price check hotkey",
                &[("hotkey", Value::Str(hotkey.to_string()))],
            );

            Some(hotkeys)
        }
        Err(err) => {
            log.warn(
                "could not register the price check hotkey. The keyboard hook still watches it.",
                &[("error", Value::Str(err.to_string()))],
            );

            None
        }
    }
}

fn start_hook(hotkey: &Hotkey, log: &Logger) -> Option<HookDriver> {
    let code = crate::driver::hotkey_driver::virtual_key_code(hotkey.key()).unwrap_or(0);

    match HookDriver::start(code, hook_modifiers(hotkey)) {
        Ok(hook) => {
            log.info("watching the hotkey with a keyboard hook as well", &[]);

            Some(hook)
        }
        Err(err) => {
            log.warn(
                "the keyboard hook did not install. The hotkey still works if Windows \
                 delivers the registration.",
                &[("error", Value::Str(err.to_string()))],
            );

            None
        }
    }
}

fn start_tray(
    state: &TrayState,
    game: GameVersion,
    hotkey: &Hotkey,
    log: &Logger,
) -> Option<TrayIcon> {
    match TrayIcon::start(state.clone(), game.as_str(), &hotkey.to_string()) {
        Ok(tray) => {
            log.info("tray icon added", &[]);

            Some(tray)
        }
        Err(err) => {
            log.warn(
                "no tray icon. The overlay still works, but nothing will show it is running.",
                &[("error", Value::Str(err.to_string()))],
            );

            None
        }
    }
}

pub fn hook_modifiers(hotkey: &Hotkey) -> poe_trader_core::controller::hotkey_match::Modifiers {
    use crate::types::Modifier;
    use poe_trader_core::controller::hotkey_match::Modifiers;

    let has = |wanted: Modifier| hotkey.modifiers().contains(&wanted);

    Modifiers {
        ctrl: has(Modifier::Ctrl),
        alt: has(Modifier::Alt),
        shift: has(Modifier::Shift),
        meta: has(Modifier::Meta),
    }
}

fn open_in_browser(url: &str) {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    let target: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

fn rebuild_data(game: GameVersion, data_dir: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("finding this binary: {e}"))?;

    let builder = exe
        .parent()
        .ok_or_else(|| "this binary has no directory".to_string())?
        .join("poe-trader-datagen.exe");

    if !builder.exists() {
        return Err(format!(
            "{} is not next to the overlay",
            builder.file_name().unwrap_or_default().to_string_lossy()
        ));
    }

    std::process::Command::new(&builder)
        .args(["--game", game.as_str(), "--out-dir", data_dir])
        .spawn()
        .map_err(|e| format!("starting {}: {e}", builder.display()))?;

    Ok(())
}
