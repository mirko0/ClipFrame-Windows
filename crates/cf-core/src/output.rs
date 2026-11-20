//! File naming for saved captures.

use std::path::{Path, PathBuf};

/// A local wall-clock timestamp (no time-zone maths needed for file names).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LocalDateTime {
    /// Four-digit year.
    pub year: u16,
    /// 1–12.
    pub month: u8,
    /// 1–31.
    pub day: u8,
    /// 0–23.
    pub hour: u8,
    /// 0–59.
    pub minute: u8,
    /// 0–59.
    pub second: u8,
}

/// The default capture file name, e.g. `ClipFrame 2026-11-19 at 23.41.05.png`.
///
/// Sorts chronologically and contains no characters Windows forbids in file names.
pub fn capture_file_name(t: LocalDateTime, extension: &str) -> String {
    format!(
        "ClipFrame {:04}-{:02}-{:02} at {:02}.{:02}.{:02}.{extension}",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    )
}

/// `dir/name`, or `dir/stem (2).ext`, `(3)`, … when the name is taken.
///
/// `exists` is injected so this stays testable without touching the disk.
pub fn unique_path(dir: &Path, name: &str, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let first = dir.join(name);
    if !exists(&first) {
        return first;
    }
    let (stem, ext) = name
        .rsplit_once('.')
        .map_or((name, None), |(s, e)| (s, Some(e)));
    (2u32..10_000)
        .map(|n| match ext {
            Some(ext) => dir.join(format!("{stem} ({n}).{ext}")),
            None => dir.join(format!("{stem} ({n})")),
        })
        .find(|candidate| !exists(candidate))
        .unwrap_or(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: LocalDateTime = LocalDateTime {
        year: 2026,
        month: 1,
        day: 9,
        hour: 7,
        minute: 5,
        second: 3,
    };

    #[test]
    fn file_name_is_zero_padded() {
        assert_eq!(
            capture_file_name(T, "png"),
            "ClipFrame 2026-01-09 at 07.05.03.png"
        );
    }

    #[test]
    fn unique_path_returns_name_when_free() {
        let p = unique_path(Path::new("C:/x"), "a.png", |_| false);
        assert_eq!(p, Path::new("C:/x/a.png"));
    }

    #[test]
    fn unique_path_appends_counter() {
        let taken = [
            Path::new("C:/x/a.png").to_path_buf(),
            Path::new("C:/x/a (2).png").to_path_buf(),
        ];
        let p = unique_path(Path::new("C:/x"), "a.png", |c| taken.iter().any(|t| t == c));
        assert_eq!(p, Path::new("C:/x/a (3).png"));
    }
}
