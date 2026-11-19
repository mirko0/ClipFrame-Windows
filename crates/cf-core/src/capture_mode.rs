//! The ways a capture region can be chosen.

use serde::{Deserialize, Serialize};

/// How the capture overlay selects the region to capture.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureMode {
    /// Drag any rectangle; clicking a window or element snaps to it.
    #[default]
    FreeSelect,
    /// Drag a rectangle only, without snapping.
    Rectangle,
    /// Pick a whole top-level window.
    Window,
    /// Pick a UI element (e.g. a DOM element in a browser) via UI Automation.
    Element,
    /// The content area of a browser window, without tabs or toolbars.
    BrowserContent,
    /// The monitor under the cursor.
    Monitor,
    /// Every monitor, stitched together.
    AllMonitors,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn serializes_as_kebab_case() {
        let json = serde_json::to_string(&CaptureMode::BrowserContent).unwrap();
        assert_eq!(json, "\"browser-content\"");
    }

    #[test]
    fn default_is_free_select() {
        assert_eq!(CaptureMode::default(), CaptureMode::FreeSelect);
    }
}
