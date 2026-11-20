//! Local wall-clock time.

use cf_core::output::LocalDateTime;
use windows::Win32::System::SystemInformation::GetLocalTime;

/// The current local date and time.
pub fn local_now() -> LocalDateTime {
    // SAFETY: GetLocalTime has no preconditions and returns a filled SYSTEMTIME.
    let t = unsafe { GetLocalTime() };
    let byte = |v: u16| u8::try_from(v).unwrap_or(0);
    LocalDateTime {
        year: t.wYear,
        month: byte(t.wMonth),
        day: byte(t.wDay),
        hour: byte(t.wHour),
        minute: byte(t.wMinute),
        second: byte(t.wSecond),
    }
}
