use std::time::Duration;

#[cfg(windows)]
use poe_wayfinder_core::controller::overlay::copy_key_sequence;
use poe_wayfinder_core::controller::overlay::{clipboard_kind, ClipboardKind};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClipboardError {
    #[error("opening the clipboard")]
    Open(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("reading the clipboard")]
    Read(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("writing the clipboard")]
    Write(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("the clipboard did not change within {waited:?}")]
    NoChange { waited: Duration },

    #[error("asking the game to copy: {sent} of {asked} key events reached it")]
    Trigger { sent: usize, asked: usize },
}

#[cfg_attr(test, mockall::automock)]
pub trait Clipboard: Send + Sync {
    fn read(&mut self) -> Result<Option<String>, ClipboardError>;

    fn write(&mut self, text: &str) -> Result<(), ClipboardError>;
}

#[cfg_attr(test, mockall::automock)]
pub trait CopyTrigger: Send + Sync {
    fn trigger_copy(&self) -> Result<(), ClipboardError>;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CopyTiming {
    pub timeout: Duration,
    pub poll_interval: Duration,
}

impl Default for CopyTiming {
    fn default() -> Self {
        Self {
            timeout: Duration::from_millis(600),
            poll_interval: Duration::from_millis(10),
        }
    }
}

pub fn copy_item<F>(
    clipboard: &mut dyn Clipboard,
    trigger: &dyn CopyTrigger,
    timing: CopyTiming,
    restore: bool,
    mut sleep: F,
) -> Result<String, ClipboardError>
where
    F: FnMut(Duration),
{
    let before = clipboard.read()?;

    trigger.trigger_copy()?;

    let mut waited = Duration::ZERO;
    let mut found: Option<String> = None;

    while waited < timing.timeout {
        sleep(timing.poll_interval);
        waited += timing.poll_interval;

        let now = clipboard.read()?;

        if let Some(text) = now {
            if clipboard_kind(&text) != ClipboardKind::NotAnItem {
                found = Some(text);

                break;
            }
        }
    }

    let Some(text) = found else {
        return Err(ClipboardError::NoChange { waited });
    };

    if restore {
        if let Some(old) = before {
            let _ = clipboard.write(&old);
        }
    }

    Ok(text)
}

#[cfg(windows)]
pub struct SystemClipboard {
    inner: arboard::Clipboard,
}

#[cfg(windows)]
impl SystemClipboard {
    pub fn new() -> Result<Self, ClipboardError> {
        let inner = arboard::Clipboard::new().map_err(|e| ClipboardError::Open(Box::new(e)))?;

        Ok(Self { inner })
    }
}

#[cfg(windows)]
impl Clipboard for SystemClipboard {
    fn read(&mut self) -> Result<Option<String>, ClipboardError> {
        match self.inner.get_text() {
            Ok(text) => Ok(Some(text)),
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(e) => Err(ClipboardError::Read(Box::new(e))),
        }
    }

    fn write(&mut self, text: &str) -> Result<(), ClipboardError> {
        self.inner
            .set_text(text.to_string())
            .map_err(|e| ClipboardError::Write(Box::new(e)))
    }
}

#[cfg(windows)]
pub struct KeyboardCopyTrigger {
    also_hold: Vec<String>,
}

#[cfg(windows)]
impl KeyboardCopyTrigger {
    pub fn new(also_hold: Vec<String>) -> Self {
        Self { also_hold }
    }
}

#[cfg(windows)]
impl Default for KeyboardCopyTrigger {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

#[cfg(windows)]
impl CopyTrigger for KeyboardCopyTrigger {
    fn trigger_copy(&self) -> Result<(), ClipboardError> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, VK_C, VK_CONTROL, VK_MENU, VK_SHIFT,
        };

        let events: Vec<INPUT> = copy_key_sequence(&self.also_hold)
            .iter()
            .filter_map(|stroke| {
                let key = match stroke.key.as_str() {
                    "Ctrl" => VK_CONTROL,
                    "C" => VK_C,
                    "Alt" => VK_MENU,
                    "Shift" => VK_SHIFT,
                    _ => return None,
                };

                Some(win::key_event(key, !stroke.down))
            })
            .collect();

        let sent = unsafe { SendInput(&events, std::mem::size_of::<INPUT>() as i32) } as usize;

        if sent != events.len() {
            return Err(ClipboardError::Trigger {
                sent,
                asked: events.len(),
            });
        }

        Ok(())
    }
}

