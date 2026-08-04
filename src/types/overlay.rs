//! Where the overlay sits on screen.
//!
//! The overlay draws over the game window and has to follow it. The game can
//! move, resize, go fullscreen or move to a different monitor, and the overlay
//! has to end up in the right place every time.
//!
//! All of that is arithmetic on rectangles, so it lives here and is tested
//! without a window.

/// A rectangle in physical screen pixels.
///
/// Physical and not logical. The game reports physical pixels and mixing the
/// two puts the overlay in the wrong place on any display that is not at 100
/// percent scaling, which is most laptops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WindowRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl WindowRect {
    /// Build a rectangle.
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Whether the rectangle covers any pixels.
    ///
    /// A minimised window reports a zero or negative size on Windows, and
    /// drawing into it wastes a frame every tick.
    pub fn is_visible(&self) -> bool {
        self.width > 0 && self.height > 0
    }

    /// The right edge.
    pub fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    /// The bottom edge.
    pub fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }

    /// Whether a point is inside.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

/// Which corner or edge a widget is pinned to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Anchor {
    TopLeft,
    #[default]
    TopRight,
    BottomLeft,
    BottomRight,
    /// Follow the mouse. Used for the price check panel.
    Cursor,
}

/// Where a widget sits relative to the game window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlayGeometry {
    pub anchor: Anchor,
    /// Distance from the anchor, in logical pixels.
    pub offset_x: f32,
    pub offset_y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for OverlayGeometry {
    fn default() -> Self {
        Self {
            anchor: Anchor::TopRight,
            offset_x: 16.0,
            offset_y: 16.0,
            width: 400.0,
            height: 600.0,
        }
    }
}

impl OverlayGeometry {
    /// Work out where to draw, in physical pixels.
    ///
    /// `scale` is the display's scale factor. `cursor` is only read for the
    /// cursor anchor.
    ///
    /// The result is clamped inside the game window, because a panel that
    /// opens half off screen cannot be read or dismissed.
    pub fn place(&self, game: WindowRect, scale: f32, cursor: (i32, i32)) -> WindowRect {
        let width = (self.width * scale).round().max(1.0) as u32;
        let height = (self.height * scale).round().max(1.0) as u32;

        let offset_x = (self.offset_x * scale).round() as i32;
        let offset_y = (self.offset_y * scale).round() as i32;

        let (x, y) = match self.anchor {
            Anchor::TopLeft => (game.x + offset_x, game.y + offset_y),
            Anchor::TopRight => (game.right() - width as i32 - offset_x, game.y + offset_y),
            Anchor::BottomLeft => (game.x + offset_x, game.bottom() - height as i32 - offset_y),
            Anchor::BottomRight => (
                game.right() - width as i32 - offset_x,
                game.bottom() - height as i32 - offset_y,
            ),
            Anchor::Cursor => (cursor.0 + offset_x, cursor.1 + offset_y),
        };

        clamp_inside(WindowRect::new(x, y, width, height), game)
    }
}

/// Push a rectangle back inside its container.
///
/// The right and bottom edges are corrected first, then the left and top, so a
/// widget larger than the window ends up flush with the top left rather than
/// off the top. Reading the first line of a too tall panel beats reading none
/// of it.
fn clamp_inside(mut rect: WindowRect, container: WindowRect) -> WindowRect {
    if !container.is_visible() {
        return rect;
    }

    if rect.right() > container.right() {
        rect.x = container.right() - rect.width as i32;
    }

    if rect.bottom() > container.bottom() {
        rect.y = container.bottom() - rect.height as i32;
    }

    if rect.x < container.x {
        rect.x = container.x;
    }

    if rect.y < container.y {
        rect.y = container.y;
    }

    rect
}

/// What the overlay is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlayState {
    /// Nothing. The overlay is invisible and takes no input.
    #[default]
    Hidden,
    /// A price check is running.
    Loading,
    /// Results are up.
    Showing,
    /// Something failed and the message is up.
    Error,
}

