use poe_wayfinder_core::controller::chat::Key;

pub fn key_code(key: Key) -> u16 {
    match key {
        Key::Enter => 0x0D,
        Key::Escape => 0x1B,
        Key::Home => 0x24,
        Key::Delete => 0x2E,
        Key::Up => 0x26,
        Key::SelectAll => 0x41,
        Key::Paste => 0x56,
        Key::Find => 0x46,
        Key::ResumeChat => 0x0D,
    }
}

pub fn needs_control(key: Key) -> bool {
    matches!(key, Key::SelectAll | Key::Paste | Key::Find)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_maps_to_a_virtual_key_code() {
        for key in [
            Key::Enter,
            Key::Escape,
            Key::Home,
            Key::Delete,
            Key::Up,
            Key::SelectAll,
            Key::Paste,
            Key::Find,
            Key::ResumeChat,
        ] {
            assert!(key_code(key) > 0, "{key:?} has no code");
        }
    }

    #[test]
    fn only_the_editing_shortcuts_are_sent_with_control() {
        assert!(needs_control(Key::SelectAll));
        assert!(needs_control(Key::Paste));
        assert!(needs_control(Key::Find));

        assert!(!needs_control(Key::Enter));
        assert!(!needs_control(Key::Escape));
        assert!(!needs_control(Key::Up));
    }

    #[test]
    fn resume_chat_is_enter_because_that_is_what_reopens_the_last_channel() {
        assert_eq!(key_code(Key::ResumeChat), key_code(Key::Enter));
    }

    #[test]
    fn paste_and_select_all_are_the_letters_the_game_expects() {
        assert_eq!(key_code(Key::Paste), 0x56);
        assert_eq!(key_code(Key::SelectAll), 0x41);
    }
}
