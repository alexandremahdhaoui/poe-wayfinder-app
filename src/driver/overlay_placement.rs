#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub visible: bool,
    pub passthrough: bool,
}

pub const PARKED: f32 = -32000.0;

pub fn placement(game: Option<Rect>, takes_input: bool, scale: f32) -> Placement {
    let scale = if scale > 0.0 { scale } else { 1.0 };

    match game {
        Some(rect) => Placement {
            x: rect.x as f32 / scale,
            y: rect.y as f32 / scale,
            width: rect.width as f32 / scale,
            height: rect.height as f32 / scale,
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
    fn a_hidpi_screen_gets_logical_points_not_pixels() {
        let panel = Rect {
            x: 1936,
            y: 24,
            width: 600,
            height: 900,
        };

        let got = placement(Some(panel), true, 1.5);

        assert_eq!((got.x, got.y), (1936.0 / 1.5, 16.0));
        assert_eq!((got.width, got.height), (400.0, 600.0));
    }

    #[test]
    fn the_panel_stays_on_a_hidpi_screen() {
        let screen_points = 2560.0 / 1.5;

        let panel = Rect {
            x: 1936,
            y: 24,
            width: 600,
            height: 900,
        };

        let got = placement(Some(panel), true, 1.5);

        assert!(
            got.x + got.width <= screen_points,
            "the panel's right edge is at {} on a {screen_points} point screen",
            got.x + got.width
        );
    }

    #[test]
    fn a_display_at_one_hundred_percent_is_unchanged() {
        let panel = Rect {
            x: 1300,
            y: 24,
            width: 600,
            height: 900,
        };

        let got = placement(Some(panel), true, 1.0);

        assert_eq!((got.x, got.y), (1300.0, 24.0));
        assert_eq!((got.width, got.height), (600.0, 900.0));
    }

    #[test]
    fn a_nonsense_scale_does_not_send_the_panel_to_infinity() {
        for scale in [0.0, -1.0, f32::NAN] {
            let got = placement(Some(GAME), true, scale);

            assert!(got.x.is_finite() && got.width > 0.0, "scale {scale}");
        }
    }

    #[test]
    fn the_window_is_never_hidden() {
        for game in [Some(GAME), None] {
            for takes_input in [true, false] {
                assert!(
                    placement(game, takes_input, 1.0).visible,
                    "{game:?} {takes_input} hides the window"
                );
            }
        }
    }

    #[test]
    fn with_a_game_the_overlay_covers_it_exactly() {
        let got = placement(Some(GAME), false, 1.0);

        assert_eq!((got.x, got.y), (100.0, 200.0));
        assert_eq!((got.width, got.height), (1920.0, 1080.0));
    }

    #[test]
    fn with_no_game_the_window_is_parked_and_tiny() {
        let got = placement(None, false, 1.0);

        assert_eq!((got.x, got.y), (PARKED, PARKED));
        assert_eq!((got.width, got.height), (1.0, 1.0));
    }

    #[test]
    fn a_parked_window_always_lets_clicks_through() {
        for takes_input in [true, false] {
            assert!(
                placement(None, takes_input, 1.0).passthrough,
                "{takes_input}"
            );
        }
    }

    #[test]
    fn the_panel_takes_clicks_only_when_it_asks_and_the_game_is_there() {
        assert!(!placement(Some(GAME), true, 1.0).passthrough);
        assert!(placement(Some(GAME), false, 1.0).passthrough);
    }

    #[test]
    fn the_parked_window_is_clear_of_any_real_monitor() {
        let leftmost_plausible_edge = -(7680.0 * 4.0);
        let parked = placement(None, false, 1.0);

        assert!(
            parked.x + parked.width < leftmost_plausible_edge,
            "parked at {} is on screen",
            parked.x
        );
    }
}
