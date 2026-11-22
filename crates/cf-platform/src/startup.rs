//! "Start with Windows": a value under the per-user `Run` key (no admin rights needed).
//! The command passes `--background`, so signing in doesn't open ClipFrame's window.

use std::path::Path;

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::w;

const RUN_KEY: windows::core::PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const VALUE: windows::core::PCWSTR = w!("ClipFrame");

/// Whether ClipFrame starts when the user signs in.
pub fn is_enabled() -> bool {
    // SAFETY: constant strings; only checks that the value exists (no buffer).
    let found = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE,
            RRF_RT_REG_SZ,
            None,
            None,
            None,
        )
    };
    found == ERROR_SUCCESS
}

/// Starts `exe` at sign-in, or stops doing so.
///
/// # Errors
/// The Windows error code when the registry can't be changed.
pub fn set_enabled(enabled: bool, exe: &Path) -> Result<(), u32> {
    let result = if enabled {
        let command: Vec<u16> = format!("\"{}\" --background", exe.display())
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let bytes = u32::try_from(command.len() * 2).unwrap_or(0);
        // SAFETY: `command` is a null-terminated UTF-16 string of `bytes` bytes that outlives
        // the call.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                VALUE,
                REG_SZ.0,
                Some(command.as_ptr().cast()),
                bytes,
            )
        }
    } else {
        // SAFETY: constant strings.
        let deleted = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE) };
        if deleted == ERROR_FILE_NOT_FOUND {
            ERROR_SUCCESS
        } else {
            deleted
        }
    };
    if result == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(result.0)
    }
}

/// Keeps an existing entry pointing at `exe` (an updated install may have moved it) and
/// up to date with the current command line.
pub fn refresh(exe: &Path) {
    if is_enabled() {
        let _ = set_enabled(true, exe);
    }
}
