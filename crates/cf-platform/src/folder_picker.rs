//! The standard Windows "Select folder" dialog.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Shell::{
    FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, IShellItem,
    SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
};
use windows::core::HSTRING;

/// Shows a modal folder picker owned by `owner` (a raw `HWND` value), starting in `start`.
/// Returns `None` when the user cancels.
///
/// Must run on a thread with COM initialised as single-threaded (any winit UI thread).
///
/// # Errors
/// When the dialog can't be created or shown.
pub fn pick_folder(
    owner: isize,
    start: &Path,
    title: &str,
) -> windows::core::Result<Option<PathBuf>> {
    // SAFETY: COM is initialised on this (UI) thread; plain object creation.
    let dialog: IFileOpenDialog =
        unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)? };
    // SAFETY: valid dialog object and strings that outlive the calls.
    unsafe {
        dialog.SetOptions(dialog.GetOptions()? | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)?;
        dialog.SetTitle(&HSTRING::from(title))?;
        if let Ok(folder) =
            SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(start.as_os_str()), None)
        {
            dialog.SetFolder(&folder)?;
        }
    }
    // SAFETY: modal; `owner` is a window of this thread (or 0 for none).
    if let Err(err) = unsafe { dialog.Show(Some(HWND(owner as *mut _))) } {
        return if err.code() == ERROR_CANCELLED.to_hresult() {
            Ok(None)
        } else {
            Err(err)
        };
    }
    // SAFETY: a result exists after a successful Show; the returned string is freed below.
    unsafe {
        let item = dialog.GetResult()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path = name.to_string().map(PathBuf::from).ok();
        CoTaskMemFree(Some(name.0.cast_const().cast()));
        Ok(path)
    }
}
