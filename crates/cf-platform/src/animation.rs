//! Turning off the window animations Windows plays when a window appears or disappears.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{DWMWA_TRANSITIONS_FORCEDISABLED, DwmSetWindowAttribute};
use windows::core::BOOL;

/// Shows and hides `window` without the zoom/fade animation (the frozen screen of the
/// capture overlay must appear exactly in place). Returns `false` if Windows refused.
pub fn disable(window: &impl HasWindowHandle) -> bool {
    let Ok(handle) = window.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return false;
    };
    let hwnd = HWND(win32.hwnd.get() as *mut _);
    let disabled = BOOL::from(true);
    // SAFETY: `hwnd` is a live window borrowed for the call; the attribute value is a BOOL
    // passed by pointer with its exact size.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            (&raw const disabled).cast(),
            u32::try_from(size_of::<BOOL>()).unwrap_or(4),
        )
    }
    .is_ok()
}
