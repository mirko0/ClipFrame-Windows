//! Putting captures on the clipboard as both PNG (keeps transparency in modern apps) and
//! `CF_DIBV5` (understood everywhere).

mod dib;

use std::thread;
use std::time::Duration;

use cf_core::image::Image;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{CF_DIBV5, CF_UNICODETEXT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
};
use windows::core::w;

pub use dib::dibv5_bytes;

/// Another app may hold the clipboard briefly; retry this many times, 15 ms apart.
const OPEN_ATTEMPTS: u32 = 10;

/// Why the clipboard could not be written.
#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
    /// Another application kept the clipboard open.
    #[error("the clipboard is in use by another application")]
    Busy,
    /// A Win32 call failed.
    #[error("clipboard call {0} failed")]
    Win32(&'static str),
}

/// Replaces the clipboard contents with `image` (and its already-encoded `png`).
///
/// Safe to call from a worker thread: it creates its own hidden owner window.
///
/// # Errors
/// [`ClipboardError`] when the clipboard stays busy or a Win32 call fails.
pub fn set_image(image: &Image, png: &[u8]) -> Result<(), ClipboardError> {
    let owner = OwnerWindow::create()?;
    let _open = OpenClipboardGuard::open(owner.0)?;
    // SAFETY: the clipboard is open and owned by `owner`.
    unsafe { EmptyClipboard() }.map_err(|_| ClipboardError::Win32("EmptyClipboard"))?;
    // SAFETY: the argument is a valid null-terminated wide string literal.
    let png_format = unsafe { RegisterClipboardFormatW(w!("PNG")) };
    if png_format != 0 {
        set_data(png_format, png)?;
    }
    set_data(u32::from(CF_DIBV5.0), &dibv5_bytes(image))
}

/// Replaces the clipboard contents with `text` (e.g. a picked colour).
///
/// # Errors
/// [`ClipboardError`] when the clipboard stays busy or a Win32 call fails.
pub fn set_text(text: &str) -> Result<(), ClipboardError> {
    let owner = OwnerWindow::create()?;
    let _open = OpenClipboardGuard::open(owner.0)?;
    // SAFETY: the clipboard is open and owned by `owner`.
    unsafe { EmptyClipboard() }.map_err(|_| ClipboardError::Win32("EmptyClipboard"))?;
    let utf16: Vec<u8> = text
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    set_data(u32::from(CF_UNICODETEXT.0), &utf16)
}

/// Copies `bytes` into a movable global block and hands it to the clipboard.
fn set_data(format: u32, bytes: &[u8]) -> Result<(), ClipboardError> {
    let global = global_from_bytes(bytes)?;
    // SAFETY: the clipboard is open (caller holds the guard); on success the system takes
    // ownership of `global`, on failure we still own it and free it below.
    let result = unsafe { SetClipboardData(format, Some(HANDLE(global.0))) };
    if result.is_err() {
        // SAFETY: ownership was not transferred, so we free the block exactly once.
        let _ = unsafe { GlobalFree(Some(global)) };
        return Err(ClipboardError::Win32("SetClipboardData"));
    }
    Ok(())
}

fn global_from_bytes(bytes: &[u8]) -> Result<HGLOBAL, ClipboardError> {
    // SAFETY: allocating a movable block of `bytes.len()` bytes has no preconditions.
    let global = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }
        .map_err(|_| ClipboardError::Win32("GlobalAlloc"))?;
    // SAFETY: `global` is a fresh movable block; GlobalLock returns its base address.
    let ptr = unsafe { GlobalLock(global) }.cast::<u8>();
    if ptr.is_null() {
        // SAFETY: we own `global` and free it once.
        let _ = unsafe { GlobalFree(Some(global)) };
        return Err(ClipboardError::Win32("GlobalLock"));
    }
    // SAFETY: `ptr` points to at least `bytes.len()` writable bytes that don't overlap
    // `bytes`; the block is unlocked right after the copy.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        let _ = GlobalUnlock(global);
    }
    Ok(global)
}

/// A hidden message-only window: `EmptyClipboard` needs a real owner, or later
/// `SetClipboardData` calls fail.
struct OwnerWindow(HWND);

impl OwnerWindow {
    fn create() -> Result<Self, ClipboardError> {
        // SAFETY: "STATIC" is a predefined window class; HWND_MESSAGE makes the window
        // message-only (never shown). It is destroyed in Drop.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                None,
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .map_err(|_| ClipboardError::Win32("CreateWindowExW"))?;
        Ok(Self(hwnd))
    }
}

impl Drop for OwnerWindow {
    fn drop(&mut self) {
        // SAFETY: the window was created by this thread in `create` and is destroyed once.
        // Clipboard data is not delayed-rendered, so it survives the owner's destruction.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

struct OpenClipboardGuard;

impl OpenClipboardGuard {
    fn open(owner: HWND) -> Result<Self, ClipboardError> {
        for attempt in 0..OPEN_ATTEMPTS {
            // SAFETY: `owner` is a valid window owned by this thread.
            if unsafe { OpenClipboard(Some(owner)) }.is_ok() {
                return Ok(Self);
            }
            if attempt + 1 < OPEN_ATTEMPTS {
                thread::sleep(Duration::from_millis(15));
            }
        }
        Err(ClipboardError::Busy)
    }
}

impl Drop for OpenClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: the clipboard was opened by this thread in `open`.
        let _ = unsafe { CloseClipboard() };
    }
}
