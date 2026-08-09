use crate::adapter::game_window_adapter::{GameWindow, GameWindowSource};
use poe_wayfinder_core::controller::game_detect;
use poe_wayfinder_core::types::GameVersion;

#[cfg_attr(test, mockall::automock)]
pub trait GameState {
    fn window(&self) -> Option<GameWindow>;

    fn cursor(&self) -> (i32, i32);

    fn scale(&self) -> f32;

    fn detect_game(&self) -> Option<GameVersion>;

    fn game_changed_from(&self, current: GameVersion) -> Option<GameVersion>;

    fn retarget(&self, game: GameVersion);
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

    fn detect_game(&self) -> Option<GameVersion> {
        game_detect::detect(
            self.source.foreground().as_deref(),
            &self.source.open_titles(),
        )
    }

    fn game_changed_from(&self, current: GameVersion) -> Option<GameVersion> {
        game_detect::detect_change(
            current,
            self.source.foreground().as_deref(),
            &self.source.open_titles(),
        )
    }

    fn retarget(&self, game: GameVersion) {
        self.source.retarget(game_detect::title_for(game));
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

    #[derive(Default)]
    struct DesktopSource {
        foreground: Option<String>,
        open: Vec<String>,
        aimed_at: std::sync::Mutex<Vec<String>>,
    }

    impl GameWindowSource for DesktopSource {
        fn find(&self) -> Result<GameWindow, WindowError> {
            Err(WindowError::NotFound {
                title: String::new(),
            })
        }

        fn cursor(&self) -> (i32, i32) {
            (0, 0)
        }

        fn scale(&self) -> f32 {
            1.0
        }

        fn retarget(&self, title: &str) {
            self.aimed_at.lock().unwrap().push(title.to_string());
        }

        fn open_titles(&self) -> Vec<String> {
            self.open.clone()
        }

        fn foreground(&self) -> Option<String> {
            self.foreground.clone()
        }
    }

    fn desktop(foreground: Option<&str>, open: &[&str]) -> DesktopSource {
        DesktopSource {
            foreground: foreground.map(|f| f.to_string()),
            open: open.iter().map(|o| o.to_string()).collect(),
            aimed_at: std::sync::Mutex::new(Vec::new()),
        }
    }

    #[test]
    fn an_empty_desktop_has_no_game_to_detect() {
        let controller = GameStateController::new(desktop(Some("Firefox"), &["Discord"]));

        assert_eq!(controller.detect_game(), None);
    }

    #[test]
    fn the_game_in_front_is_the_one_detected() {
        let controller = GameStateController::new(desktop(
            Some("Path of Exile"),
            &["Path of Exile", "Path of Exile 2"],
        ));

        assert_eq!(controller.detect_game(), Some(GameVersion::Poe1));
    }

    #[test]
    fn a_desktop_with_no_game_in_front_reports_no_change() {
        let controller = GameStateController::new(desktop(
            Some("Discord"),
            &["Path of Exile", "Path of Exile 2"],
        ));

        assert_eq!(controller.game_changed_from(GameVersion::Poe1), None);
    }

    #[test]
    fn the_other_game_coming_forward_is_reported_as_a_change() {
        let controller = GameStateController::new(desktop(
            Some("Path of Exile 2"),
            &["Path of Exile", "Path of Exile 2"],
        ));

        assert_eq!(
            controller.game_changed_from(GameVersion::Poe1),
            Some(GameVersion::Poe2)
        );
    }

    #[test]
    fn retargeting_aims_the_adapter_at_that_games_title() {
        let source = desktop(None, &[]);
        let controller = GameStateController::new(source);

        controller.retarget(GameVersion::Poe1);
        controller.retarget(GameVersion::Poe2);

        assert_eq!(
            *controller.source.aimed_at.lock().unwrap(),
            vec!["Path of Exile".to_string(), "Path of Exile 2".to_string()]
        );
    }

    #[test]
    fn a_source_that_cannot_enumerate_windows_detects_nothing_rather_than_guessing() {
        assert_eq!(
            GameStateController::new(FixedSource { found: true }).detect_game(),
            None
        );
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
