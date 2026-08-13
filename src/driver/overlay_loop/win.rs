use std::time::{Duration, Instant};

use eframe::egui;

use super::search::SearchOutcome;
use super::{OverlayLoopError, OverlaySettings};

use crate::controller::copy_controller::Copier;
use crate::controller::game_state_controller::GameState;
use crate::controller::input_controller::InputState;
use crate::controller::log_watch_controller::LogSource;
use crate::controller::overlay_controller::{Frame, OverlayModel};
use crate::controller::panel_health_controller::PanelHealth;
use crate::controller::price_check_controller::{Prices, SearchResult};
use crate::controller::price_check_loop;
use crate::controller::session_controller::Session;
use crate::controller::settings_controller::RememberedSettings;
use crate::controller::startup_controller::Press;
use crate::controller::status_controller::{LeagueSource, Status};
use crate::driver::hook_driver::HookDriver;
use crate::driver::hotkey_driver::HotkeyDriver;
use crate::driver::overlay_placement;
use crate::driver::overlay_ui_driver::{
    drop_splash_background, overlay_viewport, paint, should_paint, splash_window, status_window,
    StatusEvent, UiEvent,
};
use crate::driver::tray_driver::{accepts_hotkey, TrayAction, TrayIcon, TrayState};
use crate::logging::{Logger, Value};
use crate::types::Hotkey;
use crate::util::error_chain::render;
use poe_wayfinder_core::controller::hotkey_match::Binding;

use poe_wayfinder_core::adapter::data_adapter::GameData;
use poe_wayfinder_core::controller::game_detect;
use poe_wayfinder_core::controller::overlay_lifecycle::{
    Input, Lifecycle, Point, Rect as LifeRect,
};
use poe_wayfinder_core::controller::press_coalesce;
use poe_wayfinder_core::controller::price_check::{price_check, PriceCheck, PriceCheckOptions};
use poe_wayfinder_core::types::{GamePair, GameVersion};

const PANEL_WINDOW_TITLE: &str = "poe-wayfinder";
const FRAME_INTERVAL: Duration = Duration::from_millis(100);
const HEARTBEAT_FRAMES: i64 = 600;
const GAME_CHECK_EVERY: Duration = Duration::from_millis(1000);
const SPLASH_HOLD: Duration = Duration::from_millis(2000);
const SPLASH_FADE: Duration = Duration::from_millis(400);

pub struct OverlayLoopDriver<W, C, P, H, D, I, L, R>
where
    W: GameState + 'static,
    I: InputState + 'static,
    C: Copier + 'static,
    P: Prices + 'static,
    H: PanelHealth + 'static,
    D: GameData + 'static,
    L: LogSource + 'static,
    R: RememberedSettings + 'static,
{
    window: W,
    copier: C,
    health: H,
    input: I,
    logs: L,
    remembered: R,
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
}

