//! Freezing the desktop: every monitor captured before anything is shown.

use std::time::Instant;

use cf_core::geometry::Rect;
use cf_core::image::Frame;
use cf_core::targets::WindowSnapshot;
use cf_platform::{monitors, screen_capture, window_list};

/// All monitors, captured at one moment.
#[derive(Debug)]
pub struct FrozenDesktop {
    pub frames: Vec<Frame>,
    /// Bounding box of all monitors.
    pub bounds: Rect,
    /// Top-level windows as they were when the screen was captured.
    pub windows: WindowSnapshot,
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
    let windows = window_list::snapshot().unwrap_or_else(|err| {
        tracing::warn!("could not list windows: {err}");
        WindowSnapshot::default()
    });
    tracing::debug!(
        monitors = frames.len(),
        windows = windows.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "desktop frozen"
    );
    Ok(FrozenDesktop {
        frames,
        bounds: monitors::virtual_bounds(&monitors),
        windows,
    })
}
