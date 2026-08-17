use poe_wayfinder_core::controller::filter::augments::Augment;
use poe_wayfinder_core::controller::filter_view::{self, FilterView, FlagKey, RowKey};
use poe_wayfinder_core::controller::item_diff::{self, crafted_changes, Change};
use poe_wayfinder_core::controller::item_editor::{
    augment_options, empty_sockets, AugmentOption, ItemEditor,
};
use poe_wayfinder_core::controller::parse::shared::modifiers::ParsedModifier;
use poe_wayfinder_core::controller::price_check::PriceCheck;
use poe_wayfinder_core::controller::price_summary::{estimate_from, Estimate, Quote};
use poe_wayfinder_core::controller::rate_limit::LimiterLine;

use crate::adapter::game_window_adapter::{should_draw, GameWindow};
use crate::types::overlay::{OverlayGeometry, OverlayState, WindowRect};

#[derive(Debug, Clone, Default)]
pub struct OverlayModel {
    state: OverlayState,
    result: Option<PriceCheck>,
    total: Option<u64>,
    message: Option<String>,
    geometry: OverlayGeometry,
    anchor_cursor: (i32, i32),
    filters: FilterView,
    edited: bool,
    listings: Vec<Quote>,
    estimate: Option<Estimate>,
    augments: Vec<AugmentOption>,
    editor: ItemEditor,
    limits: Vec<LimiterLine>,
    note: Option<String>,
    last_affixes: Vec<ParsedModifier>,
    last_item_name: String,
    change: Change,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub state: OverlayState,
    pub rect: Option<WindowRect>,
    pub takes_input: bool,
}

pub trait PanelSource {
    fn state(&self) -> OverlayState;

    fn result(&self) -> Option<&PriceCheck>;

    fn total(&self) -> Option<u64>;

    fn message(&self) -> Option<&str>;

    fn filters(&self) -> &FilterView;

    fn edited(&self) -> bool;

    fn listings(&self) -> &[Quote];

    fn estimate(&self) -> Option<&Estimate>;

    fn augments(&self) -> &[AugmentOption];

    fn chosen_augment(&self) -> Option<&str>;

    fn limits(&self) -> &[LimiterLine];

    fn pacing_note(&self) -> Option<&str>;

    fn change_note(&self) -> Option<String>;
}

impl PanelSource for OverlayModel {
    fn state(&self) -> OverlayState {
        OverlayModel::state(self)
    }

    fn result(&self) -> Option<&PriceCheck> {
        OverlayModel::result(self)
    }

    fn total(&self) -> Option<u64> {
        OverlayModel::total(self)
    }

    fn message(&self) -> Option<&str> {
        OverlayModel::message(self)
    }

    fn filters(&self) -> &FilterView {
        OverlayModel::filters(self)
    }

    fn edited(&self) -> bool {
        OverlayModel::edited(self)
    }

    fn listings(&self) -> &[Quote] {
        OverlayModel::listings(self)
    }

    fn estimate(&self) -> Option<&Estimate> {
        OverlayModel::estimate(self)
    }

    fn augments(&self) -> &[AugmentOption] {
        OverlayModel::augments(self)
    }

    fn chosen_augment(&self) -> Option<&str> {
        OverlayModel::chosen_augment(self)
    }

    fn limits(&self) -> &[LimiterLine] {
        OverlayModel::limits(self)
    }

    fn pacing_note(&self) -> Option<&str> {
        OverlayModel::pacing_note(self)
    }

    fn change_note(&self) -> Option<String> {
        OverlayModel::change_since_the_last_check(self)
    }
}

impl OverlayModel {
    pub fn new(geometry: OverlayGeometry) -> Self {
        Self {
            geometry,
            ..Self::default()
        }
    }

    pub fn state(&self) -> OverlayState {
        self.state
    }

    pub fn result(&self) -> Option<&PriceCheck> {
        self.result.as_ref()
    }