impl<W, C, P, H, D, I, L, R> OverlayLoopDriver<W, C, P, H, D, I, L, R>
where
    W: GameState + 'static,
    C: Copier + 'static,
    P: Prices + 'static,
    H: PanelHealth + 'static,
    D: GameData + 'static,
    I: InputState + 'static,
    L: LogSource + 'static,
    R: RememberedSettings + 'static,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        settings: OverlaySettings,
        game: GameVersion,
        data: GamePair<D>,
        stats: usize,
        roles: crate::controller::startup_controller::Validated,
        window: W,
        copier: C,
        prices: P,
        health: H,
        input: I,
        logs: L,
        remembered: R,
        hold: poe_wayfinder_core::controller::overlay_lifecycle::HoldKey,
        refresh: super::wiring::RefreshPlan,
        log: Logger,
    ) -> Result<Self, OverlayLoopError> {
        report_window(&window, &log);

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

        let widgets = opened_widgets(&remembered);
        let window_was_found = window.window().is_some();

        Ok(Self {
            window,
            copier,
            health,
            input,
            logs,
            remembered,
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

        self.follow_game();
        self.watch_logs();
        self.draw_splash(ctx);
        self.draw_status(ctx);

        let found = self.window.window().map(|mut window| {
            window.is_foreground = window.is_foreground || self.hold_open;

            window
        });

        self.refresh_tray(found.is_some());
        self.report_window_change(found.is_some(), found);

        let mut frame = self.model.frame_scaled(found, self.window.scale());

        self.tick_lifecycle(&frame);

        if !self.life.is_drawn() {
            frame.rect = None;
        }

        frame.takes_input = self.life.takes_input();

        self.focus_locked_panel(ctx, &frame);

        self.report_panel_change(&frame, found);
        self.apply_placement(ctx, &frame);

        self.pending.extend(paint(ctx, &self.model));

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

        if events.is_empty() || !self.session.apply_all(&events) {
            return;
        }

        let Some(league) = self.session.league() else {
            return;
        };

        if league == self.settings.league {
            return;
        }

        self.log.info(
            "the game log says the league changed",
            &[
                ("was", Value::Str(self.settings.league.clone())),
                ("now", Value::Str(league.to_string())),
            ],
        );

        self.widgets.note_log(&format!("league is now {league}"));

        self.settings.league = league.to_string();
        self.tray_state.league = Some(league.to_string());
        self.remembered.remember_league(league);
        self.prices.set_league(league);
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
                            &[("augment", Value::Str(reference))],
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
        Status {
            game: match self.window.window().is_some() {
                true => Some(self.game),
                false => None,
            },
            pinned: self.settings.pinned_game.is_some(),
            window_title: self.settings.window_title.clone(),
            hotkey: self.hotkey_text.clone(),
            league: self.settings.league.clone(),
            league_source: match self.session.league_source() {
                Some(_) => LeagueSource::GameLog,
                None => LeagueSource::Configured,
            },
            origin: self.settings.data_origin.clone(),
            stats: self.data.get(self.game).stat_count(),
            items: self.data.get(self.game).item_name_count(),
            augments: self.data.get(self.game).augments().len(),
            last_refresh: self.refresh.last_refresh(self.game),
            paused: self.tray_state.paused,
            network: self.settings.network,
            limits: self.prices.limiter_report(),
            note: self.prices.pacing_note(),
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
                StatusEvent::RefreshNow => self.rebuild_data(),
                StatusEvent::TogglePaused => {
                    self.tray_state.paused = !self.tray_state.paused;
                }
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

        let reference = price_check(&text, self.data.get(self.game), self.options)
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

        let action = poe_wayfinder_core::controller::chat::stash_search(&preset.text);
        let copier = &mut self.copier;

        let sent = super::wiring::send_chat(&action, |text| copier.put(text).is_ok());

        self.log.info(
            match sent {
                true => "searched the stash",
                false => "the stash search could not be sent",
            },
            &[("text", Value::Str(preset.text.clone()))],
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
            ..
        } = self;

        let options = *options;
        let tables = data.get(*game);

        let stage = price_check_loop::prepare(
            model,
            cursor,
            || copier.copy(),
            |text| price_check(text, tables, options).map_err(|e| render(&e)),
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

        let found = self.search_for(&checked);

        let outcome = price_check_loop::settle(&mut self.model, checked, found);

        let price_check_loop::Outcome::Priced { total } = outcome else {
            return;
        };

        self.after_search();

        self.log_estimate();
        self.log_filter_rows();

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

    fn log_request(&self, checked: &PriceCheck) {
        use poe_wayfinder_core::controller::bulk;

        let have = checked
            .trade_tag
            .as_ref()
            .map(|tag| bulk::currencies_to_price_in(self.game, tag))
            .unwrap_or_default();

        self.log.debug(
            "the request being sent",
            &[
                ("endpoint", Value::Str(format!("{:?}", checked.endpoint))),
                (
                    "trade_tag",
                    Value::Str(checked.trade_tag.clone().unwrap_or_else(|| "none".into())),
                ),
                (
                    "priced_in",
                    Value::Str(match have.is_empty() {
                        true => "anything the seller offers".to_string(),
                        false => have.join(","),
                    }),
                ),
                (
                    "stat_filters",
                    Value::Int(checked.stat_filter_count() as i64),
                ),
                (
                    "item",
                    Value::Str(checked.item.info.reference_name.clone()),
                ),
                ("category", Value::Str(format!("{:?}", checked.item.category))),
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
                    ("enabled", Value::Bool(row.enabled)),
                    ("min", Value::Str(describe(row.min))),
                    ("max", Value::Str(describe(row.max))),
                    ("roll", Value::Str(describe(row.roll))),
                    (
                        "bounds",
                        Value::Str(row.bounds.map_or_else(
                            || "none".to_string(),
                            |(low, high)| format!("{low} to {high}"),
                        )),
                    ),
                ],
            );
        }
    }

    fn log_estimate(&self) {
        use poe_wayfinder_core::controller::price_summary::price_spread;

        let Some(estimate) = self.model.estimate() else {
            self.log.debug(
                "no estimate was formed",
                &[(
                    "quotes",
                    Value::Int(self.model.listings().len() as i64),
                )],
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

        let was = self.game;

        self.game = found;
        self.options = PriceCheckOptions::new(found);
        self.settings.window_title = game_detect::title_for(found).to_string();

        self.window.retarget(found);
        self.prices.set_game(found);

        if let Some(path) = super::wiring::default_client_log(found) {
            self.logs.watch(&path);
        }

        self.tray_state.stat_count = self.data.get(found).stat_count();
        self.last_search = None;

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
            ],
        );
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
        let Some(health) = self
            .health
            .check(PANEL_WINDOW_TITLE, &self.settings.window_title)
        else {
            self.log.debug("could not measure the panel window", &[]);

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
            Some(why) => self.log.error(why, &fields),
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

impl<W, C, P, H, D, I, L, R> Drop for OverlayLoopDriver<W, C, P, H, D, I, L, R>
where
    W: GameState + 'static,
    C: Copier + 'static,
    P: Prices + 'static,
    H: PanelHealth + 'static,
    D: GameData + 'static,
    I: InputState + 'static,
    L: LogSource + 'static,
    R: RememberedSettings + 'static,
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

impl<W, C, P, H, D, I, L, R> eframe::App for OverlayLoopDriver<W, C, P, H, D, I, L, R>
where
    W: GameState + 'static,
    C: Copier + 'static,
    P: Prices + 'static,
    H: PanelHealth + 'static,
    D: GameData + 'static,
    I: InputState + 'static,
    L: LogSource + 'static,
    R: RememberedSettings + 'static,
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
    match value {
        Some(value) if value.is_finite() => format!("{value}"),
        Some(value) => format!("NOT FINITE: {value}"),
        None => "unset".to_string(),
    }
}
