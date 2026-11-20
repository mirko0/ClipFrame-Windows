//! Small runtime state that must outlive the capture helper process (which exits when idle),
//! such as the last captured region for "repeat last region".
//!
//! Unlike the config, this is not user-facing: a missing or broken file simply means
//! "no state", and failures to write it are only logged by the caller.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::geometry::Rect;

/// Persisted runtime state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    /// The region of the most recent capture.
    pub last_region: Option<Rect>,
}

impl AppState {
    /// Reads the state, or the default when the file is missing or unreadable.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Writes the state, creating the folder if needed.
    ///
    /// # Errors
    /// When the folder or file cannot be written.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn round_trips_last_region() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("state.json");
        let state = AppState {
            last_region: Some(Rect::new(-1920, 10, 300, 200)),
        };

        state.save(&path).unwrap();

        assert_eq!(AppState::load(&path), state);
    }

    #[test]
    fn missing_or_broken_file_gives_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        assert_eq!(AppState::load(&path), AppState::default());
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(AppState::load(&path), AppState::default());
    }
}
