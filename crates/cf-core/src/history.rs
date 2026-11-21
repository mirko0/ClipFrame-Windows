//! Recent captures: one PNG per capture in a history folder, named by time so the folder
//! itself is the index (nothing is kept in memory). This module only names, parses, orders
//! and labels entries; reading and writing files is the app's job.

use std::path::{Path, PathBuf};

use crate::output::LocalDateTime;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// One saved capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The PNG file.
    pub path: PathBuf,
    /// When it was taken.
    pub time: LocalDateTime,
    /// Tie-breaker for captures within the same second (0, 2, 3, …).
    pub counter: u32,
}

/// The history file name for a capture taken at `t`, e.g. `2026-11-21 15.38.20.png`.
pub fn file_name(t: LocalDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}.{:02}.{:02}.png",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    )
}

/// Parses a history file name (also `… (2).png` from de-duplication).
pub fn parse(path: &Path) -> Option<Entry> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".png")?;
    let (stamp, counter) = match stem.split_once(" (") {
        Some((stamp, rest)) => (stamp, rest.strip_suffix(')')?.parse().ok()?),
        None => (stem, 0),
    };
    let (date, time) = stamp.split_once(' ')?;
    let mut d = date.split('-').map(str::parse::<u16>);
    let mut t = time.split('.').map(str::parse::<u8>);
    let parsed = LocalDateTime {
        year: d.next()?.ok()?,
        month: u8::try_from(d.next()?.ok()?).ok()?,
        day: u8::try_from(d.next()?.ok()?).ok()?,
        hour: t.next()?.ok()?,
        minute: t.next()?.ok()?,
        second: t.next()?.ok()?,
    };
    (d.next().is_none() && t.next().is_none()).then(|| Entry {
        path: path.to_path_buf(),
        time: parsed,
        counter,
    })
}

/// The history entries among `paths`, newest first (other files are ignored).
pub fn newest_first(paths: impl IntoIterator<Item = PathBuf>) -> Vec<Entry> {
    let mut entries: Vec<Entry> = paths.into_iter().filter_map(|p| parse(&p)).collect();
    entries.sort_by_key(|e| std::cmp::Reverse(key(e)));
    entries
}

fn key(e: &Entry) -> (u16, u8, u8, u8, u8, u8, u32) {
    let t = e.time;
    (
        t.year, t.month, t.day, t.hour, t.minute, t.second, e.counter,
    )
}

/// Entries beyond the newest `keep`, to delete.
pub fn excess(newest_first: &[Entry], keep: usize) -> &[Entry] {
    newest_first.get(keep..).unwrap_or(&[])
}

/// A short menu label: `15:38` today, else `21 Nov 15:38`; plus the size when known.
pub fn label(entry: &Entry, now: LocalDateTime, size: Option<(u32, u32)>) -> String {
    let t = entry.time;
    let today = (t.year, t.month, t.day) == (now.year, now.month, now.day);
    let when = if today {
        format!("{:02}:{:02}", t.hour, t.minute)
    } else {
        let month = MONTHS[usize::from(t.month.clamp(1, 12) - 1)];
        format!("{} {month} {:02}:{:02}", t.day, t.hour, t.minute)
    };
    match size {
        Some((w, h)) => format!("{when}  ·  {w} × {h}"),
        None => when,
    }
}

/// Width and height from the first 24 bytes of a PNG file (its IHDR chunk).
pub fn png_size(header: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if header.get(..8)? != SIGNATURE || header.get(12..16)? != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(header.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(header.get(20..24)?.try_into().ok()?);
    Some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: u8, hour: u8, second: u8) -> LocalDateTime {
        LocalDateTime {
            year: 2026,
            month: 11,
            day,
            hour,
            minute: 38,
            second,
        }
    }

    #[test]
    fn names_round_trip_and_junk_is_ignored() {
        let name = file_name(at(21, 15, 20));
        assert_eq!(name, "2026-11-21 15.38.20.png");
        let entry = parse(Path::new(&name)).expect("parsable");
        assert_eq!((entry.time, entry.counter), (at(21, 15, 20), 0));
        let dup = parse(Path::new("2026-11-21 15.38.20 (3).png")).expect("parsable");
        assert_eq!(dup.counter, 3);
        for junk in [
            "notes.txt",
            "2026-11-21.png",
            "2026-11-21 15.38.png",
            "a b.png",
        ] {
            assert!(parse(Path::new(junk)).is_none(), "{junk}");
        }
    }

    #[test]
    fn ordering_is_newest_first_including_same_second_duplicates() {
        let names = [
            "2026-11-21 15.38.20.png",
            "2026-11-21 15.38.20 (2).png",
            "2026-11-20 09.00.00.png",
            "readme.md",
        ];
        let entries = newest_first(names.iter().map(PathBuf::from));
        let order: Vec<_> = entries
            .iter()
            .map(|e| e.path.to_string_lossy().into_owned())
            .collect();
        assert_eq!(order, [names[1], names[0], names[2]]);
        assert_eq!(excess(&entries, 2).len(), 1);
        assert_eq!(excess(&entries, 10), []);
    }

    #[test]
    fn labels_are_short() {
        let entry = parse(Path::new("2026-11-20 09.05.00.png")).expect("parsable");
        assert_eq!(label(&entry, at(20, 23, 0), None), "09:05");
        assert_eq!(
            label(&entry, at(21, 8, 0), Some((1920, 1080))),
            "20 Nov 09:05  ·  1920 × 1080"
        );
    }

    #[test]
    fn png_size_reads_the_header() {
        let mut header = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        header.extend_from_slice(b"IHDR");
        header.extend_from_slice(&640_u32.to_be_bytes());
        header.extend_from_slice(&480_u32.to_be_bytes());
        assert_eq!(png_size(&header), Some((640, 480)));
        assert_eq!(png_size(b"not a png at all, sorry!"), None);
    }
}
