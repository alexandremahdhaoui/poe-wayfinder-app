use crate::adapter::input_state_adapter;

use poe_trader_core::controller::overlay_lifecycle::HoldKey;

#[cfg_attr(test, mockall::automock)]
pub trait InputState {
    fn hold_down(&self) -> bool;

    fn alt_alone(&self) -> bool;

    fn mouse_down(&self) -> bool;
}

pub struct InputController {
    hold: HoldKey,
}

impl InputController {
    pub fn new(hold: HoldKey) -> Self {
        Self { hold }
    }
}

impl InputState for InputController {
    fn hold_down(&self) -> bool {
        input_state_adapter::hold_down(self.hold)
    }

    fn alt_alone(&self) -> bool {
        input_state_adapter::alt_alone()
    }

    fn mouse_down(&self) -> bool {
        input_state_adapter::mouse_down()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hotkey_with_no_modifier_never_reports_a_hold() {
        assert!(!InputController::new(HoldKey::None).hold_down());
    }

    #[test]
    fn the_hold_key_is_remembered() {
        let controller = InputController::new(HoldKey::Ctrl);

        assert_eq!(controller.hold, HoldKey::Ctrl);
    }
}
