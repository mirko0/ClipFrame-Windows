//! The mouse cursor position.

use cf_core::geometry::Point;
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

/// The cursor position in physical virtual-desktop pixels, if available.
pub fn position() -> Option<Point> {
    let mut p = POINT::default();
    // SAFETY: `p` is a valid out-pointer for the duration of the call.
    unsafe { GetCursorPos(&raw mut p) }.ok()?;
    Some(Point::new(p.x, p.y))
}
