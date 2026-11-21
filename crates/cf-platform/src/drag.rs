//! Dragging a file out of ClipFrame into other apps (Explorer, chat apps, browsers, mail).
//!
//! The file's shell folder builds the data object (as when dragging from Explorer), so every target
//! that accepts dropped files works; only the small drop-source callback is implemented here.
//! Must be called on a UI thread with OLE initialised (winit does that for its windows).

use std::path::Path;

use windows::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, HWND,
};
use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::Ole::{
    DROPEFFECT, DROPEFFECT_COPY, DoDragDrop, IDropSource, IDropSource_Impl,
};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{ILFree, IShellFolder, SHBindToParent, SHParseDisplayName};
use windows::core::{BOOL, HRESULT, HSTRING, implement};

/// How a drag ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragResult {
    /// Dropped onto a target that accepted the file.
    Dropped,
    /// Cancelled (Esc, or released over nothing that accepts files).
    Cancelled,
}

/// Starts a modal drag of `file` (copy only) and returns once it has been dropped or
/// cancelled.
///
/// # Errors
/// When the file can't be resolved by the shell or OLE refuses the drag.
pub fn drag_file(file: &Path) -> windows::core::Result<DragResult> {
    let pidl = Pidl::parse(file)?;
    let mut child: *mut ITEMIDLIST = std::ptr::null_mut();
    // SAFETY: `pidl` is a valid absolute PIDL; `child` receives a pointer *into* it (not a
    // separate allocation), valid while `pidl` lives.
    let folder: IShellFolder = unsafe { SHBindToParent(pidl.0, Some(&raw mut child))? };
    // The folder's own data object for the file is what Explorer drags: it carries the file
    // list (CF_HDROP) that most drop targets need, unlike `SHCreateDataObject`.
    // SAFETY: `child` is a valid relative PIDL inside `pidl`, which outlives the call.
    let data: IDataObject =
        unsafe { folder.GetUIObjectOf(HWND::default(), &[child.cast_const()], None)? };
    let source: IDropSource = DropSource.into();
    let mut effect = DROPEFFECT::default();
    // SAFETY: both COM objects are valid; `effect` is a valid out pointer. The call is
    // modal and returns when the drag ends.
    let result = unsafe { DoDragDrop(&data, &source, DROPEFFECT_COPY, &raw mut effect) };
    match result {
        r if r == DRAGDROP_S_DROP => Ok(DragResult::Dropped),
        r if r == DRAGDROP_S_CANCEL => Ok(DragResult::Cancelled),
        r => Err(r.into()),
    }
}

/// An absolute shell item id list, freed on drop.
struct Pidl(*mut ITEMIDLIST);

impl Pidl {
    fn parse(path: &Path) -> windows::core::Result<Self> {
        let name = HSTRING::from(path.as_os_str());
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        // SAFETY: `name` is a valid null-terminated path; `pidl` receives an allocation that
        // `Drop` frees.
        unsafe { SHParseDisplayName(&name, None, &raw mut pidl, 0, None)? };
        Ok(Self(pidl))
    }
}

impl Drop for Pidl {
    fn drop(&mut self) {
        // SAFETY: allocated by SHParseDisplayName and freed exactly once.
        unsafe { ILFree(Some(self.0.cast_const())) };
    }
}

mod source {
    // The `implement` macro generates code these pedantic lints dislike.
    #![allow(clippy::inline_always, clippy::ref_as_ptr)]

    use super::{
        BOOL, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DROPEFFECT,
        HRESULT, IDropSource, IDropSource_Impl, MK_LBUTTON, MODIFIERKEYS_FLAGS, implement,
    };

    /// Drops when the left button is released, cancels on Esc, default cursors.
    #[implement(IDropSource)]
    #[derive(Debug)]
    pub struct DropSource;

    impl IDropSource_Impl for DropSource_Impl {
        fn QueryContinueDrag(&self, escape_pressed: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
            if escape_pressed.as_bool() {
                DRAGDROP_S_CANCEL
            } else if keys.0 & MK_LBUTTON.0 == 0 {
                DRAGDROP_S_DROP
            } else {
                HRESULT(0)
            }
        }

        fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
            DRAGDROP_S_USEDEFAULTCURSORS
        }
    }
}

use source::DropSource;
