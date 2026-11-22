//! The main window's home page: quick actions and the most recent captures as thumbnails
//! (click one to open it in the editor). Window pixels; `s` is the UI scale.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

use cf_core::history;
use cf_core::image::Image;
use cf_render::{Canvas, Gpu, GpuImage, LineDash, RectF, Stroke};

use super::view::{self, Mode, Target};
use crate::capture::history as store;

/// Thumbnails shown (two rows of four).
pub const MAX_THUMBS: usize = 8;
const COLUMNS: usize = 4;
const BUTTON_Y: f32 = 112.0;
const BUTTON_H: f32 = 40.0;
const GAP: f32 = 12.0;
const RECENT_Y: f32 = 184.0;
const GRID_Y: f32 = 216.0;
const THUMB_H: f32 = 96.0;
const LABEL_H: f32 = 24.0;
const ROW_GAP: f32 = 14.0;

/// A home-page button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeButton {
    NewCapture,
    OpenFolder,
    CheckUpdates,
}

/// Buttons left to right: what, label, width at 100 %, primary.
const BUTTONS: [(HomeButton, &str, f32, bool); 3] = [
    (HomeButton::NewCapture, "New capture", 150.0, true),
    (HomeButton::OpenFolder, "Open captures folder", 190.0, false),
    (HomeButton::CheckUpdates, "Check for updates", 170.0, false),
];

/// A recent capture, scaled down and on the GPU.
pub struct Thumb {
    image: GpuImage,
    label: String,
}

/// A recent capture scaled down on the CPU, not yet on the GPU.
pub struct Loaded {
    image: Image,
    label: String,
}

/// Decodes and scales down up to [`MAX_THUMBS`] recent captures from `dir` on a worker
/// thread (decoding big PNGs takes a while), then calls `done`. Unreadable ones are skipped.
pub fn load_in_background(
    dir: PathBuf,
    s: f32,
    done: impl FnOnce() + Send + 'static,
) -> Option<Receiver<Vec<Loaded>>> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // small and positive
    let (max_w, max_h) = ((cell_width(1.0) * s) as u32, (THUMB_H * s) as u32);
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("thumbnails".into())
        .spawn(move || {
            let now = cf_platform::time::local_now();
            let loaded: Vec<Loaded> = store::entries(&dir)
                .iter()
                .take(MAX_THUMBS)
                .filter_map(|entry| {
                    let bytes = std::fs::read(&entry.path).ok()?;
                    let image = cf_core::encode::decode_png(&bytes).ok()?;
                    Some(Loaded {
                        image: image.thumbnail(max_w, max_h),
                        label: history::label(entry, now, image_size(&bytes)),
                    })
                })
                .collect();
            let _ = tx.send(loaded);
            done();
        });
    spawned
        .inspect_err(|err| tracing::warn!("could not load thumbnails: {err}"))
        .ok()
        .map(|_| rx)
}

/// Puts loaded thumbnails on the GPU.
pub fn upload(gpu: &Gpu, loaded: Vec<Loaded>) -> Vec<Thumb> {
    loaded
        .into_iter()
        .filter_map(|l| {
            let image = GpuImage::upload(gpu, &l.image).ok()?;
            Some(Thumb {
                image,
                label: l.label,
            })
        })
        .collect()
}

fn image_size(png: &[u8]) -> Option<(u32, u32)> {
    history::png_size(png.get(..24)?)
}

fn cell_width(s: f32) -> f32 {
    #[allow(clippy::cast_precision_loss)] // four columns
    let columns = COLUMNS as f32;
    (content_width(1.0) - GAP * (columns - 1.0)) / columns * s
}

fn content_width(s: f32) -> f32 {
    view::WIDTH * s - view::content_left(Mode::Settings, s) - view::PAD * s
}

/// Clickable areas: the buttons and `thumbs` thumbnails.
pub fn targets(thumbs: usize, s: f32, out: &mut Vec<(RectF, Target)>) {
    let left = view::content_left(Mode::Settings, s);
    let mut x = left;
    for (button, _, width, _) in BUTTONS {
        out.push((
            RectF::new(x, BUTTON_Y * s, width * s, BUTTON_H * s),
            Target::Home(button),
        ));
        x += (width + GAP) * s;
    }
    let cell = cell_width(s);
    for index in 0..thumbs.min(MAX_THUMBS) {
        #[allow(clippy::cast_precision_loss)] // a handful of cells
        let (col, row) = ((index % COLUMNS) as f32, (index / COLUMNS) as f32);
        let rect = RectF::new(
            left + col * (cell + GAP * s),
            (GRID_Y + row * (THUMB_H + LABEL_H + ROW_GAP)) * s,
            cell,
            THUMB_H * s,
        );
        out.push((rect, Target::Recent(index)));
    }
}

