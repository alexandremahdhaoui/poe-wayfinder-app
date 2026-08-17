use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;

use super::search::SearchOutcome;
use super::{OverlayLoopError, OverlaySettings};

use crate::controller::copy_controller::Copier;
use crate::controller::frame_watch_controller::{FrameWatch, Verdict};
use crate::controller::game_state_controller::GameState;
use crate::controller::gamepad_controller::PadInput;
use crate::controller::input_controller::InputState;
use crate::controller::log_watch_controller::LogSource;
use crate::controller::overlay_controller::{Frame, OverlayModel};
use crate::controller::panel_health_controller::PanelHealth;
use crate::controller::price_check_controller::{Prices, SearchResult};
use crate::controller::price_check_loop;
use crate::controller::session_controller::Session;
use crate::controller::settings_controller::RememberedSettings;
use crate::controller::startup_controller::Press;
use crate::controller::status_controller::Status;
use crate::driver::hook_driver::HookDriver;
use crate::driver::hotkey_driver::HotkeyDriver;
use crate::driver::overlay_placement;
use crate::driver::overlay_ui_driver::{
    drop_splash_background, overlay_viewport, paint, should_paint, splash_window, status_window,
    StatusEvent, UiEvent,
};
use crate::driver::tray_driver::{accepts_hotkey, TrayAction, TrayIcon, TrayState};
use crate::logging::{Logger, Value};
use crate::types::overlay::WindowRect;
use crate::types::Hotkey;
use crate::util::error_chain::render;
use poe_wayfinder_core::controller::hotkey_match::Binding;

use poe_wayfinder_core::adapter::data_adapter::GameData;
use poe_wayfinder_core::adapter::data_adapter::Namespace;
use poe_wayfinder_core::controller::bind_capture::{Binding as BoundRow, Row};
use poe_wayfinder_core::controller::filter::augments::augment_name;
use poe_wayfinder_core::controller::game_detect;
use poe_wayfinder_core::controller::gamepad_match::{self, ControllerStatus};
use poe_wayfinder_core::controller::gamepad_nav;
use poe_wayfinder_core::controller::league_list::LeagueFrom;
use poe_wayfinder_core::controller::overlay_lifecycle::{
    Input, Lifecycle, Point, Rect as LifeRect,
};
use poe_wayfinder_core::controller::pad_focus::{self, AutoRepeat, Focus, PadEdit};
use poe_wayfinder_core::controller::press_coalesce;
use poe_wayfinder_core::controller::price_check::{
    price_check, price_check_item, PriceCheck, PriceCheckOptions,
};
use poe_wayfinder_core::controller::switching::{self, Chosen, GameChoice, LeagueChoice};
use poe_wayfinder_core::types::item::{ItemRarity, ParsedItem};
use poe_wayfinder_core::types::{GamePair, GameVersion};

const PANEL_WINDOW_TITLE: &str = "poe-wayfinder";
const FRAME_INTERVAL: Duration = Duration::from_millis(100);
const HEARTBEAT_FRAMES: i64 = 600;
const STALL_LOOK_EVERY: Duration = Duration::from_millis(1000);
const GAME_CHECK_EVERY: Duration = Duration::from_millis(1000);
const SPLASH_HOLD: Duration = Duration::from_millis(2000);
const SPLASH_FADE: Duration = Duration::from_millis(400);

pub struct OverlayLoopDriver<W, C, P, H, D, I, L, R, G>
where
    W: GameState + 'static,
    I: InputState + 'static,
    C: Copier + 'static,
    P: Prices + 'static,
    H: PanelHealth + 'static,
    D: GameData + 'static,
    L: LogSource + 'static,
    R: RememberedSettings + 'static,
    G: PadInput + 'static,
{
    window: W,
    copier: C,
    health: H,
    input: I,
    logs: L,
    remembered: R,
    gamepad: G,
    pad_held: u16,
    pad_focus: Focus,
    pad_repeat: AutoRepeat,
    session: Session,
    life: Lifecycle,
    last_tick: Instant,
    hotkeys: Option<HotkeyDriver>,
    hook: Option<HookDriver>,
    tray: Option<TrayIcon>,
    tray_state: TrayState,
    model: OverlayModel,
    runtime: tokio::runtime::Runtime,
    prices: P,
    log: Logger,
    settings: OverlaySettings,
    game: GameVersion,
    data: GamePair<D>,
    options: PriceCheckOptions,
    frames: i64,
    started: bool,
    last_press: Option<Instant>,
    window_was_found: bool,
    was_showing: bool,
    probe_due: bool,
    last_game_check: Instant,
    status_open: bool,
    locked_focused: bool,
    widgets: crate::controller::widgets_controller::Widgets,
    roles: crate::controller::startup_controller::Validated,
    awaiting_search: Option<Box<poe_wayfinder_core::controller::price_check::PriceCheck>>,
    splash_since: Option<Instant>,
    splash_keyed: bool,
    hotkey_text: String,
    refresh: super::wiring::RefreshPlan,
    last_search: Option<SearchOutcome>,
    pending: Vec<UiEvent>,
    hold_open: bool,
    ticked_at: Arc<AtomicU64>,
}

