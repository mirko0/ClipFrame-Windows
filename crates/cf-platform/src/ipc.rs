//! Waking the capture helper process from the tray process.
//!
//! The helper owns a hidden message-only window with a fixed class name. The tray finds it
//! with `FindWindowExW` and posts a request code to it — no pipes, threads or polling, and
//! the helper's existing event loop receives the message for free.

use windows::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowExW,
    GWLP_USERDATA, GetWindowLongPtrW, GetWindowThreadProcessId, HWND_MESSAGE, PostMessageW,
    RegisterClassExW, SetWindowLongPtrW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WNDCLASSEXW,
};
use windows::core::{PCWSTR, w};

/// The message carrying a request code in `WPARAM`.
const REQUEST_MESSAGE: u32 = WM_APP + 1;
const CLASS_NAME: PCWSTR = w!("ClipFrame.CaptureHelper");
/// `ERROR_CLASS_ALREADY_EXISTS` from `winerror.h`.
const CLASS_ALREADY_EXISTS: u32 = 1410;

type Callback = Box<dyn Fn(u32)>;

/// The helper's receiving end. Requests arrive on the thread that created it, during its
/// normal message loop.
pub struct RequestWindow {
    hwnd: HWND,
    // Double box: a thin pointer to the trait object is stored in the window's user data.
    _callback: Box<Callback>,
}

impl std::fmt::Debug for RequestWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestWindow")
            .field("hwnd", &self.hwnd)
            .finish_non_exhaustive()
    }
}

impl RequestWindow {
    /// Creates the window; `on_request` runs for every posted request code.
    ///
    /// # Errors
    /// When the window class or window cannot be created.
    pub fn create(on_request: impl Fn(u32) + 'static) -> windows::core::Result<Self> {
        register_class()?;
        // SAFETY: the class was registered above; HWND_MESSAGE makes the window
        // message-only (invisible, never enumerated). It is destroyed in Drop.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS_NAME,
                None,
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(GetModuleHandleW(None)?.into()),
                None,
            )?
        };
        let callback: Box<Callback> = Box::new(Box::new(on_request));
        // SAFETY: the pointer stays valid for the window's whole life: `callback` is owned
        // by the returned value, and Drop destroys the window before freeing it.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, std::ptr::from_ref(&*callback) as isize) };
        Ok(Self {
            hwnd,
            _callback: callback,
        })
    }
}

impl Drop for RequestWindow {
    fn drop(&mut self) {
        // SAFETY: created by this thread in `create`, destroyed once, before the callback
        // it points to is freed (fields drop after this body runs).
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }
}

fn register_class() -> windows::core::Result<()> {
    // SAFETY: GetModuleHandleW(None) returns this executable's module handle.
    let instance = unsafe { GetModuleHandleW(None)? };
    let class = WNDCLASSEXW {
        cbSize: u32::try_from(size_of::<WNDCLASSEXW>()).unwrap_or(0),
        lpfnWndProc: Some(window_proc),
        hInstance: instance.into(),
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    // SAFETY: `class` is fully initialised; the name and procedure are 'static.
    if unsafe { RegisterClassExW(&raw const class) } == 0 {
        // SAFETY: read right after the failing call.
        let error = unsafe { GetLastError() };
        if error.0 != CLASS_ALREADY_EXISTS {
            return Err(error.into());
        }
    }
    Ok(())
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == REQUEST_MESSAGE {
        // SAFETY: user data is 0 or the `Callback` pointer stored by `create`, which outlives
        // the window.
        let callback = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Callback;
        // SAFETY: see above; only read, never freed here.
        if let Some(callback) = unsafe { callback.as_ref() } {
            callback(u32::try_from(wparam.0).unwrap_or(u32::MAX));
        }
        return LRESULT(0);
    }
    // SAFETY: default handling for every other message.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Outcome of [`post_request`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Posted {
    /// No helper is running.
    NoHelper,
    /// Delivered to helper process `pid`; `foreground` says whether it may take focus.
    Sent {
        /// The helper's process id.
        pid: u32,
        /// Whether `AllowSetForegroundWindow` succeeded.
        foreground: bool,
    },
}

/// Posts `code` to a running helper.
///
/// First passes on the right to take the foreground (the tray just received the hotkey, the
/// helper did not), so the overlay can get keyboard focus.
pub fn post_request(code: u32) -> Posted {
    // SAFETY: searching message-only windows by a constant class name.
    let Ok(hwnd) = (unsafe { FindWindowExW(Some(HWND_MESSAGE), None, CLASS_NAME, None) }) else {
        return Posted::NoHelper;
    };
    let mut pid = 0;
    // SAFETY: `hwnd` came from FindWindowExW; `pid` is a valid out-pointer.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut pid)) };
    let foreground = allow_foreground(pid);
    // SAFETY: posting a plain integer message; no pointers cross the process boundary.
    let sent = unsafe {
        PostMessageW(
            Some(hwnd),
            REQUEST_MESSAGE,
            WPARAM(code as usize),
            LPARAM(0),
        )
    };
    if sent.is_ok() {
        Posted::Sent { pid, foreground }
    } else {
        Posted::NoHelper
    }
}

/// Lets process `pid` bring its windows to the foreground (call from the process that
/// received the user's input). Returns `false` if Windows refused.
pub fn allow_foreground(pid: u32) -> bool {
    // SAFETY: no preconditions.
    unsafe { AllowSetForegroundWindow(pid) }.is_ok()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW};

    use super::*;

    /// Pumps this thread's messages until `done` or a timeout.
    fn pump_until(done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut msg = MSG::default();
        while !done() && Instant::now() < deadline {
            // SAFETY: standard message pump on this thread with a valid MSG out-pointer.
            while unsafe { PeekMessageW(&raw mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                // SAFETY: dispatching a message just retrieved for this thread.
                unsafe { DispatchMessageW(&raw const msg) };
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn posted_request_reaches_the_window_and_stops_after_drop() {
        let received = Rc::new(Cell::new(None));
        let sink = received.clone();
        let window = RequestWindow::create(move |code| sink.set(Some(code))).unwrap();

        assert!(matches!(post_request(7), Posted::Sent { .. }));
        pump_until(|| received.get().is_some());
        assert_eq!(received.get(), Some(7));

        drop(window);
        assert_eq!(post_request(8), Posted::NoHelper, "window is gone");
    }
}
