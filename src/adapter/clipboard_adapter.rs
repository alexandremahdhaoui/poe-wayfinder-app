//! Reading the item under the cursor.
//!
//! The game has no API. The only way to read an item is to send Ctrl+C to the
//! game window and read what lands on the clipboard.
//!
//! # Why the clipboard is put back
//!
//! The user was probably in the middle of copying something. Destroying their
//! clipboard on every price check is the kind of small rudeness that makes a
//! tool annoying to live with.
//!
//! # Why there is a wait
//!
//! The copy is asynchronous. The game receives the keypress, formats the item
//! and writes the clipboard, and none of that is instant. Reading immediately
//! returns the old contents, which reads as "the same item every time".

use std::time::Duration;

use thiserror::Error;

/// Why a clipboard read failed.
#[derive(Debug, Error)]
pub enum ClipboardError {
    #[error("opening the clipboard")]
    Open(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("reading the clipboard")]
    Read(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("writing the clipboard")]
    Write(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// The clipboard never changed.
    ///
    /// Almost always means the cursor was not over an item. Saying so beats
    /// pricing whatever happened to be on the clipboard already.
    #[error("the clipboard did not change within {waited:?}")]
    NoChange { waited: Duration },
}

/// Reading and writing the system clipboard.
///
/// Declared here because this module implements it. A test supplies one backed
/// by a string, so the copy loop is testable with no windowing system.
pub trait Clipboard: Send + Sync {
    /// Current contents. None when the clipboard holds no text.
    fn read(&mut self) -> Result<Option<String>, ClipboardError>;

    /// Replace the contents.
    fn write(&mut self, text: &str) -> Result<(), ClipboardError>;
}

/// Asking the game to copy the item under the cursor.
pub trait CopyTrigger: Send + Sync {
    /// Send the copy keystroke to the game.
    fn trigger_copy(&self) -> Result<(), ClipboardError>;
}

/// How long to wait for the game to answer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CopyTiming {
    /// How long to wait in total.
    pub timeout: Duration,
    /// How long between checks.
    pub poll_interval: Duration,
}

impl Default for CopyTiming {
    fn default() -> Self {
        Self {
            // Generous. A stutter in the game is common and a failed price
            // check is far more annoying than a slightly slow one.
            timeout: Duration::from_millis(600),
            poll_interval: Duration::from_millis(10),
        }
    }
}

/// Read the item under the cursor.
///
/// Sends the copy keystroke, waits for the clipboard to change, and puts the
/// old contents back when asked.
///
/// `sleep` is injected so a test drives the wait without taking it.
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

        // Changed AND non empty. The game briefly empties the clipboard on the
        // way, and reading that empty moment yields nothing to parse.
        if now != before {
            if let Some(text) = now {
                if !text.trim().is_empty() {
                    found = Some(text);

                    break;
                }
            }
        }
    }

    let Some(text) = found else {
        return Err(ClipboardError::NoChange { waited });
    };

    if restore {
        // A failure here is not worth failing the price check over. The user
        // asked for a price, not for clipboard hygiene.
        if let Some(old) = before {
            let _ = clipboard.write(&old);
        }
    }

    Ok(text)
}

/// The real clipboard.
#[cfg(windows)]
pub struct SystemClipboard {
    inner: arboard::Clipboard,
}

#[cfg(windows)]
impl SystemClipboard {
    /// Open the system clipboard.
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
            // An empty or non text clipboard is not an error. It is the normal
            // state before the game has answered.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A clipboard backed by a string.
    struct FakeClipboard {
        content: Option<String>,
        /// What each successive read returns, so a test can script the game
        /// answering late.
        script: Vec<Option<String>>,
        reads: usize,
        writes: Vec<String>,
    }

    impl FakeClipboard {
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

