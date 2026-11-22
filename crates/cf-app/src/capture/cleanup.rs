//! Deleting old captures (Settings → Saving → "Delete old captures").
//!
//! Every file ClipFrame saves is recorded in a ledger next to its state file, so a folder
//! the user chose (which may hold their own pictures) only ever loses files ClipFrame wrote.
//! ClipFrame's own default folder is scanned as a whole, so captures saved before the ledger
//! existed are cleaned up too. Runs on a worker thread when the tray and the helper start.

use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::SystemTime;

use cf_core::retention;

/// The ledger this process records saved files in (set once at start-up).
static LEDGER: OnceLock<PathBuf> = OnceLock::new();
/// Serialises ledger writes from the delivery and export threads.
static WRITING: Mutex<()> = Mutex::new(());

/// What to clean up.
#[derive(Debug, Clone)]
pub struct Cleanup {
    /// ClipFrame's own captures folder (scanned as a whole).
    pub default_dir: PathBuf,
    /// The folder captures are saved to now.
    pub captures_dir: PathBuf,
    /// The ledger of saved files.
    pub ledger: PathBuf,
    /// Delete captures older than this; 0 keeps everything.
    pub days: u16,
}

/// The ledger file for a state file.
pub fn ledger_path(state_file: &Path) -> PathBuf {
    state_file.with_file_name("saved.txt")
}

/// Sets where [`record_saved`] writes (first call wins).
pub fn use_ledger(path: PathBuf) {
    let _ = LEDGER.set(path);
}

/// Notes that ClipFrame saved `path`.
pub fn record_saved(path: &Path) {
    let Some(ledger) = LEDGER.get() else { return };
    let _guard = WRITING.lock();
    let written = OpenOptions::new()
        .create(true)
        .append(true)
        .open(ledger)
        .and_then(|mut file| writeln!(file, "{}", path.display()));
    if let Err(err) = written {
        tracing::debug!("could not record {}: {err}", path.display());
    }
}

/// Deletes expired captures on a worker thread.
pub fn start(cleanup: Cleanup) {
    if cleanup.days == 0 {
        return;
    }
    let spawned = thread::Builder::new()
        .name("cleanup".into())
        .spawn(move || {
            let deleted = run(&cleanup, SystemTime::now());
            if deleted > 0 {
                tracing::info!(deleted, days = cleanup.days, "old captures deleted");
            }
        });
    if let Err(err) = spawned {
        tracing::warn!("could not start the clean-up: {err}");
    }
}

/// Deletes expired captures; returns how many.
fn run(cleanup: &Cleanup, now: SystemTime) -> usize {
    let _guard = WRITING.lock();
    let ledger = read_ledger(&cleanup.ledger);
    let mut candidates = ledger.clone();
    if same_folder(&cleanup.captures_dir, &cleanup.default_dir) {
        candidates.extend(scan(&cleanup.default_dir));
    }
    let mut deleted = Vec::new();
    for path in candidates {
        if deleted.contains(&path) || !retention::is_image(&path) {
            continue;
        }
        let expired = fs::metadata(&path)
            .and_then(|m| m.modified())
            .is_ok_and(|modified| retention::is_expired(modified, now, cleanup.days));
        if expired && fs::remove_file(&path).is_ok() {
            deleted.push(path);
        }
    }
    let gone: Vec<PathBuf> = ledger.iter().filter(|p| !p.exists()).cloned().collect();
    let kept = retention::prune_ledger(&ledger, &gone);
    if kept.len() != ledger.len() {
        let text = kept.iter().fold(String::new(), |mut text, p| {
            let _ = writeln!(text, "{}", p.display());
            text
        });
        if let Err(err) = fs::write(&cleanup.ledger, text) {
            tracing::debug!("could not rewrite the ledger: {err}");
        }
    }
    deleted.len()
}

fn read_ledger(path: &Path) -> Vec<PathBuf> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Files in `dir` and in its monthly (`YYYY-MM`) subfolders.
fn scan(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let is_month = entry
            .file_name()
            .to_str()
            .is_some_and(retention::is_month_folder);
        if path.is_dir() && is_month {
            let inner = fs::read_dir(&path).into_iter().flatten().flatten();
            files.extend(inner.map(|e| e.path()).filter(|p| p.is_file()));
        } else if path.is_file() {
            files.push(path);
        }
    }
    files
}

fn same_folder(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        p.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_lowercase()
    };
    norm(a) == norm(b)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("clipframe-cleanup-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap_or_default();
        dir
    }

    #[test]
    fn deletes_only_old_images_it_saved() {
        let root = temp_dir("own");
        let default_dir = root.join("ClipFrame");
        let chosen = root.join("Photos");
        fs::create_dir_all(default_dir.join("2026-01")).unwrap_or_default();
        fs::create_dir_all(&chosen).unwrap_or_default();
        let files = [
            default_dir.join("a.png"),
            default_dir.join("2026-01").join("b.png"),
            default_dir.join("notes.txt"),
            chosen.join("mine.png"),
            chosen.join("saved.png"),
        ];
        for f in &files {
            fs::write(f, b"x").unwrap_or_default();
        }
        let ledger = root.join("saved.txt");
        fs::write(&ledger, format!("{}\n", files[4].display())).unwrap_or_default();
        let later = SystemTime::now() + Duration::from_secs(40 * 24 * 3600);

        // Saving into the chosen folder: only the ledger's file there goes.
        let cleanup = Cleanup {
            default_dir: default_dir.clone(),
            captures_dir: chosen,
            ledger: ledger.clone(),
            days: 30,
        };
        assert_eq!(run(&cleanup, later), 1);
        assert!(!files[4].exists() && files[3].exists() && files[0].exists());
        assert!(read_ledger(&ledger).is_empty(), "pruned");

        // Saving into the default folder: its images go, other files stay.
        let cleanup = Cleanup {
            captures_dir: default_dir,
            ..cleanup
        };
        assert_eq!(run(&cleanup, later), 2);
        assert!(!files[0].exists() && !files[1].exists() && files[2].exists());
        assert_eq!(run(&cleanup, SystemTime::now()), 0, "nothing new is old");
        let _ = fs::remove_dir_all(root);
    }
}
