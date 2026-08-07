//! The price check hotkey, watched with a low level keyboard hook.
//!
//! An alternative to `hotkey_driver`, which uses `RegisterHotKey`.
//!
//! # Why both exist
//!
//! `RegisterHotKey` asks Windows to watch one combination. It is the smallest
//! possible ask and sees nothing else, which is why it came first. It has two
//! costs:
//!
//! - Windows can decline to deliver it. A hotkey that registers cleanly and
//!   never fires is the hardest failure this tool has.
//! - The delivery cannot be tested. Injected input carries `LLKHF_INJECTED`
//!   and the hotkey machinery ignores flagged input, so no test can press the
//!   key.
//!
//! A hook has the opposite properties. It sees injected input, which makes the
//! whole path testable end to end, and Awakened PoE Trade uses one for the
//! same job.
//!
//! # What this hook does with what it sees
//!
//! A low level hook sees every keystroke on the machine, which is a serious
//! thing to install. So this one holds none of them.
//!
//! Each event goes straight to
//! `poe_trader_core::controller::hotkey_match::fires`, which answers one
//! question and keeps nothing. No text is assembled, no history is kept, and
//! the only state that outlives a call is four modifier booleans and a counter
//! of matches. There is no keystroke log here to leak, even in principle.
//!
//! The hook never swallows a key either. Every event is passed on to whatever
//! comes next, so nothing this installs can stop the game seeing input.

use thiserror::Error;

/// Why the hook could not be installed.
#[derive(Debug, Error)]
pub enum HookError {
    #[error("installing the keyboard hook")]
    Install,

    #[error("a keyboard hook is already installed by this process")]
    AlreadyInstalled,
}

#[cfg(windows)]
mod win {
    use super::HookError;

