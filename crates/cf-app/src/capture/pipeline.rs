//! After a region is chosen: encode once, then copy, save, keep in history and notify.
//! Runs on a worker thread so the UI never waits for compression or disk I/O.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Instant;

use anyhow::Context as _;
use cf_core::encode;
use cf_core::image::Image;
use cf_core::naming::{Naming, Source};
use cf_platform::toast::{self, Toast};

use super::history::HistoryStore;
use crate::editor::EditorOptions;

/// Where a finished capture goes.
#[derive(Debug, Clone)]
pub struct Delivery {
    /// Folder to save into, or `None` to skip saving.
    pub save_dir: Option<PathBuf>,
    /// Used when saving to `save_dir` fails.
    pub fallback_dir: PathBuf,
    /// How saved files are named.
    pub naming: Naming,
    pub copy_to_clipboard: bool,
    /// Recent captures, when history is on.
    pub history: Option<HistoryStore>,
    /// Show a toast (only used when the editor doesn't open).
    pub toast: bool,
    /// Editor settings (for captures and reopened recent captures).
    pub editor: EditorOptions,
    /// Open the editor after an interactive capture.
    pub open_editor: bool,
}

/// One capture to deliver.
pub struct Job {
    pub image: Image,
    pub source: Source,
    /// Whether to show a toast for this capture.
    pub notify: bool,
}

/// Delivers a capture in the background. The handle lets the helper wait for unfinished
/// deliveries before it exits.
pub fn deliver(job: Job, delivery: Delivery) -> Option<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("capture-delivery".into())
        .spawn(move || {
            if let Err(err) = run(&job, &delivery) {
                tracing::error!("delivering capture failed: {err:#}");
            }
        })
        .inspect_err(|err| tracing::error!("could not start delivery thread: {err}"))
        .ok()
}

fn run(job: &Job, delivery: &Delivery) -> anyhow::Result<()> {
    let started = Instant::now();
    let png = encode::png(&job.image)?;
    let encoded_ms = started.elapsed().as_millis();

    if delivery.copy_to_clipboard {
        cf_platform::clipboard::set_image(&job.image, &png).context("copying to the clipboard")?;
    }
    let saved = delivery
        .save_dir
        .as_deref()
        .map(|dir| save_with_fallback(dir, delivery, &job.source, &png))
        .transpose()?;
    let kept = delivery.history.as_ref().and_then(|store| {
        store
            .add(&png)
            .inspect_err(|err| tracing::warn!("could not keep the capture in history: {err}"))
            .ok()
    });
    tracing::info!(
        width = job.image.width(),
        height = job.image.height(),
        bytes = png.len(),
        encoded_ms,
        total_ms = started.elapsed().as_millis(),
        saved = ?saved,
        copied = delivery.copy_to_clipboard,
        "capture delivered"
    );
    if job.notify {
        notify(
            &job.image,
            saved.as_deref(),
            kept.as_deref(),
            delivery.copy_to_clipboard,
        );
    }
    Ok(())
}

/// "Capture saved" with Open / Show in folder, or "Copied to the clipboard".
fn notify(image: &Image, saved: Option<&Path>, kept: Option<&Path>, copied: bool) {
    let size = format!("{} × {}", image.width(), image.height());
    let name = saved
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned());
    let toast = match (saved, &name) {
        (Some(path), Some(name)) => Toast {
            title: if copied {
                "Capture saved and copied"
            } else {
                "Capture saved"
            },
            body: name,
            image: Some(path),
            open: Some(path),
            folder: path.parent(),
        },
        _ if copied => Toast {
            title: "Copied to the clipboard",
            body: &size,
            image: kept,
            open: kept,
            folder: None,
        },
        _ => return,
    };
    if let Err(err) = toast::show(&toast) {
        tracing::warn!("could not show the notification: {err}");
    }
}

fn save_with_fallback(
    dir: &Path,
    delivery: &Delivery,
    source: &Source,
    png: &[u8],
) -> anyhow::Result<PathBuf> {
    save(dir, &delivery.naming, source, png).or_else(|err| {
        let fallback = &delivery.fallback_dir;
        tracing::warn!(
            "could not save to {}: {err:#}. If Windows Security's Controlled folder access is \
             on, allow ClipFrame under Ransomware protection → Allow an app. Saving to {} instead.",
            dir.display(),
            fallback.display()
        );
        save(fallback, &delivery.naming, source, png)
    })
}

/// Writes `png` to a new, templated file in `dir`.
pub fn save(dir: &Path, naming: &Naming, source: &Source, png: &[u8]) -> anyhow::Result<PathBuf> {
    let now = cf_platform::time::local_now();
    let path = naming.path(dir, now, source, Path::exists);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&path, png).with_context(|| format!("writing {}", path.display()))?;
    super::cleanup::record_saved(&path);
    Ok(path)
}

/// Encodes an edited image and copies it and/or writes it to `save_to` in the background.
/// When writing there fails, a new file in `fallback_dir` is used instead.
pub fn export(
    image: Image,
    copy: bool,
    save_to: Option<PathBuf>,
    fallback_dir: PathBuf,
) -> Option<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("editor-export".into())
        .spawn(move || {
            if let Err(err) = run_export(&image, copy, save_to.as_deref(), &fallback_dir) {
                tracing::error!("exporting the edited image failed: {err:#}");
            }
        })
        .inspect_err(|err| tracing::error!("could not start export thread: {err}"))
        .ok()
}

fn run_export(
    image: &Image,
    copy: bool,
    save_to: Option<&Path>,
    fallback_dir: &Path,
) -> anyhow::Result<()> {
    let png = encode::png(image)?;
    if copy {
        cf_platform::clipboard::set_image(image, &png).context("copying to the clipboard")?;
    }
    if let Some(path) = save_to {
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(path, &png));
        match written {
            Ok(()) => {
                super::cleanup::record_saved(path);
                tracing::info!(path = %path.display(), "edited image saved");
            }
            Err(err) => {
                tracing::warn!("could not write {}: {err}", path.display());
                let saved = save(fallback_dir, &Naming::default(), &Source::default(), &png)?;
                tracing::info!(path = %saved.display(), "edited image saved to the fallback folder");
            }
        }
    }
    tracing::debug!(copied = copy, "edited image exported");
    Ok(())
}
