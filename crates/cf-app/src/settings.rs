//! Loading settings and deriving per-capture options, shared by the tray and the helper.

use std::path::Path;

use cf_core::config::{self, Config, LoadOutcome};
use cf_platform::paths::AppPaths;

use crate::capture::Delivery;
use crate::editor::EditorOptions;

/// Loads the settings file, falling back to defaults (and logging why) on failure.
pub fn load_config(path: &Path) -> Config {
    match config::load_or_create(path) {
        Ok((config, outcome)) => {
            log_load_outcome(path, &outcome);
            config
        }
        Err(err) => {
            tracing::error!("{:#}; using default settings", anyhow::Error::from(err));
            Config::default()
        }
    }
}

/// Where captures go, from the current settings.
pub fn delivery(config: &Config, paths: &AppPaths) -> Delivery {
    let save_dir = config
        .output
        .auto_save
        .then(|| config.output.captures_dir(&paths.default_captures_dir));
    let editor = config.editor.open_after_capture.then(|| EditorOptions {
        update_clipboard: config.clipboard.update_on_edit,
        save_dir: config.output.captures_dir(&paths.default_captures_dir),
        fallback_dir: paths.fallback_captures_dir.clone(),
    });
    Delivery {
        save_dir,
        fallback_dir: paths.fallback_captures_dir.clone(),
        copy_to_clipboard: config.clipboard.copy_on_capture,
        editor,
    }
}

fn log_load_outcome(path: &Path, outcome: &LoadOutcome) {
    match outcome {
        LoadOutcome::Loaded => tracing::debug!(path = %path.display(), "settings loaded"),
        LoadOutcome::Upgraded => {
            tracing::info!(path = %path.display(), "settings file upgraded with new options");
        }
        LoadOutcome::CreatedDefault => {
            tracing::info!(path = %path.display(), "default settings created");
        }
        LoadOutcome::RecoveredFromInvalid { backup, reason } => tracing::warn!(
            backup = %backup.display(),
            "settings file was invalid ({reason}); reset to defaults, old file kept"
        ),
    }
}
