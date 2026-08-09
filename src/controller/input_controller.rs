use crate::adapter::input_state_adapter::RawInput;

use poe_wayfinder_core::controller::overlay_lifecycle::HoldKey;

#[cfg_attr(test, mockall::automock)]
pub trait InputState {
    fn hold_down(&self) -> bool;

    fn alt_alone(&self) -> bool;

    fn mouse_down(&self) -> bool;
}

pub struct InputController<R: RawInput> {
    raw: R,
    hold: HoldKey,
}

impl<R: RawInput> InputController<R> {
    pub fn new(raw: R, hold: HoldKey) -> Self {
        Self { raw, hold }
    }
}

impl<R: RawInput> InputState for InputController<R> {
    fn hold_down(&self) -> bool {
        self.raw.hold_down(self.hold)
    }

    fn alt_alone(&self) -> bool {
        self.raw.alt_alone()
    }

    fn mouse_down(&self) -> bool {
        self.raw.mouse_down()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::adapter::input_state_adapter::MockRawInput;

    #[test]
    fn the_configured_hold_key_reaches_the_adapter() {
        let mut raw = MockRawInput::new();
        raw.expect_hold_down()
            .withf(|hold| *hold == HoldKey::Alt)
            .times(1)
            .returning(|_| true);

        assert!(InputController::new(raw, HoldKey::Alt).hold_down());
    }

    #[test]
    fn alt_alone_passes_through_untouched() {
        let mut raw = MockRawInput::new();
        raw.expect_alt_alone().times(1).returning(|| true);

        assert!(InputController::new(raw, HoldKey::Ctrl).alt_alone());
    }

    #[test]
    fn a_click_passes_through_untouched() {
        let mut raw = MockRawInput::new();
        raw.expect_mouse_down().times(1).returning(|| false);

        assert!(!InputController::new(raw, HoldKey::Ctrl).mouse_down());
    }
}
