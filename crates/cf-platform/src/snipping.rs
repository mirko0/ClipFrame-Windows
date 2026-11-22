//! Taking over Win+Shift+S from the Windows Snipping Tool.
//!
//! Windows reserves Win+Shift+S, so it can't be registered as a normal hotkey. Instead a
//! low-level keyboard hook (on its own thread, which sleeps in `GetMessage` between key
//! presses) swallows the S of that chord before the shell sees it — so the Snipping Tool
//! never starts — and reports it. Nothing in Windows' settings changes: stopping the hook
//! gives the shortcut back.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG,
    PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN,
};

/// Virtual key of S.
const VK_S: u32 = 0x53;
/// An unassigned virtual key, pressed to stop Windows opening Start when Win is released.
const VK_DUMMY: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);
/// Marks input this module sends, so the hook ignores it.
const MARKER: usize = 0x4346_534E; // "CFSN"

type Handler = Arc<dyn Fn() + Send + Sync>;

/// Called (on the hook thread) when Win+Shift+S is pressed.
static HANDLER: Mutex<Option<Handler>> = Mutex::new(None);
/// The S of the chord went down and was swallowed; its key-up must be swallowed too.
static SWALLOWING: AtomicBool = AtomicBool::new(false);

/// Why the takeover couldn't start.
#[derive(Debug, thiserror::Error)]
pub enum TakeoverError {
    /// The hook thread could not be started.
    #[error("could not start the keyboard hook thread: {0}")]
    Thread(#[from] std::io::Error),
    /// Windows refused the hook.
    #[error("could not install the keyboard hook: {0}")]
    Hook(windows::core::Error),
}

/// Win+Shift+S goes to ClipFrame while this is alive.
#[derive(Debug)]
pub struct Takeover {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl Takeover {
    /// Starts catching Win+Shift+S; `on_shortcut` runs on the hook thread for each press,
    /// so it must return quickly (post a message, nothing more).
    ///
    /// # Errors
    /// [`TakeoverError`] when the hook thread or the hook can't be set up.
    pub fn start(on_shortcut: impl Fn() + Send + Sync + 'static) -> Result<Self, TakeoverError> {
        if let Ok(mut handler) = HANDLER.lock() {
            *handler = Some(Arc::new(on_shortcut));
        }
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("snipping-hook".into())
            .spawn(move || hook_thread(&ready_tx))?;
        match ready_rx.recv() {
            Ok(Ok(thread_id)) => Ok(Self {
                thread_id,
                thread: Some(thread),
            }),
            Ok(Err(err)) => {
                let _ = thread.join();
                Err(TakeoverError::Hook(err))
            }
            Err(_) => {
                let _ = thread.join();
                Err(TakeoverError::Hook(windows::core::Error::empty()))
            }
        }
    }
}

impl Drop for Takeover {
    fn drop(&mut self) {
        // SAFETY: posting to a thread id we created; harmless if it has already exited.
        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if let Ok(mut handler) = HANDLER.lock() {
            *handler = None;
        }
        SWALLOWING.store(false, Ordering::SeqCst);
    }
}

fn hook_thread(ready: &mpsc::Sender<windows::core::Result<u32>>) {
    // SAFETY: installs a global low-level hook with a valid callback and this module's
    // handle; it is removed below on the same thread.
    let hook = unsafe {
        GetModuleHandleW(None).and_then(|module| {
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), Some(module.into()), 0)
        })
    };
    let hook = match hook {
        Ok(hook) => hook,
        Err(err) => {
            let _ = ready.send(Err(err));
            return;
        }
    };
    // SAFETY: no preconditions.
    let _ = ready.send(Ok(unsafe { GetCurrentThreadId() }));
    let mut msg = MSG::default();
    // SAFETY: a standard message loop on this thread; it ends with WM_QUIT (or an error).
    unsafe {
        while GetMessageW(&raw mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&raw const msg);
            DispatchMessageW(&raw const msg);
        }
        let _ = UnhookWindowsHookEx(hook);
    }
}

/// What to do with a key event of S.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Decision {
    /// Hide the event from everything else (the shell, the focused app).
    swallow: bool,
    /// Report the shortcut.
    trigger: bool,
}

/// `down`: S went down (or repeats); `chord`: Win and Shift held without Ctrl or Alt;
/// `swallowing`: an earlier S-down of the chord was swallowed and its key-up is pending.
const fn decide(down: bool, chord: bool, swallowing: bool) -> Decision {
    if swallowing {
        // Auto-repeat while held, then the key-up: keep hiding them, report once.
        return Decision {
            swallow: true,
            trigger: false,
        };
    }
    let hit = down && chord;
    Decision {
        swallow: hit,
        trigger: hit,
    }
}

/// Handles an S key event; returns whether to hide it.
fn on_s(down: bool) -> bool {
    let swallowing = SWALLOWING.load(Ordering::SeqCst);
    let decision = decide(down, down && chord_held(), swallowing);
    if decision.swallow {
        SWALLOWING.store(down, Ordering::SeqCst);
    }
    if decision.trigger {
        keep_start_menu_closed();
        notify();
    }
    decision.swallow
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == i32::try_from(HC_ACTION).unwrap_or(0) {
        // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, lparam points to a KBDLLHOOKSTRUCT.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if info.vkCode == VK_S && info.dwExtraInfo != MARKER {
            #[allow(clippy::cast_possible_truncation)] // message ids fit in u32
            let message = wparam.0 as u32;
            let down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
            if on_s(down) {
                return LRESULT(1);
            }
        }
    }
    // SAFETY: passes the event on unchanged.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn chord_held() -> bool {
    let held = |key: VIRTUAL_KEY| {
        // SAFETY: no preconditions; the high bit means "down now".
        let state = unsafe { GetAsyncKeyState(i32::from(key.0)) };
        state < 0
    };
    (held(VK_LWIN) || held(VK_RWIN)) && held(VK_SHIFT) && !held(VK_CONTROL) && !held(VK_MENU)
}

/// Windows opens Start when Win is released with no other key in between; the S was
/// hidden, so press an unassigned key instead.
fn keep_start_menu_closed() {
    let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_DUMMY,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: MARKER,
            },
        },
    };
    let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
    // SAFETY: two well-formed keyboard inputs of the size passed.
    let _ = unsafe { SendInput(&inputs, i32::try_from(size_of::<INPUT>()).unwrap_or(0)) };
}

fn notify() {
    let handler = HANDLER.lock().ok().and_then(|h| h.clone());
    if let Some(handler) = handler {
        handler();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chord_is_reported_once_and_its_key_up_hidden() {
        assert_eq!(
            decide(true, true, false),
            Decision {
                swallow: true,
                trigger: true
            }
        );
        let repeat = decide(true, true, true);
        assert!(repeat.swallow && !repeat.trigger);
        let up = decide(false, false, true);
        assert!(up.swallow && !up.trigger);
    }

    #[test]
    fn plain_s_is_left_alone() {
        let plain = Decision {
            swallow: false,
            trigger: false,
        };
        assert_eq!(decide(true, false, false), plain);
        assert_eq!(decide(false, false, false), plain);
    }
}
