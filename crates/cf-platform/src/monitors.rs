//! Enumerating the connected monitors.

use cf_core::geometry::Rect;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::core::BOOL;

/// `MONITORINFOF_PRIMARY` from `WinUser.h`.
const PRIMARY_FLAG: u32 = 1;

/// One connected monitor, in physical virtual-desktop pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorInfo {
    /// Full monitor bounds.
    pub rect: Rect,
    /// Bounds excluding the taskbar and docked toolbars.
    pub work_area: Rect,
    /// Whether this is the primary monitor.
    pub is_primary: bool,
}

/// Lists all monitors (requires per-monitor DPI awareness for physical coordinates, which
/// the application manifest provides).
///
/// # Errors
/// When Windows fails to enumerate the monitors.
pub fn enumerate() -> windows::core::Result<Vec<MonitorInfo>> {
    let mut handles: Vec<HMONITOR> = Vec::new();
    // SAFETY: the callback only runs during this call and `handles` outlives it; the LPARAM
    // carries a pointer to `handles`, which the callback casts back to the same type.
    unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect_monitor),
            LPARAM(std::ptr::from_mut(&mut handles) as isize),
        )
    }
    .ok()?;
    handles.into_iter().map(monitor_info).collect()
}

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    // SAFETY: `data` is the `&mut Vec<HMONITOR>` passed by `enumerate`, valid for the
    // duration of EnumDisplayMonitors, and not aliased elsewhere meanwhile.
    let handles = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
    handles.push(monitor);
    true.into()
}

fn monitor_info(monitor: HMONITOR) -> windows::core::Result<MonitorInfo> {
    let mut info = MONITORINFO {
        cbSize: u32::try_from(size_of::<MONITORINFO>()).unwrap_or(0),
        ..Default::default()
    };
    // SAFETY: `monitor` came from EnumDisplayMonitors; `info` has `cbSize` set as required.
    unsafe { GetMonitorInfoW(monitor, &raw mut info) }.ok()?;
    Ok(MonitorInfo {
        rect: to_rect(info.rcMonitor),
        work_area: to_rect(info.rcWork),
        is_primary: info.dwFlags & PRIMARY_FLAG != 0,
    })
}

pub(crate) fn to_rect(r: RECT) -> Rect {
    Rect::from_edges(r.left, r.top, r.right, r.bottom)
}

/// The bounding rectangle of all monitors.
pub fn virtual_bounds(monitors: &[MonitorInfo]) -> Rect {
    monitors
        .iter()
        .map(|m| m.rect)
        .reduce(Rect::union)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    #[ignore = "needs a real desktop session"]
    fn enumerates_at_least_one_primary_monitor() {
        let monitors = enumerate().unwrap();
        assert!(monitors.iter().any(|m| m.is_primary));
        assert!(monitors.iter().all(|m| !m.rect.is_empty()));
    }

    #[test]
    fn virtual_bounds_covers_all_monitors() {
        let a = MonitorInfo {
            rect: Rect::new(-1920, 200, 1920, 1080),
            work_area: Rect::default(),
            is_primary: false,
        };
        let b = MonitorInfo {
            rect: Rect::new(0, 0, 2560, 1440),
            is_primary: true,
            ..a
        };
        assert_eq!(virtual_bounds(&[a, b]), Rect::new(-1920, 0, 4480, 1440));
    }
}
