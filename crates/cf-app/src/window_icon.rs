//! Gives app windows the ClipFrame icon embedded in the executable (see `build.rs`); winit
//! windows otherwise show a blank icon in the title bar and taskbar.

use winit::dpi::PhysicalSize;
use winit::platform::windows::{IconExtWindows as _, WindowExtWindows as _};
use winit::window::{Icon, Window};

/// Resource ID `winresource` gives the application icon.
const ICON_RESOURCE: u16 = 1;

/// Sets the title-bar and taskbar icons at sizes that suit `window`'s UI scale.
pub fn apply(window: &Window) {
    let scale = window.scale_factor();
    window.set_window_icon(load(16.0 * scale));
    window.set_taskbar_icon(load(32.0 * scale));
}

fn load(side: f64) -> Option<Icon> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // small, positive
    let side = side.round().max(16.0) as u32;
    Icon::from_resource(ICON_RESOURCE, Some(PhysicalSize::new(side, side)))
        .inspect_err(|err| tracing::debug!("could not load the window icon: {err}"))
        .ok()
}
