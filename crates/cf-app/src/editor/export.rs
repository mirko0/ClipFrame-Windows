//! Flattening the edited image for the clipboard and files.

use std::path::{Path, PathBuf};
use std::time::Instant;

use cf_core::output;
use cf_render::Gpu;

use super::{Editor, rect_of, render};
use crate::capture::pipeline;

impl Editor {
    /// Saves to a new file the first time, then overwrites that file.
    pub(super) fn save(&mut self, gpu: &Gpu) {
        let path = self.saved_path.clone().unwrap_or_else(|| {
            let name = output::capture_file_name(cf_platform::time::local_now(), "png");
            output::unique_path(&self.options.save_dir, &name, Path::exists)
        });
        self.export(gpu, false, Some(path.clone()));
        self.saved_path = Some(path);
        self.status = Some(("Saved".into(), Instant::now()));
    }

    /// Renders the edited image (cropped) and hands it to a background thread.
    pub(super) fn export(&mut self, gpu: &Gpu, copy: bool, save_to: Option<PathBuf>) {
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
        match flattened {
            Ok(image) => {
                if copy {
                    self.exported_revision = doc.revision();
                }
                let fallback = self.options.fallback_dir.clone();
                self.jobs
                    .extend(pipeline::export(image, copy, save_to, fallback));
            }
            Err(err) => tracing::error!("could not render the edited image: {err:#}"),
        }
    }
}
