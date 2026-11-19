//! User configuration, stored as JSON.
//!
//! Every section uses `#[serde(default)]`, so a config file written by an older version (or
//! edited by hand with fields missing) still loads, with missing values taken from defaults.

mod store;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use store::{ConfigError, LoadOutcome, load_or_create, save};

use crate::capture_mode::CaptureMode;
use crate::hotkey::{DEFAULT_CAPTURE_HOTKEY, Hotkey};

/// Schema version written by this build.
pub const CURRENT_VERSION: u32 = 1;

/// The complete user configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Schema version, for future migrations.
    pub version: u32,
    /// Global hotkeys.
    pub hotkeys: HotkeyConfig,
    /// Capture behaviour.
    pub capture: CaptureConfig,
    /// Where and how captures are saved.
    pub output: OutputConfig,
    /// Clipboard behaviour.
    pub clipboard: ClipboardConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            hotkeys: HotkeyConfig::default(),
            capture: CaptureConfig::default(),
            output: OutputConfig::default(),
            clipboard: ClipboardConfig::default(),
        }
    }
}

/// Global hotkeys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeyConfig {
    /// Opens the capture overlay.
    pub capture: Hotkey,
    /// Captures the previously selected region again; disabled when `None`.
    pub repeat_last: Option<Hotkey>,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            capture: DEFAULT_CAPTURE_HOTKEY,
            repeat_last: None,
        }
    }
}

/// Capture behaviour.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    /// Mode the overlay starts in when opened with the capture hotkey.
    pub default_mode: CaptureMode,
}

/// Where captures are saved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    /// Folder for saved captures; `None` uses the platform default (`Pictures\ClipFrame`).
    pub save_dir: Option<PathBuf>,
}

impl OutputConfig {
    /// The folder captures are saved to, falling back to `default_dir`.
    pub fn captures_dir(&self, default_dir: &Path) -> PathBuf {
        self.save_dir
            .clone()
            .unwrap_or_else(|| default_dir.to_path_buf())
    }
}

/// Clipboard behaviour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_field_names)]
pub struct ClipboardConfig {
    /// Copy the image to the clipboard as soon as it is captured.
    pub copy_on_capture: bool,
    /// Re-copy the image to the clipboard after each edit in the editor.
    pub update_on_edit: bool,
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        Self {
            copy_on_capture: true,
            update_on_edit: true,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn default_round_trips_through_json() {
        let json = serde_json::to_string_pretty(&Config::default()).unwrap();
        assert_eq!(
            serde_json::from_str::<Config>(&json).unwrap(),
            Config::default()
        );
    }

    #[test]
    fn missing_fields_take_defaults() {
        let config: Config =
            serde_json::from_str(r#"{ "clipboard": { "update_on_edit": false } }"#).unwrap();
        assert!(config.clipboard.copy_on_capture);
        assert!(!config.clipboard.update_on_edit);
        assert_eq!(config.hotkeys.capture, DEFAULT_CAPTURE_HOTKEY);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let config: Config = serde_json::from_str(r#"{ "from_the_future": 42 }"#).unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn hotkeys_and_modes_are_human_readable() {
        let json = serde_json::to_string(&Config::default()).unwrap();
        assert!(json.contains(r#""capture":"Alt+Shift+S""#));
        assert!(json.contains(r#""default_mode":"free-select""#));
    }

    #[test]
    fn captures_dir_prefers_configured_folder() {
        let default = Path::new(r"C:\Pictures\ClipFrame");
        let mut output = OutputConfig::default();
        assert_eq!(output.captures_dir(default), default);
        output.save_dir = Some(PathBuf::from(r"D:\Shots"));
        assert_eq!(output.captures_dir(default), Path::new(r"D:\Shots"));
    }
}
