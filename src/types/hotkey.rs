//! Hotkey parsing.
//!
//! A hotkey arrives from config as text such as `Ctrl+D` or `Ctrl+Alt+Shift+F5`
//! and has to become something an input hook can compare against.
//!
//! Parsing is strict. A hotkey the user typed wrong should fail at startup with
//! a message, not silently never fire. A hotkey that never fires is the hardest
//! kind of bug to report, because there is nothing to see.

use std::fmt;

/// A modifier key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    /// The Windows key.
    Meta,
}

impl Modifier {
    /// The canonical spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Modifier::Ctrl => "Ctrl",
            Modifier::Alt => "Alt",
            Modifier::Shift => "Shift",
            Modifier::Meta => "Meta",
        }
    }

    /// Read a modifier name, accepting the spellings users actually type.
    fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Some(Modifier::Ctrl),
            "alt" => Some(Modifier::Alt),
            "shift" => Some(Modifier::Shift),
            "meta" | "win" | "super" | "cmd" => Some(Modifier::Meta),
            _ => None,
        }
    }
}

/// The non modifier key of a hotkey.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    /// A letter or digit. Always stored upper case.
    Char(char),
    /// A function key. 1 through 24.
    Function(u8),
    Escape,
    Space,
    Tab,
    Enter,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
}

impl Key {
    /// The canonical spelling.
    pub fn as_string(&self) -> String {
        match self {
            Key::Char(c) => c.to_string(),
            Key::Function(n) => format!("F{n}"),
            Key::Escape => "Escape".into(),
            Key::Space => "Space".into(),
            Key::Tab => "Tab".into(),
            Key::Enter => "Enter".into(),
            Key::Backspace => "Backspace".into(),
            Key::Delete => "Delete".into(),
            Key::Insert => "Insert".into(),
            Key::Home => "Home".into(),
            Key::End => "End".into(),
            Key::PageUp => "PageUp".into(),
            Key::PageDown => "PageDown".into(),
            Key::Up => "Up".into(),
            Key::Down => "Down".into(),
            Key::Left => "Left".into(),
            Key::Right => "Right".into(),
        }
    }

    /// Read a key name.
    fn parse(text: &str) -> Option<Self> {
        // A function key is F followed by a number. Checked before the single
        // character case so `F5` does not read as the letter F.
        if let Some(digits) = text.strip_prefix(['F', 'f']) {
            if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                let n: u8 = digits.parse().ok()?;

                // There is no F0 and no keyboard has past F24.
                if (1..=24).contains(&n) {
                    return Some(Key::Function(n));
                }

                return None;
            }
        }

        let named = match text.to_ascii_lowercase().as_str() {
            "escape" | "esc" => Some(Key::Escape),
            "space" => Some(Key::Space),
            "tab" => Some(Key::Tab),
            "enter" | "return" => Some(Key::Enter),
            "backspace" => Some(Key::Backspace),
            "delete" | "del" => Some(Key::Delete),
            "insert" | "ins" => Some(Key::Insert),
            "home" => Some(Key::Home),
            "end" => Some(Key::End),
            "pageup" | "pgup" => Some(Key::PageUp),
            "pagedown" | "pgdn" => Some(Key::PageDown),
            "up" => Some(Key::Up),
            "down" => Some(Key::Down),
            "left" => Some(Key::Left),
            "right" => Some(Key::Right),
            _ => None,
        };

        if named.is_some() {
            return named;
        }

        let mut chars = text.chars();
        let c = chars.next()?;

        if chars.next().is_some() {
            return None;
        }

        if !c.is_ascii_alphanumeric() {
            return None;
        }

        Some(Key::Char(c.to_ascii_uppercase()))
    }
}

/// Why a hotkey could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyError {
    /// The text was empty or only separators.
    Empty,
    /// A part was not a modifier and was not the last part.
    UnknownModifier(String),
    /// The final part was not a key.
    UnknownKey(String),
    /// Every part was a modifier, so there is no key to press.
    NoKey,
    /// The same modifier appeared twice.
    DuplicateModifier(Modifier),
}

impl fmt::Display for HotkeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HotkeyError::Empty => write!(f, "the hotkey is empty"),
            HotkeyError::UnknownModifier(m) => write!(f, "{m:?} is not a modifier"),
            HotkeyError::UnknownKey(k) => write!(f, "{k:?} is not a key"),
            HotkeyError::NoKey => write!(f, "the hotkey names only modifiers"),
            HotkeyError::DuplicateModifier(m) => {
                write!(f, "{:?} appears twice", m.as_str())
            }
        }
    }
}

impl std::error::Error for HotkeyError {}

/// One hotkey.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hotkey {
    /// Sorted and deduplicated, so two spellings of the same hotkey compare
    /// equal. `Ctrl+Alt+D` and `Alt+Ctrl+D` are one hotkey.
    modifiers: Vec<Modifier>,
    key: Key,
}

