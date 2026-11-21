//! Everything the user can trigger, from a hotkey, the tray menu or (later) the UI.

/// A user-triggered command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Open the capture overlay.
    Capture,
    /// Capture the previously selected region again.
    RepeatLast,
    /// Open the captures folder in Explorer.
    OpenCapturesFolder,
    /// Open the settings window.
    OpenSettings,
    /// Open the settings file in a text editor.
    OpenConfig,
    /// Re-read the settings file and re-register hotkeys.
    ReloadConfig,
    /// Open the logs folder in Explorer.
    OpenLogs,
    /// Open the n-th most recent capture (0 = newest) in the editor.
    OpenRecent(u8),
    /// Install a downloaded update and restart.
    InstallUpdate,
    /// Exit ClipFrame.
    Quit,
}
