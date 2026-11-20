//! Grabbing screen pixels with GDI.
//!
//! GDI `BitBlt` is fast enough for still captures (single-digit ms per 4K monitor) and has no
//! start-up cost. HDR-correct capture via DXGI / Windows.Graphics.Capture will replace it
//! behind the same function signature (see PLAN §9).

use cf_core::geometry::Rect;
use cf_core::image::Image;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, GetDC, HBITMAP, HDC, HGDIOBJ, ReleaseDC,
    SRCCOPY, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CURSOR_SHOWING, CURSORINFO, DI_NORMAL, DrawIconEx, GetCursorInfo, GetIconInfo, ICONINFO,
};

/// Why a capture failed.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    /// The rectangle is empty.
    #[error("nothing to capture: empty rectangle")]
    Empty,
    /// A GDI call failed.
    #[error("GDI call {0} failed")]
    Gdi(&'static str),
}

/// Captures `rect` (virtual-desktop pixels) into an opaque BGRA image.
///
/// # Errors
/// [`CaptureError`] when the rectangle is empty or a GDI call fails.
pub fn capture_rect(rect: Rect, include_cursor: bool) -> Result<Image, CaptureError> {
    if rect.is_empty() {
        return Err(CaptureError::Empty);
    }
    let screen = ScreenDc::get()?;
    let mem = MemoryDc::compatible_with(screen.0)?;
    let (bitmap, bits) = DibSection::new(screen.0, rect.width, rect.height)?;
    let _selected = Selected::new(mem.0, bitmap.0.into());
    // SAFETY: both DCs are valid for this scope; the source rectangle lies on the desktop.
    unsafe {
        BitBlt(
            mem.0,
            0,
            0,
            rect.width,
            rect.height,
            Some(screen.0),
            rect.x,
            rect.y,
            SRCCOPY | CAPTUREBLT,
        )
    }
    .map_err(|_| CaptureError::Gdi("BitBlt"))?;
    if include_cursor {
        draw_cursor(mem.0, rect);
    }
    // SAFETY: GdiFlush has no preconditions; it ensures the DIB bits are written.
    let _ = unsafe { GdiFlush() };
    Ok(read_opaque_pixels(bits, rect))
}

/// Copies the DIB bits out and forces alpha to 255 (GDI leaves it undefined).
fn read_opaque_pixels(bits: *const u8, rect: Rect) -> Image {
    let width = u32::try_from(rect.width).unwrap_or(0);
    let height = u32::try_from(rect.height).unwrap_or(0);
    let len = width as usize * height as usize * 4;
    // SAFETY: `bits` points to the DIB section's `len` bytes (32-bpp, width × height), which
    // stays alive until the `DibSection` guard drops after this function returns.
    let mut pixels = unsafe { std::slice::from_raw_parts(bits, len) }.to_vec();
    for px in pixels.chunks_exact_mut(4) {
        px[3] = 255;
    }
    Image::from_bgra(width, height, pixels).unwrap_or_else(|| Image::transparent(width, height))
}

fn draw_cursor(dc: HDC, rect: Rect) {
    let mut info = CURSORINFO {
        cbSize: u32::try_from(size_of::<CURSORINFO>()).unwrap_or(0),
        ..Default::default()
    };
    // SAFETY: `info.cbSize` is set as GetCursorInfo requires.
    if unsafe { GetCursorInfo(&raw mut info) }.is_err() || info.flags != CURSOR_SHOWING {
        return;
    }
    let hotspot = cursor_hotspot(info);
    let x = info.ptScreenPos.x - rect.x - hotspot.x;
    let y = info.ptScreenPos.y - rect.y - hotspot.y;
    // SAFETY: `dc` is a valid memory DC; the cursor handle came from GetCursorInfo.
    let _ = unsafe { DrawIconEx(dc, x, y, info.hCursor.into(), 0, 0, 0, None, DI_NORMAL) };
}

