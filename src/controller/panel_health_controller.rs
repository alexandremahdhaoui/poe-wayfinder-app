use crate::adapter::window_probe_adapter::WindowProbe;

use poe_wayfinder_core::controller::panel_visible::{explain, visibility, Measured, Visibility};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    pub verdict: Visibility,
    pub advice: Option<&'static str>,
    pub measured: Measured,
}

#[cfg_attr(test, mockall::automock)]
pub trait PanelHealth {
    fn check(&self, panel_title: &str, game_title: &str) -> Option<Health>;
}

pub struct PanelHealthController<P: WindowProbe> {
    probe: P,
}

impl<P: WindowProbe> PanelHealthController<P> {
    pub fn new(probe: P) -> Self {
        Self { probe }
    }
}

impl<P: WindowProbe> PanelHealth for PanelHealthController<P> {
    fn check(&self, panel_title: &str, game_title: &str) -> Option<Health> {
        let measured = self.probe.measure(panel_title, game_title).ok()?;
        let verdict = visibility(measured);

        Some(Health {
            verdict,
            advice: explain(verdict),
            measured,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::adapter::window_probe_adapter::{MockWindowProbe, ProbeError};

    use poe_wayfinder_core::controller::panel_visible::Rect;

    fn measured(x: i32, above_game: bool) -> Measured {
        Measured {
            window: Rect {
                x,
                y: 24,
                width: 600,
                height: 900,
            },
            desktop: Rect {
                x: 0,
                y: 0,
                width: 2560,
                height: 1600,
            },
            shown: true,
            above_game,
        }
    }

    #[test]
    fn a_panel_on_screen_and_in_front_needs_no_advice() {
        let mut probe = MockWindowProbe::new();
        probe
            .expect_measure()
            .returning(|_, _| Ok(measured(1936, true)));

        let got = PanelHealthController::new(probe)
            .check("poe-wayfinder", "Path of Exile 2")
            .expect("a reading");

        assert_eq!(got.verdict, Visibility::Visible);
        assert_eq!(got.advice, None);
    }

    #[test]
    fn a_panel_off_the_screen_carries_advice() {
        let mut probe = MockWindowProbe::new();
        probe
            .expect_measure()
            .returning(|_, _| Ok(measured(2904, true)));

        let got = PanelHealthController::new(probe)
            .check("poe-wayfinder", "Path of Exile 2")
            .expect("a reading");

        assert_eq!(got.verdict, Visibility::OffScreen);
        assert!(got.advice.is_some());
    }

    #[test]
    fn a_panel_behind_the_game_is_reported() {
        let mut probe = MockWindowProbe::new();
        probe
            .expect_measure()
            .returning(|_, _| Ok(measured(1936, false)));

        let got = PanelHealthController::new(probe)
            .check("poe-wayfinder", "Path of Exile 2")
            .expect("a reading");

        assert_eq!(got.verdict, Visibility::BehindGame);
    }

    #[test]
    fn a_window_that_cannot_be_measured_reports_nothing_rather_than_guessing() {
        let mut probe = MockWindowProbe::new();
        probe.expect_measure().returning(|_, _| {
            Err(ProbeError::NotFound {
                title: "poe-wayfinder".to_string(),
            })
        });

        assert!(PanelHealthController::new(probe)
            .check("poe-wayfinder", "Path of Exile 2")
            .is_none());
    }

    #[test]
    fn the_titles_reach_the_probe_unchanged() {
        let mut probe = MockWindowProbe::new();
        probe
            .expect_measure()
            .withf(|panel, game| panel == "poe-wayfinder" && game == "Path of Exile 2")
            .times(1)
            .returning(|_, _| Ok(measured(1936, true)));

        assert!(PanelHealthController::new(probe)
            .check("poe-wayfinder", "Path of Exile 2")
            .is_some());
    }
}
