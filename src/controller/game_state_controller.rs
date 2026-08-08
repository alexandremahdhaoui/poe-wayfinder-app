use crate::adapter::game_window_adapter::{GameWindow, GameWindowSource};

#[cfg_attr(test, mockall::automock)]
pub trait GameState {
    fn window(&self) -> Option<GameWindow>;

    fn cursor(&self) -> (i32, i32);

    fn scale(&self) -> f32;
}

pub struct GameStateController<S: GameWindowSource> {
    source: S,
}

impl<S: GameWindowSource> GameStateController<S> {
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S: GameWindowSource> GameState for GameStateController<S> {
    fn window(&self) -> Option<GameWindow> {
        self.source.find().ok()
    }

    fn cursor(&self) -> (i32, i32) {
        self.source.cursor()
    }

    fn scale(&self) -> f32 {
        self.source.scale()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::adapter::game_window_adapter::WindowError;
    use crate::types::overlay::WindowRect;

    struct FixedSource {
        found: bool,
    }

    impl GameWindowSource for FixedSource {
        fn find(&self) -> Result<GameWindow, WindowError> {
            match self.found {
                true => Ok(GameWindow {
                    rect: WindowRect::new(0, 0, 1920, 1080),
                    is_foreground: true,
                }),
                false => Err(WindowError::NotFound {
                    title: "Path of Exile 2".to_string(),
                }),
            }
        }

        fn cursor(&self) -> (i32, i32) {
            (10, 20)
        }

        fn scale(&self) -> f32 {
            1.5
        }
    }

    #[test]
    fn a_missing_window_reads_as_absent_rather_than_an_error() {
        assert!(GameStateController::new(FixedSource { found: false })
            .window()
            .is_none());
    }

    #[test]
    fn a_found_window_is_handed_over() {
        let got = GameStateController::new(FixedSource { found: true })
            .window()
            .expect("a window");

        assert_eq!(got.rect.width, 1920);
        assert!(got.is_foreground);
    }

    #[test]
    fn the_cursor_and_scale_pass_straight_through() {
        let controller = GameStateController::new(FixedSource { found: true });

        assert_eq!(controller.cursor(), (10, 20));
        assert!((controller.scale() - 1.5).abs() < f32::EPSILON);
    }
}
