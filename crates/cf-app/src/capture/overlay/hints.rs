//! The one-line help shown above the mode bar.

use cf_core::capture_mode::CaptureMode;

const ADJUST: &str =
    "Enter or double-click to capture · arrows move · Ctrl+arrows resize · Esc cancel";

/// Help for `mode`; `adjusting` when a selection waits for Enter, empty while dragging.
pub fn hint(mode: CaptureMode, adjusting: bool, dragging: bool) -> String {
    if dragging {
        return String::new();
    }
    if adjusting {
        return ADJUST.to_owned();
    }
    let action = match mode {
        CaptureMode::FreeSelect => "Drag to select or click a window",
        CaptureMode::Rectangle => "Drag to select",
        CaptureMode::Window => "Click a window",
        CaptureMode::Element => "Click an element · scroll for parent/child",
        CaptureMode::BrowserContent => "Click a browser to capture its page",
        CaptureMode::Monitor | CaptureMode::AllMonitors => "Click a monitor",
    };
    format!("{action} · Esc cancel")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_follows_state() {
        assert!(hint(CaptureMode::Element, false, false).starts_with("Click an element"));
        assert_eq!(hint(CaptureMode::Window, true, false), ADJUST);
        assert_eq!(hint(CaptureMode::FreeSelect, false, true), "");
    }
}
