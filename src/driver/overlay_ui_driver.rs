use poe_wayfinder_core::controller::filter_view::{modifier_text, FlagKey, Row, RowKey};
use poe_wayfinder_core::controller::switching::{GameChoice, LeagueChoice};
use poe_wayfinder_core::types::item::ItemRarity;

use crate::controller::overlay_controller::{Frame, PanelSource};
use crate::types::overlay::OverlayState;

#[derive(Debug, Clone, PartialEq)]
pub enum StatusEvent {
    MarkMap { matcher: String, set: String },
    HideToTray,
    Quit,
    RefreshNow,
    TogglePaused,
    ChooseLeague(LeagueChoice),
    ChooseGame(GameChoice),
    Bound(poe_wayfinder_core::controller::bind_capture::Binding),
    CopyCsv(String),
    PriceByName(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    Dismiss,
    SearchStash(String),
    OpenInBrowser,
    Research,
    ToggleRow(RowKey),
    SetMin(RowKey, Option<f64>),
    SetMax(RowKey, Option<f64>),
    ToggleFlag(FlagKey),
    InvertFlag(FlagKey),
    SetAllStats(bool),
    CycleName,
    ToggleOnline,
    ChooseAugment(String),
    ClearAugment,
}

pub fn rarity_colour(rarity: ItemRarity) -> (u8, u8, u8) {
    match rarity {
        ItemRarity::Normal => (200, 200, 200),
        ItemRarity::Magic => (136, 136, 255),
        ItemRarity::Rare => (255, 255, 119),
        ItemRarity::Unique => (175, 96, 37),
    }
}

pub fn modifier_line(text: &str, roll: Option<f64>, decimals: bool) -> String {
    modifier_text(text, roll, decimals)
}

pub const NO_VALUE: &str = "–";
pub const NO_LIMIT_ABOVE: &str = "no max";
pub const NO_LIMIT_BELOW: &str = "no min";

pub fn format_value(value: f64, decimals: bool) -> String {
    if !value.is_finite() {
        return NO_VALUE.to_string();
    }

    match decimals {
        true => format!("{value:.2}"),
        false => format!("{}", value.round() as i64),
    }
}

pub fn roll_caption(row: &Row) -> Option<String> {
    let roll = row.roll?;

    let Some((low, high)) = row.bounds else {
        return Some(format_value(roll, row.decimals));
    };

    Some(format!(
        "{} of {}–{}",
        format_value(roll, row.decimals),
        format_value(low, row.decimals),
        format_value(high, row.decimals)
    ))
}

pub fn search_button_label(edited: bool) -> &'static str {
    match edited {
        true => "Search with these filters",
        false => "Search again",
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PanelText {
    pub title: String,
    pub subtitle: Option<String>,
    pub body: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn panel_text(model: &dyn PanelSource) -> PanelText {
    match model.state() {
        OverlayState::Hidden => PanelText {
            title: String::new(),
            subtitle: None,
            body: Vec::new(),
            warnings: Vec::new(),
        },

        OverlayState::Loading => PanelText {
            title: "Checking price".into(),
            subtitle: None,
            body: Vec::new(),
            warnings: Vec::new(),
        },

        OverlayState::Error => PanelText {
            title: "Price check failed".into(),
            subtitle: None,
            body: model
                .message()
                .map(|m| vec![m.to_string()])
                .unwrap_or_default(),
            warnings: Vec::new(),
        },

        OverlayState::Showing => showing_text(model),
    }
}

fn showing_text(model: &dyn PanelSource) -> PanelText {
    let Some(check) = model.result() else {
        return PanelText {
            title: "No result".into(),
            subtitle: None,
            body: vec!["The price check returned nothing.".into()],
            warnings: Vec::new(),
        };
    };

    let item = &check.item;

    let title = if item.info.reference_name.is_empty() {
        item.info.name.clone()
    } else {
        item.info.reference_name.clone()
    };

    let subtitle = match model.total() {
        Some(0) => Some("No listings match".to_string()),
        Some(1) => Some("1 listing".to_string()),
        Some(n) => Some(format!("{n} listings")),
        None => None,
    };

    let mut body = Vec::new();

    if let Some(rarity) = item.rarity {
        body.push(format!("Rarity: {}", rarity.as_str()));
    }

    if let Some(level) = item.item_level {
        body.push(format!("Item Level: {level}"));
    }

    if let Some(quality) = item.quality {
        body.push(format!("Quality: {quality}%"));
    }

    if item.is_unidentified {
        body.push("Unidentified, priced by base and item level".into());
    }

    body.push(format!("Filters: {}", check.stat_filter_count()));

    let mut warnings = Vec::new();

    for unknown in &item.unknown_modifiers {
        warnings.push(format!("Not recognised: {}", unknown.text));
    }

    if item.is_corrupted {
        body.push("Corrupted".into());
    }

    PanelText {
        title,
        subtitle,
        body,
        warnings,
    }
}

pub fn should_paint(frame: &Frame) -> bool {
    frame.rect.is_some()
}
#[cfg(windows)]
mod win {
    use super::{
        format_value, modifier_line, panel_text, rarity_colour, roll_caption, search_button_label,
        Frame, PanelSource, StatusEvent, UiEvent, NO_LIMIT_ABOVE, NO_LIMIT_BELOW,
    };

    use eframe::egui;
    use poe_wayfinder_core::controller::filter_view::{
        gauge_edit, influence_labels, tier_label, FilterView, FlagRow, Row,
    };
    use poe_wayfinder_core::controller::gamepad_match::PadFamily;
    use poe_wayfinder_core::controller::help;
    use poe_wayfinder_core::controller::item_editor::AugmentOption;
    use poe_wayfinder_core::controller::pad_focus;
    use poe_wayfinder_core::controller::price_summary::{
        online_count, price_headline, price_spread, stack_value, Estimate, Quote,
    };
    use poe_wayfinder_core::controller::rate_limit::LimiterLine;
    use poe_wayfinder_core::types::GameVersion;

    use std::time::SystemTime;

    use crate::controller::status_controller::{headline, health, rows, Health, Status};
    use crate::controller::widgets_controller::{Tab, Widgets};
    use poe_wayfinder_core::controller::map_check::Verdict;
    use poe_wayfinder_core::controller::settings_view::{bounds_of, sliders, switches};

    const PANEL_BACKGROUND: egui::Color32 = egui::Color32::from_rgb(14, 14, 18);
    const SECTION_BACKGROUND: egui::Color32 = egui::Color32::from_rgb(24, 24, 30);
    const ROW_BACKGROUND: egui::Color32 = egui::Color32::from_rgb(34, 34, 42);
    const GAUGE_TRACK: egui::Color32 = egui::Color32::from_rgb(52, 52, 62);
    const GAUGE_FILL: egui::Color32 = egui::Color32::from_rgb(72, 132, 196);
    const GAUGE_TICK: egui::Color32 = egui::Color32::from_rgb(236, 226, 190);
    const ONLINE_DOT: egui::Color32 = egui::Color32::from_rgb(96, 190, 110);
    const OFFLINE_DOT: egui::Color32 = egui::Color32::from_rgb(120, 120, 130);
    const WARNING: egui::Color32 = egui::Color32::from_rgb(240, 180, 60);
    const MUTED: egui::Color32 = egui::Color32::from_rgb(150, 150, 162);
    const ACCENT: egui::Color32 = egui::Color32::from_rgb(226, 200, 130);

    const GAUGE_HIT_HEIGHT: f32 = 14.0;
    const GAUGE_BAR_HEIGHT: f32 = 6.0;
    const GAUGE_HANDLE_RADIUS: f32 = 4.0;

    const MOD_ENABLED: egui::Color32 = egui::Color32::from_rgb(224, 224, 232);
    const MOD_DISABLED: egui::Color32 = egui::Color32::from_rgb(112, 112, 124);
    const MOD_UNDERLINE: egui::Color32 = egui::Color32::from_rgb(72, 132, 196);
    const DISABLED_FADE: f32 = 0.45;

    const LISTINGS_SHOWN: usize = 8;
    const FOOTER_HEIGHT: f32 = 30.0;

    pub fn overlay_viewport(frame: &Frame) -> egui::ViewportBuilder {
        let mut builder = egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_taskbar(false)
            .with_resizable(false)
            .with_icon(icon_data(crate::assets::window_icon()));

        if let Some(rect) = frame.rect {
            builder = builder
                .with_position(egui::pos2(rect.x as f32, rect.y as f32))
                .with_inner_size(egui::vec2(rect.width as f32, rect.height as f32));
        }

        builder.with_mouse_passthrough(!frame.takes_input)
    }

    #[derive(Debug, Clone, Copy)]
    pub struct PadView {
        pub focus: pad_focus::Focus,
        pub connected: bool,
    }

    fn mark_for(pad: Option<&PadView>, index: usize) -> Option<PadView> {
        let pad = pad?;

        match pad.focus.row == index {
            true => Some(*pad),
            false => None,
        }
    }

    fn focus_frame(marked: Option<PadView>) -> egui::Frame {
        match marked {
            Some(_) => egui::Frame::new()
                .stroke(egui::Stroke::new(1.0_f32, ACCENT))
                .corner_radius(4.0)
                .inner_margin(2.0),
            None => egui::Frame::new().inner_margin(2.0),
        }
    }

    fn box_stroke(marked: Option<PadView>, column: pad_focus::Column) -> egui::Stroke {
        let Some(pad) = marked else {
            return egui::Stroke::NONE;
        };

        if pad.focus.column != column {
            return egui::Stroke::NONE;
        }

        match pad.focus.editing {
            true => egui::Stroke::new(2.0_f32, GAUGE_FILL),
            false => egui::Stroke::new(1.0_f32, ACCENT),
        }
    }

    pub fn paint(
        ctx: &egui::Context,
        model: &dyn PanelSource,
        pad: Option<&PadView>,
    ) -> Vec<UiEvent> {
        let text = panel_text(model);
        let mut events = Vec::new();

        style(ctx);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(PANEL_BACKGROUND.gamma_multiply(0.96))
                    .inner_margin(10.0)
                    .corner_radius(8.0),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);

                header(ui, model, &text, &mut events);

                ui.add_space(2.0);

                egui::ScrollArea::vertical()
                    .max_height((ui.available_height() - FOOTER_HEIGHT).max(60.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        price_banner(ui, model);
                        augment_picker(ui, model, &mut events);
                        filters(ui, model.filters(), &mut events, pad);
                        listing_rows(ui, model);
                    });

                footer(ui, model, &mut events, pad);
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            events.push(UiEvent::Dismiss);
        }

        let clicked_off = ctx.input(|i| i.pointer.any_click()) && !ctx.is_pointer_over_area();

        if clicked_off {
            events.push(UiEvent::Dismiss);
        }

        events
    }

    fn style(ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();

        style.visuals.widgets.inactive.weak_bg_fill = ROW_BACKGROUND;
        style.visuals.widgets.hovered.weak_bg_fill = GAUGE_TRACK;
        style.visuals.widgets.active.weak_bg_fill = GAUGE_FILL;
        style.visuals.override_text_color = Some(egui::Color32::from_rgb(214, 214, 222));
        style.visuals.panel_fill = PANEL_BACKGROUND;
        style.spacing.button_padding = egui::vec2(6.0, 2.0);

        ctx.set_style(style);
    }

    fn header(
        ui: &mut egui::Ui,
        model: &dyn PanelSource,
        text: &super::PanelText,
        events: &mut Vec<UiEvent>,
    ) {
        ui.horizontal(|ui| {
            let colour = model
                .result()
                .and_then(|c| c.item.rarity)
                .map(rarity_colour)
                .map(|(r, g, b)| egui::Color32::from_rgb(r, g, b))
                .unwrap_or(ACCENT);

            ui.label(
                egui::RichText::new(&text.title)
                    .size(15.0)
                    .strong()
                    .color(colour),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("Close")
                    .on_hover_text("Circle on a pad, or Escape")
                    .clicked()
                {
                    events.push(UiEvent::Dismiss);
                }
            });
        });

        ui.horizontal_wrapped(|ui| {
            if let Some(subtitle) = &text.subtitle {
                ui.label(egui::RichText::new(subtitle).small().color(MUTED));
            }

            for line in &text.body {
                ui.label(egui::RichText::new(line).small().color(MUTED));
            }
        });

        name_row(ui, model.filters(), events);

        for warning in &text.warnings {
            ui.label(egui::RichText::new(warning).small().color(WARNING));
        }

        if model.result().is_some_and(|c| c.has_unknown_modifiers()) {
            ui.label(
                egui::RichText::new(
                    "The price may be wrong. Rebuild the data with poe-wayfinder-datagen.",
                )
                .small()
                .color(MUTED),
            );
        }

        let influences = model
            .result()
            .map(|c| influence_labels(&c.item))
            .unwrap_or_default();

        if !influences.is_empty() {
            ui.label(
                egui::RichText::new(influences.join(", "))
                    .small()
                    .color(ACCENT),
            );
        }
    }

