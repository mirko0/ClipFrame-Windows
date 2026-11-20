//! Capture orchestration inside the helper process: delay → freeze → overlay → crop → deliver.
//!
//! The GPU device lives as long as this controller, i.e. as long as the helper process. When
//! the helper exits after its idle period, the driver memory goes with it (see PLAN §2).

mod frozen;
mod overlay;
mod pipeline;

use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::Instant;

use anyhow::Context as _;
use cf_core::capture_mode::CaptureMode;
use cf_core::capture_request::CaptureRequest;
use cf_core::config::Config;
use cf_core::geometry::Rect;
use cf_core::image::{self, Frame};
use cf_core::state::AppState;
use cf_render::Gpu;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use self::frozen::FrozenDesktop;
use self::overlay::{Outcome, Overlay, OverlayOptions};
pub use self::pipeline::Delivery;

/// Owns the overlay, the GPU device and capture state.
pub struct CaptureController {
    gpu: Option<Gpu>,
    overlay: Option<Overlay>,
    delayed: Option<(Instant, CaptureRequest)>,
    deliveries: Vec<JoinHandle<()>>,
    state_file: PathBuf,
}

impl CaptureController {
    pub const fn new(state_file: PathBuf) -> Self {
        Self {
            gpu: None,
            overlay: None,
            delayed: None,
            deliveries: Vec::new(),
            state_file,
        }
    }

    /// Whether an overlay is open or a delayed capture is pending.
    pub const fn is_busy(&self) -> bool {
        self.overlay.is_some() || self.delayed.is_some()
    }

    /// Starts a capture now, or schedules it when a delay is configured.
    pub fn request(
        &mut self,
        event_loop: &ActiveEventLoop,
        request: CaptureRequest,
        config: &Config,
        delivery: &Delivery,
    ) {
        if self.is_busy() {
            tracing::debug!(?request, "already capturing; request ignored");
            return;
        }
        let delay = config.capture.delay();
        if delay.is_zero() {
            self.start(event_loop, request, config, delivery);
        } else {
            tracing::info!(delay_secs = delay.as_secs(), "capture scheduled");
            self.delayed = Some((Instant::now() + delay, request));
        }
    }

    /// When a delayed capture is due, if any.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.delayed.map(|(at, _)| at)
    }

    /// Starts a delayed capture once it is due.
    pub fn on_timer(&mut self, event_loop: &ActiveEventLoop, config: &Config, delivery: &Delivery) {
        if let Some((_, request)) = self.delayed.filter(|(at, _)| Instant::now() >= *at) {
            self.delayed = None;
            self.start(event_loop, request, config, delivery);
        }
    }

    /// Routes a window event to the overlay. Returns `false` if the window isn't ours.
    pub fn on_window_event(
        &mut self,
        id: WindowId,
        event: WindowEvent,
        delivery: &Delivery,
    ) -> bool {
        let (Some(overlay), Some(gpu)) = (self.overlay.as_mut(), self.gpu.as_ref()) else {
            return false;
        };
        if !overlay.owns(id) {
            return false;
        }
        match overlay.handle_event(gpu, id, event) {
            Outcome::Continue => {}
            Outcome::Cancel => self.close_overlay(None, delivery),
            Outcome::Capture(region) => self.close_overlay(Some(region), delivery),
        }
        true
    }

    /// Blocks until every capture handed to the delivery pipeline is saved/copied.
    pub fn wait_for_deliveries(&mut self) {
        for handle in self.deliveries.drain(..) {
            if handle.join().is_err() {
                tracing::error!("a delivery thread panicked");
            }
        }
    }

    fn start(
        &mut self,
        event_loop: &ActiveEventLoop,
        request: CaptureRequest,
        config: &Config,
        delivery: &Delivery,
    ) {
        let result = match request {
            CaptureRequest::Interactive => self.start_interactive(event_loop, config, delivery),
            CaptureRequest::RepeatLast => self.repeat_last(config, delivery),
        };
        if let Err(err) = result {
            tracing::error!("capture failed: {err:#}");
            self.overlay = None;
        }
    }

    fn start_interactive(
        &mut self,
        event_loop: &ActiveEventLoop,
        config: &Config,
        delivery: &Delivery,
    ) -> anyhow::Result<()> {
        let started = Instant::now();
        let include_cursor = config.capture.include_cursor;
        if config.capture.default_mode == CaptureMode::AllMonitors {
            let desktop = frozen::freeze(include_cursor)?;
            self.deliver_region(&desktop.frames, desktop.bounds, delivery);
            return Ok(());
        }
        // Capture the screen on a worker while the GPU device is created here.
        let freezing = std::thread::spawn(move || frozen::freeze(include_cursor));
        if self.gpu.is_none() {
            self.gpu = Some(Gpu::new().context("initialising the GPU")?);
        }
        let desktop = freezing
            .join()
            .map_err(|_| anyhow::anyhow!("screen capture thread panicked"))??;
        let gpu = self.gpu.as_ref().context("GPU unavailable")?;
        let FrozenDesktop { frames, bounds } = desktop;
        let options = OverlayOptions {
            mode: effective_mode(config.capture.default_mode),
            confirm_with_enter: config.capture.confirm_with_enter,
        };
        self.overlay = Some(Overlay::open(event_loop, gpu, frames, bounds, options)?);
        tracing::info!(elapsed_ms = started.elapsed().as_millis(), "overlay shown");
        Ok(())
    }

    fn repeat_last(&mut self, config: &Config, delivery: &Delivery) -> anyhow::Result<()> {
        let Some(region) = AppState::load(&self.state_file).last_region else {
            tracing::info!("no previous region to repeat");
            return Ok(());
        };
        let desktop = frozen::freeze(config.capture.include_cursor)?;
        self.deliver_region(&desktop.frames, region, delivery);
        Ok(())
    }

    fn close_overlay(&mut self, region: Option<Rect>, delivery: &Delivery) {
        let Some(overlay) = self.overlay.take() else {
            return;
        };
        let frames = overlay.into_frames(); // drops windows and their GPU resources
        tracing::info!(captured = region.is_some(), "overlay closed");
        if let Some(region) = region {
            self.deliver_region(&frames, region, delivery);
        }
    }

    fn deliver_region(&mut self, frames: &[Frame], region: Rect, delivery: &Delivery) {
        self.remember(region);
        let Some(image) = image::crop(frames, region) else {
            tracing::warn!(?region, "empty capture region");
            return;
        };
        self.deliveries.retain(|handle| !handle.is_finished());
        self.deliveries
            .extend(pipeline::deliver(image, delivery.clone()));
    }

    fn remember(&self, region: Rect) {
        let state = AppState {
            last_region: Some(region),
        };
        if let Err(err) = state.save(&self.state_file) {
            tracing::warn!("could not save last region: {err}");
        }
    }
}

/// Modes that arrive in later milestones fall back to free select for now.
fn effective_mode(mode: CaptureMode) -> CaptureMode {
    match mode {
        CaptureMode::Window | CaptureMode::Element | CaptureMode::BrowserContent => {
            tracing::info!(?mode, "mode not available yet (M2); using free select");
            CaptureMode::FreeSelect
        }
        other => other,
    }
}