impl Hotkey {
    /// Read a hotkey from config text.
    pub fn parse(text: &str) -> Result<Self, HotkeyError> {
        let parts: Vec<&str> = text
            .split('+')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();

        let Some((last, rest)) = parts.split_last() else {
            return Err(HotkeyError::Empty);
        };

        let mut modifiers = Vec::new();

        for part in rest {
            let Some(modifier) = Modifier::parse(part) else {
                return Err(HotkeyError::UnknownModifier((*part).to_string()));
            };

            if modifiers.contains(&modifier) {
                return Err(HotkeyError::DuplicateModifier(modifier));
            }

            modifiers.push(modifier);
        }

        // A trailing modifier means the user wrote only modifiers. Saying so
        // beats reporting it as an unknown key.
        if Modifier::parse(last).is_some() {
            return Err(HotkeyError::NoKey);
        }

        let Some(key) = Key::parse(last) else {
            return Err(HotkeyError::UnknownKey((*last).to_string()));
        };

        modifiers.sort_unstable();

        Ok(Self { modifiers, key })
    }

    /// The modifiers, sorted.
    pub fn modifiers(&self) -> &[Modifier] {
        &self.modifiers
    }

    /// The key.
    pub fn key(&self) -> &Key {
        &self.key
    }

    /// Whether a key press with these modifiers held triggers this hotkey.
    ///
    /// The modifier set has to match exactly. `Ctrl+D` must not fire on
    /// `Ctrl+Shift+D`, because that is usually a different binding in the game.
    pub fn matches(&self, key: &Key, held: &[Modifier]) -> bool {
        if &self.key != key {
            return false;
        }

        let mut held: Vec<Modifier> = held.to_vec();
        held.sort_unstable();
        held.dedup();

        held == self.modifiers
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for modifier in &self.modifiers {
            write!(f, "{}+", modifier.as_str())?;
        }

        write!(f, "{}", self.key.as_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_simple_hotkey_parses() {
        let h = Hotkey::parse("Ctrl+D").unwrap();

        assert_eq!(h.modifiers(), &[Modifier::Ctrl]);
        assert_eq!(h.key(), &Key::Char('D'));
    }

    #[test]
    fn a_bare_key_parses() {
        let h = Hotkey::parse("F5").unwrap();

        assert!(h.modifiers().is_empty());
        assert_eq!(h.key(), &Key::Function(5));
    }

    #[test]
    fn modifier_order_does_not_change_the_hotkey() {
        // Two spellings of one hotkey must compare equal, or a config edit
        // that only reorders them looks like a different binding.
        let a = Hotkey::parse("Ctrl+Alt+D").unwrap();
        let b = Hotkey::parse("Alt+Ctrl+D").unwrap();

        assert_eq!(a, b);
    }

    #[test]
    fn key_case_does_not_matter() {
        assert_eq!(
            Hotkey::parse("ctrl+d").unwrap(),
            Hotkey::parse("CTRL+D").unwrap()
        );
    }

    #[test]
    fn whitespace_around_parts_is_ignored() {
        assert_eq!(
            Hotkey::parse(" Ctrl + D ").unwrap(),
            Hotkey::parse("Ctrl+D").unwrap()
        );
    }

    #[test]
    fn every_modifier_spelling_users_type_is_accepted() {
        for (text, want) in [
            ("Ctrl+D", Modifier::Ctrl),
            ("Control+D", Modifier::Ctrl),
            ("Alt+D", Modifier::Alt),
            ("Shift+D", Modifier::Shift),
            ("Win+D", Modifier::Meta),
            ("Super+D", Modifier::Meta),
            ("Cmd+D", Modifier::Meta),
            ("Meta+D", Modifier::Meta),
        ] {
            assert_eq!(Hotkey::parse(text).unwrap().modifiers(), &[want], "{text}");
        }
    }

    #[test]
    fn a_function_key_is_not_read_as_the_letter_f() {
        // F5 must be the function key, not F followed by a stray 5.
        assert_eq!(Hotkey::parse("F5").unwrap().key(), &Key::Function(5));
        assert_eq!(Hotkey::parse("F12").unwrap().key(), &Key::Function(12));
    }

    #[test]
    fn a_lone_f_is_the_letter() {
        assert_eq!(Hotkey::parse("F").unwrap().key(), &Key::Char('F'));
    }

    #[test]
    fn a_function_key_beyond_the_real_range_is_rejected() {
        // There is no F0 and no keyboard has past F24. Accepting one would
        // produce a hotkey that can never fire.
        assert!(Hotkey::parse("F0").is_err());
        assert!(Hotkey::parse("F25").is_err());
        assert!(Hotkey::parse("F99").is_err());
    }

    #[test]
    fn every_named_key_parses() {
        for (text, want) in [
            ("Escape", Key::Escape),
            ("Esc", Key::Escape),
            ("Space", Key::Space),
            ("Tab", Key::Tab),
            ("Enter", Key::Enter),
            ("Return", Key::Enter),
            ("Backspace", Key::Backspace),
            ("Delete", Key::Delete),
            ("Del", Key::Delete),
            ("Insert", Key::Insert),
            ("Home", Key::Home),
            ("End", Key::End),
            ("PageUp", Key::PageUp),
            ("PgDn", Key::PageDown),
            ("Up", Key::Up),
            ("Down", Key::Down),
            ("Left", Key::Left),
            ("Right", Key::Right),
        ] {
            assert_eq!(Hotkey::parse(text).unwrap().key(), &want, "{text}");
        }
    }

    #[test]
    fn a_digit_is_a_valid_key() {
        assert_eq!(Hotkey::parse("Ctrl+1").unwrap().key(), &Key::Char('1'));
    }

    #[test]
    fn empty_text_is_rejected() {
        // A hotkey that never fires is the hardest kind of bug to report,
        // because there is nothing to see.
        assert_eq!(Hotkey::parse("").unwrap_err(), HotkeyError::Empty);
        assert_eq!(Hotkey::parse("+++").unwrap_err(), HotkeyError::Empty);
    }

    #[test]
    fn a_hotkey_of_only_modifiers_is_rejected() {
        assert_eq!(Hotkey::parse("Ctrl").unwrap_err(), HotkeyError::NoKey);
        assert_eq!(Hotkey::parse("Ctrl+Alt").unwrap_err(), HotkeyError::NoKey);
    }

    #[test]
    fn an_unknown_modifier_names_itself() {
        // The message has to name the part so a user can find it in config.
        let err = Hotkey::parse("Hyper+D").unwrap_err();

        assert_eq!(err, HotkeyError::UnknownModifier("Hyper".into()));
        assert!(err.to_string().contains("Hyper"));
    }

    #[test]
    fn an_unknown_key_names_itself() {
        let err = Hotkey::parse("Ctrl+Banana").unwrap_err();

        assert_eq!(err, HotkeyError::UnknownKey("Banana".into()));
        assert!(err.to_string().contains("Banana"));
    }

    #[test]
    fn a_repeated_modifier_is_rejected() {
        // It is always a typo and silently accepting it hides the mistake.
        assert_eq!(
            Hotkey::parse("Ctrl+Ctrl+D").unwrap_err(),
            HotkeyError::DuplicateModifier(Modifier::Ctrl)
        );
    }

    #[test]
    fn a_punctuation_key_is_rejected() {
        // The input hook cannot report these portably, so accepting one would
        // produce a hotkey that never fires.
        assert!(Hotkey::parse("Ctrl+;").is_err());
        assert!(Hotkey::parse("Ctrl+é").is_err());
    }

    #[test]
    fn a_hotkey_renders_back_to_its_canonical_text() {
        assert_eq!(
            Hotkey::parse("alt+ctrl+d").unwrap().to_string(),
            "Ctrl+Alt+D"
        );
        assert_eq!(Hotkey::parse("f5").unwrap().to_string(), "F5");
        assert_eq!(
            Hotkey::parse("ctrl+esc").unwrap().to_string(),
            "Ctrl+Escape"
        );
    }

    #[test]
    fn rendering_round_trips_through_parsing() {
        for text in ["Ctrl+D", "Ctrl+Alt+Shift+F5", "Space", "Meta+Up"] {
            let h = Hotkey::parse(text).unwrap();

            assert_eq!(Hotkey::parse(&h.to_string()).unwrap(), h, "{text}");
        }
    }

    #[test]
    fn a_press_with_the_exact_modifiers_matches() {
        let h = Hotkey::parse("Ctrl+D").unwrap();

        assert!(h.matches(&Key::Char('D'), &[Modifier::Ctrl]));
    }

    #[test]
    fn an_extra_held_modifier_does_not_match() {
        // Ctrl+Shift+D is usually a different binding in the game. Firing on
        // it would steal a keypress the user meant for something else.
        let h = Hotkey::parse("Ctrl+D").unwrap();

        assert!(!h.matches(&Key::Char('D'), &[Modifier::Ctrl, Modifier::Shift]));
    }

    #[test]
    fn a_missing_modifier_does_not_match() {
        let h = Hotkey::parse("Ctrl+D").unwrap();

        assert!(!h.matches(&Key::Char('D'), &[]));
    }

    #[test]
    fn a_different_key_does_not_match() {
        let h = Hotkey::parse("Ctrl+D").unwrap();

        assert!(!h.matches(&Key::Char('E'), &[Modifier::Ctrl]));
    }

    #[test]
    fn the_order_modifiers_are_reported_in_does_not_matter() {
        // An input hook reports them in whatever order it saw them.
        let h = Hotkey::parse("Ctrl+Alt+D").unwrap();

        assert!(h.matches(&Key::Char('D'), &[Modifier::Alt, Modifier::Ctrl]));
    }

    #[test]
    fn a_modifier_reported_twice_still_matches() {
        // Both physical control keys down is still one Ctrl.
        let h = Hotkey::parse("Ctrl+D").unwrap();

        assert!(h.matches(&Key::Char('D'), &[Modifier::Ctrl, Modifier::Ctrl]));
    }

    #[test]
    fn a_bare_hotkey_does_not_match_when_a_modifier_is_held() {
        let h = Hotkey::parse("F5").unwrap();

        assert!(h.matches(&Key::Function(5), &[]));
        assert!(!h.matches(&Key::Function(5), &[Modifier::Ctrl]));
    }
}
