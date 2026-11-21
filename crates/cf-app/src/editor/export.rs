//! Flattening the edited image for the clipboard and files.

use std::path::{Path, PathBuf};
use std::time::Instant;

use cf_core::annotate::Area;
use cf_core::encode;
use cf_core::image::Image;
use cf_platform::drag::DragResult;
use cf_render::Gpu;

use super::{Editor, rect_of, render};
use crate::capture::pipeline;
use crate::pin::Pinned;

impl Editor {
    /// Saves to a new file the first time, then overwrites that file.
    pub(super) fn save(&mut self, gpu: &Gpu) {
        let path = self.saved_path.clone().unwrap_or_else(|| {
            let now = cf_platform::time::local_now();
            let naming = &self.options.naming;
            naming.path(&self.options.save_dir, now, &self.source, Path::exists)
        });
        self.export(gpu, false, Some(path.clone()));
        self.saved_path = Some(path);
        self.status = Some(("Saved".into(), Instant::now()));
    }

    /// Renders the edited image (cropped) and hands it to a background thread.
    pub(super) fn export(&mut self, gpu: &Gpu, copy: bool, save_to: Option<PathBuf>) {
        let Some((image, _)) = self.flatten(gpu) else {
            return;
        };
        if copy {
            self.exported_revision = self.state.document().revision();
        }
        let fallback = self.options.fallback_dir.clone();
        self.jobs
            .extend(pipeline::export(image, copy, save_to, fallback));
    }

    /// Prepares a pin of the edited image where it was captured; `false` on failure.
    pub(super) fn pin(&mut self, gpu: &Gpu) -> bool {
        let Some((image, area)) = self.flatten(gpu) else {
            return false;
        };
        #[allow(clippy::cast_possible_truncation)] // whole pixels within the image
        let at = (
            self.region.x + area.min.x as i32,
            self.region.y + area.min.y as i32,
        );
        self.pinned = Some(Pinned { image, at });
        true
    }

    /// Writes the edited image to a temporary file and drags it into another app.
    pub(super) fn drag_out(&mut self, gpu: &Gpu) {
        let Some((image, _)) = self.flatten(gpu) else {
            return;
        };
        let dir = std::env::temp_dir().join("ClipFrame").join("drag");
        forget_old_drags(&dir);
        let written = encode::png(&image)
            .map_err(anyhow::Error::from)
            .and_then(|png| pipeline::save(&dir, &self.options.naming, &self.source, &png));
        let path = match written {
            Ok(path) => path,
            Err(err) => {
                tracing::error!("could not prepare the drag: {err:#}");
                return;
            }
        };
        tracing::debug!(path = %path.display(), "dragging the image out");
        match cf_platform::drag::drag_file(&path) {
            Ok(DragResult::Dropped) => self.say("Dropped"),
            Ok(DragResult::Cancelled) => {}
            Err(err) => tracing::warn!("drag failed: {err}"),
        }
    }

    /// The pin prepared by [`Self::pin`], once.
    pub fn take_pin(&mut self) -> Option<Pinned> {
        self.pinned.take()
    }

    /// The edited image as it would be saved, and the area of the capture it covers.
    fn flatten(&mut self, gpu: &Gpu) -> Option<(Image, Area)> {
        self.state.commit_text();
        let doc = self.state.document();
        let area = doc.crop().unwrap_or_else(|| self.whole());
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // whole pixels
        let size = (area.width() as u32, area.height() as u32);
        let backdrop = self.backdrop();
        let flattened = cf_render::render_to_image(gpu, size, |canvas| {
            canvas.set_transform(1.0, (-area.min.x, -area.min.y));
            canvas.draw_image(&self.base, rect_of(self.whole()), None, false);
            render::document(canvas, &backdrop, doc, None);
        });
        flattened
            .inspect_err(|err| tracing::error!("could not render the edited image: {err:#}"))
            .ok()
            .map(|image| (image, area))
    }
}

/// Deletes dragged files older than ten minutes (targets copy them on drop).
fn forget_old_drags(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let old = |entry: &std::fs::DirEntry| {
        entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > std::time::Duration::from_secs(600))
    };
    for entry in entries.flatten().filter(old) {
        let _ = std::fs::remove_file(entry.path());
    }
}
