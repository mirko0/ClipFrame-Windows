//! Where ClipFrame keeps its config, logs and captures.
//!
//! Normal install: config in `%APPDATA%\ClipFrame`, logs in `%LOCALAPPDATA%\ClipFrame\logs`,
//! captures in `Pictures\ClipFrame`.
//!
//! Portable mode (a file named `portable` next to the exe): everything under `<exe dir>\data`.

use std::io;
use std::path::{Path, PathBuf};

const APP_DIR: &str = "ClipFrame";
const CONFIG_FILE: &str = "config.json";
const STATE_FILE: &str = "state.json";
const PORTABLE_MARKER: &str = "portable";

/// Resolved application directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    /// The JSON config file.
    pub config_file: PathBuf,
    /// Folder for rolling log files.
    pub log_dir: PathBuf,
    /// Captures folder used when the config doesn't set one.
    pub default_captures_dir: PathBuf,
    /// Always-writable folder used when saving to the chosen folder fails (for example when
    /// Windows' Controlled Folder Access blocks the Pictures folder).
    pub fallback_captures_dir: PathBuf,
    /// Small runtime state shared by the tray and the capture helper.
    pub state_file: PathBuf,
    /// Whether portable mode is active.
    pub is_portable: bool,
}

/// Failure to determine the application directories.
#[derive(Debug, thiserror::Error)]
pub enum PathsError {
    /// The running executable's location is unknown.
    #[error("cannot locate the ClipFrame executable")]
    Executable(#[source] io::Error),
    /// Windows did not report a required known folder.
    #[error("Windows did not report the {0} folder")]
    MissingKnownFolder(&'static str),
}

/// Raw facts about the environment that [`AppPaths`] is derived from.
#[derive(Debug, Clone, Default)]
pub struct PathInputs {
    /// Folder containing the executable.
    pub exe_dir: PathBuf,
    /// `%APPDATA%`.
    pub roaming_dir: Option<PathBuf>,
    /// `%LOCALAPPDATA%`.
    pub local_dir: Option<PathBuf>,
    /// The user's Pictures folder.
    pub pictures_dir: Option<PathBuf>,
    /// Whether the portable marker file exists next to the exe.
    pub portable_marker_exists: bool,
}

impl PathInputs {
    /// Reads the inputs from the current process and user profile.
    ///
    /// # Errors
    /// [`PathsError::Executable`] when the executable path cannot be determined.
    pub fn from_environment() -> Result<Self, PathsError> {
        let exe = std::env::current_exe().map_err(PathsError::Executable)?;
        let exe_dir = exe.parent().map_or_else(PathBuf::new, Path::to_path_buf);
        Ok(Self {
            portable_marker_exists: exe_dir.join(PORTABLE_MARKER).is_file(),
            exe_dir,
            roaming_dir: dirs::config_dir(),
            local_dir: dirs::data_local_dir(),
            pictures_dir: dirs::picture_dir(),
        })
    }
}

impl AppPaths {
    /// Resolves the directories for the current process.
    ///
    /// # Errors
    /// See [`PathsError`].
    pub fn resolve() -> Result<Self, PathsError> {
        Self::from_inputs(&PathInputs::from_environment()?)
    }

    /// Derives the directories from `inputs`; pure, so it can be tested.
    ///
    /// # Errors
    /// [`PathsError::MissingKnownFolder`] when `%APPDATA%` or `%LOCALAPPDATA%` is unknown.
    pub fn from_inputs(inputs: &PathInputs) -> Result<Self, PathsError> {
        if inputs.portable_marker_exists {
            let base = inputs.exe_dir.join("data");
            return Ok(Self {
                config_file: base.join(CONFIG_FILE),
                log_dir: base.join("logs"),
                default_captures_dir: base.join("captures"),
                fallback_captures_dir: base.join("captures"),
                state_file: base.join(STATE_FILE),
                is_portable: true,
            });
        }
        let roaming = known(inputs.roaming_dir.as_deref(), "AppData")?.join(APP_DIR);
        let local = known(inputs.local_dir.as_deref(), "LocalAppData")?.join(APP_DIR);
        let default_captures_dir = inputs
            .pictures_dir
            .as_ref()
            .map_or_else(|| local.join("captures"), |pictures| pictures.join(APP_DIR));
        Ok(Self {
            config_file: roaming.join(CONFIG_FILE),
            log_dir: local.join("logs"),
            default_captures_dir,
            fallback_captures_dir: local.join("captures"),
            state_file: local.join(STATE_FILE),
            is_portable: false,
        })
    }
}

fn known<'a>(dir: Option<&'a Path>, name: &'static str) -> Result<&'a Path, PathsError> {
    dir.ok_or(PathsError::MissingKnownFolder(name))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn installed_inputs() -> PathInputs {
        PathInputs {
            exe_dir: PathBuf::from(r"C:\Program Files\ClipFrame"),
            roaming_dir: Some(PathBuf::from(r"C:\Users\u\AppData\Roaming")),
            local_dir: Some(PathBuf::from(r"C:\Users\u\AppData\Local")),
            pictures_dir: Some(PathBuf::from(r"C:\Users\u\Pictures")),
            portable_marker_exists: false,
        }
    }

    #[test]
    fn installed_mode_uses_user_folders() {
        let paths = AppPaths::from_inputs(&installed_inputs()).unwrap();

        assert_eq!(
            paths.config_file,
            Path::new(r"C:\Users\u\AppData\Roaming\ClipFrame\config.json")
        );
        assert_eq!(
            paths.log_dir,
            Path::new(r"C:\Users\u\AppData\Local\ClipFrame\logs")
        );
        assert_eq!(
            paths.default_captures_dir,
            Path::new(r"C:\Users\u\Pictures\ClipFrame")
        );
        assert!(!paths.is_portable);
    }

    #[test]
    fn portable_mode_keeps_everything_next_to_exe() {
        let inputs = PathInputs {
            portable_marker_exists: true,
            ..installed_inputs()
        };

        let paths = AppPaths::from_inputs(&inputs).unwrap();

        assert!(paths.is_portable);
        assert!(
            paths
                .config_file
                .starts_with(r"C:\Program Files\ClipFrame\data")
        );
        assert!(
            paths
                .log_dir
                .starts_with(r"C:\Program Files\ClipFrame\data")
        );
    }

    #[test]
    fn missing_pictures_folder_falls_back_to_local_app_data() {
        let inputs = PathInputs {
            pictures_dir: None,
            ..installed_inputs()
        };

        let paths = AppPaths::from_inputs(&inputs).unwrap();

        assert_eq!(
            paths.default_captures_dir,
            Path::new(r"C:\Users\u\AppData\Local\ClipFrame\captures")
        );
    }

    #[test]
    fn missing_app_data_is_an_error() {
        let inputs = PathInputs {
            roaming_dir: None,
            ..installed_inputs()
        };

        assert!(matches!(
            AppPaths::from_inputs(&inputs),
            Err(PathsError::MissingKnownFolder("AppData"))
        ));
    }
}
