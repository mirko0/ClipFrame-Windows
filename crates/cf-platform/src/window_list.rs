//! Snapshot of the visible top-level windows, topmost first.

use std::ffi::c_void;

use cf_core::geometry::Rect;
use cf_core::targets::{WindowInfo, WindowSnapshot};
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DWMWINDOWATTRIBUTE, DwmGetWindowAttribute,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetClassNameW, GetWindowLongW, GetWindowRect,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, WS_EX_TRANSPARENT,
};
use windows::core::BOOL;

/// Desktop background windows: hovering bare desktop should not select them.
const DESKTOP_CLASSES: [&str; 2] = ["Progman", "WorkerW"];

/// Lists the visible, uncloaked, non-minimised top-level windows of other processes, in
/// z-order. Rectangles are the visible frame (without the invisible resize borders).
///
/// # Errors
/// When Windows fails to enumerate the windows.
pub fn snapshot() -> windows::core::Result<WindowSnapshot> {
    let mut handles: Vec<HWND> = Vec::new();
    // SAFETY: the callback only runs during this call and `handles` outlives it; the LPARAM
    // carries a pointer to `handles`, which the callback casts back to the same type.
    unsafe {
        EnumWindows(
            Some(collect_window),
            LPARAM(std::ptr::from_mut(&mut handles) as isize),
        )
    }?;
    let own_pid = std::process::id();
    let windows = handles
        .into_iter()
        .filter(|&hwnd| is_candidate(hwnd, own_pid))
        .filter_map(|hwnd| {
            Some(WindowInfo {
                id: hwnd.0 as usize,
                rect: visible_bounds(hwnd)?,
            })
        })
        .collect();
    Ok(WindowSnapshot::new(windows))
}

unsafe extern "system" fn collect_window(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: `data` is the `&mut Vec<HWND>` passed by `snapshot`, valid for the duration
    // of EnumWindows, and not aliased elsewhere meanwhile.
    let handles = unsafe { &mut *(data.0 as *mut Vec<HWND>) };
    handles.push(hwnd);
    true.into()
}

fn is_candidate(hwnd: HWND, own_pid: u32) -> bool {
    // SAFETY: plain queries on a window handle; a handle destroyed meanwhile just fails.
    let (visible, minimised, ex_style) = unsafe {
        (
            IsWindowVisible(hwnd).as_bool(),
            IsIconic(hwnd).as_bool(),
            u32::from_ne_bytes(GetWindowLongW(hwnd, GWL_EXSTYLE).to_ne_bytes()),
        )
    };
    let click_through = ex_style & WS_EX_TRANSPARENT.0 != 0;
    visible
        && !minimised
        && !click_through
        && !is_cloaked(hwnd)
        && process_id(hwnd) != own_pid
        && !DESKTOP_CLASSES.contains(&class_name(hwnd).as_str())
}

fn process_id(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    // SAFETY: `pid` is a valid out pointer for the duration of the call.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut pid)) };
    pid
}

fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 64];
    // SAFETY: the slice is a valid, writable buffer; Windows writes at most its length.
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..usize::try_from(len).unwrap_or(0)])
}

/// Cloaked windows are "visible" but not drawn: other virtual desktops, suspended UWP apps.
fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    dwm_attribute(hwnd, DWMWA_CLOAKED, &mut cloaked) && cloaked != 0
}

fn visible_bounds(hwnd: HWND) -> Option<Rect> {
    let mut rect = RECT::default();
    let found = dwm_attribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut rect)
        // SAFETY: `rect` is a valid out pointer for the duration of the call.
        || unsafe { GetWindowRect(hwnd, &raw mut rect) }.is_ok();
    found.then(|| Rect::from_edges(rect.left, rect.top, rect.right, rect.bottom))
}

fn dwm_attribute<T>(hwnd: HWND, attribute: DWMWINDOWATTRIBUTE, value: &mut T) -> bool {
    let size = u32::try_from(size_of::<T>()).unwrap_or(0);
    // SAFETY: `value` is a valid, writable `T` of exactly `size` bytes, which is the size
    // the attribute expects (u32 for CLOAKED, RECT for EXTENDED_FRAME_BOUNDS).
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            attribute,
            std::ptr::from_mut(value).cast::<c_void>(),
            size,
        )
    }
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_enumerates_without_error() {
        assert!(snapshot().is_ok());
    }
}