fn cursor_hotspot(info: CURSORINFO) -> POINT {
    let mut icon = ICONINFO::default();
    // SAFETY: the cursor handle is valid; GetIconInfo fills `icon` and hands us ownership of
    // its bitmaps, which are deleted right after.
    if unsafe { GetIconInfo(info.hCursor.into(), &raw mut icon) }.is_err() {
        return POINT::default();
    }
    for bitmap in [icon.hbmMask, icon.hbmColor] {
        if !bitmap.is_invalid() {
            // SAFETY: we own these bitmaps (see above) and delete each once.
            let _ = unsafe { DeleteObject(bitmap.into()) };
        }
    }
    let to_i32 = |v: u32| i32::try_from(v).unwrap_or(0);
    POINT {
        x: to_i32(icon.xHotspot),
        y: to_i32(icon.yHotspot),
    }
}

// ---- RAII guards: every GDI resource is released exactly once, even on early return ----

struct ScreenDc(HDC);

impl ScreenDc {
    fn get() -> Result<Self, CaptureError> {
        // SAFETY: GetDC(None) returns the screen DC or null; released in Drop.
        let dc = unsafe { GetDC(None) };
        if dc.is_invalid() {
            Err(CaptureError::Gdi("GetDC"))
        } else {
            Ok(Self(dc))
        }
    }
}

impl Drop for ScreenDc {
    fn drop(&mut self) {
        // SAFETY: the DC was obtained with GetDC(None) and is released once.
        unsafe { ReleaseDC(None, self.0) };
    }
}

struct MemoryDc(HDC);

impl MemoryDc {
    fn compatible_with(dc: HDC) -> Result<Self, CaptureError> {
        // SAFETY: `dc` is a valid DC; the new DC is deleted in Drop.
        let mem = unsafe { CreateCompatibleDC(Some(dc)) };
        if mem.is_invalid() {
            Err(CaptureError::Gdi("CreateCompatibleDC"))
        } else {
            Ok(Self(mem))
        }
    }
}

impl Drop for MemoryDc {
    fn drop(&mut self) {
        // SAFETY: created by CreateCompatibleDC and deleted once.
        let _ = unsafe { DeleteDC(self.0) };
    }
}

struct DibSection(HBITMAP);

impl DibSection {
    /// A top-down 32-bpp DIB and a pointer to its pixel bits.
    fn new(dc: HDC, width: i32, height: i32) -> Result<(Self, *const u8), CaptureError> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: u32::try_from(size_of::<BITMAPINFOHEADER>()).unwrap_or(0),
                biWidth: width,
                biHeight: -height, // negative = top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        // SAFETY: `info` describes a valid 32-bpp DIB; `bits` receives the pixel pointer,
        // owned by the returned bitmap, which is deleted in Drop.
        let bitmap = unsafe {
            CreateDIBSection(
                Some(dc),
                &raw const info,
                DIB_RGB_COLORS,
                &raw mut bits,
                None,
                0,
            )
        }
        .map_err(|_| CaptureError::Gdi("CreateDIBSection"))?;
        Ok((Self(bitmap), bits.cast_const().cast()))
    }
}

impl Drop for DibSection {
    fn drop(&mut self) {
        // SAFETY: created by CreateDIBSection; deselected (by `Selected`) before this runs.
        let _ = unsafe { DeleteObject(self.0.into()) };
    }
}

/// Selects an object into a DC and restores the previous one on drop.
struct Selected {
    dc: HDC,
    previous: HGDIOBJ,
}

impl Selected {
    fn new(dc: HDC, object: HGDIOBJ) -> Self {
        // SAFETY: `dc` and `object` are valid; the previous object is restored in Drop.
        let previous = unsafe { SelectObject(dc, object) };
        Self { dc, previous }
    }
}

impl Drop for Selected {
    fn drop(&mut self) {
        // SAFETY: restores the object that was selected before `new`.
        unsafe { SelectObject(self.dc, self.previous) };
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn empty_rect_is_rejected() {
        assert!(matches!(
            capture_rect(Rect::new(0, 0, 0, 10), false),
            Err(CaptureError::Empty)
        ));
    }

    #[test]
    #[ignore = "needs a real desktop session"]
    fn captures_opaque_pixels_of_requested_size() {
        let image = capture_rect(Rect::new(0, 0, 64, 32), true).unwrap();
        assert_eq!((image.width(), image.height()), (64, 32));
        assert!(image.bgra().chunks_exact(4).all(|px| px[3] == 255));
    }
}
