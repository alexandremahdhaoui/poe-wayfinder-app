use thiserror::Error;

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

    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::mpsc::{self, Receiver};

    use poe_wayfinder_core::controller::hotkey_match::{
        Binding, KeyEvent, Modifiers, Reaction, Watcher,
    };

    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
        WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    pub const MAX_BINDINGS: usize = 8;

    static FIRED: [AtomicU32; MAX_BINDINGS] = [
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
        AtomicU32::new(0),
    ];

    static WATCHER: std::sync::Mutex<Option<Watcher>> = std::sync::Mutex::new(None);

    static INSTALLED: AtomicBool = AtomicBool::new(false);

    pub struct HookDriver {
        seen: [u32; MAX_BINDINGS],
        thread_id: u32,
        stopped: Receiver<()>,
    }

    impl HookDriver {
        pub fn start(bindings: Vec<Binding>) -> Result<Self, HookError> {
            if INSTALLED.swap(true, Ordering::SeqCst) {
                return Err(HookError::AlreadyInstalled);
            }

            for counter in FIRED.iter() {
                counter.store(0, Ordering::SeqCst);
            }

            if let Ok(mut watcher) = WATCHER.lock() {
                *watcher = Some(Watcher::new(
                    bindings.into_iter().take(MAX_BINDINGS).collect(),
                ));
            }

            let (ready_tx, ready_rx) = mpsc::channel();
            let (stop_tx, stop_rx) = mpsc::channel();

            std::thread::spawn(move || {
                let id = unsafe { GetCurrentThreadId() };

                let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) };

                let Ok(hook) = hook else {
                    let _ = ready_tx.send(None);

                    return;
                };

                let _ = ready_tx.send(Some(id));

                let mut message = MSG::default();

                while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
                    unsafe {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }

                let _ = unsafe { UnhookWindowsHookEx(hook) };

                let _ = stop_tx.send(());
            });

            match ready_rx.recv() {
                Ok(Some(thread_id)) => Ok(Self {
                    seen: [0; MAX_BINDINGS],
                    thread_id,
                    stopped: stop_rx,
                }),
                _ => {
                    INSTALLED.store(false, Ordering::SeqCst);

                    Err(HookError::Install)
                }
            }
        }

        pub fn fired(&mut self) -> Option<usize> {
            let mut hit = None;

            for (index, counter) in FIRED.iter().enumerate() {
                let now = counter.load(Ordering::SeqCst);

                if now != self.seen[index] && hit.is_none() {
                    hit = Some(index);
                }

                self.seen[index] = now;
            }

            hit
        }

        pub fn stop(self) {
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

    static CTRL: AtomicBool = AtomicBool::new(false);
    static ALT: AtomicBool = AtomicBool::new(false);
    static SHIFT: AtomicBool = AtomicBool::new(false);
    static META: AtomicBool = AtomicBool::new(false);

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

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code < 0 {
            return CallNextHookEx(None, code, wparam, lparam);
        }

        let message = wparam.0 as u32;
        let down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        let up = message == WM_KEYUP || message == WM_SYSKEYUP;

        if down || up {
            let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let key = info.vkCode as u16;

            if !track_modifier(key, down) {
                let event = KeyEvent {
                    code: key,
                    down,
                    modifiers: Modifiers {
                        ctrl: CTRL.load(Ordering::SeqCst),
                        alt: ALT.load(Ordering::SeqCst),
                        shift: SHIFT.load(Ordering::SeqCst),
                        meta: META.load(Ordering::SeqCst),
                    },
                };

                let reaction = match WATCHER.lock() {
                    Ok(mut held) => match held.as_mut() {
                        Some(watcher) => watcher.react(event),
                        None => Reaction::Ignore,
                    },
                    Err(_) => Reaction::Ignore,
                };

                if let Reaction::Fire { binding } = reaction {
                    if let Some(counter) = FIRED.get(binding) {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                }

                if reaction.eats_the_key() {
                    return LRESULT(1);
                }
            }
        }

        CallNextHookEx(None, code, wparam, lparam)
    }
}

#[cfg(windows)]
pub use win::{HookDriver, MAX_BINDINGS};

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
