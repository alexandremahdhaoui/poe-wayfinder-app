use crate::adapter::clipboard_adapter::{copy_item, Clipboard, CopyTiming, CopyTrigger};
use crate::util::error_chain::render;

#[cfg_attr(test, mockall::automock)]
pub trait Copier {
    fn copy(&mut self) -> Result<String, String>;
}

pub struct CopyController<C: Clipboard, T: CopyTrigger> {
    clipboard: C,
    trigger: T,
    timing: CopyTiming,
    restore: bool,
}

impl<C: Clipboard, T: CopyTrigger> CopyController<C, T> {
    pub fn new(clipboard: C, trigger: T, timing: CopyTiming, restore: bool) -> Self {
        Self {
            clipboard,
            trigger,
            timing,
            restore,
        }
    }
}

impl<C: Clipboard, T: CopyTrigger> Copier for CopyController<C, T> {
    fn copy(&mut self) -> Result<String, String> {
        copy_item(
            &mut self.clipboard,
            &self.trigger,
            self.timing,
            self.restore,
            std::thread::sleep,
        )
        .map_err(|e| render(&e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::adapter::clipboard_adapter::ClipboardError;

    use std::sync::atomic::{AtomicUsize, Ordering};

    struct ScriptedClipboard {
        reads: Vec<Option<String>>,
        at: usize,
        written: Vec<String>,
    }

    impl ScriptedClipboard {
        fn new(reads: Vec<Option<String>>) -> Self {
            Self {
                reads,
                at: 0,
                written: Vec::new(),
            }
        }
    }

    impl Clipboard for ScriptedClipboard {
        fn read(&mut self) -> Result<Option<String>, ClipboardError> {
            let value = self.reads.get(self.at).cloned().unwrap_or(None);
            self.at += 1;

            Ok(value)
        }

        fn write(&mut self, text: &str) -> Result<(), ClipboardError> {
            self.written.push(text.to_string());

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
    }

    impl CopyTrigger for CountingTrigger {
        fn trigger_copy(&self) -> Result<(), ClipboardError> {
            self.fired.fetch_add(1, Ordering::Relaxed);

            Ok(())
        }
    }

    fn fast() -> CopyTiming {
        CopyTiming {
            timeout: std::time::Duration::from_millis(50),
            poll_interval: std::time::Duration::from_millis(1),
        }
    }

    #[test]
    fn a_changed_clipboard_is_returned_as_the_item() {
        let clipboard =
            ScriptedClipboard::new(vec![Some("old".into()), Some("Item Class: Rings".into())]);

        let got = CopyController::new(clipboard, CountingTrigger::new(), fast(), false)
            .copy()
            .expect("a copy");

        assert_eq!(got, "Item Class: Rings");
    }

    #[test]
    fn a_clipboard_that_never_changes_reports_the_whole_chain() {
        let clipboard = ScriptedClipboard::new(vec![Some("old".into())]);

        let err = CopyController::new(clipboard, CountingTrigger::new(), fast(), false)
            .copy()
            .expect_err("a failure");

        assert!(!err.is_empty(), "the reason must survive as text");
    }

    #[test]
    fn the_copy_keystroke_is_sent() {
        let clipboard =
            ScriptedClipboard::new(vec![Some("old".into()), Some("Item Class: Rings".into())]);
        let trigger = CountingTrigger::new();

        let mut controller = CopyController::new(clipboard, trigger, fast(), false);
        let _ = controller.copy();

        assert_eq!(controller.trigger.fired.load(Ordering::Relaxed), 1);
    }
}
