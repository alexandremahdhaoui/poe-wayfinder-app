use poe_wayfinder_core::controller::filter_view::{modifier_text, FlagKey, Row, RowKey};
use poe_wayfinder_core::types::item::ItemRarity;

use crate::controller::overlay_controller::{Frame, PanelSource};
use crate::types::overlay::OverlayState;

#[derive(Debug, Clone, PartialEq)]
pub enum StatusEvent {
    HideToTray,
    Quit,
    RefreshNow,
    TogglePaused,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    Dismiss,
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

pub fn format_value(value: f64, decimals: bool) -> String {
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
        Frame, PanelSource, StatusEvent, UiEvent,
    };

    use eframe::egui;
    use poe_wayfinder_core::controller::filter_view::{
        influence_labels, tier_label, FilterView, FlagRow, Row,
    };
    use poe_wayfinder_core::controller::item_editor::AugmentOption;
    use poe_wayfinder_core::controller::price_summary::{
        online_count, price_headline, price_spread, stack_value, Estimate, Quote,
    };
    use poe_wayfinder_core::controller::rate_limit::LimiterLine;

    use std::time::SystemTime;

    use crate::controller::status_controller::{headline, health, rows, Health, Status};

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

    const LISTINGS_SHOWN: usize = 8;
    const FOOTER_HEIGHT: f32 = 30.0;

    pub fn overlay_viewport(frame: &Frame) -> egui::ViewportBuilder {
        let mut builder = egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_taskbar(false)
            .with_resizable(false);

        if let Some(rect) = frame.rect {
            builder = builder
                .with_position(egui::pos2(rect.x as f32, rect.y as f32))
                .with_inner_size(egui::vec2(rect.width as f32, rect.height as f32));
        }

        builder.with_mouse_passthrough(!frame.takes_input)
    }

    pub fn paint(ctx: &egui::Context, model: &dyn PanelSource) -> Vec<UiEvent> {
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
                        filters(ui, model.filters(), &mut events);
                        listing_rows(ui, model);
                    });

                footer(ui, model, &mut events);
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
                if ui.button("✕").on_hover_text("close").clicked() {
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

    fn filters(ui: &mut egui::Ui, view: &FilterView, events: &mut Vec<UiEvent>) {
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
                for row in &view.numerics {
                    numeric_row(ui, row, events);
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

            for row in &view.stats {
                stat_row(ui, row, events);
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

    fn numeric_row(ui: &mut egui::Ui, row: &Row, events: &mut Vec<UiEvent>) {
        ui.horizontal(|ui| {
            checkbox(ui, row, events);

            ui.label(egui::RichText::new(&row.label).small());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                bounds_inputs(ui, row, events);
            });
        });
    }

    fn stat_row(ui: &mut egui::Ui, row: &Row, events: &mut Vec<UiEvent>) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                checkbox(ui, row, events);

                let label = modifier_line(&row.label, row.roll, row.decimals);

                ui.add(
                    egui::Label::new(egui::RichText::new(label).small().color(match row.enabled {
                        true => egui::Color32::from_rgb(214, 214, 222),
                        false => MUTED,
                    }))
                    .truncate(),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    bounds_inputs(ui, row, events);

                    if let Some(tier) = tier_label(row.tier) {
                        ui.label(egui::RichText::new(tier).small().color(ACCENT));
                    }
                });
            });

            contributor_line(ui, row);

            if row.bounds.is_some() {
                gauge(ui, row);
            }
        });
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

    fn checkbox(ui: &mut egui::Ui, row: &Row, events: &mut Vec<UiEvent>) {
        let mark = match row.enabled {
            true => "☑",
            false => "☐",
        };

        let hover = roll_caption(row).unwrap_or_else(|| "switch this filter".to_string());

        if ui
            .add(egui::Button::new(mark).frame(false))
            .on_hover_text(hover)
            .clicked()
        {
            events.push(UiEvent::ToggleRow(row.key));
        }
    }

    fn bounds_inputs(ui: &mut egui::Ui, row: &Row, events: &mut Vec<UiEvent>) {
        let step = match row.decimals {
            true => 0.01,
            false => 1.0,
        };

        let mut max = row.max.unwrap_or(f64::NAN);
        let max_widget = egui::DragValue::new(&mut max)
            .speed(step)
            .custom_formatter(|value, _| match value.is_nan() {
                true => "max".to_string(),
                false => format_value(value, row.decimals),
            });

        if ui.add_sized([52.0, 18.0], max_widget).changed() {
            events.push(UiEvent::SetMax(row.key, finite(max)));
        }

        let mut min = row.min.unwrap_or(f64::NAN);
        let min_widget = egui::DragValue::new(&mut min)
            .speed(step)
            .custom_formatter(|value, _| match value.is_nan() {
                true => "min".to_string(),
                false => format_value(value, row.decimals),
            });

        if ui.add_sized([52.0, 18.0], min_widget).changed() {
            events.push(UiEvent::SetMin(row.key, finite(min)));
        }
    }

    fn finite(value: f64) -> Option<f64> {
        value.is_finite().then_some(value)
    }

    fn gauge(ui: &mut egui::Ui, row: &Row) {
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 5.0), egui::Sense::hover());

        let painter = ui.painter();

        painter.rect_filled(rect, 3.0, GAUGE_TRACK);

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
                GAUGE_FILL,
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
                GAUGE_TICK,
            );
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
            let (dot, hint) = match listing.online {
                true => (ONLINE_DOT, "online"),
                false => (OFFLINE_DOT, "offline"),
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
        });
    }

    fn footer(ui: &mut egui::Ui, model: &dyn PanelSource, events: &mut Vec<UiEvent>) {
        ui.separator();

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

    pub fn status_viewport() -> egui::ViewportBuilder {
        egui::ViewportBuilder::default()
            .with_title("PoE Wayfinder")
            .with_inner_size(STATUS_SIZE)
            .with_min_inner_size(STATUS_MIN)
            .with_resizable(true)
    }

    pub fn status_window(
        ctx: &egui::Context,
        status: &Status,
        now: SystemTime,
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

                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::new()
                            .fill(PANEL_BACKGROUND)
                            .inner_margin(egui::Margin::same(14)),
                    )
                    .show(ctx, |ui| {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| status_body(ui, status, now));
                    });

                if ctx.input(|i| i.viewport().close_requested()) {
                    events.push(StatusEvent::HideToTray);
                }
            },
        );

        events
    }

    fn status_body(ui: &mut egui::Ui, status: &Status, now: SystemTime) -> Vec<StatusEvent> {
        let mut events = Vec::new();

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

        if let Some(note) = &status.note {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(note).small().color(WARNING));
        }

        ui.add_space(8.0);
        rate_limit_line(ui, &status.limits);

        events
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
pub use win::{overlay_viewport, paint, status_viewport, status_window, STATUS_VIEWPORT};

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