    impl Clipboard for FakeClipboard {
        fn read(&mut self) -> Result<Option<String>, ClipboardError> {
            // The first read is the "before" snapshot and always sees the
            // initial content. Every later read walks the script.
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

    /// Counts the keystrokes it was asked to send.
    ///
    /// An atomic rather than a RefCell. `CopyTrigger` requires `Sync` and
    /// asserting that by hand would be a lie waiting to become a data race.
    struct FakeTrigger {
        fired: AtomicUsize,
    }

    impl FakeTrigger {
        fn new() -> Self {
            Self {
                fired: AtomicUsize::new(0),
            }
        }

        fn count(&self) -> usize {
            self.fired.load(Ordering::Relaxed)
        }
    }

    impl CopyTrigger for FakeTrigger {
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
    fn a_changed_clipboard_is_returned() {
        let mut clip = FakeClipboard::scripted(Some("old"), vec![Some("Item Class: Rings")]);

        let got = copy_item(&mut clip, &FakeTrigger::new(), fast(), false, no_sleep).unwrap();

        assert_eq!(got, "Item Class: Rings");
    }

    #[test]
    fn the_copy_keystroke_is_sent_once() {
        let mut clip = FakeClipboard::scripted(Some("old"), vec![Some("new")]);
        let trigger = FakeTrigger::new();

        copy_item(&mut clip, &trigger, fast(), false, no_sleep).unwrap();

        assert_eq!(trigger.count(), 1);
    }

    #[test]
    fn the_old_clipboard_is_put_back() {
        // The user was probably mid copy. Destroying their clipboard on every
        // price check makes the tool annoying to live with.
        let mut clip = FakeClipboard::scripted(Some("my notes"), vec![Some("Item Class: Rings")]);

        copy_item(&mut clip, &FakeTrigger::new(), fast(), true, no_sleep).unwrap();

        assert_eq!(clip.writes, vec!["my notes"]);
    }

    #[test]
    fn the_old_clipboard_is_left_alone_when_restore_is_off() {
        let mut clip = FakeClipboard::scripted(Some("my notes"), vec![Some("Item Class: Rings")]);

        copy_item(&mut clip, &FakeTrigger::new(), fast(), false, no_sleep).unwrap();

        assert!(clip.writes.is_empty());
    }

    #[test]
    fn an_empty_starting_clipboard_needs_nothing_put_back() {
        let mut clip = FakeClipboard::scripted(None, vec![Some("Item Class: Rings")]);

        let got = copy_item(&mut clip, &FakeTrigger::new(), fast(), true, no_sleep).unwrap();

        assert_eq!(got, "Item Class: Rings");
        assert!(clip.writes.is_empty());
    }

    #[test]
    fn a_late_answer_is_still_caught() {
        // The game formats the item and writes the clipboard, and none of that
        // is instant. Giving up on the first check reads as "the same item
        // every time".
        let mut clip = FakeClipboard::scripted(
            Some("old"),
            vec![
                Some("old"),
                Some("old"),
                Some("old"),
                Some("Item Class: Rings"),
            ],
        );

        let got = copy_item(&mut clip, &FakeTrigger::new(), fast(), false, no_sleep).unwrap();

        assert_eq!(got, "Item Class: Rings");
    }

    #[test]
    fn the_empty_moment_during_a_copy_is_not_read_as_the_item() {
        // The game briefly empties the clipboard on the way. Reading that
        // moment yields nothing to parse.
        let mut clip = FakeClipboard::scripted(
            Some("old"),
            vec![None, Some("  "), Some("Item Class: Rings")],
        );

        let got = copy_item(&mut clip, &FakeTrigger::new(), fast(), false, no_sleep).unwrap();

        assert_eq!(got, "Item Class: Rings");
    }

    #[test]
    fn a_clipboard_that_never_changes_fails_rather_than_returning_the_old_text() {
        // Almost always means the cursor was not over an item. Pricing the old
        // clipboard would show a confident answer about the wrong thing.
        let mut clip = FakeClipboard::holding(Some("my notes"));

        let err = copy_item(&mut clip, &FakeTrigger::new(), fast(), false, no_sleep).unwrap_err();

        assert!(matches!(err, ClipboardError::NoChange { .. }));
    }

    #[test]
    fn the_failure_message_says_how_long_it_waited() {
        let mut clip = FakeClipboard::holding(Some("my notes"));

        let err = copy_item(&mut clip, &FakeTrigger::new(), fast(), false, no_sleep).unwrap_err();

        assert!(err.to_string().contains("did not change"));
    }

    #[test]
    fn the_wait_is_bounded_by_the_timeout() {
        let mut clip = FakeClipboard::holding(Some("my notes"));
        let slept = RefCell::new(Duration::ZERO);

        let _ = copy_item(&mut clip, &FakeTrigger::new(), fast(), false, |d| {
            *slept.borrow_mut() += d;
        });

        assert_eq!(*slept.borrow(), Duration::from_millis(100));
    }

    #[test]
    fn a_successful_copy_stops_waiting_early() {
        // A price check that always took the full timeout would feel broken.
        let mut clip = FakeClipboard::scripted(Some("old"), vec![Some("new")]);
        let slept = RefCell::new(Duration::ZERO);

        copy_item(&mut clip, &FakeTrigger::new(), fast(), false, |d| {
            *slept.borrow_mut() += d;
        })
        .unwrap();

        assert_eq!(*slept.borrow(), Duration::from_millis(10));
    }

    #[test]
    fn the_default_timing_is_generous_enough_for_a_stutter() {
        // A failed price check is far more annoying than a slightly slow one.
        let t = CopyTiming::default();

        assert!(t.timeout >= Duration::from_millis(500));
        assert!(t.poll_interval <= Duration::from_millis(20));
    }
}
