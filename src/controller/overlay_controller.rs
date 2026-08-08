use poe_trader_core::controller::price_check::PriceCheck;

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
}

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub state: OverlayState,
    pub rect: Option<WindowRect>,
    pub takes_input: bool,
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

    pub fn finish(&mut self, result: PriceCheck, total: u64) {
        self.state = OverlayState::Showing;
        self.result = Some(result);
        self.total = Some(total);
        self.message = None;
    }

    pub fn fail(&mut self, message: &str) {
        self.state = OverlayState::Error;
        self.message = Some(message.to_string());

        self.result = None;
        self.total = None;
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
    use poe_trader_core::controller::bulk::Endpoint;
    use poe_trader_core::types::item::ParsedItem;
    use poe_trader_core::types::query::TradeQuery;

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
}
