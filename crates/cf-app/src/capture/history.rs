//! Recent captures on disk: written by the delivery thread, listed by the tray's "Recent"
//! menu, reopened in the editor by the helper. Nothing is held in memory.

use std::fs;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use cf_core::history::{self, Entry};
use cf_core::image::Image;
use cf_core::{encode, output};

/// Where recent captures live and how many are kept.
#[derive(Debug, Clone)]
pub struct HistoryStore {
    pub dir: PathBuf,
    pub keep: usize,
}

impl HistoryStore {
    /// Stores an encoded capture and deletes the oldest beyond `keep`.
    ///
    /// # Errors
    /// When the folder or file cannot be written.
    pub fn add(&self, png: &[u8]) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.dir)?;
        let name = history::file_name(cf_platform::time::local_now());
        let path = output::unique_path(&self.dir, &name, Path::exists);
        fs::write(&path, png)?;
        for old in history::excess(&entries(&self.dir), self.keep) {
            if let Err(err) = fs::remove_file(&old.path) {
                tracing::debug!("could not remove {}: {err}", old.path.display());
            }
        }
        Ok(path)
    }

    /// Decodes the `index`-th most recent capture.
    ///
    /// # Errors
    /// When there is no such entry or it can't be read.
    pub fn load(&self, index: usize) -> anyhow::Result<Image> {
        let list = entries(&self.dir);
        let entry = list.get(index).context("no such recent capture")?;
        let bytes =
            fs::read(&entry.path).with_context(|| format!("reading {}", entry.path.display()))?;
        Ok(encode::decode_png(&bytes)?)
    }
}

/// Recent captures in `dir`, newest first (empty when the folder doesn't exist).
pub fn entries(dir: &Path) -> Vec<Entry> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    history::newest_first(read.filter_map(|e| e.ok().map(|e| e.path())))
}

/// Pixel size of a stored capture, read from its PNG header only.
pub fn size_of(entry: &Entry) -> Option<(u32, u32)> {
    let mut header = [0u8; 24];
    fs::File::open(&entry.path)
        .and_then(|mut file| file.read_exact(&mut header))
        .ok()?;
    history::png_size(&header)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn keeps_only_the_newest_and_loads_them_back() {
        let dir = tempfile::tempdir().unwrap();
        let store = HistoryStore {
            dir: dir.path().join("history"),
            keep: 2,
        };
        let image = Image::from_bgra(2, 1, vec![1, 2, 3, 255, 4, 5, 6, 255]).unwrap();
        let png = encode::png(&image).unwrap();
        for _ in 0..3 {
            store.add(&png).unwrap();
        }
        let list = entries(&store.dir);
        assert_eq!(list.len(), 2);
        assert_eq!(size_of(&list[0]), Some((2, 1)));
        assert_eq!(store.load(0).unwrap(), image);
        assert!(store.load(5).is_err());
    }
}
