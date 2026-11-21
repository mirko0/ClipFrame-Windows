//! UI Automation queries: the nested UI elements under a point, and a window's document
//! (in browsers: the page content, without tabs and toolbars).
//!
//! The overlay covers the whole screen, so `ElementFromPoint` would only ever find the
//! overlay. Instead the tree of the window under the pointer (known from the window
//! snapshot) is descended, keeping the smallest child that contains the point.
//!
//! [`UiAutomation`] is not `Send`: create it on the thread that uses it. That thread must
//! not own windows (UI Automation clients must not run on a UI thread).

use std::ffi::c_void;
use std::marker::PhantomData;
use std::mem::ManuallyDrop;

use cf_core::geometry::{Point, Rect};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_I4};
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationCacheRequest,
    IUIAutomationCondition, IUIAutomationElement, IUIAutomationTreeWalker, TreeScope_Descendants,
    UIA_BoundingRectanglePropertyId, UIA_ControlTypePropertyId, UIA_DocumentControlTypeId,
};
use windows::core::Interface as _;

/// Deepest nesting followed (guards against cyclic or absurd trees).
const MAX_DEPTH: usize = 64;
/// Most children inspected per element (huge lists would make a hit-test slow).
const MAX_CHILDREN: usize = 2000;
/// Upper bound for one cross-process call, so a hung application can't stall the picker.
const TIMEOUT_MS: u32 = 1000;

/// A UI Automation client bound to the current thread.
pub struct UiAutomation {
    uia: IUIAutomation,
    walker: IUIAutomationTreeWalker,
    cache: IUIAutomationCacheRequest,
    document: IUIAutomationCondition,
    // Declared last: COM objects above are released before COM is uninitialised.
    _com: ComApartment,
}

impl std::fmt::Debug for UiAutomation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiAutomation").finish_non_exhaustive()
    }
}

impl UiAutomation {
    /// Initialises COM (multithreaded) on this thread and creates the client.
    ///
    /// # Errors
    /// When COM or UI Automation is unavailable.
    pub fn new() -> windows::core::Result<Self> {
        let com = ComApartment::enter()?;
        // SAFETY: COM is initialised on this thread; plain object creation and setters.
        unsafe {
            let uia: IUIAutomation = CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?;
            if let Ok(uia2) = uia.cast::<IUIAutomation2>() {
                uia2.SetConnectionTimeout(TIMEOUT_MS)?;
                uia2.SetTransactionTimeout(TIMEOUT_MS)?;
            }
            let walker = uia.ControlViewWalker()?;
            let cache = uia.CreateCacheRequest()?;
            cache.AddProperty(UIA_BoundingRectanglePropertyId)?;
            let document = uia.CreatePropertyCondition(
                UIA_ControlTypePropertyId,
                &variant_i32(UIA_DocumentControlTypeId.0),
            )?;
            Ok(Self {
                uia,
                walker,
                cache,
                document,
                _com: com,
            })
        }
    }

    /// Bounds of the elements of `window` that contain `p`, innermost first, not including
    /// the window itself. Empty when the window exposes no children there.
    ///
    /// # Errors
    /// When the window is gone or its process doesn't answer in time.
    pub fn elements_at(&self, window: usize, p: Point) -> windows::core::Result<Vec<Rect>> {
        // SAFETY: COM is initialised on this thread; the handle may be stale, which UI
        // Automation reports as an error rather than undefined behaviour.
        let mut parent = unsafe { self.uia.ElementFromHandle(hwnd(window)) }?;
        let mut chain = Vec::new();
        for _ in 0..MAX_DEPTH {
            let Some((child, rect)) = self.child_at(&parent, p) else {
                break;
            };
            chain.push(rect);
            parent = child;
        }
        chain.reverse();
        Ok(chain)
    }

    /// Bounds of the first document element in `window` (a browser's page area).
    ///
    /// # Errors
    /// When the window is gone or its process doesn't answer in time.
    pub fn document_rect(&self, window: usize) -> windows::core::Result<Option<Rect>> {
        // SAFETY: as in `elements_at`.
        let root = unsafe { self.uia.ElementFromHandle(hwnd(window)) }?;
        // SAFETY: COM is initialised; `document` and `cache` belong to this client. A
        // missing document comes back as an error (null element), meaning "none".
        let found =
            unsafe { root.FindFirstBuildCache(TreeScope_Descendants, &self.document, &self.cache) };
        Ok(found.ok().as_ref().and_then(cached_rect))
    }

    /// The smallest child of `parent` containing `p` (later siblings win ties: they are
    /// usually drawn on top).
    fn child_at(
        &self,
        parent: &IUIAutomationElement,
        p: Point,
    ) -> Option<(IUIAutomationElement, Rect)> {
        let mut best: Option<(IUIAutomationElement, Rect)> = None;
        // SAFETY: COM is initialised; walker and cache belong to this client.
        let mut next = unsafe {
            self.walker
                .GetFirstChildElementBuildCache(parent, &self.cache)
        };
        for _ in 0..MAX_CHILDREN {
            let Ok(element) = next else { break };
            let rect = cached_rect(&element).filter(|r| r.contains(p));
            let smaller = |r: &Rect| best.as_ref().is_none_or(|(_, b)| r.area() <= b.area());
            if let Some(rect) = rect.filter(smaller) {
                best = Some((element.clone(), rect));
            }
            // SAFETY: as above.
            next = unsafe {
                self.walker
                    .GetNextSiblingElementBuildCache(&element, &self.cache)
            };
        }
        best
    }
}

fn cached_rect(element: &IUIAutomationElement) -> Option<Rect> {
    // SAFETY: the bounding rectangle is in the cache request used to fetch `element`.
    let r = unsafe { element.CachedBoundingRectangle() }.ok()?;
    let rect = Rect::from_edges(r.left, r.top, r.right, r.bottom);
    (!rect.is_empty()).then_some(rect)
}

fn hwnd(window: usize) -> HWND {
    HWND(window as *mut c_void)
}

fn variant_i32(value: i32) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_I4,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { lVal: value },
            }),
        },
    }
}

/// COM initialised on the current thread for as long as this lives (not `Send`: it must be
/// dropped on the thread that created it).
pub(crate) struct ComApartment(PhantomData<*const ()>);

impl ComApartment {
    pub(crate) fn enter() -> windows::core::Result<Self> {
        // SAFETY: initialises COM for this thread; balanced by `CoUninitialize` in `drop`.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
        Ok(Self(PhantomData))
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY: balances the successful `CoInitializeEx` in `enter` on the same thread
        // (the type is not `Send`).
        unsafe { CoUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn client_can_be_created_and_tolerates_stale_windows() {
        let uia = UiAutomation::new().expect("UI Automation is part of Windows");
        // A handle that is no window: an error or nothing, never a crash.
        let stale = 0x0BAD_F00D;
        assert!(
            uia.elements_at(stale, Point::new(0, 0))
                .map_or(true, |v| v.is_empty())
        );
        assert!(uia.document_rect(stale).map_or(true, |d| d.is_none()));
    }
}
