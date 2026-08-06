//! Deciding what the overlay shows and where.
//!
//! Pure. It takes the game window, the cursor and what the price check
//! produced, and returns what to draw. The drawing itself is the driver's job.
//!
//! Splitting it this way is what makes the overlay's behaviour testable. Every
//! rule below is one a user would notice, and none of them needs a window to
//! check.

use poe_trader_core::controller::price_check::PriceCheck;

use crate::adapter::game_window_adapter::{should_draw, GameWindow};
use crate::types::overlay::{OverlayGeometry, OverlayState, WindowRect};

/// What the overlay is currently holding.
#[derive(Debug, Clone, Default)]
pub struct OverlayModel {
    state: OverlayState,
    /// The last successful check.
    result: Option<PriceCheck>,
    /// How many listings matched.
    total: Option<u64>,
    /// The message to show in the error state.
    message: Option<String>,
    geometry: OverlayGeometry,
    /// Where the cursor was when the check started.
    ///
    /// Frozen at that moment, so the panel does not chase the mouse while the
    /// user is reading it.
    anchor_cursor: (i32, i32),
}

/// What the driver should draw this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub state: OverlayState,
    /// Where to put the window. None means draw nothing.
    pub rect: Option<WindowRect>,
    /// Whether the window should take mouse input.
    pub takes_input: bool,
}

impl OverlayModel {
    /// A hidden overlay.
    pub fn new(geometry: OverlayGeometry) -> Self {
        Self {
            geometry,
            ..Self::default()
        }
    }

    /// What it is showing.
    pub fn state(&self) -> OverlayState {
        self.state
    }

    /// The last successful check.
    pub fn result(&self) -> Option<&PriceCheck> {
        self.result.as_ref()
    }

    /// How many listings matched.
    pub fn total(&self) -> Option<u64> {
        self.total
    }

    /// The error message.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// A price check has started.
    ///
    /// The cursor is frozen here, so the panel does not chase the mouse while
    /// the user reads it.
    pub fn start(&mut self, cursor: (i32, i32)) {
        self.state = OverlayState::Loading;
        self.anchor_cursor = cursor;
        self.message = None;

        // The old result is kept until the new one lands. Clearing it makes
        // the panel flash empty on every check.
    }

    /// A price check finished.
    pub fn finish(&mut self, result: PriceCheck, total: u64) {
        self.state = OverlayState::Showing;
        self.result = Some(result);
        self.total = Some(total);
        self.message = None;
    }

    /// A price check failed.
    pub fn fail(&mut self, message: &str) {
        self.state = OverlayState::Error;
        self.message = Some(message.to_string());

        // The stale result is dropped. Showing an old price next to an error
        // reads as if the old price were the answer.
        self.result = None;
        self.total = None;
    }

    /// The user dismissed the overlay.
    pub fn hide(&mut self) {
        self.state = OverlayState::Hidden;
        self.message = None;
    }

    /// Work out what to draw.
    ///
    /// Returns a frame with no rectangle when nothing should be drawn, so the
    /// driver has one thing to check rather than three.
    pub fn frame(&self, window: Option<GameWindow>) -> Frame {
        let hidden = Frame {
            state: self.state,
            rect: None,
            takes_input: false,
        };

        if !self.state.is_visible() {
            return hidden;
        }

        // No game window means the game closed or minimised while the panel
        // was up. Drawing over the desktop is worse than showing nothing.
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

    /// Work out what to draw at a given display scale.
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
        // Anything else covers the game before the user has asked for
        // anything.
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
        // Taking them would swallow a click meant for the game, which is what
        // the user is actually playing.
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
        // Clearing it on start makes the panel flash empty on every check.
        let mut m = model();
        m.start((0, 0));
        m.finish(check(), 57);

        m.start((0, 0));

        assert!(m.result().is_some());
        assert_eq!(m.total(), Some(57));
    }

    #[test]
    fn a_failure_drops_the_stale_result() {
        // Showing an old price next to an error reads as if the old price were
        // the answer.
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
        // The user alt tabbed. Drawing over another application is the fastest
        // way to make a tool feel broken.
        let mut m = model();
        m.start((0, 0));
        m.finish(check(), 1);

        assert_eq!(m.frame(Some(game(false))).rect, None);
    }

    #[test]
    fn nothing_is_drawn_when_the_game_window_is_gone() {
        // The game closed or minimised while the panel was up. Drawing over
        // the desktop is worse than showing nothing.
        let mut m = model();
        m.start((0, 0));
        m.finish(check(), 1);

        assert_eq!(m.frame(None).rect, None);
    }

    #[test]
    fn the_state_is_still_reported_when_nothing_is_drawn() {
        // The driver needs to know the panel is still logically up, so it
        // reappears when the user alt tabs back.
        let mut m = model();
        m.start((0, 0));
        m.finish(check(), 1);

        assert_eq!(m.frame(Some(game(false))).state, OverlayState::Showing);
    }

    #[test]
    fn the_panel_stays_where_the_check_started() {
        // Chasing the mouse while the user reads it makes the panel unusable.
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

        // The cursor was at 100,100 which is outside the moved window, so the
        // panel is clamped back inside it.
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
