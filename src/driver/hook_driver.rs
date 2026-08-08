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

    static WANTED_CODE: AtomicU16 = AtomicU16::new(0);
    static WANTED_MODS: AtomicU32 = AtomicU32::new(0);

    static FIRED: AtomicU32 = AtomicU32::new(0);

    static INSTALLED: AtomicBool = AtomicBool::new(false);

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

    pub struct HookDriver {
        seen: u32,
        thread_id: u32,
        stopped: Receiver<()>,
    }

    impl HookDriver {
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

        pub fn fired(&mut self) -> bool {
            let now = FIRED.load(Ordering::SeqCst);
            let any = now != self.seen;

            self.seen = now;

            any
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