    pub fn total(&self) -> Option<u64> {
        self.total
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub fn start(&mut self, cursor: (i32, i32)) {
        self.state = OverlayState::Loading;
        self.anchor_cursor = cursor;
        self.message = None;
    }

    pub fn filters(&self) -> &FilterView {
        &self.filters
    }

    pub fn edited(&self) -> bool {
        self.edited
    }

    pub fn set_enabled(&mut self, key: RowKey, enabled: bool) {
        if let Some(row) = self.filters.row_mut(key) {
            row.enabled = enabled;
            self.edited = true;
        }
    }

    pub fn set_min(&mut self, key: RowKey, min: Option<f64>) {
        if let Some(row) = self.filters.row_mut(key) {
            row.min = min;
            row.enabled = true;
            self.edited = true;
        }
    }

    pub fn set_max(&mut self, key: RowKey, max: Option<f64>) {
        if let Some(row) = self.filters.row_mut(key) {
            row.max = max;
            row.enabled = true;
            self.edited = true;
        }
    }

    pub fn set_all_stats(&mut self, enabled: bool) {
        self.filters.set_all_stats(enabled);
        self.edited = true;
    }

    pub fn cycle_name(&mut self) {
        self.filters.cycle_name();
        self.edited = true;
    }

    pub fn toggle_online(&mut self) {
        use poe_wayfinder_core::types::query::Status;

        let Some(check) = self.result.as_mut() else {
            return;
        };

        check.query.status = match check.query.status {
            Status::Online => Status::Any,
            Status::Any => Status::Online,
        };

        self.edited = true;
    }

    pub fn set_flag(&mut self, key: FlagKey, enabled: bool, value: bool) {
        if let Some(row) = self.filters.flag_mut(key) {
            row.enabled = enabled;
            row.value = value;
            self.edited = true;
        }
    }

    pub fn listings(&self) -> &[Quote] {
        &self.listings
    }

    pub fn estimate(&self) -> Option<&Estimate> {
        self.estimate.as_ref()
    }

    pub fn augments(&self) -> &[AugmentOption] {
        &self.augments
    }

    pub fn chosen_augment(&self) -> Option<&str> {
        self.editor.chosen()
    }

    pub fn limits(&self) -> &[LimiterLine] {
        &self.limits
    }

    pub fn note(&mut self, note: &str) {
        self.note = Some(note.to_string());
    }

    pub fn pacing_note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    pub fn set_limits(&mut self, limits: Vec<LimiterLine>) {
        self.limits = limits;
    }

    pub fn set_listings(&mut self, listings: Vec<Quote>) {
        self.estimate = estimate_from(&listings);
        self.listings = listings;
    }

    pub fn offer_augments(&mut self, augments: &[Augment]) {
        self.augments = match self.result.as_ref() {
            Some(check) => augment_options(augments, &check.item, empty_sockets(&check.item)),
            None => Vec::new(),
        };
    }

    pub fn choose_augment(&mut self, reference_name: &str, augments: &[Augment]) -> bool {
        let Some(check) = self.result.as_mut() else {
            return false;
        };

        let item = check.item.clone();
        let applied = self
            .editor
            .choose(reference_name, augments, &item, &mut check.query);

        if applied {
            self.rebuild_filters();
        }

        applied
    }

    pub fn clear_augment(&mut self) {
        let Some(check) = self.result.as_mut() else {
            return;
        };

        self.editor.clear_augment(&mut check.query);
        self.rebuild_filters();
    }

    fn rebuild_filters(&mut self) {
        if let Some(check) = self.result.as_ref() {
            self.filters = filter_view::build(check);
            self.edited = true;
        }
    }

    pub fn edited_check(&self) -> Option<PriceCheck> {
        let mut check = self.result.clone()?;

        filter_view::apply(&self.filters, &mut check.query);

        Some(check)
    }

    pub fn finish(&mut self, result: PriceCheck, total: u64) {
        self.state = OverlayState::Showing;
        self.change = match self.same_item_as_last(&result) {
            true => crafted_changes(&self.last_affixes, &result.item.modifiers),
            false => Change::default(),
        };
        self.last_affixes = result.item.modifiers.clone();
        self.last_item_name = result.item.info.reference_name.clone();
        self.filters = filter_view::build(&result);
        self.result = Some(result);
        self.total = Some(total);
        self.message = None;
        self.edited = false;
        self.listings = Vec::new();
        self.estimate = None;
        self.augments = Vec::new();
        self.editor = ItemEditor::default();
        self.note = None;
    }

    fn same_item_as_last(&self, result: &PriceCheck) -> bool {
        !self.last_item_name.is_empty() && self.last_item_name == result.item.info.reference_name
    }

    pub fn change_since_the_last_check(&self) -> Option<String> {
        match self.change.is_empty() {
            true => None,
            false => Some(item_diff::caption(&self.change)),
        }
    }

    pub fn fail(&mut self, message: &str) {
        self.state = OverlayState::Error;
        self.message = Some(message.to_string());

        self.result = None;
        self.total = None;
        self.filters = FilterView::default();
        self.edited = false;
        self.listings = Vec::new();
        self.estimate = None;
        self.augments = Vec::new();
        self.editor = ItemEditor::default();
    }

    pub fn warn(&mut self, message: &str) {
        self.state = OverlayState::Showing;
        self.message = Some(message.to_string());
    }

    pub fn hide(&mut self) {
        self.state = OverlayState::Hidden;
        self.message = None;
    }

    pub fn frame(&self, window: Option<GameWindow>) -> Frame {
        let hidden = Frame {
            state: self.state,
            rect: None,
            takes_input: false,
        };

        if !self.state.is_visible() {
            return hidden;
        }

        let Some(window) = window else {
            return hidden;
        };

        if !should_draw(&window) {
            return hidden;
        }

        Frame {
            state: self.state,
            rect: Some(self.geometry.place(window.rect, 1.0, self.anchor_cursor)),
            takes_input: self.state.takes_input(),
        }
    }

    pub fn frame_scaled(&self, window: Option<GameWindow>, scale: f32) -> Frame {
        let mut frame = self.frame(window);

        if let (Some(window), Some(_)) = (window, frame.rect) {
            frame.rect = Some(self.geometry.place(window.rect, scale, self.anchor_cursor));
        }

        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::overlay::Anchor;
    use poe_wayfinder_core::controller::bulk::Endpoint;
    use poe_wayfinder_core::types::item::ParsedItem;
    use poe_wayfinder_core::types::query::TradeQuery;

    fn game(foreground: bool) -> GameWindow {
        GameWindow {
            rect: WindowRect::new(0, 0, 1920, 1080),
            is_foreground: foreground,
        }
    }

    fn check() -> PriceCheck {
        PriceCheck {
            item: ParsedItem::default(),
            query: TradeQuery::default(),
            endpoint: Endpoint::Search,
            trade_tag: None,
            sources: Vec::new(),
        }
    }

    fn model() -> OverlayModel {
        OverlayModel::new(OverlayGeometry {
            anchor: Anchor::Cursor,
            offset_x: 0.0,
            offset_y: 0.0,
            width: 400.0,
            height: 300.0,
        })
    }

    #[test]
    fn a_new_overlay_is_hidden_and_draws_nothing() {
        let m = model();

        assert_eq!(m.state(), OverlayState::Hidden);
        assert_eq!(m.frame(Some(game(true))).rect, None);
    }

    #[test]
    fn starting_a_check_shows_the_loading_state() {
        let mut m = model();

        m.start((500, 400));

        assert_eq!(m.state(), OverlayState::Loading);
        assert!(m.frame(Some(game(true))).rect.is_some());
    }

    #[test]
    fn the_loading_overlay_takes_no_clicks() {
        let mut m = model();
        m.start((500, 400));

        assert!(!m.frame(Some(game(true))).takes_input);
    }

    #[test]
    fn a_finished_check_shows_its_result_and_takes_clicks() {
        let mut m = model();
        m.start((500, 400));
        m.finish(check(), 57);

        let f = m.frame(Some(game(true)));

        assert_eq!(m.state(), OverlayState::Showing);
        assert_eq!(m.total(), Some(57));
        assert!(m.result().is_some());
        assert!(f.takes_input);
    }

    #[test]
    fn the_old_result_survives_until_the_new_one_lands() {
        let mut m = model();
        m.start((0, 0));
        m.finish(check(), 57);

        m.start((0, 0));

        assert!(m.result().is_some());
        assert_eq!(m.total(), Some(57));
    }

    #[test]
    fn a_failure_drops_the_stale_result() {
        let mut m = model();
        m.start((0, 0));
        m.finish(check(), 57);

        m.fail("the trade api refused the search");

        assert_eq!(m.state(), OverlayState::Error);
        assert!(m.result().is_none());
        assert_eq!(m.total(), None);
        assert_eq!(m.message(), Some("the trade api refused the search"));
    }

    #[test]
    fn an_error_overlay_takes_clicks_so_it_can_be_dismissed() {
        let mut m = model();
        m.fail("something broke");

        assert!(m.frame(Some(game(true))).takes_input);
    }

    #[test]
    fn a_new_check_clears_the_old_error() {
        let mut m = model();
        m.fail("something broke");

        m.start((0, 0));

        assert_eq!(m.message(), None);
    }

    #[test]
    fn dismissing_hides_the_overlay() {
        let mut m = model();
        m.start((0, 0));
        m.finish(check(), 1);

        m.hide();

        assert_eq!(m.state(), OverlayState::Hidden);
        assert_eq!(m.frame(Some(game(true))).rect, None);
    }

    #[test]
    fn nothing_is_drawn_when_the_game_is_in_the_background() {
        let mut m = model();
        m.start((0, 0));
        m.finish(check(), 1);

        assert_eq!(m.frame(Some(game(false))).rect, None);
    }

    #[test]
    fn nothing_is_drawn_when_the_game_window_is_gone() {
        let mut m = model();
        m.start((0, 0));
        m.finish(check(), 1);

        assert_eq!(m.frame(None).rect, None);
    }

    #[test]
    fn the_state_is_still_reported_when_nothing_is_drawn() {
        let mut m = model();
        m.start((0, 0));
        m.finish(check(), 1);

        assert_eq!(m.frame(Some(game(false))).state, OverlayState::Showing);
    }

    #[test]
    fn the_panel_stays_where_the_check_started() {
        let mut m = model();
        m.start((500, 400));
        m.finish(check(), 1);

        let first = m.frame(Some(game(true))).rect.unwrap();
        let second = m.frame(Some(game(true))).rect.unwrap();

        assert_eq!(first, second);
        assert_eq!(first.x, 500);
        assert_eq!(first.y, 400);
    }

    #[test]
    fn a_second_check_moves_the_panel_to_the_new_cursor() {
        let mut m = model();
        m.start((500, 400));
        m.finish(check(), 1);

        m.start((900, 200));

        assert_eq!(m.frame(Some(game(true))).rect.unwrap().x, 900);
    }

    #[test]
    fn the_panel_follows_the_game_window() {
        let mut m = model();
        m.start((100, 100));
        m.finish(check(), 1);

        let moved = GameWindow {
            rect: WindowRect::new(2000, 0, 1920, 1080),
            is_foreground: true,
        };

        let rect = m.frame(Some(moved)).rect.unwrap();

        assert!(rect.x >= 2000);
    }

    #[test]
    fn a_scaled_display_scales_the_panel() {
        let mut m = model();
        m.start((0, 0));
        m.finish(check(), 1);

        let one = m.frame_scaled(Some(game(true)), 1.0).rect.unwrap();
        let two = m.frame_scaled(Some(game(true)), 2.0).rect.unwrap();

        assert_eq!(two.width, one.width * 2);
        assert_eq!(two.height, one.height * 2);
    }

    #[test]
    fn a_hidden_overlay_is_still_hidden_at_any_scale() {
        let m = model();

        assert_eq!(m.frame_scaled(Some(game(true)), 2.0).rect, None);
    }

    fn life_check() -> PriceCheck {
        use poe_wayfinder_core::controller::filter::stat_filters::FilterSource;
        use poe_wayfinder_core::types::query::{Range, StatFilter, StatGroup};
        use poe_wayfinder_core::types::stat::StatRoll;

        let mut query = TradeQuery::default();
        query.stats.push(StatGroup::all(vec![StatFilter::range(
            "explicit.stat_life",
            Range::at_least(70.0),
        )]));

        PriceCheck {
            item: ParsedItem {
                item_level: Some(78),
                is_corrupted: true,
                ..ParsedItem::default()
            },
            query,
            endpoint: Endpoint::Search,
            trade_tag: None,
            sources: vec![FilterSource {
                id: "explicit.stat_life".into(),
                text: "+80 to maximum Life".into(),
                roll: Some(StatRoll {
                    value: 80.0,
                    min: 60.0,
                    max: 100.0,
                    ..StatRoll::default()
                }),
                tier: Some(3),
                contributors: Vec::new(),
            }],
        }
    }

    fn stat_key() -> RowKey {
        RowKey::Stat { group: 0, index: 0 }
    }

    #[test]
    fn a_finished_check_offers_its_filters_for_editing() {
        let mut m = model();
        m.finish(life_check(), 57);

        assert_eq!(m.filters().stats.len(), 1);
        assert_eq!(m.filters().stats[0].label, "+80 to maximum Life");
    }

    #[test]
    fn a_fresh_result_counts_as_unedited() {
        let mut m = model();
        m.finish(life_check(), 57);

        assert!(!m.edited());
    }

    #[test]
    fn raising_a_minimum_marks_the_panel_edited() {
        let mut m = model();
        m.finish(life_check(), 57);

        m.set_min(stat_key(), Some(95.0));

        assert!(m.edited());
    }

    #[test]
    fn the_edited_minimum_reaches_the_query_that_gets_searched() {
        let mut m = model();
        m.finish(life_check(), 57);

        m.set_min(stat_key(), Some(95.0));

        let check = m.edited_check().expect("a check");

        assert_eq!(check.query.stats[0].filters[0].range.min, Some(95.0));
    }

    #[test]
    fn the_stored_result_is_left_alone_so_an_edit_can_be_undone() {
        let mut m = model();
        m.finish(life_check(), 57);

        m.set_min(stat_key(), Some(95.0));

        assert_eq!(
            m.result().expect("a result").query.stats[0].filters[0]
                .range
                .min,
            Some(70.0)
        );
    }

    #[test]
    fn typing_a_minimum_turns_the_filter_on_because_that_is_what_was_meant() {
        let mut m = model();
        m.finish(life_check(), 57);

        m.set_enabled(stat_key(), false);
        m.set_min(stat_key(), Some(95.0));

        assert!(!m.edited_check().expect("a check").query.stats[0].filters[0].disabled);
    }

    #[test]
    fn turning_a_filter_off_stops_it_constraining_the_search() {
        let mut m = model();
        m.finish(life_check(), 57);

        m.set_enabled(stat_key(), false);

        assert!(m.edited_check().expect("a check").query.stats[0].filters[0].disabled);
    }

    #[test]
    fn a_corrupted_item_offers_a_toggle_that_reaches_the_query() {
        let mut m = model();
        m.finish(life_check(), 57);

        m.set_flag(FlagKey::Corrupted, true, true);

        assert_eq!(
            m.edited_check()
                .expect("a check")
                .query
                .filters
                .misc_filters
                .corrupted,
            Some(true)
        );
    }

    #[test]
    fn an_item_level_typed_by_hand_reaches_the_query() {
        let mut m = model();
        m.finish(life_check(), 57);

        m.set_min(
            RowKey::Numeric(poe_wayfinder_core::controller::filter_view::NumericKey::ItemLevel),
            Some(84.0),
        );

        assert_eq!(
            m.edited_check()
                .expect("a check")
                .query
                .filters
                .type_filters
                .ilvl
                .min,
            Some(84.0)
        );
    }

    #[test]
    fn a_new_result_replaces_the_filters_and_clears_the_edit_mark() {
        let mut m = model();
        m.finish(life_check(), 57);
        m.set_min(stat_key(), Some(95.0));

        m.finish(check(), 1);

        assert!(!m.edited());
        assert!(m.filters().stats.is_empty());
    }

    #[test]
    fn a_failure_drops_the_filters_along_with_the_result() {
        let mut m = model();
        m.finish(life_check(), 57);

        m.fail("the trade api refused the search");

        assert!(m.filters().is_empty());
        assert!(m.edited_check().is_none());
    }

    #[test]
    fn editing_a_row_that_is_not_there_changes_nothing() {
        let mut m = model();
        m.finish(life_check(), 57);

        m.set_min(RowKey::Stat { group: 9, index: 9 }, Some(1.0));

        assert!(!m.edited());
    }

    #[test]
    fn a_maximum_can_be_set_on_its_own() {
        let mut m = model();
        m.finish(life_check(), 57);

        m.set_max(stat_key(), Some(120.0));

        assert_eq!(
            m.edited_check().expect("a check").query.stats[0].filters[0]
                .range
                .max,
            Some(120.0)
        );
    }

    fn ring_with(references: &[&str]) -> PriceCheck {
        use poe_wayfinder_core::controller::parse::shared::modifiers::ParsedModifier;
        use poe_wayfinder_core::types::item::BaseInfo;
        use poe_wayfinder_core::types::modifier::{Generation, ModifierInfo};
        use poe_wayfinder_core::types::stat::ParsedStat;

        PriceCheck {
            item: poe_wayfinder_core::types::item::ParsedItem {
                info: BaseInfo {
                    reference_name: "Sapphire Ring".into(),
                    ..BaseInfo::default()
                },
                modifiers: references
                    .iter()
                    .map(|reference| ParsedModifier {
                        info: ModifierInfo {
                            generation: Some(Generation::Prefix),
                            ..ModifierInfo::default()
                        },
                        stats: vec![ParsedStat {
                            reference: (*reference).to_string(),
                            matched: (*reference).to_string(),
                            roll: None,
                        }],
                    })
                    .collect(),
                ..poe_wayfinder_core::types::item::ParsedItem::default()
            },
            query: poe_wayfinder_core::types::query::TradeQuery::default(),
            endpoint: poe_wayfinder_core::controller::bulk::Endpoint::Search,
            trade_tag: None,
            sources: Vec::new(),
        }
    }

    #[test]
    fn the_first_check_of_an_item_reports_no_change() {
        let mut model = OverlayModel::default();

        model.finish(ring_with(&["+# to maximum Life"]), 1);

        assert_eq!(model.change_since_the_last_check(), None);
    }

    #[test]
    fn checking_the_same_base_again_reports_what_it_gained() {
        let mut model = OverlayModel::default();

        model.finish(ring_with(&["+# to maximum Life"]), 1);
        model.finish(
            ring_with(&["+# to maximum Life", "+#% to Fire Resistance"]),
            1,
        );

        assert_eq!(
            model.change_since_the_last_check().as_deref(),
            Some("1 gained since the last check")
        );
    }

    #[test]
    fn an_unchanged_item_reports_nothing_rather_than_a_line_saying_so() {
        let mut model = OverlayModel::default();

        model.finish(ring_with(&["+# to maximum Life"]), 1);
        model.finish(ring_with(&["+# to maximum Life"]), 1);

        assert_eq!(model.change_since_the_last_check(), None);
    }
}
