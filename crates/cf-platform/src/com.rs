//! COM apartments.
//!
//! Windows Runtime calls (OCR, toasts) must not init and uninit COM around each call:
//! `windows-rs` caches activation factories process-wide, and a factory created in an
//! apartment that has since been torn down crashes on its next use. Instead, [`keep_mta`]
//! keeps one multithreaded apartment alive for the whole process, which every thread that
//! hasn't initialised COM itself joins implicitly.

use std::sync::OnceLock;

use windows::Win32::System::Com::{CO_MTA_USAGE_COOKIE, CoIncrementMTAUsage};

/// Makes sure the process-wide MTA exists (idempotent, cheap after the first call).
///
/// # Errors
/// When COM cannot create the apartment.
pub fn keep_mta() -> windows::core::Result<()> {
    static MTA: OnceLock<Result<(), windows::core::HRESULT>> = OnceLock::new();
    MTA.get_or_init(|| {
        // SAFETY: no preconditions. The cookie is deliberately never passed to
        // CoDecrementMTAUsage: the apartment lives until the process exits.
        let cookie: windows::core::Result<CO_MTA_USAGE_COOKIE> = unsafe { CoIncrementMTAUsage() };
        cookie.map(drop).map_err(|e| e.code())
    })
    .map_err(windows::core::Error::from)
}
