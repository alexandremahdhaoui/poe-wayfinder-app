use crate::controller::overlay_controller::{Frame, OverlayModel};
use crate::types::overlay::OverlayState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiEvent {
    Dismiss,
    OpenInBrowser,
    ToggleFilter(usize),
    Research,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PanelText {
    pub title: String,
    pub subtitle: Option<String>,
    pub body: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn panel_text(model: &OverlayModel) -> PanelText {
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

fn showing_text(model: &OverlayModel) -> PanelText {
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
    use super::{panel_text, Frame, OverlayModel, UiEvent};

    use eframe::egui;

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

    pub fn paint(ctx: &egui::Context, model: &OverlayModel) -> Vec<UiEvent> {
        let text = panel_text(model);
        let mut events = Vec::new();

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::from_rgba_unmultiplied(16, 16, 20, 235))
                    .inner_margin(12.0)
                    .corner_radius(6.0),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(&text.title);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("close").clicked() {
                            events.push(UiEvent::Dismiss);
                        }
                    });
                });

                if let Some(subtitle) = &text.subtitle {
                    ui.label(egui::RichText::new(subtitle).weak());
                }

                ui.separator();

                for line in &text.body {
                    ui.label(line);
                }

                for warning in &text.warnings {
                    ui.label(
                        egui::RichText::new(warning).color(egui::Color32::from_rgb(240, 180, 60)),
                    );
                }

                if !text.warnings.is_empty() {
                    ui.label(
                        egui::RichText::new(
                            "The price may be wrong. Rebuild the data with poe-trader-datagen.",
                        )
                        .weak(),
                    );
                }

                ui.separator();

                ui.horizontal(|ui| {
                    if ui.button("Open in browser").clicked() {
                        events.push(UiEvent::OpenInBrowser);
                    }

                    if ui.button("Search again").clicked() {
                        events.push(UiEvent::Research);
                    }
                });
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
}

#[cfg(windows)]
pub use win::{overlay_viewport, paint};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::overlay::{OverlayGeometry, WindowRect};
    use poe_trader_core::controller::bulk::Endpoint;
    use poe_trader_core::controller::price_check::PriceCheck;
    use poe_trader_core::types::item::{BaseInfo, ItemRarity, ParsedItem, UnknownModifier};
    use poe_trader_core::types::modifier::ModifierType;
    use poe_trader_core::types::query::TradeQuery;

    fn check(item: ParsedItem) -> PriceCheck {
        PriceCheck {
            item,
            query: TradeQuery::default(),
            endpoint: Endpoint::Search,
            trade_tag: None,
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
}