/// What the home page shows besides its targets.
pub struct HomeLook<'a> {
    pub thumbs: &'a [Thumb],
    pub hover: Option<Target>,
    /// Recent captures are kept (Settings → Saving).
    pub history_on: bool,
    /// Thumbnails are still being loaded.
    pub loading: bool,
    pub hotkey: &'a str,
}

pub fn paint(canvas: &Canvas<'_>, targets: &[(RectF, Target)], look: &HomeLook<'_>, s: f32) {
    let left = view::content_left(Mode::Settings, s);
    for (rect, target) in targets {
        let hovered = look.hover == Some(*target);
        match target {
            Target::Home(button) => {
                let (_, label, _, primary) = BUTTONS
                    .iter()
                    .find(|(b, ..)| b == button)
                    .copied()
                    .unwrap_or(BUTTONS[0]);
                view::button(canvas, *rect, label, (primary, hovered), s);
            }
            Target::Recent(index) => {
                if let Some(thumb) = look.thumbs.get(*index) {
                    thumbnail(canvas, *rect, thumb, hovered, s);
                }
            }
            _ => {}
        }
    }
    if let Some(block) = canvas.text_sized("Recent captures", 15.0 * s, true) {
        canvas.draw_text(&block, left, RECENT_Y * s, view::ink());
    }
    if look.thumbs.is_empty() {
        let text = if look.loading {
            "Loading…".to_owned()
        } else if look.history_on {
            format!(
                "Nothing yet. Press {} to take your first capture.",
                look.hotkey
            )
        } else {
            "Recent captures are off. Turn them on under Saving.".to_owned()
        };
        if let Some(block) = canvas.text_sized(&text, 13.5 * s, false) {
            canvas.draw_text(&block, left, (GRID_Y + 4.0) * s, view::muted());
        }
    }
}

fn thumbnail(canvas: &Canvas<'_>, rect: RectF, thumb: &Thumb, hovered: bool, s: f32) {
    canvas.fill_rounded(rect, 6.0 * s, view::field());
    #[allow(clippy::cast_precision_loss)] // thumbnail sizes
    let (w, h) = (thumb.image.width() as f32, thumb.image.height() as f32);
    let fit = (rect.w / w.max(1.0)).min(rect.h / h.max(1.0)).min(1.0);
    let (dw, dh) = (w * fit, h * fit);
    let dest = RectF::new(
        rect.x + (rect.w - dw) / 2.0,
        rect.y + (rect.h - dh) / 2.0,
        dw,
        dh,
    );
    canvas.draw_image(&thumb.image, dest, None, true);
    if hovered {
        let frame = Stroke {
            width: 2.0 * s,
            color: view::accent(),
            dash: LineDash::Solid,
        };
        canvas.stroke_rect(rect.inflate(-s), &frame);
    }
    if let Some(block) = canvas.text_sized(&thumb.label, 12.0 * s, false) {
        canvas.push_clip(RectF::new(rect.x, rect.bottom(), rect.w, LABEL_H * s));
        canvas.draw_text(
            &block,
            rect.x + 2.0 * s,
            rect.bottom() + 5.0 * s,
            view::muted(),
        );
        canvas.pop_clip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_targets_fit_the_content_area() {
        let mut out = Vec::new();
        targets(MAX_THUMBS + 3, 1.5, &mut out);
        assert_eq!(out.len(), BUTTONS.len() + MAX_THUMBS);
        let right = (view::WIDTH - view::PAD) * 1.5 + 0.01;
        assert!(
            out.iter()
                .all(|(r, _)| r.right() <= right && r.bottom() <= view::HEIGHT * 1.5)
        );
        for (i, (a, _)) in out.iter().enumerate() {
            for (b, _) in &out[i + 1..] {
                let apart =
                    a.right() <= b.x || b.right() <= a.x || a.bottom() <= b.y || b.bottom() <= a.y;
                assert!(apart, "{a:?} overlaps {b:?}");
            }
        }
    }
}