#[cfg(windows)]
mod win {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };

    pub fn key_event(key: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    dwFlags: if up {
                        KEYEVENTF_KEYUP
                    } else {
                        Default::default()
                    },
                    ..Default::default()
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct ScriptedClipboard {
        content: Option<String>,
        script: Vec<Option<String>>,
        reads: usize,
        writes: Vec<String>,
    }

    impl ScriptedClipboard {
        fn holding(text: Option<&str>) -> Self {
            Self {
                content: text.map(str::to_string),
                script: Vec::new(),
                reads: 0,
                writes: Vec::new(),
            }
        }

        fn scripted(initial: Option<&str>, script: Vec<Option<&str>>) -> Self {
            Self {
                content: initial.map(str::to_string),
                script: script.into_iter().map(|s| s.map(str::to_string)).collect(),
                reads: 0,
                writes: Vec::new(),
            }
        }
    }

    impl Clipboard for ScriptedClipboard {
        fn read(&mut self) -> Result<Option<String>, ClipboardError> {
            if self.reads > 0 {
                if let Some(next) = self.script.get(self.reads - 1) {
                    self.content = next.clone();
                }
            }

            self.reads += 1;

            Ok(self.content.clone())
        }

        fn write(&mut self, text: &str) -> Result<(), ClipboardError> {
            self.writes.push(text.to_string());
            self.content = Some(text.to_string());

            Ok(())
        }
    }

    struct CountingTrigger {
        fired: AtomicUsize,
    }

    impl CountingTrigger {
        fn new() -> Self {
            Self {
                fired: AtomicUsize::new(0),
            }
        }

        fn count(&self) -> usize {
            self.fired.load(Ordering::Relaxed)
        }
    }

    impl CopyTrigger for CountingTrigger {
        fn trigger_copy(&self) -> Result<(), ClipboardError> {
            self.fired.fetch_add(1, Ordering::Relaxed);

            Ok(())
        }
    }

    fn fast() -> CopyTiming {
        CopyTiming {
            timeout: Duration::from_millis(100),
            poll_interval: Duration::from_millis(10),
        }
    }

    fn no_sleep(_: Duration) {}

    #[test]
    fn the_copy_keystroke_is_sent_exactly_once_generated_mock() {
        let mut trigger = MockCopyTrigger::new();
        trigger.expect_trigger_copy().times(1).returning(|| Ok(()));

        let mut clip = ScriptedClipboard::scripted(Some("old"), vec![Some("Item Class: Rings")]);

        let got = copy_item(&mut clip, &trigger, fast(), false, no_sleep).unwrap();

        assert_eq!(got, "Item Class: Rings");
    }

    #[test]
    fn a_trigger_that_refuses_stops_the_copy_generated_mock() {
        let mut trigger = MockCopyTrigger::new();
        trigger.expect_trigger_copy().times(1).returning(|| {
            Err(ClipboardError::NoChange {
                waited: Duration::from_millis(1),
            })
        });

        let mut clip = ScriptedClipboard::holding(Some("old"));

        assert!(copy_item(&mut clip, &trigger, fast(), false, no_sleep).is_err());
    }

    #[test]
    fn a_changed_clipboard_is_returned() {
        let mut clip = ScriptedClipboard::scripted(Some("old"), vec![Some("Item Class: Rings")]);

        let got = copy_item(&mut clip, &CountingTrigger::new(), fast(), false, no_sleep).unwrap();

        assert_eq!(got, "Item Class: Rings");
    }

    #[test]
    fn the_copy_keystroke_is_sent_once() {
        let mut clip = ScriptedClipboard::scripted(Some("old"), vec![Some("Item Class: Rings")]);
        let trigger = CountingTrigger::new();

        copy_item(&mut clip, &trigger, fast(), false, no_sleep).unwrap();

        assert_eq!(trigger.count(), 1);
    }

    #[test]
    fn the_old_clipboard_is_put_back() {
        let mut clip =
            ScriptedClipboard::scripted(Some("my notes"), vec![Some("Item Class: Rings")]);

        copy_item(&mut clip, &CountingTrigger::new(), fast(), true, no_sleep).unwrap();

        assert_eq!(clip.writes, vec!["my notes"]);
    }

    #[test]
    fn the_old_clipboard_is_left_alone_when_restore_is_off() {
        let mut clip =
            ScriptedClipboard::scripted(Some("my notes"), vec![Some("Item Class: Rings")]);

        copy_item(&mut clip, &CountingTrigger::new(), fast(), false, no_sleep).unwrap();

        assert!(clip.writes.is_empty());
    }

    #[test]
    fn an_empty_starting_clipboard_needs_nothing_put_back() {
        let mut clip = ScriptedClipboard::scripted(None, vec![Some("Item Class: Rings")]);

        let got = copy_item(&mut clip, &CountingTrigger::new(), fast(), true, no_sleep).unwrap();

        assert_eq!(got, "Item Class: Rings");
        assert!(clip.writes.is_empty());
    }

    #[test]
    fn a_late_answer_is_still_caught() {
        let mut clip = ScriptedClipboard::scripted(
            Some("old"),
            vec![
                Some("old"),
                Some("old"),
                Some("old"),
                Some("Item Class: Rings"),
            ],
        );

        let got = copy_item(&mut clip, &CountingTrigger::new(), fast(), false, no_sleep).unwrap();

        assert_eq!(got, "Item Class: Rings");
    }

    #[test]
    fn the_empty_moment_during_a_copy_is_not_read_as_the_item() {
        let mut clip = ScriptedClipboard::scripted(
            Some("old"),
            vec![None, Some("  "), Some("Item Class: Rings")],
        );

        let got = copy_item(&mut clip, &CountingTrigger::new(), fast(), false, no_sleep).unwrap();

        assert_eq!(got, "Item Class: Rings");
    }

    #[test]
    fn a_clipboard_that_never_changes_fails_rather_than_returning_the_old_text() {
        let mut clip = ScriptedClipboard::holding(Some("my notes"));

        let err =
            copy_item(&mut clip, &CountingTrigger::new(), fast(), false, no_sleep).unwrap_err();

        assert!(matches!(err, ClipboardError::NoChange { .. }));
    }

    #[test]
    fn the_failure_message_says_how_long_it_waited() {
        let mut clip = ScriptedClipboard::holding(Some("my notes"));

        let err =
            copy_item(&mut clip, &CountingTrigger::new(), fast(), false, no_sleep).unwrap_err();

        assert!(err.to_string().contains("did not change"));
    }

    #[test]
    fn the_wait_is_bounded_by_the_timeout() {
        let mut clip = ScriptedClipboard::holding(Some("my notes"));
        let slept = RefCell::new(Duration::ZERO);

        let _ = copy_item(&mut clip, &CountingTrigger::new(), fast(), false, |d| {
            *slept.borrow_mut() += d;
        });

        assert_eq!(*slept.borrow(), Duration::from_millis(100));
    }

    #[test]
    fn a_successful_copy_stops_waiting_early() {
        let mut clip = ScriptedClipboard::scripted(Some("old"), vec![Some("Item Class: Rings")]);
        let slept = RefCell::new(Duration::ZERO);

        copy_item(&mut clip, &CountingTrigger::new(), fast(), false, |d| {
            *slept.borrow_mut() += d;
        })
        .unwrap();

        assert_eq!(*slept.borrow(), Duration::from_millis(10));
    }

    #[test]
    fn the_default_timing_is_generous_enough_for_a_stutter() {
        let t = CopyTiming::default();

        assert!(t.timeout >= Duration::from_millis(500));
        assert!(t.poll_interval <= Duration::from_millis(20));
    }

    #[test]
    fn pricing_the_same_item_twice_works() {
        let mut clip =
            ScriptedClipboard::scripted(Some("Item Class: Rings"), vec![Some("Item Class: Rings")]);

        let got = copy_item(&mut clip, &CountingTrigger::new(), fast(), false, |_| {}).unwrap();

        assert_eq!(got, "Item Class: Rings");
    }

    #[test]
    fn a_clipboard_that_never_holds_an_item_times_out() {
        let mut clip = ScriptedClipboard::scripted(
            Some("my notes"),
            vec![Some("still my notes"), Some("and again")],
        );

        let got = copy_item(&mut clip, &CountingTrigger::new(), fast(), false, |_| {});

        assert!(matches!(got, Err(ClipboardError::NoChange { .. })));
    }

    #[test]
    fn a_foreign_client_item_is_returned_rather_than_timing_out() {
        let mut clip =
            ScriptedClipboard::scripted(Some("old"), vec![Some("Класс предмета: Кольца")]);

        let got = copy_item(&mut clip, &CountingTrigger::new(), fast(), false, |_| {}).unwrap();

        assert!(got.starts_with("Класс предмета"));
    }
}
