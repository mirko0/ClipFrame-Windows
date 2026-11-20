//! Bringing a window to the foreground (needed so the overlay receives keyboard input).

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;

/// Makes `window` the foreground window. Returns `false` when Windows refused (the
/// foreground lock: only a process with foreground rights may take focus).
pub fn bring_to_front(window: &impl HasWindowHandle) -> bool {
    let Ok(handle) = window.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return false;
    };
    let hwnd = HWND(win32.hwnd.get() as *mut _);
    // SAFETY: `hwnd` belongs to a live window borrowed for the duration of the call.
    unsafe { SetForegroundWindow(hwnd) }.as_bool()
}
