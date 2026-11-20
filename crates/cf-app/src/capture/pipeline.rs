//! After a region is chosen: encode once, then save to disk and/or copy to the clipboard.
//! Runs on a worker thread so the UI never waits for compression or disk I/O.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Instant;

use anyhow::Context as _;
use cf_core::image::Image;
use cf_core::{encode, output};

/// Where a finished capture goes.
#[derive(Debug, Clone)]
pub struct Delivery {
    /// Folder to save into, or `None` to skip saving.
    pub save_dir: Option<PathBuf>,
    /// Used when saving to `save_dir` fails.
    pub fallback_dir: PathBuf,
    pub copy_to_clipboard: bool,
}

/// Delivers `image` in the background. The handle lets the helper wait for unfinished
/// deliveries before it exits.
pub fn deliver(image: Image, delivery: Delivery) -> Option<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("capture-delivery".into())
        .spawn(move || {
            if let Err(err) = run(&image, &delivery) {
                tracing::error!("delivering capture failed: {err:#}");
            }
        })
        .inspect_err(|err| tracing::error!("could not start delivery thread: {err}"))
        .ok()
}

fn run(image: &Image, delivery: &Delivery) -> anyhow::Result<()> {
    if delivery.save_dir.is_none() && !delivery.copy_to_clipboard {
        return Ok(());
    }
    let started = Instant::now();
    let png = encode::png(image)?;
    let encoded_ms = started.elapsed().as_millis();

    if delivery.copy_to_clipboard {
        cf_platform::clipboard::set_image(image, &png).context("copying to the clipboard")?;
    }
    let saved = delivery
        .save_dir
        .as_deref()
        .map(|dir| save_with_fallback(dir, &delivery.fallback_dir, &png))
        .transpose()?;
    tracing::info!(
        width = image.width(),
        height = image.height(),
        bytes = png.len(),
        encoded_ms,
        total_ms = started.elapsed().as_millis(),
        saved = ?saved,
        copied = delivery.copy_to_clipboard,
        "capture delivered"
    );
    Ok(())
}

fn save_with_fallback(dir: &Path, fallback: &Path, png: &[u8]) -> anyhow::Result<PathBuf> {
    save(dir, png).or_else(|err| {
        tracing::warn!(
            "could not save to {}: {err:#}. If Windows Security's Controlled folder access is              on, allow ClipFrame under Ransomware protection → Allow an app. Saving to {} instead.",
            dir.display(),
            fallback.display()
        );
        save(fallback, png)
    })
}

fn save(dir: &Path, png: &[u8]) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let name = output::capture_file_name(cf_platform::time::local_now(), "png");
    let path = output::unique_path(dir, &name, Path::exists);
    std::fs::write(&path, png).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}
