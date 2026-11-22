//! Loading settings and deriving per-capture options, shared by the tray and the helper.

use std::path::Path;

use cf_core::config::{self, Config, LoadOutcome};
use cf_platform::paths::AppPaths;

use crate::capture::Delivery;
use crate::capture::cleanup::{self, Cleanup};
use crate::capture::history::HistoryStore;
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
    let editor = EditorOptions {
        update_clipboard: config.clipboard.update_on_edit,
        save_dir: config.output.captures_dir(&paths.default_captures_dir),
        fallback_dir: paths.fallback_captures_dir.clone(),
        spotlight_dim: config.editor.spotlight_opacity(),
        naming: config.output.naming(),
    };
    let history = (config.history.keep > 0).then(|| HistoryStore {
        dir: paths.history_dir.clone(),
        keep: usize::from(config.history.keep),
    });
    Delivery {
        save_dir,
        fallback_dir: paths.fallback_captures_dir.clone(),
        naming: config.output.naming(),
        copy_to_clipboard: config.clipboard.copy_on_capture,
        history,
        toast: config.notifications.toast,
        editor,
        open_editor: config.editor.open_after_capture,
    }
}

/// Starts recording saved files and deletes expired captures in the background.
pub fn clean_up(config: &Config, paths: &AppPaths) {
    let ledger = cleanup::ledger_path(&paths.state_file);
    cleanup::use_ledger(ledger.clone());
    cleanup::start(Cleanup {
        default_dir: paths.default_captures_dir.clone(),
        captures_dir: config.output.captures_dir(&paths.default_captures_dir),
        ledger,
        days: config.output.delete_after_days,
    });
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
