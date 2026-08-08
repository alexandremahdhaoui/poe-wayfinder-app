use poe_trader_core::controller::overlay_lifecycle::HoldKey;

#[cfg(windows)]
pub fn hold_down(hold: HoldKey) -> bool {
    let code = match hold {
        HoldKey::Ctrl => 0x11,
        HoldKey::Alt => 0x12,
        HoldKey::Shift => 0x10,
        HoldKey::None => return false,
    };

    down(code)
}

#[cfg(windows)]
pub fn alt_alone() -> bool {
    down(0x12) && !down(0x11) && !down(0x10) && !down(0x5B) && !down(0x5C)
}

#[cfg(windows)]
pub fn mouse_down() -> bool {
    down(0x01) || down(0x02)
}

#[cfg(windows)]
fn down(code: i32) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    (unsafe { GetAsyncKeyState(code) } as u16 & 0x8000) != 0
}

#[cfg(not(windows))]
pub fn hold_down(_hold: HoldKey) -> bool {
    false
}

#[cfg(not(windows))]
pub fn alt_alone() -> bool {
    false
}

#[cfg(not(windows))]
pub fn mouse_down() -> bool {
    false
}

pub fn hold_key_for(modifiers: &[crate::types::Modifier]) -> HoldKey {
    use crate::types::Modifier;

    match modifiers.first() {
        Some(Modifier::Ctrl) => HoldKey::Ctrl,
        Some(Modifier::Alt) => HoldKey::Alt,
        Some(Modifier::Shift) => HoldKey::Shift,
        Some(Modifier::Meta) | None => HoldKey::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::types::Modifier;

    #[test]
    fn the_first_modifier_is_the_one_held() {
        assert_eq!(hold_key_for(&[Modifier::Ctrl]), HoldKey::Ctrl);
        assert_eq!(hold_key_for(&[Modifier::Alt]), HoldKey::Alt);
        assert_eq!(hold_key_for(&[Modifier::Shift]), HoldKey::Shift);
    }

    #[test]
    fn a_hotkey_with_no_modifier_has_nothing_to_hold() {
        assert_eq!(hold_key_for(&[]), HoldKey::None);
    }

    #[test]
    fn the_windows_key_is_not_a_hold_key() {
        assert_eq!(hold_key_for(&[Modifier::Meta]), HoldKey::None);
    }

    #[test]
    fn a_combination_holds_on_its_first_modifier() {
        assert_eq!(
            hold_key_for(&[Modifier::Ctrl, Modifier::Shift]),
            HoldKey::Ctrl
        );
    }
}
