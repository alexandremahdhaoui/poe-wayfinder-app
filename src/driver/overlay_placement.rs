//! Where to put the overlay window, and why it is never hidden.
//!
//! # The bug this exists for
//!
//! The overlay used to hide its window whenever the game was not found, which
//! is the obvious way to stay out of the way. It made the whole tool dead.
//!
//! Windows does not repaint a hidden window. eframe runs the frame loop from a
//! repaint, so hiding the window stops the loop. The hotkey is only read inside
//! that loop. So the overlay started, found no game, hid itself, ran one frame
//! and then sat there forever: tray icon showing, hotkey registered, hook
//! installed, and nothing being read.
//!
//! It was worst in the ordinary case. Start the overlay, then launch the game,
//! and the loop that would have noticed the game had already stopped.
//!
//! Measured, not guessed: a frame counter logged one frame and never a second.
//!
//! # The rule
//!
//! The window is always visible to Windows. When there is nothing to draw over,
//! it is one pixel, fully click through, parked far off the desktop where no
//! monitor reaches. It keeps repainting, so the loop keeps running, and there
//! is nothing to see.

/// A game window's position on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    /// Unsigned, matching the window driver. A window cannot have a negative
    /// size, and the position can, which is why the two halves differ.
    pub width: u32,
    pub height: u32,
}

/// Where to put the overlay this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Always true. Kept as a field so the reason is visible at the call site
    /// rather than being an absence somebody helpfully adds back.
    pub visible: bool,
    /// Whether clicks go through to whatever is underneath.
    pub passthrough: bool,
}

/// Far enough off the desktop that no monitor arrangement reaches it.
///
/// Negative because a second monitor placed left of the primary one uses
/// negative coordinates, and a large negative is past any real one. Windows
/// stores window coordinates as 32 bit signed, so this is comfortably valid.
pub const PARKED: f32 = -32000.0;

/// Decide the placement.
///
/// `takes_input` is ignored when there is no game, because a parked window must
/// never swallow a click wherever it ends up.
pub fn placement(game: Option<Rect>, takes_input: bool) -> Placement {
    match game {
        Some(rect) => Placement {
            x: rect.x as f32,
            y: rect.y as f32,
            width: rect.width as f32,
            height: rect.height as f32,
            visible: true,
            passthrough: !takes_input,
        },

        None => Placement {
            x: PARKED,
            y: PARKED,
            width: 1.0,
            height: 1.0,
            visible: true,
            passthrough: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAME: Rect = Rect {
        x: 100,
        y: 200,
        width: 1920,
        height: 1080,
    };

    #[test]
    fn the_window_is_never_hidden() {
        // The whole point. Windows does not repaint a hidden window, eframe
        // runs the frame loop from a repaint, and the hotkey is only read
        // inside that loop. Hiding it stops everything.
        for game in [Some(GAME), None] {
            for takes_input in [true, false] {
                assert!(
                    placement(game, takes_input).visible,
                    "{game:?} {takes_input} hides the window"
                );
            }
        }
    }

    #[test]
    fn with_a_game_the_overlay_covers_it_exactly() {
        let got = placement(Some(GAME), false);

        assert_eq!((got.x, got.y), (100.0, 200.0));
        assert_eq!((got.width, got.height), (1920.0, 1080.0));
    }

    #[test]
    fn with_no_game_the_window_is_parked_and_tiny() {
        let got = placement(None, false);

        assert_eq!((got.x, got.y), (PARKED, PARKED));
        assert_eq!((got.width, got.height), (1.0, 1.0));
    }

    #[test]
    fn a_parked_window_always_lets_clicks_through() {
        // Whatever the panel thinks it wants. A one pixel window nobody can see
        // that eats a click is the worst possible failure of this design.
        for takes_input in [true, false] {
            assert!(placement(None, takes_input).passthrough, "{takes_input}");
        }
    }

    #[test]
    fn the_panel_takes_clicks_only_when_it_asks_and_the_game_is_there() {
        assert!(!placement(Some(GAME), true).passthrough);
        assert!(placement(Some(GAME), false).passthrough);
    }

    #[test]
    fn the_parked_window_is_clear_of_any_real_monitor() {
        // A second monitor placed left of the primary one uses negative
        // coordinates, so merely negative is not enough. This checks the parked
        // window's right edge is still left of a very wide arrangement: eight
        // 4K monitors in a row, all of them to the left of the primary.
        let leftmost_plausible_edge = -(7680.0 * 4.0);
        let parked = placement(None, false);

        assert!(
            parked.x + parked.width < leftmost_plausible_edge,
            "parked at {} is on screen",
            parked.x
        );
    }
}
