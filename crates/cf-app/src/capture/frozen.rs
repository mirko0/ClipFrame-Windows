//! Freezing the desktop: every monitor captured before anything is shown.

use std::time::Instant;

use cf_core::geometry::Rect;
use cf_core::image::Frame;
use cf_platform::{monitors, screen_capture};

/// All monitors, captured at one moment.
#[derive(Debug)]
pub struct FrozenDesktop {
    pub frames: Vec<Frame>,
    /// Bounding box of all monitors.
    pub bounds: Rect,
}

pub fn freeze(include_cursor: bool) -> anyhow::Result<FrozenDesktop> {
    let started = Instant::now();
    let monitors = monitors::enumerate()?;
    let frames = monitors
        .iter()
        .map(|m| {
            let image = screen_capture::capture_rect(m.rect, include_cursor)?;
            Ok(Frame {
                rect: m.rect,
                image,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    tracing::debug!(
        monitors = frames.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "desktop frozen"
    );
    Ok(FrozenDesktop {
        frames,
        bounds: monitors::virtual_bounds(&monitors),
    })
}