    use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};
    use std::sync::mpsc::{self, Receiver};

    use poe_trader_core::controller::hotkey_match::{fires, is_modifier_code, KeyEvent, Modifiers};

    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
        WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    /// The combination being watched.
    ///
    /// Statics because the hook procedure Windows calls is a bare function
    /// pointer with nowhere to hang state, and there is one hook per process.
    static WANTED_CODE: AtomicU16 = AtomicU16::new(0);
    static WANTED_MODS: AtomicU32 = AtomicU32::new(0);

    /// How many times the combination has fired.
    ///
    /// A counter rather than a channel, because a hook procedure must return
    /// fast and must not block. The frame loop reads the difference.
    static FIRED: AtomicU32 = AtomicU32::new(0);

    /// Whether a hook is already installed by this process.
    static INSTALLED: AtomicBool = AtomicBool::new(false);

    /// Pack the modifier flags into an integer, for the atomic.
    fn pack(m: Modifiers) -> u32 {
        (m.ctrl as u32) | (m.alt as u32) << 1 | (m.shift as u32) << 2 | (m.meta as u32) << 3
    }

    fn unpack(bits: u32) -> Modifiers {
        Modifiers {
            ctrl: bits & 1 != 0,
            alt: bits & 2 != 0,
            shift: bits & 4 != 0,
            meta: bits & 8 != 0,
        }
    }

    /// The hook, for as long as this is alive.
    pub struct HookDriver {
        seen: u32,
        thread_id: u32,
        stopped: Receiver<()>,
    }

    impl HookDriver {
        /// Install the hook and watch for one combination.
        pub fn start(code: u16, modifiers: Modifiers) -> Result<Self, HookError> {
            if INSTALLED.swap(true, Ordering::SeqCst) {
                return Err(HookError::AlreadyInstalled);
            }

            WANTED_CODE.store(code, Ordering::SeqCst);
            WANTED_MODS.store(pack(modifiers), Ordering::SeqCst);
            FIRED.store(0, Ordering::SeqCst);

            let (ready_tx, ready_rx) = mpsc::channel();
            let (stop_tx, stop_rx) = mpsc::channel();

            std::thread::spawn(move || {
                // SAFETY: no arguments and no failure mode.
                let id = unsafe { GetCurrentThreadId() };

                // SAFETY: `hook_proc` matches HOOKPROC. A null module and a
                // zero thread id install a global low level hook, which is the
                // only kind WH_KEYBOARD_LL supports.
                let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) };

                let Ok(hook) = hook else {
                    let _ = ready_tx.send(None);

                    return;
                };

                let _ = ready_tx.send(Some(id));

                // A low level hook is only called while its thread pumps
                // messages. Without this loop the hook is installed and silent.
                let mut message = MSG::default();

                // SAFETY: `message` is a live, correctly sized MSG.
                while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
                    unsafe {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }

                // SAFETY: removing the hook this thread installed.
                let _ = unsafe { UnhookWindowsHookEx(hook) };

                let _ = stop_tx.send(());
            });

            match ready_rx.recv() {
                Ok(Some(thread_id)) => Ok(Self {
                    seen: 0,
                    thread_id,
                    stopped: stop_rx,
                }),
                _ => {
                    INSTALLED.store(false, Ordering::SeqCst);

                    Err(HookError::Install)
                }
            }
        }

        /// Whether the combination fired since the last call.
        ///
        /// Drained, like the other driver. Two presses during one frame are
        /// one price check, because the rate limiter is not a suggestion.
        pub fn fired(&mut self) -> bool {
            let now = FIRED.load(Ordering::SeqCst);
            let any = now != self.seen;

            self.seen = now;

            any
        }

        /// Stop the hook and wait for its thread to let go.
        pub fn stop(self) {
            // SAFETY: posting a quit to a thread this struct owns.
            let _ = unsafe {
                PostThreadMessageW(
                    self.thread_id,
                    windows::Win32::UI::WindowsAndMessaging::WM_QUIT,
                    WPARAM(0),
                    LPARAM(0),
                )
            };

            let _ = self.stopped.recv_timeout(std::time::Duration::from_secs(1));

            INSTALLED.store(false, Ordering::SeqCst);
        }
    }

    /// Read the modifier state from the event's own flags and the key itself.
    ///
    /// A low level hook is told which key moved, not what else is held, so the
    /// modifier state is tracked as keys go down and come up.
    static CTRL: AtomicBool = AtomicBool::new(false);
    static ALT: AtomicBool = AtomicBool::new(false);
    static SHIFT: AtomicBool = AtomicBool::new(false);
    static META: AtomicBool = AtomicBool::new(false);

    /// Update the tracked modifiers, and say whether this key was one.
    fn track_modifier(code: u16, down: bool) -> bool {
        let slot = match code {
            0x10 | 0xA0 | 0xA1 => &SHIFT,
            0x11 | 0xA2 | 0xA3 => &CTRL,
            0x12 | 0xA4 | 0xA5 => &ALT,
            0x5B | 0x5C => &META,
            _ => return false,
        };

        slot.store(down, Ordering::SeqCst);

        true
    }

    /// Windows calls this for every keystroke on the machine.
    ///
    /// # Safety
    ///
    /// Called by Windows with a valid `KBDLLHOOKSTRUCT` when `code` is
    /// `HC_ACTION`. Nothing else calls it.
    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // Negative means the hook must pass it on without looking.
        if code < 0 {
            return CallNextHookEx(None, code, wparam, lparam);
        }

        let message = wparam.0 as u32;
        let down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        let up = message == WM_KEYUP || message == WM_SYSKEYUP;

        if down || up {
            let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let key = info.vkCode as u16;

            // A modifier only updates the tracked state. Treating it as the
            // combination's key would fire on the Ctrl rather than the letter.
            if !track_modifier(key, down) && down {
                let event = KeyEvent {
                    code: key,
                    down: true,
                    modifiers: Modifiers {
                        ctrl: CTRL.load(Ordering::SeqCst),
                        alt: ALT.load(Ordering::SeqCst),
                        shift: SHIFT.load(Ordering::SeqCst),
                        meta: META.load(Ordering::SeqCst),
                    },
                };

                let wanted_code = WANTED_CODE.load(Ordering::SeqCst);
                let wanted = unpack(WANTED_MODS.load(Ordering::SeqCst));

                if !is_modifier_code(wanted_code) && fires(event, wanted_code, wanted) {
                    FIRED.fetch_add(1, Ordering::SeqCst);
                }
            }
        }

        // Always passed on. A hook that swallowed a key would stop the game
        // seeing input, which is never worth it for a price check.
        CallNextHookEx(None, code, wparam, lparam)
    }
}

#[cfg(windows)]
pub use win::HookDriver;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_says_what_failed() {
        for (error, wanted) in [
            (HookError::Install, "keyboard hook"),
            (HookError::AlreadyInstalled, "already installed"),
        ] {
            assert!(
                error.to_string().contains(wanted),
                "{error} does not mention {wanted}"
            );
        }
    }
}
