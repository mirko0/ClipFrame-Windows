//! Loading and saving the config file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::Config;

/// What happened while loading the config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadOutcome {
    /// The existing file was read.
    Loaded,
    /// No file existed; defaults were written.
    CreatedDefault,
    /// The file was unreadable as config; it was moved to `backup` and defaults were written.
    RecoveredFromInvalid {
        /// Where the broken file was moved.
        backup: PathBuf,
        /// The parse error.
        reason: String,
    },
}

/// Failure to read or write the config file.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A filesystem operation failed.
    #[error("could not access {}", path.display())]
    Io {
        /// File or folder involved.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: io::Error,
    },
    /// The config could not be serialized.
    #[error("could not serialize config")]
    Serialize(#[from] serde_json::Error),
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> ConfigError + '_ {
    move |source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Loads the config at `path`, creating it with defaults when missing.
///
/// A file that cannot be parsed is renamed to `<name>.invalid` (so the user's edits are not
/// lost) and replaced with defaults.
///
/// # Errors
/// Returns [`ConfigError`] when the file cannot be read, backed up or written.
pub fn load_or_create(path: &Path) -> Result<(Config, LoadOutcome), ConfigError> {
    match fs::read_to_string(path) {
        Ok(text) => parse_or_recover(path, &text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let config = Config::default();
            save(path, &config)?;
            Ok((config, LoadOutcome::CreatedDefault))
        }
        Err(err) => Err(io_error(path)(err)),
    }
}

fn parse_or_recover(path: &Path, text: &str) -> Result<(Config, LoadOutcome), ConfigError> {
    let err = match serde_json::from_str::<Config>(text) {
        Ok(config) => return Ok((config, LoadOutcome::Loaded)),
        Err(err) => err,
    };
    let backup = path.with_extension("json.invalid");
    fs::rename(path, &backup).map_err(io_error(path))?;
    let config = Config::default();
    save(path, &config)?;
    Ok((
        config,
        LoadOutcome::RecoveredFromInvalid {
            backup,
            reason: err.to_string(),
        },
    ))
}

/// Writes `config` to `path` atomically (write to a temp file, then rename over).
///
/// # Errors
/// Returns [`ConfigError`] when the folder or file cannot be written.
pub fn save(path: &Path, config: &Config) -> Result<(), ConfigError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(io_error(dir))?;
    }
    let json = serde_json::to_string_pretty(config)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(io_error(&tmp))?;
    fs::rename(&tmp, path).map_err(io_error(path))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn config_path(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("nested").join("config.json")
    }

    #[test]
    fn creates_default_file_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_path(&dir);

        let (config, outcome) = load_or_create(&path).unwrap();

        assert_eq!(outcome, LoadOutcome::CreatedDefault);
        assert_eq!(config, Config::default());
        assert!(path.exists());
    }

    #[test]
    fn saved_config_loads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_path(&dir);
        let mut config = Config::default();
        config.clipboard.copy_on_capture = false;

        save(&path, &config).unwrap();
        let (loaded, outcome) = load_or_create(&path).unwrap();

        assert_eq!(outcome, LoadOutcome::Loaded);
        assert_eq!(loaded, config);
    }

    #[test]
    fn invalid_file_is_backed_up_and_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_path(&dir);
        save(&path, &Config::default()).unwrap();
        fs::write(&path, r#"{ "hotkeys": { "capture": "Nope+Q" } }"#).unwrap();

        let (config, outcome) = load_or_create(&path).unwrap();

        let LoadOutcome::RecoveredFromInvalid { backup, .. } = outcome else {
            unreachable!("expected recovery, got {outcome:?}");
        };
        assert_eq!(config, Config::default());
        assert!(fs::read_to_string(backup).unwrap().contains("Nope+Q"));
        assert_eq!(load_or_create(&path).unwrap().1, LoadOutcome::Loaded);
    }

    #[test]
    fn save_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = config_path(&dir);

        save(&path, &Config::default()).unwrap();

        assert!(!path.with_extension("json.tmp").exists());
    }
}
