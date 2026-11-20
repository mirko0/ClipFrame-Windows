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
    /// Resource-usage trade-offs.
    pub performance: PerformanceConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            hotkeys: HotkeyConfig::default(),
            capture: CaptureConfig::default(),
            output: OutputConfig::default(),
            clipboard: ClipboardConfig::default(),
            performance: PerformanceConfig::default(),
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

/// Longest allowed capture delay.
pub const MAX_DELAY_SECS: u8 = 30;

/// Capture behaviour.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    /// Mode the overlay starts in when opened with the capture hotkey.
    pub default_mode: CaptureMode,
    /// Draw the mouse cursor into the capture.
    pub include_cursor: bool,
    /// Keep the selection adjustable after the mouse is released; capture with Enter or a
    /// double-click. When off, releasing the mouse captures immediately.
    pub confirm_with_enter: bool,
    /// Seconds to wait between the hotkey and freezing the screen (0–30).
    pub delay_secs: u8,
}

impl CaptureConfig {
    /// The delay, clamped to [`MAX_DELAY_SECS`].
    pub fn delay(&self) -> std::time::Duration {
        std::time::Duration::from_secs(u64::from(self.delay_secs.min(MAX_DELAY_SECS)))
    }
}

/// Where captures are saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    /// Save every capture to disk automatically.
    pub auto_save: bool,
    /// Folder for saved captures; `None` uses the platform default (`Pictures\ClipFrame`).
    pub save_dir: Option<PathBuf>,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            auto_save: true,
            save_dir: None,
        }
    }
}

/// How long the GPU device stays initialised after the last capture window closes.
///
/// Keeping it costs some memory but makes the next capture appear faster.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeepGpuReady {
    /// Release it immediately (lowest memory).
    Never,
    /// Release it after five idle minutes.
    #[default]
    FiveMinutes,
    /// Never release it (fastest capture).
    Always,
}

impl KeepGpuReady {
    /// Idle time before release; `None` means never release.
    pub const fn idle_timeout(self) -> Option<std::time::Duration> {
        match self {
            Self::Never => Some(std::time::Duration::ZERO),
            Self::FiveMinutes => Some(std::time::Duration::from_secs(300)),
            Self::Always => None,
        }
    }
}

/// Resource-usage trade-offs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PerformanceConfig {
    /// See [`KeepGpuReady`].
    pub keep_gpu_ready: KeepGpuReady,
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