impl OverlayState {
    /// Whether the overlay should be drawn at all.
    pub fn is_visible(self) -> bool {
        self != OverlayState::Hidden
    }

    /// Whether the overlay should take mouse input.
    ///
    /// Only when results are up. Taking clicks while loading would swallow a
    /// click the user meant for the game, and the game is the thing they are
    /// actually playing.
    pub fn takes_input(self) -> bool {
        self == OverlayState::Showing || self == OverlayState::Error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game() -> WindowRect {
        WindowRect::new(100, 50, 1920, 1080)
    }

    #[test]
    fn a_rectangle_reports_its_edges() {
        let r = WindowRect::new(10, 20, 100, 200);

        assert_eq!(r.right(), 110);
        assert_eq!(r.bottom(), 220);
    }

    #[test]
    fn a_zero_sized_rectangle_is_not_visible() {
        // A minimised window reports this on Windows, and drawing into it
        // wastes a frame every tick.
        assert!(!WindowRect::new(0, 0, 0, 1080).is_visible());
        assert!(!WindowRect::new(0, 0, 1920, 0).is_visible());
        assert!(WindowRect::new(0, 0, 1, 1).is_visible());
    }

    #[test]
    fn a_point_inside_is_contained_and_the_far_edges_are_not() {
        let r = WindowRect::new(10, 20, 100, 200);

        assert!(r.contains(10, 20));
        assert!(r.contains(109, 219));
        // The far edges belong to the next pixel.
        assert!(!r.contains(110, 219));
        assert!(!r.contains(109, 220));
        assert!(!r.contains(9, 20));
    }

    #[test]
    fn a_top_right_widget_hangs_off_the_right_edge() {
        let g = OverlayGeometry {
            anchor: Anchor::TopRight,
            offset_x: 16.0,
            offset_y: 16.0,
            width: 400.0,
            height: 600.0,
        };

        let got = g.place(game(), 1.0, (0, 0));

        assert_eq!(got.right(), game().right() - 16);
        assert_eq!(got.y, game().y + 16);
    }

    #[test]
    fn a_top_left_widget_hangs_off_the_left_edge() {
        let g = OverlayGeometry {
            anchor: Anchor::TopLeft,
            ..OverlayGeometry::default()
        };

        let got = g.place(game(), 1.0, (0, 0));

        assert_eq!(got.x, game().x + 16);
        assert_eq!(got.y, game().y + 16);
    }

    #[test]
    fn a_bottom_anchored_widget_sits_above_the_bottom_edge() {
        for anchor in [Anchor::BottomLeft, Anchor::BottomRight] {
            let g = OverlayGeometry {
                anchor,
                ..OverlayGeometry::default()
            };

            let got = g.place(game(), 1.0, (0, 0));

            assert_eq!(got.bottom(), game().bottom() - 16, "{anchor:?}");
        }
    }

    #[test]
    fn the_overlay_follows_the_game_window() {
        // The game can be moved at any time and the overlay has to end up in
        // the right place without being told.
        let g = OverlayGeometry::default();

        let before = g.place(WindowRect::new(0, 0, 1920, 1080), 1.0, (0, 0));
        let after = g.place(WindowRect::new(500, 300, 1920, 1080), 1.0, (0, 0));

        assert_eq!(after.x - before.x, 500);
        assert_eq!(after.y - before.y, 300);
    }

    #[test]
    fn a_scaled_display_scales_the_size_and_the_offset() {
        // Mixing logical and physical pixels puts the overlay in the wrong
        // place on any display above 100 percent, which is most laptops.
        let g = OverlayGeometry {
            anchor: Anchor::TopLeft,
            offset_x: 10.0,
            offset_y: 10.0,
            width: 100.0,
            height: 200.0,
        };

        let got = g.place(WindowRect::new(0, 0, 3840, 2160), 2.0, (0, 0));

        assert_eq!(got.x, 20);
        assert_eq!(got.y, 20);
        assert_eq!(got.width, 200);
        assert_eq!(got.height, 400);
    }

    #[test]
    fn a_cursor_anchored_widget_follows_the_mouse() {
        let g = OverlayGeometry {
            anchor: Anchor::Cursor,
            offset_x: 8.0,
            offset_y: 8.0,
            width: 100.0,
            height: 100.0,
        };

        let got = g.place(game(), 1.0, (500, 400));

        assert_eq!(got.x, 508);
        assert_eq!(got.y, 408);
    }

    #[test]
    fn a_widget_that_would_open_off_the_right_edge_is_pulled_back() {
        // A panel that opens half off screen cannot be read or dismissed.
        let g = OverlayGeometry {
            anchor: Anchor::Cursor,
            offset_x: 0.0,
            offset_y: 0.0,
            width: 400.0,
            height: 100.0,
        };

        let got = g.place(game(), 1.0, (game().right() - 50, 100));

        assert_eq!(got.right(), game().right());
    }

    #[test]
    fn a_widget_that_would_open_off_the_bottom_edge_is_pulled_up() {
        let g = OverlayGeometry {
            anchor: Anchor::Cursor,
            offset_x: 0.0,
            offset_y: 0.0,
            width: 100.0,
            height: 400.0,
        };

        let got = g.place(game(), 1.0, (200, game().bottom() - 50));

        assert_eq!(got.bottom(), game().bottom());
    }

    #[test]
    fn a_widget_that_would_open_off_the_left_edge_is_pushed_right() {
        let g = OverlayGeometry {
            anchor: Anchor::Cursor,
            offset_x: -500.0,
            offset_y: 0.0,
            width: 100.0,
            height: 100.0,
        };

        let got = g.place(game(), 1.0, (game().x + 10, 100));

        assert_eq!(got.x, game().x);
    }

    #[test]
    fn a_widget_taller_than_the_window_is_flush_with_the_top() {
        // Reading the first line of a too tall panel beats reading none of it.
        let g = OverlayGeometry {
            anchor: Anchor::Cursor,
            offset_x: 0.0,
            offset_y: 0.0,
            width: 100.0,
            height: 2000.0,
        };

        let got = g.place(WindowRect::new(0, 0, 800, 600), 1.0, (10, 10));

        assert_eq!(got.y, 0);
        // The width fits, so the horizontal position is untouched.
        assert_eq!(got.x, 10);
    }

    #[test]
    fn an_invisible_game_window_clamps_nothing() {
        // Clamping into a zero sized window would put the overlay at a
        // meaningless position and hide it entirely once the game restores.
        let g = OverlayGeometry {
            anchor: Anchor::Cursor,
            offset_x: 0.0,
            offset_y: 0.0,
            width: 100.0,
            height: 100.0,
        };

        let got = g.place(WindowRect::new(0, 0, 0, 0), 1.0, (500, 400));

        assert_eq!(got.x, 500);
        assert_eq!(got.y, 400);
    }

    #[test]
    fn a_widget_never_collapses_to_zero_size() {
        // A zero sized surface fails to create on Windows.
        let g = OverlayGeometry {
            width: 0.0,
            height: 0.0,
            ..OverlayGeometry::default()
        };

        let got = g.place(game(), 1.0, (0, 0));

        assert!(got.is_visible());
    }

    #[test]
    fn a_hidden_overlay_is_neither_drawn_nor_clickable() {
        let s = OverlayState::Hidden;

        assert!(!s.is_visible());
        assert!(!s.takes_input());
    }

    #[test]
    fn a_loading_overlay_is_drawn_but_takes_no_clicks() {
        // Taking clicks while loading swallows a click the user meant for the
        // game, and the game is what they are actually playing.
        let s = OverlayState::Loading;

        assert!(s.is_visible());
        assert!(!s.takes_input());
    }

    #[test]
    fn a_showing_overlay_takes_clicks() {
        assert!(OverlayState::Showing.takes_input());
        assert!(OverlayState::Error.takes_input());
    }

    #[test]
    fn the_overlay_starts_hidden() {
        // Anything else covers the game before the user has asked for
        // anything.
        assert_eq!(OverlayState::default(), OverlayState::Hidden);
    }
}