impl<W, C, P, H, D, I, L, R, G> OverlayLoopDriver<W, C, P, H, D, I, L, R, G>
where
    W: GameState + 'static,
    C: Copier + 'static,
    P: Prices + 'static,
    H: PanelHealth + 'static,
    D: GameData + 'static,
    I: InputState + 'static,
    L: LogSource + 'static,
    R: RememberedSettings + 'static,
    G: PadInput + 'static,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        settings: OverlaySettings,
        game: GameVersion,
        data: GamePair<D>,
        stats: usize,
        mut roles: crate::controller::startup_controller::Validated,
        window: W,
        copier: C,
        prices: P,
        health: H,
        input: I,
        logs: L,
        remembered: R,
        mut gamepad: G,
        hold: poe_wayfinder_core::controller::overlay_lifecycle::HoldKey,
        refresh: super::wiring::RefreshPlan,
        log: Logger,
    ) -> Result<Self, OverlayLoopError> {
        report_window(&window, &log);

        if let Some(bound) = remembered
            .bound_hotkey()
            .and_then(|held| Hotkey::parse(&held).ok())
        {
            log.info(
                "a price check key you bound is used instead of the configured one",
                &[
                    ("bound", Value::Str(bound.to_string())),
                    ("configured", Value::Str(roles.hotkey.to_string())),
                ],
            );

            roles.hotkey = bound;
        }

        if let Some(bound) = remembered.bound_chord() {
            log.info(
                "a pad chord you bound is used instead of the configured one",
                &[("bound", Value::Str(bound.clone()))],
            );

            gamepad.rebind(gamepad_match::parse_chord(&bound));
        }

        let hotkeys_wanted = roles.every_hotkey();

        let Some(hotkey) = hotkeys_wanted.first().cloned() else {
            return Err(OverlayLoopError::Window {
                message: "no price check hotkey is configured".to_string(),
            });
        };

        let hotkeys = start_registration(&hotkey, &log);
        let hook = start_hook(&hotkeys_wanted, &log);

        let tray_state = TrayState {
            game_found: window.window().is_some(),
            paused: false,
            has_search: false,
            league: Some(settings.league.clone()),
            stat_count: stats,
        };

        let tray = start_tray(&tray_state, game, &hotkey, &log);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|source| OverlayLoopError::Runtime { source })?;

        let mut model = OverlayModel::new(super::wiring::build_geometry());

        model.start(window.cursor());
        model.fail(&format!(
            "Ready. Press {hotkey} with the cursor over an item."
        ));

        let mut widgets = opened_widgets(&remembered);

        widgets.bound_hotkey = remembered
            .bound_hotkey()
            .unwrap_or_else(|| hotkey.to_string());
        widgets.bound_chord = remembered
            .bound_chord()
            .unwrap_or_else(|| settings.gamepad_chord.clone());
        let window_was_found = window.window().is_some();
        let ticked_at = watch_for_a_stall(&settings.log_level);

        Ok(Self {
            window,
            copier,
            health,
            input,
            logs,
            remembered,
            gamepad,
            pad_held: 0,
            pad_focus: Focus::default(),
            pad_repeat: AutoRepeat::default(),
            session: Session::from_config(&settings.league),
            life: Lifecycle::new(hold),
            last_tick: Instant::now(),
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
            frames: 0,
            started: false,
            last_press: None,
            window_was_found,
            was_showing: false,
            probe_due: false,
            last_game_check: Instant::now(),
            status_open: false,
            locked_focused: false,
            widgets,
            roles,
            awaiting_search: None,
            splash_since: Some(Instant::now()),
            splash_keyed: false,
            hotkey_text: hotkey.to_string(),
            refresh,
            last_search: None,
            pending: Vec::new(),
            hold_open: super::wiring::panel_hold(),
            ticked_at,
        })
    }

    pub fn run(self) -> Result<(), OverlayLoopError> {
        let first = self
            .model
            .frame_scaled(self.window.window(), self.window.scale());

        let native_options = eframe::NativeOptions {
            viewport: overlay_viewport(&first),
            ..eframe::NativeOptions::default()
        };

        let result = eframe::run_native(
            PANEL_WINDOW_TITLE,
            native_options,
            Box::new(move |_cc| Ok(Box::new(self))),
        );

        result.map_err(|err| OverlayLoopError::Window {
            message: err.to_string(),
        })
    }

    fn frame(&mut self, ctx: &egui::Context) {
        self.ticked_at
            .store(crate::util::elapsed::millis() as u64, Ordering::Relaxed);

        self.advance_search();
        self.heartbeat();

        if self.probe_due {
            self.probe_due = false;
            self.probe_panel();
        }

        let asked = self.collect_actions();
        self.dispatch(asked, ctx);

        match self.read_hotkey() {
            Some(Press::Check) => self.run_price_check(false),
            Some(Press::Locked) => self.run_price_check(true),
            Some(Press::ToggleOverlay) => self.toggle_overlay(),
            Some(Press::Command { index }) => self.send_command(index),
            Some(Press::StashSearch { index }) => self.send_stash_search(index),
            Some(Press::OpenLink { site }) => self.open_link(site),
            None => {}
        }

        if self.read_gamepad() {
            match self.life.is_drawn() {
                true => self.close_from_pad(),
                false => self.run_price_check(true),
            }
        }

        self.navigate_from_pad(FRAME_INTERVAL);

        self.follow_game();
        self.watch_logs();
        self.draw_splash(ctx);
        self.draw_status(ctx);

        let found = self.window.window().map(|mut window| {
            window.is_foreground = window.is_foreground || self.hold_open;

            window
        });

        let game_foreground = found.map(|window| window.is_foreground);

        self.refresh_tray(found.is_some());
        self.report_window_change(found.map(|window| window.rect));

        let mut frame = self.model.frame_scaled(found, self.window.scale());

        self.tick_lifecycle(&frame);

        if !self.life.is_drawn() {
            frame.rect = None;
        }

        frame.takes_input = self.life.takes_input();

        self.focus_locked_panel(ctx, &frame);

        self.report_panel_change(&frame, game_foreground);
        self.apply_placement(ctx, &frame);

        let pad_view = crate::driver::overlay_ui_driver::PadView {
            focus: self.pad_focus,
            connected: self.gamepad.connected(),
        };

        self.pending
            .extend(paint(ctx, &self.model, Some(&pad_view)));

        ctx.request_repaint_after(FRAME_INTERVAL);
    }

    fn tick_lifecycle(&mut self, frame: &Frame) {
        if let Some(rect) = frame.rect {
            self.life.ready(LifeRect {
                x: rect.x,
                y: rect.y,
                width: rect.width as i32,
                height: rect.height as i32,
            });
        }

        let (x, y) = self.window.cursor();
        let elapsed = self.last_tick.elapsed().as_millis() as u64;

        self.last_tick = Instant::now();

        let before = self.life.phase();

        let pointer = match self.hold_open {
            true => self.life.origin(),
            false => Point { x, y },
        };

        self.life.tick(Input {
            pointer,
            hold_down: self.input.hold_down(),
            alt_alone: !self.hold_open && self.input.alt_alone(),
            clicked: !self.hold_open && self.input.mouse_down(),
            elapsed_ms: elapsed,
        });

        let after = self.life.phase();

        if before != after {
            self.log.info(
                "the panel lifecycle moved",
                &[
                    ("from", Value::Str(format!("{before:?}"))),
                    ("to", Value::Str(format!("{after:?}"))),
                    ("pointer", Value::Str(format!("{x},{y}"))),
                ],
            );
        }
    }

    fn watch_logs(&mut self) {
        let Ok(events) = self.logs.poll() else {
            return;
        };

        for event in &events {
            if let Some(happening) = super::wiring::as_happening(event) {
                self.widgets.note_happening(&happening);
            }
        }

        if events.is_empty() {
            return;
        }

        let changed = self.session.apply_all(&events);

        self.log.debug(
            "the game log said something",
            &[
                ("events", Value::Int(events.len() as i64)),
                ("changed_the_session", Value::Bool(changed)),
                (
                    "league_in_the_log",
                    Value::Str(self.session.league().unwrap_or("none").to_string()),
                ),
                (
                    "league_source",
                    Value::Str(format!("{:?}", self.session.league_source())),
                ),
                ("league_in_use", Value::Str(self.settings.league.clone())),
            ],
        );

        if !changed {
            return;
        }

        let Some(league) = self.session.league().map(str::to_string) else {
            return;
        };

        if league == self.settings.league {
            return;
        }

        if *self.settings.league_choice.get(self.game) != LeagueChoice::Automatic {
            self.log.info(
                "the game log named a league but this game's league was chosen by hand, so \
                 the search keeps the chosen one",
                &[
                    ("in_the_log", Value::Str(league)),
                    ("league_in_use", Value::Str(self.settings.league.clone())),
                    ("game", Value::Str(self.game.as_str().to_string())),
                ],
            );

            return;
        }

        self.log.info(
            "the game log says the league changed",
            &[
                ("was", Value::Str(self.settings.league.clone())),
                ("now", Value::Str(league.clone())),
            ],
        );

        self.set_league(Chosen {
            name: league,
            from: LeagueFrom::GameLog,
        });
    }

    fn set_league(&mut self, chosen: Chosen) {
        if chosen.name.trim().is_empty() {
            return;
        }

        let was = std::mem::replace(&mut self.settings.league, chosen.name.clone());

        self.settings.league_from = chosen.from;
        self.tray_state.league = Some(chosen.name.clone());

        self.prices.set_league(&chosen.name);
        self.remembered.remember_league(self.game, &chosen.name);

        if was == chosen.name {
            return;
        }

        self.widgets
            .note_log(&format!("league is now {}", chosen.name));

        self.log.info(
            "the league changed",
            &[
                ("was", Value::Str(was)),
                ("now", Value::Str(chosen.name)),
                ("source", Value::Str(chosen.from.as_str().to_string())),
                ("game", Value::Str(self.game.as_str().to_string())),
                ("endpoint", Value::Str(self.prices.search_endpoint())),
            ],
        );
    }

    fn choose_league(&mut self, chosen: LeagueChoice) {
        let game = self.game;
        let known = self.settings.known_leagues.get(game).clone();

        let keep = self
            .remembered
            .last_league(game)
            .unwrap_or_else(|| self.settings.league.clone());

        let now = switching::league_now(&chosen, &known, &keep);
        let pinned = matches!(chosen, LeagueChoice::Named(_));

        *self.settings.league_choice.get_mut(game) = chosen;

        self.remembered.pin_league(game, pinned);

        self.log.info(
            "a league was chosen in the panel",
            &[
                ("league", Value::Str(now.name.clone())),
                ("source", Value::Str(now.from.as_str().to_string())),
                ("pinned", Value::Bool(pinned)),
                ("game", Value::Str(game.as_str().to_string())),
                ("known_leagues", Value::Int(known.len() as i64)),
            ],
        );

        self.set_league(now);
    }

    fn choose_game(&mut self, chosen: GameChoice) {
        let detected = self.window.detect_game();
        let wanted = switching::game_now(chosen, detected, self.game);

        self.settings.pinned_game = match chosen {
            GameChoice::Pinned(game) => Some(game),
            GameChoice::Automatic => None,
        };

        self.log.info(
            "a game was chosen in the panel",
            &[
                ("pinned", Value::Bool(self.settings.pinned_game.is_some())),
                ("watching", Value::Str(wanted.as_str().to_string())),
                (
                    "detected",
                    Value::Str(
                        detected
                            .map(|game| game.as_str())
                            .unwrap_or("none")
                            .to_string(),
                    ),
                ),
            ],
        );

        if wanted != self.game {
            self.switch_game(wanted);
        }
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
            self.log.debug(
                "the panel asked for something",
                &[
                    ("event", Value::Str(format!("{event:?}"))),
                    (
                        "enabled_filters",
                        Value::Int(self.model.filters().enabled_count() as i64),
                    ),
                ],
            );

            match event {
                UiEvent::SearchStash(text) => self.search_the_stash_for(&text),
                UiEvent::OpenInBrowser => asked.push(TrayAction::OpenInBrowser),
                UiEvent::Research => asked.push(TrayAction::Research),
                UiEvent::Dismiss => {
                    self.model.hide();
                    self.life.dismiss();
                }
                UiEvent::ToggleRow(key) => {
                    let enabled = self
                        .model
                        .filters()
                        .numerics
                        .iter()
                        .chain(self.model.filters().stats.iter())
                        .find(|row| row.key == key)
                        .map(|row| row.enabled)
                        .unwrap_or(false);

                    self.model.set_enabled(key, !enabled);

                    let now = self
                        .model
                        .filters()
                        .numerics
                        .iter()
                        .chain(self.model.filters().stats.iter())
                        .filter(|row| row.key == key)
                        .map(|row| row.enabled)
                        .collect::<Vec<_>>();

                    self.log.debug(
                        "a filter row was toggled",
                        &[
                            ("key", Value::Str(format!("{key:?}"))),
                            ("was", Value::Bool(enabled)),
                            ("asked_for", Value::Bool(!enabled)),
                            ("rows_with_this_key", Value::Int(now.len() as i64)),
                            ("now", Value::Str(format!("{now:?}"))),
                        ],
                    );
                }
                UiEvent::SetMin(key, value) => {
                    self.log.debug(
                        "a filter minimum was set, which also enables the row",
                        &[
                            ("key", Value::Str(format!("{key:?}"))),
                            ("value", Value::Str(describe(value))),
                        ],
                    );

                    self.model.set_min(key, value);
                }
                UiEvent::SetMax(key, value) => {
                    self.log.debug(
                        "a filter maximum was set, which also enables the row",
                        &[
                            ("key", Value::Str(format!("{key:?}"))),
                            ("value", Value::Str(describe(value))),
                        ],
                    );

                    self.model.set_max(key, value);
                }
                UiEvent::ToggleFlag(key) => {
                    let flag = self
                        .model
                        .filters()
                        .flags
                        .iter()
                        .find(|row| row.key == key)
                        .map(|row| (row.enabled, row.value))
                        .unwrap_or((false, true));

                    self.model.set_flag(key, !flag.0, flag.1);
                }
                UiEvent::InvertFlag(key) => {
                    let flag = self
                        .model
                        .filters()
                        .flags
                        .iter()
                        .find(|row| row.key == key)
                        .map(|row| row.value)
                        .unwrap_or(true);

                    self.model.set_flag(key, true, !flag);
                }
                UiEvent::SetAllStats(enabled) => self.model.set_all_stats(enabled),
                UiEvent::CycleName => self.model.cycle_name(),
                UiEvent::ToggleOnline => self.model.toggle_online(),
                UiEvent::ChooseAugment(reference) => {
                    let augments: Vec<_> = self.data.get(self.game).augments().to_vec();

                    if self.model.choose_augment(&reference, &augments) {
                        self.log.info(
                            "an augment was socketed into the item",
                            &[
                                (
                                    "augment",
                                    Value::Str(augment_name(&augments, &reference).to_string()),
                                ),
                                ("reference", Value::Str(reference)),
                            ],
                        );
                    }
                }
                UiEvent::ClearAugment => {
                    self.model.clear_augment();
                    self.log.info("the augment was taken back off", &[]);
                }
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

                TrayAction::OpenStatus => self.show_status(true),
            }
        }
    }

    fn draw_splash(&mut self, ctx: &egui::Context) {
        let Some(since) = self.splash_since else {
            return;
        };

        let elapsed = since.elapsed();

        let fade = match elapsed.checked_sub(SPLASH_HOLD) {
            None => 1.0,
            Some(gone) => 1.0 - (gone.as_secs_f32() / SPLASH_FADE.as_secs_f32()).min(1.0),
        };

        let skipped = splash_window(ctx, fade);

        if !self.splash_keyed {
            self.splash_keyed = drop_splash_background();
        }

        if skipped || fade <= 0.0 {
            self.splash_since = None;
            self.show_status(true);

            self.log
                .info("the splash is done", &[("skipped", Value::Bool(skipped))]);
        }

        ctx.request_repaint();
    }

    fn show_status(&mut self, open: bool) {
        if self.status_open == open {
            return;
        }

        self.status_open = open;

        self.log.info(
            match open {
                true => "the status window is open",
                false => "the status window is hidden. The tray icon brings it back.",
            },
            &[],
        );
    }

    fn status(&mut self) -> Status {
        let known = self.settings.known_leagues.get(self.game).clone();
        let chosen = self.settings.league_choice.get(self.game).clone();
        let remembered = self.remembered.last_league(self.game);

        let league_menu = switching::league_menu(switching::Known {
            fetched: &known,
            remembered: remembered.as_deref(),
            in_use: &self.settings.league,
            chosen: &chosen,
        });

        let game_menu = switching::game_menu(self.settings.pinned_game);

        Status {
            game: match self.window.window().is_some() {
                true => Some(self.game),
                false => None,
            },
            pinned: self.settings.pinned_game.is_some(),
            window_title: self.settings.window_title.clone(),
            hotkey: self.hotkey_text.clone(),
            league: self.settings.league.clone(),
            league_source: self.settings.league_from,
            league_menu,
            game_menu,
            origin: self.settings.data_origin.clone(),
            stats: self.data.get(self.game).stat_count(),
            items: self.data.get(self.game).item_name_count(),
            augments: self.data.get(self.game).augments().len(),
            last_refresh: self.refresh.last_refresh(self.game),
            paused: self.tray_state.paused,
            network: self.settings.network,
            limits: self.prices.limiter_report(),
            note: self.prices.pacing_note(),
            client_log_found: self.settings.client_log_found,
            pad_held: self.gamepad.held(),
            pad_family: self.gamepad.family(),
            controller: gamepad_match::controller_caption(&ControllerStatus {
                chord: self.gamepad.chord(),
                connected: self.gamepad.connected(),
                family: self.gamepad.family(),
            }),
        }
    }

    fn draw_status(&mut self, ctx: &egui::Context) {
        if !self.status_open {
            return;
        }

        let status = self.status();

        let names = self.data.get(self.game).every_name();
        let bindings = self.bindings_shown();
        let now_ms = crate::util::elapsed::millis() as u64;

        for event in status_window(
            ctx,
            &status,
            std::time::SystemTime::now(),
            &mut self.widgets,
            &names,
            &bindings,
            now_ms,
        ) {
            match event {
                StatusEvent::MarkMap { matcher, set } => {
                    self.remembered.remember_verdict(&matcher, &set);
                }
                StatusEvent::HideToTray => self.show_status(false),
                StatusEvent::Quit => {
                    self.log.info("quit chosen from the status window", &[]);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                StatusEvent::Bound(bound) => self.apply_binding(bound),
                StatusEvent::CopyCsv(csv) => {
                    let put = self.copier.put(&csv).is_ok();

                    self.log.info(
                        "the priced items were copied as csv",
                        &[
                            ("rows", Value::Int(csv.lines().count() as i64)),
                            ("on_the_clipboard", Value::Bool(put)),
                        ],
                    );
                }
                StatusEvent::PriceByName(name) => self.price_a_base_by_name(&name),
                StatusEvent::ForgetOutdatedMaps => {
                    let dropped = self.widgets.forget_outdated();

                    for matcher in &dropped {
                        self.remembered.forget_verdict(matcher);
                    }

                    self.log.info(
                        "marked map mods the data no longer has were forgotten",
                        &[("count", Value::Int(dropped.len() as i64))],
                    );
                }
                StatusEvent::RefreshNow => self.rebuild_data(),
                StatusEvent::TogglePaused => {
                    self.tray_state.paused = !self.tray_state.paused;
                }
                StatusEvent::ChooseLeague(chosen) => self.choose_league(chosen),
                StatusEvent::ChooseGame(chosen) => self.choose_game(chosen),
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
        let Some(checked) = self.model.edited_check() else {
            self.model.warn("Nothing to search again yet.");

            return;
        };

        self.log.info(
            "searching again",
            &[(
                "filters",
                Value::Int(self.model.filters().enabled_count() as i64),
            )],
        );

        match self.search_for(&checked) {
            Ok(total) => {
                self.model.finish(checked, total);
                self.after_search();
            }
            Err(message) => self.model.warn(&message),
        }
    }

    fn after_search(&mut self) {
        let augments: Vec<_> = self.data.get(self.game).augments().to_vec();

        self.model.offer_augments(&augments);
        self.model.set_limits(self.prices.limiter_report());

        if let Some(note) = self.prices.pacing_note() {
            self.model.note(&note);
        }

        let Some(outcome) = self.last_search.clone() else {
            return;
        };

        let result = SearchResult {
            id: outcome.id.clone(),
            result: outcome.ids.clone(),
            total: outcome.total,
        };

        let listings = self
            .runtime
            .block_on(self.prices.listings_for(&result, outcome.exchange));

        match listings {
            Ok(listings) => {
                self.log.info(
                    "read the listings",
                    &[("listings", Value::Int(listings.len() as i64))],
                );

                self.model.set_listings(listings);
            }
            Err(err) => self.log.warn(
                "the listings could not be read, so no price is offered",
                &[("error", Value::Str(render(&err)))],
            ),
        }

        self.model.set_limits(self.prices.limiter_report());

        self.log.debug(
            "rate limit pools",
            &[
                ("pools", Value::Str(self.prices.limiter_names().join(","))),
                (
                    "burst_millis",
                    Value::Int(self.prices.burst_hint(3).as_millis() as i64),
                ),
            ],
        );
    }

    fn price_a_base_by_name(&mut self, name: &str) {
        let cursor = self.window.cursor();
        let Self {
            model,
            data,
            game,
            options,
            log,
            ..
        } = self;

        let options = options.clone();
        let tables = data.get(*game);

        let stage = price_check_loop::prepare(
            model,
            cursor,
            || Ok(name.to_string()),
            |text| {
                let found = [Namespace::Unique, Namespace::Item, Namespace::Gem]
                    .into_iter()
                    .find_map(|namespace| {
                        tables
                            .items_by_name(text, namespace, *game)
                            .into_iter()
                            .next()
                            .map(|info| (namespace, info))
                    });

                let Some((namespace, info)) = found else {
                    return Err(format!("no base is named {text:?}"));
                };

                let mut item = ParsedItem::virtual_item(info);
                item.rarity = Some(match namespace {
                    Namespace::Unique => ItemRarity::Unique,
                    _ => ItemRarity::Normal,
                });

                let parsed = Ok(price_check_item(item, tables, &options));

                log_parsed(log, &parsed);

                parsed
            },
        );

        let price_check_loop::Stage::Ready(checked) = stage else {
            self.log.warn(
                "pricing a base by name stopped before it could search",
                &[("name", Value::Str(name.to_string()))],
            );

            return;
        };

        self.log.info(
            "a base is priced with no item in hand",
            &[
                ("name", Value::Str(name.to_string())),
                (
                    "stat_rows",
                    Value::Int(self.model.filters().stats.len() as i64),
                ),
            ],
        );

        self.awaiting_search = Some(checked);
    }

    fn rebuild_data(&mut self) {
        self.log.info(
            "refreshing the game data on request",
            &[("game", Value::Str(self.game.as_str().to_string()))],
        );

        self.refresh.forget(self.game);
        self.refresh.start(vec![self.game], &self.log);

        self.model
            .warn("Refreshing the game data. It is used from the next launch.");
    }

    fn search_for(
        &mut self,
        checked: &poe_wayfinder_core::controller::price_check::PriceCheck,
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
            ids: result.result,
        });

        Ok(total)
    }

    fn read_hotkey(&mut self) -> Option<Press> {
        let by_registration = self.hotkeys.as_mut().is_some_and(|h| h.fired());
        let by_hook = self.hook.as_mut().and_then(|h| h.fired());

        let press = match (by_registration, by_hook) {
            (_, Some(index)) => self.roles.role_of(index),
            (true, None) => Press::Check,
            (false, None) => return None,
        };

        if !press_coalesce::accept(self.last_press.map(|at| at.elapsed())) {
            self.log
                .debug("the same press reported twice. Ignored.", &[]);

            return None;
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

            return None;
        }

        Some(press)
    }

    fn read_gamepad(&mut self) -> bool {
        if !self.gamepad.fired() {
            return false;
        }

        if !accepts_hotkey(&self.tray_state) {
            self.log.info(
                "controller chord ignored",
                &[
                    ("paused", Value::Bool(self.tray_state.paused)),
                    ("game_found", Value::Bool(self.tray_state.game_found)),
                ],
            );

            return false;
        }

        self.last_press = Some(Instant::now());

        self.log.info(
            "controller chord held",
            &[(
                "chord",
                Value::Str(match self.gamepad.chord() {
                    Some(chord) => gamepad_match::describe_for(self.gamepad.family(), chord.mask),
                    None => String::new(),
                }),
            )],
        );

        true
    }

    fn navigate_from_pad(&mut self, since_last: Duration) {
        let now = self.gamepad.held();
        let repeated = self.pad_repeat.tick(now, since_last);
        let pressed = gamepad_nav::newly_pressed(self.pad_held, now) | repeated;

        self.pad_held = now;

        if pressed == 0 || !self.life.is_drawn() {
            return;
        }

        let rows = self.pad_rows();

        for edit in pad_focus::react(&mut self.pad_focus, pressed, rows) {
            self.apply_pad_edit(edit);
        }

        self.log.debug(
            "the pad moved in the panel",
            &[
                ("row", Value::Int(self.pad_focus.row as i64)),
                ("column", Value::Str(format!("{:?}", self.pad_focus.column))),
                ("editing", Value::Bool(self.pad_focus.editing)),
            ],
        );
    }

    fn pad_rows(&self) -> usize {
        let filters = self.model.filters();

        filters.numerics.len() + filters.stats.len()
    }

    fn pad_row_key(&self) -> Option<poe_wayfinder_core::controller::filter_view::RowKey> {
        let filters = self.model.filters();

        filters
            .numerics
            .iter()
            .chain(filters.stats.iter())
            .nth(self.pad_focus.row)
            .map(|row| row.key)
    }

    fn pad_row_value(&self, column: pad_focus::Column) -> f64 {
        let filters = self.model.filters();

        let Some(row) = filters
            .numerics
            .iter()
            .chain(filters.stats.iter())
            .nth(self.pad_focus.row)
        else {
            return 0.0;
        };

        let held = match column {
            pad_focus::Column::Max => row.max.or(row.bounds.map(|(_, high)| high)),
            _ => row.min.or(row.bounds.map(|(low, _)| low)),
        };

        held.filter(|value| value.is_finite())
            .or(row.roll)
            .unwrap_or(0.0)
    }

    fn apply_pad_edit(&mut self, edit: PadEdit) {
        let Some(key) = self.pad_row_key() else {
            return;
        };

        let event = match edit {
            PadEdit::Close => {
                self.close_from_pad();

                return;
            }
            PadEdit::Search => UiEvent::Research,
            PadEdit::Toggle => UiEvent::ToggleRow(key),
            PadEdit::AdjustMin(step) => {
                let value = self.pad_row_value(pad_focus::Column::Min) + step;

                UiEvent::SetMin(key, Some(value))
            }
            PadEdit::AdjustMax(step) => {
                let value = self.pad_row_value(pad_focus::Column::Max) + step;

                UiEvent::SetMax(key, Some(value))
            }
        };

        self.log.info(
            "the pad changed the search",
            &[("edit", Value::Str(format!("{edit:?}")))],
        );

        self.pending.push(event);
    }

    fn apply_binding(&mut self, bound: BoundRow) {
        match bound.row {
            Row::Keyboard => self.rebind_key(&bound.text),
            Row::Pad => self.rebind_pad(&bound.text),
        }
    }

    fn rebind_key(&mut self, text: &str) {
        let Ok(hotkey) = Hotkey::parse(text) else {
            self.log.warn(
                "that key cannot be bound",
                &[("key", Value::Str(text.to_string()))],
            );

            return;
        };

        let previous = self.roles.hotkey.clone();

        self.roles.hotkey = hotkey.clone();

        let Some(fresh) = start_hook(&self.roles.every_hotkey(), &self.log) else {
            self.roles.hotkey = previous;

            self.log.error(
                "the new key could not be hooked, so the old one is kept and nothing changed",
                &[("key", Value::Str(text.to_string()))],
            );

            return;
        };

        if let Some(old) = self.hook.replace(fresh) {
            old.stop();
        }

        if let Some(registered) = start_registration(&hotkey, &self.log) {
            self.hotkeys = Some(registered);
        }

        self.hotkey_text = hotkey.to_string();
        self.widgets.bound_hotkey = text.to_string();
        self.remembered.remember_hotkey(text);

        self.log.info(
            "the price check key was rebound and is live",
            &[
                ("was", Value::Str(previous.to_string())),
                ("now", Value::Str(hotkey.to_string())),
            ],
        );
    }

    fn rebind_pad(&mut self, text: &str) {
        let chord = gamepad_match::parse_chord(text);

        self.gamepad.rebind(chord);
        self.widgets.bound_chord = text.to_string();
        self.remembered.remember_chord(text);

        self.log.info(
            "the pad chord was rebound and is live",
            &[("chord", Value::Str(text.to_string()))],
        );
    }

    fn close_from_pad(&mut self) {
        self.model.hide();
        self.life.dismiss();
        self.locked_focused = false;

        let handed_back = self.window.hand_back_the_foreground();

        self.log.info(
            "the panel was closed from the pad",
            &[("game_has_the_foreground", Value::Bool(handed_back))],
        );
    }

    fn send_command(&mut self, index: usize) {
        let Some((_, command)) = self.roles.commands.get(index).cloned() else {
            return;
        };

        let action =
            poe_wayfinder_core::controller::chat::type_in_chat(&command.text, command.send);

        let copier = &mut self.copier;

        let sent = super::wiring::send_chat(&action, |text| copier.put(text).is_ok());

        self.log.info(
            match sent {
                true => "sent a chat command",
                false => "the chat command could not be sent",
            },
            &[
                ("text", Value::Str(command.text.clone())),
                ("send", Value::Bool(command.send)),
            ],
        );

        self.drain_hotkeys();
    }

    fn bindings_shown(&self) -> Vec<(String, String)> {
        let mut out = vec![(self.hotkey_text.clone(), "price check".to_string())];

        for locked in &self.roles.locked {
            out.push((locked.to_string(), "price check, stays open".to_string()));
        }

        if let Some(overlay) = &self.roles.overlay {
            out.push((overlay.to_string(), "grab or release the panel".to_string()));
        }

        for (key, command) in &self.roles.commands {
            out.push((key.to_string(), command.text.clone()));
        }

        for (key, preset) in &self.roles.searches {
            out.push((key.to_string(), format!("stash search {}", preset.text)));
        }

        for (key, site) in &self.roles.links {
            out.push((key.to_string(), format!("open the {}", site.as_str())));
        }

        out
    }

    fn record_priced(&mut self, listings: u64) {
        let Some(result) = self.model.result() else {
            return;
        };

        let name = match result.item.info.name.is_empty() {
            true => result.item.info.reference_name.clone(),
            false => result.item.info.name.clone(),
        };

        let estimate = self.model.estimate();

        self.widgets
            .record(poe_wayfinder_core::controller::library::Logged {
                name,
                amount: estimate.as_ref().map(|e| e.median),
                currency: estimate
                    .as_ref()
                    .map(|e| e.currency.clone())
                    .unwrap_or_default(),
                listings,
                at_ms: crate::util::elapsed::millis() as u64,
            });

        self.widgets.check_map(&result.item, 1);

        let tables = self.data.get(self.game);

        self.widgets
            .mark_outdated_verdicts(1, |matcher| tables.stat_by_matcher(matcher).is_some());

        if !self.widgets.outdated().is_empty() {
            self.log.info(
                "some marked map mods are not in the game data any more",
                &[("count", Value::Int(self.widgets.outdated().len() as i64))],
            );
        }
    }

    fn open_link(&mut self, site: poe_wayfinder_core::controller::item_links::Site) {
        let text = match self.copier.copy() {
            Ok(text) => text,
            Err(message) => {
                self.log.warn(
                    "could not copy the item for a reference link",
                    &[("error", Value::Str(message))],
                );

                self.drain_hotkeys();

                return;
            }
        };

        let reference = price_check(&text, self.data.get(self.game), &self.options)
            .ok()
            .map(|checked| checked.item.info.reference_name.clone())
            .unwrap_or_default();

        match poe_wayfinder_core::controller::item_links::url(site, self.game, &reference, &text) {
            Some(url) => {
                self.log.info(
                    "opening a reference site",
                    &[
                        ("site", Value::Str(site.as_str().to_string())),
                        ("url", Value::Str(url.clone())),
                    ],
                );

                open_in_browser(&url);
            }
            None => self.log.warn(
                "there is nothing to look up on that site",
                &[("site", Value::Str(site.as_str().to_string()))],
            ),
        }

        self.drain_hotkeys();
    }

    fn send_stash_search(&mut self, index: usize) {
        let Some((_, preset)) = self.roles.searches.get(index).cloned() else {
            return;
        };

        self.search_the_stash_for(&preset.text);
    }

    fn search_the_stash_for(&mut self, text: &str) {
        let action = poe_wayfinder_core::controller::chat::stash_search(text);
        let copier = &mut self.copier;

        let sent = super::wiring::send_chat(&action, |text| copier.put(text).is_ok());

        self.log.info(
            match sent {
                true => "searched the stash",
                false => "the stash search could not be sent",
            },
            &[("text", Value::Str(text.to_string()))],
        );

        self.drain_hotkeys();
    }

    fn toggle_overlay(&mut self) {
        let grabbed = self.life.toggle_locked();

        self.locked_focused = false;

        self.log.info(
            match grabbed {
                true => "the overlay key grabbed the panel",
                false => "the overlay key released the panel",
            },
            &[],
        );

        self.drain_hotkeys();
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

    fn run_price_check(&mut self, locked: bool) {
        self.log.info(
            "price check hotkey pressed",
            &[
                crate::util::elapsed::field(),
                ("locked", Value::Bool(locked)),
            ],
        );

        let cursor = self.window.cursor();

        let origin = Point {
            x: cursor.0,
            y: cursor.1,
        };

        self.locked_focused = false;

        match locked {
            true => self.life.begin_locked(origin),
            false => self.life.begin(origin),
        }

        let Self {
            model,
            copier,
            data,
            game,
            options,
            log,
            ..
        } = self;

        let options = options.clone();
        let tables = data.get(*game);

        let stage = price_check_loop::prepare(
            model,
            cursor,
            || {
                let copied = copier.copy();

                log_clipboard(log, &copied);

                copied
            },
            |text| {
                let parsed = price_check(text, tables, &options).map_err(|e| render(&e));

                log_parsed(log, &parsed);

                parsed
            },
        );

        let price_check_loop::Stage::Ready(checked) = stage else {
            self.log.warn(
                "the price check stopped before it could search",
                &[
                    ("why", Value::Str(format!("{stage:?}"))),
                    (
                        "message",
                        Value::Str(self.model.message().unwrap_or_default().to_string()),
                    ),
                    crate::util::elapsed::field(),
                ],
            );

            return;
        };

        let view = self.model.filters();

        self.log.info(
            "the panel is up",
            &[
                crate::util::elapsed::field(),
                ("stat_rows", Value::Int(view.stats.len() as i64)),
                ("numeric_rows", Value::Int(view.numerics.len() as i64)),
                ("flag_rows", Value::Int(view.flags.len() as i64)),
            ],
        );

        self.log_filter_rows();

        self.awaiting_search = Some(checked);
    }

    fn advance_search(&mut self) {
        let Some(checked) = self.awaiting_search.take() else {
            return;
        };

        self.log.info(
            "searching",
            &[
                ("url", Value::Str(self.prices.search_endpoint())),
                ("game", Value::Str(self.game.as_str().to_string())),
                crate::util::elapsed::field(),
            ],
        );

        self.log_request(&checked);
        self.log_quality_scaling(&checked);

        let found = self.search_for(&checked);

        let outcome = price_check_loop::settle(&mut self.model, checked, found);

        let price_check_loop::Outcome::Priced { total } = outcome else {
            return;
        };

        self.after_search();

        self.log_estimate();

        self.record_priced(total);

        let view = self.model.filters();

        self.log.info(
            "price check finished",
            &[
                ("listings", Value::Int(total as i64)),
                ("stat_rows", Value::Int(view.stats.len() as i64)),
                ("numeric_rows", Value::Int(view.numerics.len() as i64)),
                ("flag_rows", Value::Int(view.flags.len() as i64)),
                ("enabled", Value::Int(view.enabled_count() as i64)),
                ("augments", Value::Int(self.model.augments().len() as i64)),
                ("quotes", Value::Int(self.model.listings().len() as i64)),
                ("price", Value::Str(self.priced_at())),
                ("league", Value::Str(self.settings.league.clone())),
                crate::util::elapsed::field(),
            ],
        );

        self.drain_hotkeys();
    }

    fn log_quality_scaling(&self, checked: &PriceCheck) {
        let item = &checked.item;
        let quality = f64::from(item.quality.unwrap_or(0));
        let equipment = &checked.query.filters.equipment_filters;

        for (label, printed, asked) in [
            ("armour", item.armour.ar, equipment.ar.min),
            ("evasion", item.armour.ev, equipment.ev.min),
            ("energy_shield", item.armour.es, equipment.es.min),
            ("physical_dps", item.weapon.physical, equipment.pdps.min),
        ] {
            let Some(printed) = printed.filter(|value| *value > 0.0) else {
                continue;
            };

            let Some(asked) = asked else {
                continue;
            };

            self.log.info(
                "a property is searched at the value it would have at twenty quality",
                &[
                    ("property", Value::Str(label.to_string())),
                    ("printed", Value::Str(format!("{printed:.0}"))),
                    ("quality", Value::Str(format!("{quality:.0}"))),
                    ("searched", Value::Str(format!("{asked:.0}"))),
                ],
            );
        }
    }

    fn log_request(&self, checked: &PriceCheck) {
        use poe_wayfinder_core::controller::bulk::{self, Endpoint};

        let priced_in = match (checked.endpoint, &checked.trade_tag) {
            (Endpoint::Exchange, Some(tag)) => {
                let have = bulk::currencies_to_price_in(self.game, tag);

                match have.is_empty() {
                    true => "anything the seller offers".to_string(),
                    false => have.join(","),
                }
            }
            _ => "not an exchange, no have list is sent".to_string(),
        };

        self.log.debug(
            "the request being sent",
            &[
                ("endpoint", Value::Str(format!("{:?}", checked.endpoint))),
                (
                    "trade_tag",
                    Value::Str(checked.trade_tag.clone().unwrap_or_else(|| "none".into())),
                ),
                ("priced_in", Value::Str(priced_in)),
                (
                    "stat_filters",
                    Value::Int(checked.stat_filter_count() as i64),
                ),
                ("item", Value::Str(checked.item.info.reference_name.clone())),
                (
                    "category",
                    Value::Str(format!("{:?}", checked.item.category)),
                ),
            ],
        );
    }

    fn log_filter_rows(&self) {
        let view = self.model.filters();

        for row in view.stats.iter().chain(view.numerics.iter()) {
            self.log.debug(
                "a filter row on the panel",
                &[
                    ("label", Value::Str(row.label.clone())),
                    ("key", Value::Str(format!("{:?}", row.key))),
                    ("enabled", Value::Bool(row.enabled)),
                    ("min", Value::Str(describe(row.min))),
                    ("max", Value::Str(describe(row.max))),
                    ("roll", Value::Str(describe(row.roll))),
                    (
                        "bounds",
                        Value::Str(crate::util::log_fields::span(row.bounds)),
                    ),
                    ("tier", Value::Str(crate::util::log_fields::count(row.tier))),
                ],
            );
        }

        self.log.debug(
            "the flag rows on the panel",
            &[
                (
                    "on",
                    Value::Str(
                        view.flags
                            .iter()
                            .filter(|row| row.enabled)
                            .map(|row| format!("{}={}", row.label, row.value))
                            .collect::<Vec<_>>()
                            .join(","),
                    ),
                ),
                ("rows", Value::Int(view.flags.len() as i64)),
                ("name_mode", Value::Str(format!("{:?}", view.name.mode))),
            ],
        );
    }

    fn log_estimate(&self) {
        use poe_wayfinder_core::controller::price_summary::price_spread;

        let Some(estimate) = self.model.estimate() else {
            self.log.debug(
                "no estimate was formed",
                &[("quotes", Value::Int(self.model.listings().len() as i64))],
            );

            return;
        };

        self.log.debug(
            "the estimate behind the headline",
            &[
                ("currency", Value::Str(estimate.currency.clone())),
                ("spread", Value::Str(price_spread(estimate))),
                ("low", Value::Str(format!("{}", estimate.low))),
                ("median", Value::Str(format!("{}", estimate.median))),
                ("high", Value::Str(format!("{}", estimate.high))),
                ("counted", Value::Int(estimate.counted as i64)),
                ("outliers_dropped", Value::Int(estimate.outliers as i64)),
            ],
        );

        for quote in self.model.listings().iter().take(10) {
            self.log.debug(
                "a listing the price came from",
                &[
                    ("amount", Value::Str(format!("{}", quote.amount))),
                    ("currency", Value::Str(quote.currency.clone())),
                    ("online", Value::Bool(quote.online)),
                ],
            );
        }
    }

    fn priced_at(&self) -> String {
        self.model.estimate().map_or_else(
            || "none".to_string(),
            poe_wayfinder_core::controller::price_summary::price_spread,
        )
    }

    fn follow_game(&mut self) {
        if self.settings.pinned_game.is_some() || self.settings.pinned_title {
            return;
        }

        if self.last_game_check.elapsed() < GAME_CHECK_EVERY {
            return;
        }

        self.last_game_check = Instant::now();

        let Some(found) = self.window.game_changed_from(self.game) else {
            return;
        };

        self.switch_game(found);
    }

    fn switch_game(&mut self, found: GameVersion) {
        let was = self.game;

        self.game = found;
        self.options = PriceCheckOptions::new(found);

        if !self.settings.pinned_title {
            self.settings.window_title = game_detect::title_for(found).to_string();

            self.window.retarget(found);
        }

        self.prices.set_game(found);

        if let Some(path) = super::wiring::default_client_log(found) {
            self.logs.watch(&path);
        }

        self.tray_state.stat_count = self.data.get(found).stat_count();
        self.last_search = None;

        self.follow_league(found);

        self.model.fail(&format!(
            "Now watching {}. Press the hotkey over an item.",
            self.settings.window_title
        ));

        self.log.info(
            "the game changed",
            &[
                ("from", Value::Str(was.as_str().to_string())),
                ("to", Value::Str(found.as_str().to_string())),
                (
                    "window_title",
                    Value::Str(self.settings.window_title.clone()),
                ),
                ("stats", Value::Int(self.tray_state.stat_count as i64)),
                ("league", Value::Str(self.settings.league.clone())),
                ("endpoint", Value::Str(self.prices.search_endpoint())),
            ],
        );
    }

    fn follow_league(&mut self, game: GameVersion) {
        let known = self.settings.known_leagues.get(game).clone();
        let chosen = self.settings.league_choice.get(game).clone();
        let remembered = self.remembered.last_league(game);

        if remembered.is_none() && known.is_empty() && chosen == LeagueChoice::Automatic {
            self.log.warn(
                "the game changed and this build has never learned a league for the new game, \
                 so the search keeps the one it had. Restart to read the trade site's league \
                 list for it.",
                &[
                    ("game", Value::Str(game.as_str().to_string())),
                    ("league_in_use", Value::Str(self.settings.league.clone())),
                ],
            );

            return;
        }

        let keep = remembered.unwrap_or_else(|| self.settings.league.clone());

        self.set_league(switching::league_now(&chosen, &known, &keep));
    }

    fn refresh_tray(&mut self, found: bool) {
        self.tray_state.game_found = found;
        self.tray_state.has_search = self.model.result().is_some();

        if let Some(tray) = &self.tray {
            tray.update(self.tray_state.clone());
        }
    }

    fn report_window_change(&mut self, found: Option<WindowRect>) {
        let visible = found.is_some();

        if visible == self.window_was_found {
            return;
        }

        self.window_was_found = visible;

        match found {
            Some(rect) => self.log.info(
                "the game window appeared",
                &[
                    ("width", Value::Int(i64::from(rect.width))),
                    ("height", Value::Int(i64::from(rect.height))),
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

    fn report_panel_change(&mut self, frame: &Frame, game_foreground: Option<bool>) {
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
                    Value::Bool(game_foreground.unwrap_or(false)),
                ),
                ("game_found", Value::Bool(game_foreground.is_some())),
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
        let Some(health) = self
            .health
            .check(PANEL_WINDOW_TITLE, &self.settings.window_title)
        else {
            self.log.debug(
                "the panel window could not be measured, so its placement is unchecked",
                &[
                    ("panel_title", Value::Str(PANEL_WINDOW_TITLE.to_string())),
                    ("game_title", Value::Str(self.settings.window_title.clone())),
                ],
            );

            return;
        };

        let measured = health.measured;
        let verdict = health.verdict;

        let fields = [
            ("verdict", Value::Str(format!("{verdict:?}"))),
            ("landed", Value::Str(rect_text(measured.window))),
            ("desktop", Value::Str(rect_text(measured.desktop))),
            ("above_game", Value::Bool(measured.above_game)),
        ];

        match health.advice {
            None => self.log.info("the panel is where it should be", &fields),
            Some(why) => self.log.warn(why, &fields),
        }
    }

    fn focus_locked_panel(&mut self, ctx: &egui::Context, frame: &Frame) {
        if !self.life.is_locked() || !frame.takes_input || self.locked_focused {
            return;
        }

        self.locked_focused = true;

        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);

        self.log
            .info("the locked panel took focus and will stay open", &[]);
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

impl<W, C, P, H, D, I, L, R, G> Drop for OverlayLoopDriver<W, C, P, H, D, I, L, R, G>
where
    W: GameState + 'static,
    C: Copier + 'static,
    P: Prices + 'static,
    H: PanelHealth + 'static,
    D: GameData + 'static,
    I: InputState + 'static,
    L: LogSource + 'static,
    R: RememberedSettings + 'static,
    G: PadInput + 'static,
{
    fn drop(&mut self) {
        if let Some(hook) = self.hook.take() {
            hook.stop();
        }
    }
}

fn rect_text(rect: poe_wayfinder_core::controller::panel_visible::Rect) -> String {
    format!("{}x{} at {},{}", rect.width, rect.height, rect.x, rect.y)
}

fn report_window<W: GameState>(window: &W, log: &Logger) {
    match window.window() {
        Some(found) => log.info(
            "found the game window",
            &[
                ("width", Value::Int(i64::from(found.rect.width))),
                ("height", Value::Int(i64::from(found.rect.height))),
                ("foreground", Value::Bool(found.is_foreground)),
                ("scale", Value::Str(format!("{:.2}", window.scale()))),
            ],
        ),
        None => log.warn("the game window is not open yet", &[]),
    }
}

impl<W, C, P, H, D, I, L, R, G> eframe::App for OverlayLoopDriver<W, C, P, H, D, I, L, R, G>
where
    W: GameState + 'static,
    C: Copier + 'static,
    P: Prices + 'static,
    H: PanelHealth + 'static,
    D: GameData + 'static,
    I: InputState + 'static,
    L: LogSource + 'static,
    R: RememberedSettings + 'static,
    G: PadInput + 'static,
{
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame(ctx);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
}

fn opened_widgets<R: RememberedSettings>(
    remembered: &R,
) -> crate::controller::widgets_controller::Widgets {
    let mut widgets = crate::controller::widgets_controller::Widgets::default();

    widgets.open_notes(&remembered.notes());
    widgets.remember_verdicts(remembered.map_verdicts());

    widgets
}

fn watch_for_a_stall(log_level: &str) -> Arc<AtomicU64> {
    let ticked_at = Arc::new(AtomicU64::new(crate::util::elapsed::millis() as u64));
    let watched = Arc::clone(&ticked_at);
    let log = Logger::new(log_level, "poe-wayfinder-frames");

    std::thread::spawn(move || {
        let mut watch = FrameWatch::default();

        while Arc::strong_count(&watched) > 1 {
            std::thread::sleep(STALL_LOOK_EVERY);

            let now = crate::util::elapsed::millis() as u64;

            match watch.look(now, watched.load(Ordering::Relaxed)) {
                Some(Verdict::Stalled { silent_ms }) => log.warn(
                    "the frame loop has stopped ticking, so the hotkey answers nothing until \
                     it comes back",
                    &[
                        ("silent_ms", Value::Int(silent_ms as i64)),
                        crate::util::elapsed::field(),
                    ],
                ),
                Some(Verdict::Recovered { stalled_for_ms }) => log.info(
                    "the frame loop is ticking again and the hotkey is being read",
                    &[
                        ("stalled_for_ms", Value::Int(stalled_for_ms as i64)),
                        crate::util::elapsed::field(),
                    ],
                ),
                Some(Verdict::Ticking) | None => {}
            }
        }
    });

    ticked_at
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
                &[("error", Value::Str(render(&err)))],
            );

            None
        }
    }
}

pub fn binding_for(hotkey: &Hotkey) -> Binding {
    Binding {
        code: crate::driver::hotkey_driver::virtual_key_code(hotkey.key()).unwrap_or(0),
        modifiers: hook_modifiers(hotkey),
    }
}

fn start_hook(hotkeys: &[Hotkey], log: &Logger) -> Option<HookDriver> {
    let bindings: Vec<Binding> = hotkeys.iter().map(binding_for).collect();

    match HookDriver::start(bindings) {
        Ok(hook) => {
            log.info(
                "watching the hotkeys with a keyboard hook as well",
                &[("hotkeys", Value::Int(hotkeys.len() as i64))],
            );

            Some(hook)
        }
        Err(err) => {
            log.warn(
                "the keyboard hook did not install. The hotkey still works if Windows \
                 delivers the registration.",
                &[("error", Value::Str(render(&err)))],
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
                &[("error", Value::Str(render(&err)))],
            );

            None
        }
    }
}

pub fn hook_modifiers(hotkey: &Hotkey) -> poe_wayfinder_core::controller::hotkey_match::Modifiers {
    use crate::types::Modifier;
    use poe_wayfinder_core::controller::hotkey_match::Modifiers;

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

fn describe(value: Option<f64>) -> String {
    crate::util::log_fields::number(value)
}

fn log_clipboard(log: &Logger, copied: &Result<String, String>) {
    match copied {
        Ok(text) => log.debug(
            "the clipboard the item was read from",
            &[
                ("chars", Value::Int(text.len() as i64)),
                ("lines", Value::Int(text.lines().count() as i64)),
                (
                    "first_line",
                    Value::Str(crate::util::log_fields::first_line(text)),
                ),
            ],
        ),
        Err(message) => log.debug(
            "nothing could be copied, so there is no item to price",
            &[("error", Value::Str(message.clone()))],
        ),
    }
}

fn log_parsed(log: &Logger, parsed: &Result<PriceCheck, String>) {
    use crate::util::log_fields::{count, text};

    let Ok(checked) = parsed else {
        return;
    };

    log.debug(
        "what the parser made of the item",
        &[
            ("name", Value::Str(checked.item.info.name.clone())),
            ("base", Value::Str(checked.item.info.reference_name.clone())),
            ("rarity", Value::Str(format!("{:?}", checked.item.rarity))),
            (
                "category",
                Value::Str(format!("{:?}", checked.item.category)),
            ),
            ("item_level", Value::Str(count(checked.item.item_level))),
            (
                "quality",
                Value::Int(i64::from(checked.item.quality.unwrap_or(0))),
            ),
            ("corrupted", Value::Bool(checked.item.is_corrupted)),
            ("unidentified", Value::Bool(checked.item.is_unidentified)),
            ("modifiers", Value::Int(checked.item.modifiers.len() as i64)),
            (
                "unknown_modifiers",
                Value::Int(checked.item.unknown_modifiers.len() as i64),
            ),
            (
                "data_trade_tag",
                Value::Str(text(&checked.item.info.trade_tag)),
            ),
            ("routed_to", Value::Str(format!("{:?}", checked.endpoint))),
            (
                "stat_filters",
                Value::Int(checked.stat_filter_count() as i64),
            ),
            ("searchable", Value::Bool(checked.constrains_something())),
        ],
    );
}
