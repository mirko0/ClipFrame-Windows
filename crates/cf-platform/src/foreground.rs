//! The window the user was working in when a capture started, for `{app}` and `{title}` in
//! file names.

use std::path::Path;

use cf_core::naming::Source;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
};
use windows::core::PWSTR;

/// Program name and title of the foreground window; empty fields when unknown (no
/// foreground window, or a process this user may not query).
pub fn source() -> Source {
    // SAFETY: no preconditions; may return a null handle.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return Source::default();
    }
    Source {
        app: program_name(hwnd).unwrap_or_default(),
        title: title(hwnd),
    }
}

fn title(hwnd: HWND) -> String {
    // SAFETY: plain query on a window handle (0 when it has none or is gone).
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    let mut buffer = vec![0u16; usize::try_from(len).unwrap_or(0) + 1];
    // SAFETY: the buffer is writable for its whole length; Windows writes at most that.
    let written = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..usize::try_from(written).unwrap_or(0)])
}

fn program_name(hwnd: HWND) -> Option<String> {
    let mut pid = 0u32;
    // SAFETY: `pid` is a valid out pointer for the duration of the call.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut pid)) };
    // SAFETY: limited query rights only; the handle is closed by `ProcessHandle`.
    let process =
        ProcessHandle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?);
    let mut buffer = [0u16; 1024];
    let mut len = u32::try_from(buffer.len()).unwrap_or(0);
    // SAFETY: `buffer` holds `len` UTF-16 units and `len` is updated to the written count.
    unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &raw mut len,
        )
    }
    .ok()?;
    let path = String::from_utf16_lossy(&buffer[..usize::try_from(len).unwrap_or(0)]);
    Path::new(&path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
}

struct ProcessHandle(HANDLE);

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful OpenProcess and is closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_never_fails() {
        // Whatever is in front (possibly nothing on a CI runner), this must not panic.
        let _ = source();
    }
}