    fn name_row(ui: &mut egui::Ui, view: &FilterView, events: &mut Vec<UiEvent>) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("searching").small().color(MUTED));

            let button = egui::Button::new(
                egui::RichText::new(view.name.caption())
                    .small()
                    .color(ACCENT),
            )
            .fill(ROW_BACKGROUND);

            if ui
                .add(button)
                .on_hover_text(format!("click to widen: {}", view.name.mode.next().label()))
                .clicked()
            {
                events.push(UiEvent::CycleName);
            }
        });
    }

    fn price_banner(ui: &mut egui::Ui, model: &dyn PanelSource) {
        let Some(estimate) = model.estimate() else {
            return;
        };

        section(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(price_headline(estimate))
                        .size(17.0)
                        .strong()
                        .color(ACCENT),
                );

                ui.label(
                    egui::RichText::new(price_spread(estimate))
                        .small()
                        .color(MUTED),
                );
            });

            ui.label(
                egui::RichText::new(estimate_caption(estimate, model.listings()))
                    .small()
                    .color(MUTED),
            );

            if let Some(note) = model.pacing_note() {
                ui.label(egui::RichText::new(note).small().color(MUTED));
            }

            if let Some(stack) = model.result().and_then(|c| c.item.stack_size) {
                if let Some(line) = stack_value(estimate, stack.value) {
                    ui.label(egui::RichText::new(line).small().color(MUTED));
                }
            }
        });
    }

    fn estimate_caption(estimate: &Estimate, listings: &[Quote]) -> String {
        let online = online_count(listings);

        let mut caption = format!(
            "middle of {} asking prices, {online} online",
            estimate.counted
        );

        if estimate.outliers > 0 {
            caption.push_str(&format!(", {} outlier(s) left out", estimate.outliers));
        }

        caption
    }

    fn augment_picker(ui: &mut egui::Ui, model: &dyn PanelSource, events: &mut Vec<UiEvent>) {
        let options = model.augments();

        if options.is_empty() {
            return;
        }

        section(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Socket an augment")
                        .small()
                        .color(MUTED),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if model.chosen_augment().is_some()
                        && ui
                            .button(egui::RichText::new("take it off").small())
                            .clicked()
                    {
                        events.push(UiEvent::ClearAugment);
                    }
                });
            });

            egui::ScrollArea::vertical()
                .max_height(96.0)
                .id_salt("augments")
                .show(ui, |ui| {
                    for option in options {
                        augment_button(ui, option, model.chosen_augment(), events);
                    }
                });
        });
    }

    fn augment_button(
        ui: &mut egui::Ui,
        option: &AugmentOption,
        chosen: Option<&str>,
        events: &mut Vec<UiEvent>,
    ) {
        let picked = chosen == Some(option.reference_name.as_str());

        let caption = format!(
            "{}  —  {}",
            option.name,
            modifier_line(&option.text, Some(option.value), false)
        );

        let button = egui::Button::new(egui::RichText::new(caption).small())
            .fill(match picked {
                true => GAUGE_FILL,
                false => ROW_BACKGROUND,
            })
            .min_size(egui::vec2(ui.available_width(), 0.0));

        if ui.add(button).clicked() {
            events.push(UiEvent::ChooseAugment(option.reference_name.clone()));
        }
    }

    fn filters(
        ui: &mut egui::Ui,
        view: &FilterView,
        events: &mut Vec<UiEvent>,
        pad: Option<&PadView>,
    ) {
        if view.is_empty() {
            ui.label(
                egui::RichText::new("Nothing to filter on.")
                    .small()
                    .color(MUTED),
            );

            return;
        }

        if !view.flags.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for flag in &view.flags {
                    flag_chip(ui, flag, events);
                }
            });
        }

        if !view.numerics.is_empty() {
            section(ui, |ui| {
                for (index, row) in view.numerics.iter().enumerate() {
                    numeric_row(ui, row, events, mark_for(pad, index));
                }
            });
        }

        if view.stats.is_empty() {
            return;
        }

        section(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Modifiers").small().color(MUTED));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let all = view.all_stats_on();

                    let caption = match all {
                        true => "none",
                        false => "all",
                    };

                    if ui
                        .button(egui::RichText::new(caption).small())
                        .on_hover_text("switch every modifier filter at once")
                        .clicked()
                    {
                        events.push(UiEvent::SetAllStats(!all));
                    }
                });
            });

            for (index, row) in view.stats.iter().enumerate() {
                stat_row(ui, row, events, mark_for(pad, view.numerics.len() + index));
            }
        });
    }

    fn section<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) {
        egui::Frame::new()
            .fill(SECTION_BACKGROUND)
            .inner_margin(6.0)
            .corner_radius(4.0)
            .show(ui, add);
    }

    fn flag_chip(ui: &mut egui::Ui, flag: &FlagRow, events: &mut Vec<UiEvent>) {
        let caption = match (flag.enabled, flag.value) {
            (true, false) => format!("not {}", flag.label),
            _ => flag.label.clone(),
        };

        let chip = egui::Button::new(egui::RichText::new(caption).small())
            .fill(match flag.enabled {
                true => GAUGE_FILL,
                false => ROW_BACKGROUND,
            })
            .corner_radius(10.0);

        let response = ui.add(chip).on_hover_text("right click to invert");

        if response.clicked() {
            events.push(UiEvent::ToggleFlag(flag.key));
        }

        if response.secondary_clicked() {
            events.push(UiEvent::InvertFlag(flag.key));
        }
    }

    fn numeric_row(
        ui: &mut egui::Ui,
        row: &Row,
        events: &mut Vec<UiEvent>,
        marked: Option<PadView>,
    ) {
        focus_frame(marked).show(ui, |ui| {
            ui.horizontal(|ui| {
                mod_line(ui, row, &row.label.clone(), events);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    bounds_inputs(ui, row, events, marked);
                });
            });
        });
    }

    fn stat_row(ui: &mut egui::Ui, row: &Row, events: &mut Vec<UiEvent>, marked: Option<PadView>) {
        focus_frame(marked).show(ui, |ui| {
            ui.horizontal(|ui| {
                let label = modifier_line(&row.label, row.roll, row.decimals);

                mod_line(ui, row, &label, events);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    bounds_inputs(ui, row, events, marked);

                    if let Some(tier) = tier_label(row.tier) {
                        ui.label(
                            egui::RichText::new(tier)
                                .small()
                                .color(dim(ACCENT, row.enabled)),
                        );
                    }
                });
            });

            contributor_line(ui, row);

            if row.bounds.is_some() {
                gauge(ui, row, events);
            }
        });
    }

    fn toggle_hint(row: &Row) -> String {
        let action = match row.enabled {
            true => "click to drop this from the search",
            false => "click to search on this again",
        };

        match roll_caption(row) {
            Some(caption) => format!("{caption}\n{action}"),
            None => action.to_string(),
        }
    }

    fn dim(colour: egui::Color32, enabled: bool) -> egui::Color32 {
        match enabled {
            true => colour,
            false => colour.gamma_multiply(DISABLED_FADE),
        }
    }

    fn mod_line(ui: &mut egui::Ui, row: &Row, label: &str, events: &mut Vec<UiEvent>) {
        let text = egui::RichText::new(label).small().color(match row.enabled {
            true => MOD_ENABLED,
            false => MOD_DISABLED,
        });

        let response = ui
            .add(
                egui::Label::new(text)
                    .truncate()
                    .sense(egui::Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(toggle_hint(row));

        if row.enabled {
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(response.rect.left() - 3.0, response.rect.bottom() + 1.0),
                    egui::pos2(response.rect.right() + 3.0, response.rect.bottom() + 2.0),
                ),
                0.0,
                MOD_UNDERLINE,
            );
        }

        if response.clicked() {
            events.push(UiEvent::ToggleRow(row.key));
        }
    }

    fn contributor_line(ui: &mut egui::Ui, row: &Row) {
        if row.contributors.len() < 2 {
            return;
        }

        ui.label(
            egui::RichText::new(format!("adds up {}", row.contributors.join(" + ")))
                .small()
                .color(MUTED),
        );
    }

    fn bounds_inputs(
        ui: &mut egui::Ui,
        row: &Row,
        events: &mut Vec<UiEvent>,
        marked: Option<PadView>,
    ) {
        let step = drag_speed(row);

        let max_unset = row.max.is_none();
        let mut max = row
            .max
            .or_else(|| row.bounds.map(|(_, high)| high))
            .unwrap_or(0.0);

        let decimals = row.decimals;
        let max_widget =
            egui::DragValue::new(&mut max)
                .speed(step)
                .custom_formatter(move |value, _| match max_unset || !value.is_finite() {
                    true => NO_LIMIT_ABOVE.to_string(),
                    false => format_value(value, decimals),
                });

        let max_frame = egui::Frame::new().stroke(box_stroke(marked, pad_focus::Column::Max));

        if max_frame
            .show(ui, |ui| ui.add_sized([56.0, 18.0], max_widget))
            .inner
            .on_hover_text("highest value to match. Drag it, or click the bar below.")
            .changed()
            && finite(max) != row.max
        {
            events.push(UiEvent::SetMax(row.key, finite(max)));
        }

        let min_unset = row.min.is_none();
        let mut min = row
            .min
            .or_else(|| row.bounds.map(|(low, _)| low))
            .unwrap_or(0.0);

        let min_widget =
            egui::DragValue::new(&mut min)
                .speed(step)
                .custom_formatter(move |value, _| match min_unset || !value.is_finite() {
                    true => NO_LIMIT_BELOW.to_string(),
                    false => format_value(value, decimals),
                });

        let min_frame = egui::Frame::new().stroke(box_stroke(marked, pad_focus::Column::Min));

        if min_frame
            .show(ui, |ui| ui.add_sized([56.0, 18.0], min_widget))
            .inner
            .on_hover_text("lowest value to match. Drag it, or click the bar below.")
            .changed()
            && finite(min) != row.min
        {
            events.push(UiEvent::SetMin(row.key, finite(min)));
        }
    }

    fn drag_speed(row: &Row) -> f64 {
        let floor = match row.decimals {
            true => 0.01,
            false => 0.05,
        };

        match row.bounds {
            Some((low, high)) if high > low => ((high - low) / 120.0).max(floor),
            _ => match row.decimals {
                true => 0.01,
                false => 0.5,
            },
        }
    }

    fn finite(value: f64) -> Option<f64> {
        value.is_finite().then_some(value)
    }

    fn gauge(ui: &mut egui::Ui, row: &Row, events: &mut Vec<UiEvent>) {
        let (hit, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), GAUGE_HIT_HEIGHT),
            egui::Sense::click_and_drag(),
        );

        let rect =
            egui::Rect::from_center_size(hit.center(), egui::vec2(hit.width(), GAUGE_BAR_HEIGHT));

        if row.bounds.is_some() && row.enabled {
            response
                .clone()
                .on_hover_cursor(egui::CursorIcon::PointingHand);
        }

        if let Some(pointer) = response
            .interact_pointer_pos()
            .filter(|_| row.enabled && (response.clicked() || response.dragged()))
        {
            let ratio = f64::from((pointer.x - rect.left()) / rect.width().max(1.0));

            if let Some(edit) = gauge_edit(row, ratio) {
                events.push(match edit.sets_min {
                    true => UiEvent::SetMin(row.key, Some(edit.value)),
                    false => UiEvent::SetMax(row.key, Some(edit.value)),
                });
            }
        }

        let painter = ui.painter();

        painter.rect_filled(rect, 3.0, dim(GAUGE_TRACK, row.enabled));

        let Some((low, high)) = row.bounds else {
            return;
        };

        let at = |value: f64| {
            let ratio = ((value - low) / (high - low)).clamp(0.0, 1.0) as f32;

            rect.left() + rect.width() * ratio
        };

        let left = row.min.map(&at).unwrap_or(rect.left());
        let right = row.max.map(&at).unwrap_or(rect.right());

        if row.enabled && right > left {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(left, rect.top()),
                    egui::pos2(right, rect.bottom()),
                ),
                3.0,
                dim(GAUGE_FILL, row.enabled),
            );
        }

        if let Some(share) = row.percent_of_bounds() {
            let x = rect.left() + rect.width() * share as f32;

            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(x - 1.0, rect.top() - 1.0),
                    egui::pos2(x + 1.0, rect.bottom() + 1.0),
                ),
                0.0,
                dim(GAUGE_TICK, row.enabled),
            );
        }

        if row.enabled {
            for handle in [row.min.map(&at), row.max.map(&at)].into_iter().flatten() {
                painter.circle_filled(
                    egui::pos2(handle, rect.center().y),
                    GAUGE_HANDLE_RADIUS,
                    GAUGE_FILL,
                );

                painter.circle_stroke(
                    egui::pos2(handle, rect.center().y),
                    GAUGE_HANDLE_RADIUS,
                    egui::Stroke::new(1.0_f32, GAUGE_TICK),
                );
            }
        }
    }

    fn listing_rows(ui: &mut egui::Ui, model: &dyn PanelSource) {
        let listings = model.listings();

        if listings.is_empty() {
            return;
        }

        section(ui, |ui| {
            ui.label(
                egui::RichText::new("Cheapest listings")
                    .small()
                    .color(MUTED),
            );

            for listing in listings.iter().take(LISTINGS_SHOWN) {
                listing_row(ui, listing);
            }

            if listings.len() > LISTINGS_SHOWN {
                ui.label(
                    egui::RichText::new(format!(
                        "and {} more on the trade site",
                        listings.len() - LISTINGS_SHOWN
                    ))
                    .small()
                    .color(MUTED),
                );
            }
        });
    }

    fn listing_row(ui: &mut egui::Ui, listing: &Quote) {
        ui.horizontal(|ui| {
            use poe_wayfinder_core::controller::bulk::{seller_status, SellerStatus};

            let (dot, hint) = match seller_status(listing.online, listing.away) {
                SellerStatus::Online => (ONLINE_DOT, "online"),
                SellerStatus::Afk => (WARNING, "online but away, so may not answer"),
                SellerStatus::Offline => (OFFLINE_DOT, "offline"),
            };

            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());

            ui.painter().circle_filled(rect.center(), 3.5, dot);

            response.on_hover_text(hint);

            ui.label(
                egui::RichText::new(format!(
                    "{} {}",
                    format_value(listing.amount, listing.amount.fract() != 0.0),
                    listing.currency
                ))
                .small()
                .strong(),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(listing.account.clone())
                            .small()
                            .color(MUTED),
                    )
                    .truncate(),
                );
            });
        })
        .response
        .on_hover_ui(|ui| listed_item_tooltip(ui, &listing.details));
    }

    fn listed_item_tooltip(
        ui: &mut egui::Ui,
        details: &poe_wayfinder_core::controller::listing::DisplayItem,
    ) {
        use poe_wayfinder_core::controller::listing::{tier_of, DisplayLine, LineKind};

        if details.title.is_empty() && details.explicit_mods.is_empty() {
            ui.label(
                egui::RichText::new("no detail was returned")
                    .small()
                    .color(MUTED),
            );

            return;
        }

        for line in &details.title {
            ui.label(egui::RichText::new(line).strong());
        }

        for property in &details.properties {
            ui.label(
                egui::RichText::new(format!("{}: {}", property.label, property.value))
                    .small()
                    .color(MUTED),
            );
        }

        let blocks = [
            &details.enchant_mods,
            &details.rune_mods,
            &details.implicit_mods,
            &details.fractured_mods,
            &details.explicit_mods,
            &details.crafted_mods,
            &details.desecrated_mods,
            &details.mutated_mods,
        ];

        for block in blocks {
            let tiers: Vec<Option<String>> = block.iter().map(|line| line.tier.clone()).collect();

            if let Some(worst) = tier_of(&tiers) {
                ui.label(
                    egui::RichText::new(format!("tiers {worst}"))
                        .small()
                        .color(ACCENT),
                );
            }

            for line in block {
                ui.label(mod_colour(line));
            }
        }

        for tag in &details.tags {
            ui.label(mod_colour(tag));
        }

        fn mod_colour(line: &DisplayLine) -> egui::RichText {
            let colour = match line.kind {
                LineKind::Enchant | LineKind::Augmented => ACCENT,
                LineKind::Fractured | LineKind::Desecrated | LineKind::Mutated => WARNING,
                LineKind::Unmet => egui::Color32::from_rgb(226, 96, 96),
                _ => egui::Color32::from_rgb(214, 214, 222),
            };

            egui::RichText::new(&line.text).small().color(colour)
        }
    }

    fn footer(
        ui: &mut egui::Ui,
        model: &dyn PanelSource,
        events: &mut Vec<UiEvent>,
        pad: Option<&PadView>,
    ) {
        ui.separator();

        if let Some(pad) = pad.filter(|pad| pad.connected) {
            ui.label(
                egui::RichText::new(pad_focus::hints(pad.focus.editing))
                    .small()
                    .color(MUTED),
            );
        }

        ui.horizontal(|ui| {
            let label = search_button_label(model.edited());

            let search =
                egui::Button::new(egui::RichText::new(label).small()).fill(match model.edited() {
                    true => GAUGE_FILL,
                    false => ROW_BACKGROUND,
                });

            if ui.add(search).clicked() {
                events.push(UiEvent::Research);
            }

            if ui
                .button(egui::RichText::new("browser").small())
                .on_hover_text("open this search on the trade site")
                .clicked()
            {
                events.push(UiEvent::OpenInBrowser);
            }

            events.extend(stash_search_buttons(ui, model));

            let online_only = model
                .result()
                .map(|c| c.query.status == poe_wayfinder_core::types::query::Status::Online)
                .unwrap_or(true);

            let caption = match online_only {
                true => "online only",
                false => "any seller",
            };

            if ui
                .button(egui::RichText::new(caption).small())
                .on_hover_text("include sellers who are offline")
                .clicked()
            {
                events.push(UiEvent::ToggleOnline);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                rate_limit_line(ui, model.limits());
            });
        });
    }

    fn rate_limit_line(ui: &mut egui::Ui, limits: &[LimiterLine]) {
        if limits.is_empty() {
            return;
        }

        let tight = limits.iter().any(LimiterLine::is_tight);

        let caption = limits
            .iter()
            .map(LimiterLine::caption)
            .collect::<Vec<String>>()
            .join("  ");

        ui.label(egui::RichText::new(caption).small().color(match tight {
            true => WARNING,
            false => MUTED,
        }))
        .on_hover_text("how much of the trade api rate limit is used");
    }

    pub const STATUS_VIEWPORT: &str = "poe-wayfinder-status";
    const STATUS_SIZE: [f32; 2] = [560.0, 520.0];
    const STATUS_MIN: [f32; 2] = [440.0, 380.0];

    pub fn icon_data(image: &crate::assets::Image) -> std::sync::Arc<egui::IconData> {
        std::sync::Arc::new(egui::IconData {
            rgba: image.rgba.to_vec(),
            width: image.width,
            height: image.height,
        })
    }

    pub fn status_viewport() -> egui::ViewportBuilder {
        egui::ViewportBuilder::default()
            .with_title("PoE Wayfinder")
            .with_inner_size(STATUS_SIZE)
            .with_min_inner_size(STATUS_MIN)
            .with_resizable(true)
            .with_icon(icon_data(crate::assets::window_icon()))
    }

    pub const SPLASH_VIEWPORT: &str = "poe-wayfinder-splash";
    pub const SPLASH_WINDOW_TITLE: &str = "PoE Wayfinder is starting";
    const SPLASH_SIDE: f32 = 420.0;
    const SPLASH_HEIGHT: f32 = 500.0;

    fn screen_centre(points_per_pixel: f32) -> egui::Pos2 {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

        let width = unsafe { GetSystemMetrics(SM_CXSCREEN) } as f32;
        let height = unsafe { GetSystemMetrics(SM_CYSCREEN) } as f32;

        let scale = if points_per_pixel > 0.0 {
            points_per_pixel
        } else {
            1.0
        };

        egui::pos2(
            (width / scale - SPLASH_SIDE) / 2.0,
            (height / scale - SPLASH_HEIGHT) / 2.0,
        )
    }

    pub fn drop_splash_background() -> bool {
        use windows::core::HSTRING;
        use windows::Win32::Foundation::COLORREF;
        use windows::Win32::UI::WindowsAndMessaging::{
            FindWindowW, GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW,
            GWL_EXSTYLE, LWA_COLORKEY, WS_EX_LAYERED,
        };

        let title = HSTRING::from(SPLASH_WINDOW_TITLE);

        let Ok(handle) = (unsafe { FindWindowW(None, &title) }) else {
            return false;
        };

        if handle.is_invalid() {
            return false;
        }

        unsafe {
            let style = GetWindowLongPtrW(handle, GWL_EXSTYLE);

            if style & WS_EX_LAYERED.0 as isize != 0 {
                return true;
            }

            SetWindowLongPtrW(handle, GWL_EXSTYLE, style | WS_EX_LAYERED.0 as isize);

            SetLayeredWindowAttributes(handle, COLORREF(0x00_00_00), 0, LWA_COLORKEY).is_ok()
        }
    }

    pub fn splash_window(ctx: &egui::Context, fade: f32) -> bool {
        let mut dismissed = false;

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of(SPLASH_VIEWPORT),
            egui::ViewportBuilder::default()
                .with_title(SPLASH_WINDOW_TITLE)
                .with_inner_size([SPLASH_SIDE, SPLASH_HEIGHT])
                .with_position(screen_centre(ctx.pixels_per_point()))
                .with_decorations(false)
                .with_transparent(true)
                .with_always_on_top()
                .with_taskbar(false)
                .with_resizable(false),
            |ctx, _class| {
                let id = egui::Id::new(SPLASH_VIEWPORT);

                let texture = match ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
                    Some(texture) => texture,
                    None => {
                        let image = crate::assets::SPLASH;

                        let loaded = ctx.load_texture(
                            "splash",
                            egui::ColorImage::from_rgba_unmultiplied(
                                [image.width as usize, image.height as usize],
                                image.rgba,
                            ),
                            egui::TextureOptions::LINEAR,
                        );

                        ctx.data_mut(|d| d.insert_temp(id, loaded.clone()));

                        loaded
                    }
                };

                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        let tint = egui::Color32::from_white_alpha((fade * 255.0) as u8);

                        ui.vertical_centered(|ui| {
                            ui.add(
                                egui::Image::new(&texture)
                                    .maintain_aspect_ratio(true)
                                    .fit_to_exact_size(egui::vec2(SPLASH_SIDE, SPLASH_SIDE))
                                    .tint(tint),
                            );

                            egui::Frame::new()
                                .fill(SECTION_BACKGROUND.gamma_multiply(fade))
                                .inner_margin(egui::Margin::symmetric(22, 10))
                                .corner_radius(10)
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new("PoE Wayfinder")
                                            .size(34.0)
                                            .strong()
                                            .color(ACCENT.gamma_multiply(fade)),
                                    );
                                });
                        });
                    });

                if ctx.input(|i| {
                    i.pointer.any_click()
                        || i.viewport().close_requested()
                        || !i.events.is_empty()
                            && i.events
                                .iter()
                                .any(|e| matches!(e, egui::Event::Key { .. }))
                }) {
                    dismissed = true;
                }
            },
        );

        dismissed
    }

    pub fn status_window(
        ctx: &egui::Context,
        status: &Status,
        now: SystemTime,
        widgets: &mut Widgets,
        names: &[String],
        bindings: &[(String, String)],
        now_ms: u64,
    ) -> Vec<StatusEvent> {
        let mut events = Vec::new();

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of(STATUS_VIEWPORT),
            status_viewport(),
            |ctx, _class| {
                style(ctx);

                egui::TopBottomPanel::bottom("status-actions")
                    .frame(
                        egui::Frame::new()
                            .fill(PANEL_BACKGROUND)
                            .inner_margin(egui::Margin::symmetric(14, 10)),
                    )
                    .show(ctx, |ui| {
                        events.extend(status_actions(ui, status));
                    });

                let from_tabs = egui::CentralPanel::default()
                    .frame(
                        egui::Frame::new()
                            .fill(PANEL_BACKGROUND)
                            .inner_margin(egui::Margin::same(14)),
                    )
                    .show(ctx, |ui| {
                        widget_tabs(ui, widgets);

                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| match widgets.tab {
                                Tab::Status => status_body(ui, status, now),
                                _ => widget_body(
                                    ui,
                                    widgets,
                                    names,
                                    bindings,
                                    now_ms,
                                    status.client_log_found,
                                    status.game == Some(GameVersion::Poe2),
                                    status.pad_held,
                                    status.pad_family,
                                ),
                            })
                            .inner
                    });

                events.extend(from_tabs.inner);

                if ctx.input(|i| i.viewport().close_requested()) {
                    events.push(StatusEvent::HideToTray);
                }
            },
        );

        events
    }

    pub fn widget_tabs(ui: &mut egui::Ui, widgets: &mut Widgets) {
        ui.horizontal_wrapped(|ui| {
            for tab in Tab::every() {
                let picked = widgets.tab == tab;

                if ui.selectable_label(picked, tab.title()).clicked() {
                    widgets.show(tab);
                }
            }
        });

        ui.add_space(8.0);
    }

    fn binding_rows(
        ui: &mut egui::Ui,
        widgets: &mut Widgets,
        pad_held: u16,
        pad_family: PadFamily,
    ) -> Vec<StatusEvent> {
        use poe_wayfinder_core::controller::bind_capture::{caption, prompt, shown, Row};

        let mut events = Vec::new();

        ui.label(egui::RichText::new("Price check").size(13.0).color(ACCENT));

        for (row, text) in [
            (Row::Keyboard, widgets.bound_hotkey.clone()),
            (Row::Pad, widgets.bound_chord.clone()),
        ] {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(caption(row)).small().color(MUTED));
                ui.label(egui::RichText::new(shown(row, &text, pad_family)).size(13.0));

                let listening = widgets.capture.listening_to();

                if ui
                    .button(egui::RichText::new(prompt(listening, row)).small())
                    .clicked()
                {
                    match listening == Some(row) {
                        true => widgets.capture.stop(),
                        false => widgets.capture.listen(row),
                    }
                }
            });
        }

        if let Some(bound) = capture_now(ui.ctx(), widgets, pad_held, pad_family) {
            events.push(StatusEvent::Bound(bound));
        }

        events
    }

    fn capture_now(
        ctx: &egui::Context,
        widgets: &mut Widgets,
        pad_held: u16,
        pad_family: PadFamily,
    ) -> Option<poe_wayfinder_core::controller::bind_capture::Binding> {
        use poe_wayfinder_core::controller::bind_capture::Row;

        match widgets.capture.listening_to()? {
            Row::Pad => widgets.capture.from_pad(pad_held, pad_family),
            Row::Keyboard => {
                let pressed = ctx.input(|input| {
                    input.events.iter().find_map(|event| match event {
                        egui::Event::Key {
                            key,
                            pressed: true,
                            modifiers,
                            ..
                        } => Some((key.name().to_string(), *modifiers)),
                        _ => None,
                    })
                })?;

                widgets
                    .capture
                    .from_key(&pressed.0, pressed.1.ctrl, pressed.1.shift, pressed.1.alt)
            }
        }
    }

    fn stash_search_buttons(ui: &mut egui::Ui, model: &dyn PanelSource) -> Vec<UiEvent> {
        use poe_wayfinder_core::controller::item_links::{same_priced_from_note, similar_items};

        let mut events = Vec::new();

        let Some(check) = model.result() else {
            return events;
        };

        if let Some(text) = similar_items(&check.item.info.name) {
            if ui
                .button(egui::RichText::new("similar").small())
                .on_hover_text("search your own stash for this item by name")
                .clicked()
            {
                events.push(UiEvent::SearchStash(text));
            }
        }

        if let Some(text) = check.item.note.as_deref().and_then(same_priced_from_note) {
            if ui
                .button(egui::RichText::new("same price").small())
                .on_hover_text("search your own stash for everything priced the same")
                .clicked()
            {
                events.push(UiEvent::SearchStash(text));
            }
        }

        events
    }

    fn base_percentile_note(ui: &mut egui::Ui, model: &dyn PanelSource) {
        use poe_wayfinder_core::controller::filter::item_property::base_percentile_filter;

        let Some(check) = model.result() else {
            return;
        };

        let Some(base) = base_percentile_filter(check.item.base_percentile) else {
            return;
        };

        let colour = match base.enabled {
            true => ACCENT,
            false => MUTED,
        };

        ui.label(
            egui::RichText::new(format!("base {:.0}%", base.value))
                .small()
                .color(colour),
        )
        .on_hover_text(
            "how good this item's base roll is out of the range it could have had. \
             The trade site cannot search on it, so this is information only.",
        );
    }

    fn missing_note(ui: &mut egui::Ui, missing: &help::Missing) {
        ui.label(egui::RichText::new(missing.what).size(13.0).color(ACCENT));
        ui.label(egui::RichText::new(missing.why).small().color(MUTED));
        ui.add_space(2.0);
        ui.label(egui::RichText::new(missing.how).small());
    }

    pub fn widget_body(
        ui: &mut egui::Ui,
        widgets: &mut Widgets,
        names: &[String],
        bindings: &[(String, String)],
        now_ms: u64,
        client_log_found: bool,
        poe2: bool,
        pad_held: u16,
        pad_family: PadFamily,
    ) -> Vec<StatusEvent> {
        let mut events = Vec::new();

        if !client_log_found && matches!(widgets.tab, Tab::Log | Tab::Leveling) {
            missing_note(ui, &help::client_log_is_missing(poe2));

            return events;
        }

        match widgets.tab {
            Tab::Status => {}

            Tab::Library => {
                let logged = widgets.logged();

                if logged.is_empty() {
                    ui.label(egui::RichText::new("Nothing priced yet.").color(MUTED));
                }

                for total in widgets.totals() {
                    ui.label(egui::RichText::new(format!("Session total {total}")).color(ACCENT));
                }

                ui.add_space(4.0);

                for line in logged {
                    ui.label(line);
                }

                if !widgets.library.is_empty() {
                    ui.add_space(6.0);

                    if ui
                        .button(egui::RichText::new("copy as csv").small())
                        .on_hover_text("puts the whole session on the clipboard")
                        .clicked()
                    {
                        events.push(StatusEvent::CopyCsv(
                            poe_wayfinder_core::controller::library::to_csv(
                                widgets.library.entries(),
                            ),
                        ));
                    }
                }
            }

            Tab::Maps => {
                ui.label(egui::RichText::new(widgets.map_headline()).color(
                    match widgets.map_verdict() {
                        Verdict::Deadly => egui::Color32::from_rgb(226, 96, 96),
                        Verdict::Warning => WARNING,
                        _ => MUTED,
                    },
                ));

                ui.label(
                    egui::RichText::new(format!("{} marked", widgets.marked()))
                        .small()
                        .color(MUTED),
                );

                ui.add_space(4.0);

                let mut cycled = None;

                for (index, concern) in widgets.concerns.iter().enumerate() {
                    let colour = match concern.verdict {
                        Verdict::Deadly => egui::Color32::from_rgb(226, 96, 96),
                        Verdict::Warning => WARNING,
                        Verdict::Good => ONLINE_DOT,
                        _ => MUTED,
                    };

                    if ui
                        .selectable_label(
                            concern.verdict.is_coloured(),
                            egui::RichText::new(&concern.text).small().color(colour),
                        )
                        .on_hover_text("click to mark it deadly, then warning, then good")
                        .clicked()
                    {
                        cycled = Some(index);
                    }
                }

                if let Some(index) = cycled {
                    if let Some((matcher, set)) = widgets.cycle_verdict(index) {
                        events.push(StatusEvent::MarkMap { matcher, set });
                    }
                }
            }

            Tab::Search => {
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut widgets.needle);
                });

                ui.add_space(4.0);

                let mut clicked = None;
                let mut priced = None;

                for hit in widgets.hits(names) {
                    let starred = widgets.starred.is_starred(&hit.name);

                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(starred, &hit.name)
                            .on_hover_text("click to keep it in the list")
                            .clicked()
                        {
                            clicked = Some(hit.name.clone());
                        }

                        if ui
                            .small_button("Price")
                            .on_hover_text("price this base with no item in hand")
                            .clicked()
                        {
                            priced = Some(hit.name.clone());
                        }
                    });
                }

                if let Some(name) = clicked {
                    widgets.starred.toggle_star(&name);
                }

                if let Some(name) = priced {
                    events.push(StatusEvent::PriceByName(name));
                }

                if !widgets.starred.starred().is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Kept").small().color(MUTED));

                    for name in widgets.starred.starred() {
                        ui.label(egui::RichText::new(name).small());
                    }

                    if ui.button("Clear kept").clicked() {
                        widgets.starred.clear_stars();
                    }
                }
            }

            Tab::Notes => {
                let mut text = widgets.notepad.text().to_string();

                if ui.text_edit_multiline(&mut text).changed() {
                    widgets.notepad.write(&text);
                }

                ui.label(
                    egui::RichText::new(widgets.note_line())
                        .small()
                        .color(MUTED),
                );
            }

            Tab::Log => {
                if widgets.log_lines.is_empty() {
                    ui.label(egui::RichText::new("The client log is quiet.").color(MUTED));
                }

                for line in widgets.log_lines.iter().rev().take(40) {
                    ui.label(egui::RichText::new(line).small());
                }
            }

            Tab::Leveling => {
                ui.label(
                    egui::RichText::new(widgets.background.who_you_are())
                        .size(13.0)
                        .color(ACCENT),
                );
                ui.label(
                    egui::RichText::new(widgets.background.where_you_are())
                        .small()
                        .color(MUTED),
                );

                ui.add_space(8.0);

                match widgets.levelling_step() {
                    Some(step) => {
                        ui.label(
                            egui::RichText::new(format!("Act {}, {}", step.act, step.zone))
                                .size(15.0)
                                .color(ACCENT),
                        );
                        ui.label(egui::RichText::new(step.doing).small());
                    }
                    None => {
                        ui.label(
                            egui::RichText::new("No character seen in the client log yet.")
                                .color(MUTED),
                        );
                    }
                }

                ui.add_space(8.0);
                ui.label(egui::RichText::new("Next").small().color(MUTED));

                for step in widgets.levelling_next() {
                    ui.label(
                        egui::RichText::new(format!(
                            "level {}  act {}  {}",
                            step.from_level, step.act, step.zone
                        ))
                        .small(),
                    );
                }
            }

            Tab::Settings => {
                events.extend(binding_rows(ui, widgets, pad_held, pad_family));

                ui.separator();

                ui.label(
                    egui::RichText::new(
                        "These live in settings.json beside the app, and in the spec.",
                    )
                    .small()
                    .color(MUTED),
                );

                ui.add_space(6.0);

                for field in switches() {
                    ui.label(egui::RichText::new(field.label()).size(13.0));
                    ui.label(egui::RichText::new(field.explains()).small().color(MUTED));
                    ui.add_space(4.0);
                }

                for field in sliders() {
                    let bounds = bounds_of(field).expect("a slider has bounds");

                    ui.label(egui::RichText::new(field.label()).size(13.0));
                    ui.label(
                        egui::RichText::new(format!(
                            "{}, between {} and {}",
                            field.explains(),
                            bounds.low,
                            bounds.high
                        ))
                        .small()
                        .color(MUTED),
                    );
                    ui.add_space(4.0);
                }
            }
            Tab::Help => {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("Timer {}", widgets.elapsed(now_ms)))
                            .size(18.0)
                            .color(ACCENT),
                    );

                    if ui
                        .button(match widgets.stopwatch.is_running() {
                            true => "Stop",
                            false => "Start",
                        })
                        .clicked()
                    {
                        widgets.stopwatch.toggle(now_ms);
                    }

                    if ui.button("Reset").clicked() {
                        widgets.reset_timer();
                    }
                });

                ui.add_space(8.0);

                let (listed, width) = widgets.help(bindings);

                for entry in listed {
                    ui.label(
                        egui::RichText::new(format!(
                            "{:width$}   {}",
                            entry.keys,
                            entry.does,
                            width = width
                        ))
                        .small()
                        .monospace(),
                    );
                }
            }
        }
        events
    }

    fn status_body(ui: &mut egui::Ui, status: &Status, now: SystemTime) -> Vec<StatusEvent> {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("PoE Wayfinder")
                    .size(20.0)
                    .strong()
                    .color(ACCENT),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                health_pill(ui, health(status));
            });
        });

        ui.add_space(6.0);
        ui.label(egui::RichText::new(headline(status)).size(13.0));
        ui.add_space(12.0);

        egui::Frame::new()
            .fill(SECTION_BACKGROUND)
            .inner_margin(egui::Margin::same(10))
            .corner_radius(4)
            .show(ui, |ui| {
                for (label, value) in rows(status, now) {
                    status_row(ui, label, &value);
                }
            });

        let events = switchers(ui, status);

        if let Some(note) = &status.note {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(note).small().color(WARNING));
        }

        ui.add_space(8.0);
        rate_limit_line(ui, &status.limits);

        events
    }

    fn switchers(ui: &mut egui::Ui, status: &Status) -> Vec<StatusEvent> {
        let mut events = Vec::new();

        ui.add_space(10.0);

        ui.horizontal_wrapped(|ui| {
            switcher_label(ui, "Watch");

            for option in &status.game_menu {
                if ui
                    .selectable_label(option.selected, option.label)
                    .on_hover_text("pin a game, or let the window in front decide")
                    .clicked()
                    && !option.selected
                {
                    events.push(StatusEvent::ChooseGame(option.choice));
                }
            }
        });

        ui.horizontal(|ui| {
            switcher_label(ui, "Search");

            egui::ComboBox::from_id_salt("league-picker")
                .selected_text(chosen_league(status))
                .width(260.0)
                .show_ui(ui, |ui| {
                    for option in &status.league_menu.options {
                        if ui
                            .selectable_label(option.selected, &option.label)
                            .clicked()
                            && !option.selected
                        {
                            events.push(StatusEvent::ChooseLeague(option.choice.clone()));
                        }
                    }
                });
        });

        ui.label(
            egui::RichText::new(status.league_menu.caption)
                .small()
                .color(match status.league_menu.list_was_read {
                    true => MUTED,
                    false => WARNING,
                }),
        );

        events
    }

    fn switcher_label(ui: &mut egui::Ui, text: &str) {
        ui.add_sized(
            [82.0, 18.0],
            egui::Label::new(egui::RichText::new(text).small().color(MUTED)),
        );
    }

    fn chosen_league(status: &Status) -> String {
        status
            .league_menu
            .options
            .iter()
            .find(|option| option.selected)
            .map(|option| option.label.clone())
            .unwrap_or_else(|| status.league.clone())
    }

    fn status_actions(ui: &mut egui::Ui, status: &Status) -> Vec<StatusEvent> {
        let mut events = Vec::new();

        let pause = match status.paused {
            true => "Resume",
            false => "Pause",
        };

        ui.horizontal_wrapped(|ui| {
            if ui.button(pause).clicked() {
                events.push(StatusEvent::TogglePaused);
            }

            if ui
                .button("Refresh data")
                .on_hover_text("fetch the stat and item tables from the trade site now")
                .clicked()
            {
                events.push(StatusEvent::RefreshNow);
            }

            if ui
                .button("Hide to tray")
                .on_hover_text("keeps running. The tray icon brings this back.")
                .clicked()
            {
                events.push(StatusEvent::HideToTray);
            }

            if ui.button("Quit").clicked() {
                events.push(StatusEvent::Quit);
            }
        });

        events
    }

    fn status_row(ui: &mut egui::Ui, label: &str, value: &str) {
        ui.horizontal(|ui| {
            ui.add_sized(
                [82.0, 18.0],
                egui::Label::new(egui::RichText::new(label).small().color(MUTED)),
            );

            ui.label(egui::RichText::new(value).size(13.0));
        });
    }

    fn health_pill(ui: &mut egui::Ui, health: Health) {
        let (text, colour) = match health {
            Health::Ready => ("Ready", ONLINE_DOT),
            Health::Waiting => ("Waiting", MUTED),
            Health::Paused => ("Paused", WARNING),
            Health::Degraded => ("Needs attention", WARNING),
        };

        egui::Frame::new()
            .fill(ROW_BACKGROUND)
            .inner_margin(egui::Margin::symmetric(8, 3))
            .corner_radius(9)
            .show(ui, |ui| {
                ui.label(egui::RichText::new(text).small().color(colour));
            });
    }
}
#[cfg(windows)]
pub use win::{
    drop_splash_background, overlay_viewport, paint, splash_window, status_viewport, status_window,
    widget_body, widget_tabs, PadView, SPLASH_VIEWPORT, SPLASH_WINDOW_TITLE, STATUS_VIEWPORT,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::overlay_controller::OverlayModel;
    use crate::types::overlay::{OverlayGeometry, WindowRect};
    use poe_wayfinder_core::controller::bulk::Endpoint;
    use poe_wayfinder_core::controller::price_check::PriceCheck;
    use poe_wayfinder_core::types::item::{BaseInfo, ItemRarity, ParsedItem, UnknownModifier};
    use poe_wayfinder_core::types::modifier::ModifierType;
    use poe_wayfinder_core::types::query::TradeQuery;

    #[test]
    fn an_unbounded_value_never_reaches_the_panel_as_a_saturated_integer() {
        for value in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            assert_eq!(format_value(value, false), NO_VALUE, "{value}");
            assert_eq!(format_value(value, true), NO_VALUE, "{value}");
        }
    }

    #[test]
    fn the_saturating_cast_is_what_printed_i64_max_in_the_panel() {
        assert_eq!(f64::INFINITY.round() as i64, 9_223_372_036_854_775_807);
        assert_ne!(format_value(f64::INFINITY, false), "9223372036854775807");
    }

    #[test]
    fn an_ordinary_value_is_unaffected() {
        assert_eq!(format_value(17.4, false), "17");
        assert_eq!(format_value(17.4, true), "17.40");
        assert_eq!(format_value(-3.0, false), "-3");
    }

    fn check(item: ParsedItem) -> PriceCheck {
        PriceCheck {
            item,
            query: TradeQuery::default(),
            endpoint: Endpoint::Search,
            trade_tag: None,
            sources: Vec::new(),
        }
    }

    fn ring() -> ParsedItem {
        ParsedItem {
            rarity: Some(ItemRarity::Rare),
            item_level: Some(78),
            info: BaseInfo {
                name: "Doom Loop".into(),
                reference_name: "Sapphire Ring".into(),
                ..BaseInfo::default()
            },
            ..ParsedItem::default()
        }
    }

    fn showing(item: ParsedItem, total: u64) -> OverlayModel {
        let mut m = OverlayModel::new(OverlayGeometry::default());
        m.start((0, 0));
        m.finish(check(item), total);

        m
    }

    #[test]
    fn a_hidden_overlay_says_nothing() {
        let m = OverlayModel::new(OverlayGeometry::default());

        let t = panel_text(&m);

        assert!(t.title.is_empty());
        assert!(t.body.is_empty());
    }

    #[test]
    fn a_loading_overlay_says_it_is_working() {
        let mut m = OverlayModel::new(OverlayGeometry::default());
        m.start((0, 0));

        assert_eq!(panel_text(&m).title, "Checking price");
    }

    #[test]
    fn a_failed_check_shows_the_reason() {
        let mut m = OverlayModel::new(OverlayGeometry::default());
        m.fail("the trade api refused the search");

        let t = panel_text(&m);

        assert_eq!(t.title, "Price check failed");
        assert_eq!(t.body, vec!["the trade api refused the search"]);
    }

    #[test]
    fn a_result_is_titled_with_the_base_type() {
        let t = panel_text(&showing(ring(), 57));

        assert_eq!(t.title, "Sapphire Ring");
    }

    #[test]
    fn an_item_with_no_base_type_falls_back_to_its_name() {
        let mut item = ring();
        item.info.reference_name = String::new();

        assert_eq!(panel_text(&showing(item, 1)).title, "Doom Loop");
    }

    #[test]
    fn the_listing_count_is_singular_when_there_is_one() {
        assert_eq!(
            panel_text(&showing(ring(), 1)).subtitle.as_deref(),
            Some("1 listing")
        );
    }

    #[test]
    fn the_listing_count_is_plural_otherwise() {
        assert_eq!(
            panel_text(&showing(ring(), 57)).subtitle.as_deref(),
            Some("57 listings")
        );
    }

    #[test]
    fn no_listings_is_said_plainly_and_not_as_zero() {
        assert_eq!(
            panel_text(&showing(ring(), 0)).subtitle.as_deref(),
            Some("No listings match")
        );
    }

    #[test]
    fn the_body_carries_the_facts_the_user_checks() {
        let t = panel_text(&showing(ring(), 57));

        assert!(t.body.iter().any(|l| l.contains("Rare")));
        assert!(t.body.iter().any(|l| l.contains("78")));
    }

    #[test]
    fn corruption_is_stated_because_it_changes_the_price() {
        let item = ParsedItem {
            is_corrupted: true,
            ..ring()
        };

        assert!(panel_text(&showing(item, 1))
            .body
            .iter()
            .any(|l| l == "Corrupted"));
    }

    #[test]
    fn an_uncorrupted_item_says_nothing_about_corruption() {
        assert!(!panel_text(&showing(ring(), 1))
            .body
            .iter()
            .any(|l| l == "Corrupted"));
    }

    #[test]
    fn an_unrecognised_modifier_is_warned_about() {
        let item = ParsedItem {
            unknown_modifiers: vec![UnknownModifier {
                text: "Grants Sudden Enlightenment".into(),
                kind: ModifierType::Explicit,
            }],
            ..ring()
        };

        let t = panel_text(&showing(item, 57));

        assert_eq!(t.warnings.len(), 1);
        assert!(t.warnings[0].contains("Grants Sudden Enlightenment"));
    }

    #[test]
    fn a_fully_understood_item_carries_no_warning() {
        assert!(panel_text(&showing(ring(), 57)).warnings.is_empty());
    }

    #[test]
    fn every_unrecognised_modifier_gets_its_own_line() {
        let item = ParsedItem {
            unknown_modifiers: vec![
                UnknownModifier {
                    text: "First".into(),
                    kind: ModifierType::Explicit,
                },
                UnknownModifier {
                    text: "Second".into(),
                    kind: ModifierType::Implicit,
                },
            ],
            ..ring()
        };

        assert_eq!(panel_text(&showing(item, 1)).warnings.len(), 2);
    }

    #[test]
    fn a_showing_state_with_no_result_says_so_rather_than_going_blank() {
        let mut m = OverlayModel::new(OverlayGeometry::default());
        m.start((0, 0));
        m.finish(check(ring()), 1);
        m.fail("x");
        m.start((0, 0));

        let t = panel_text(&m);

        assert_eq!(t.title, "Checking price");
    }

    #[test]
    fn a_frame_with_a_rectangle_is_painted() {
        let frame = Frame {
            state: OverlayState::Showing,
            rect: Some(WindowRect::new(0, 0, 100, 100)),
            takes_input: true,
        };

        assert!(should_paint(&frame));
    }

    #[test]
    fn a_frame_with_no_rectangle_is_not_painted() {
        let frame = Frame {
            state: OverlayState::Showing,
            rect: None,
            takes_input: false,
        };

        assert!(!should_paint(&frame));
    }

    fn row(roll: Option<f64>, bounds: Option<(f64, f64)>, decimals: bool) -> Row {
        Row {
            key: RowKey::Stat { group: 0, index: 0 },
            label: "+80 to maximum Life".into(),
            enabled: true,
            min: Some(70.0),
            max: None,
            roll,
            bounds,
            decimals,
            tier: None,
            contributors: Vec::new(),
        }
    }

    #[test]
    fn a_whole_number_is_shown_without_a_decimal_point() {
        assert_eq!(format_value(78.0, false), "78");
    }

    #[test]
    fn a_rounded_value_does_not_read_as_one_less() {
        assert_eq!(format_value(77.6, false), "78");
    }

    #[test]
    fn a_dps_value_keeps_the_precision_that_separates_two_weapons() {
        assert_eq!(format_value(310.55, true), "310.55");
    }

    #[test]
    fn a_negative_value_survives_formatting() {
        assert_eq!(format_value(-12.0, false), "-12");
    }

    #[test]
    fn a_roll_is_captioned_with_the_tier_it_came_from() {
        assert_eq!(
            roll_caption(&row(Some(80.0), Some((60.0, 100.0)), false)).as_deref(),
            Some("80 of 60\u{2013}100")
        );
    }

    #[test]
    fn a_roll_with_no_known_tier_is_captioned_with_the_value_alone() {
        assert_eq!(
            roll_caption(&row(Some(80.0), None, false)).as_deref(),
            Some("80")
        );
    }

    #[test]
    fn a_filter_with_no_roll_has_nothing_to_caption() {
        assert_eq!(roll_caption(&row(None, None, false)), None);
    }

    #[test]
    fn the_search_button_says_what_it_will_do_after_an_edit() {
        assert_eq!(search_button_label(true), "Search with these filters");
        assert_eq!(search_button_label(false), "Search again");
    }
}
