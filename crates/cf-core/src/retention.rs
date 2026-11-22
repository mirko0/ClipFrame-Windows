//! Automatic clean-up of old captures: which files may be deleted.
//!
//! Only images are ever deleted, and only those ClipFrame saved: every file in its own
//! default captures folder (and the monthly subfolders in it), plus files it recorded in its
//! ledger of saved paths (for a folder the user chose, which may hold other pictures).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// File extensions ClipFrame saves.
const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "bmp"];

/// Seconds in a day.
const DAY: u64 = 24 * 60 * 60;

/// Whether `path` is an image ClipFrame could have saved.
pub fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// Whether a file last modified at `modified` is older than `days` at `now`. `days == 0`
/// keeps everything; a modification time in the future is never old.
pub fn is_expired(modified: SystemTime, now: SystemTime, days: u16) -> bool {
    if days == 0 {
        return false;
    }
    let limit = Duration::from_secs(u64::from(days) * DAY);
    now.duration_since(modified).is_ok_and(|age| age > limit)
}

/// Whether `name` is a monthly folder (`YYYY-MM`) that ClipFrame creates.
pub fn is_month_folder(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() == 7
        && bytes[4] == b'-'
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..].iter().all(u8::is_ascii_digit)
}

/// The ledger after a clean-up: `entries` without the deleted paths and duplicates, newest
/// last.
pub fn prune_ledger(entries: &[PathBuf], deleted: &[PathBuf]) -> Vec<PathBuf> {
    let mut kept: Vec<PathBuf> = Vec::new();
    for entry in entries.iter().rev() {
        if !deleted.contains(entry) && !kept.contains(entry) {
            kept.push(entry.clone());
        }
    }
    kept.reverse();
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_images_are_candidates() {
        assert!(is_image(Path::new(r"C:\x\a.PNG")));
        assert!(is_image(Path::new("b.jpeg")));
        assert!(!is_image(Path::new("notes.txt")));
        assert!(!is_image(Path::new("folder")));
    }

    #[test]
    fn age_is_compared_in_whole_days() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100 * DAY);
        let days_ago = |d: u64| now - Duration::from_secs(d * DAY);
        assert!(is_expired(days_ago(8), now, 7));
        assert!(!is_expired(days_ago(6), now, 7));
        assert!(!is_expired(days_ago(99), now, 0), "0 keeps everything");
        assert!(!is_expired(now + Duration::from_secs(60), now, 7), "future");
    }

    #[test]
    fn month_folders_are_recognised() {
        assert!(is_month_folder("2026-11"));
        assert!(!is_month_folder("Holiday"));
        assert!(!is_month_folder("2026-1"));
        assert!(!is_month_folder("2026_11"));
    }

    #[test]
    fn ledger_drops_deleted_and_repeated_paths() {
        let p = |s: &str| PathBuf::from(s);
        let entries = [p("a"), p("b"), p("a"), p("c")];
        assert_eq!(prune_ledger(&entries, &[p("b")]), vec![p("a"), p("c")]);
    }
}
