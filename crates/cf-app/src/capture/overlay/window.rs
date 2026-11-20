//! One borderless, topmost overlay window per monitor, showing that monitor's frozen frame.

use cf_core::geometry::Rect;
use cf_core::image::Image;
use cf_render::{Canvas, Gpu, GpuImage, WindowSurface};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::platform::windows::WindowAttributesExtWindows as _;
use winit::window::{Window, WindowId, WindowLevel};

/// UI text size in pixels at 100 % scaling.
const TEXT_PX: f32 = 13.0;

/// A monitor-sized overlay window and its GPU resources.
pub struct OverlayWindow {
    // Field order matters: the surface must be released before its window.
    surface: WindowSurface,
    pub frozen: GpuImage,
    pub window: Window,
    /// Monitor bounds in virtual-desktop pixels.
    pub monitor: Rect,
}

impl OverlayWindow {
    /// Creates the (still hidden) window and uploads `frame` to the GPU.
    pub fn create(
        event_loop: &ActiveEventLoop,
        gpu: &Gpu,
        monitor: Rect,
        frame: &Image,
    ) -> anyhow::Result<Self> {
        let size = physical_size(monitor);
        let attributes = Window::default_attributes()
            .with_title("ClipFrame capture")
            .with_decorations(false)
            .with_resizable(false)
            .with_visible(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_position(PhysicalPosition::new(monitor.x, monitor.y))
            .with_inner_size(size)
            .with_skip_taskbar(true);
        let window = event_loop.create_window(attributes)?;
        if window.inner_size() != size {
            // Created across a DPI boundary; Windows may have rescaled it.
            let _ = window.request_inner_size(size);
            window.set_outer_position(PhysicalPosition::new(monitor.x, monitor.y));
        }
        let scale = scale_of(&window);
        let surface = WindowSurface::new(gpu, &window, (size.width, size.height), TEXT_PX * scale)?;
        let frozen = GpuImage::upload(gpu, frame)?;
        Ok(Self {
            surface,
            frozen,
            window,
            monitor,
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    /// The monitor's UI scale (1.0 at 100 %).
    pub fn scale(&self) -> f32 {
        scale_of(&self.window)
    }

    /// Re-applies the monitor size after a DPI change.
    pub fn keep_monitor_size(&self) {
        let _ = self.window.request_inner_size(physical_size(self.monitor));
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if let Err(err) = self.surface.resize((size.width, size.height)) {
            tracing::warn!("could not resize overlay surface: {err:#}");
        }
    }

    pub fn render(&self, gpu: &Gpu, paint: impl FnOnce(&Canvas<'_>)) {
        if let Err(err) = self.surface.draw(gpu, paint) {
            tracing::warn!("overlay frame failed: {err:#}");
        }
    }
}

fn scale_of(window: &Window) -> f32 {
    #[allow(clippy::cast_possible_truncation)] // scale factors are small (1.0–4.0)
    let scale = window.scale_factor() as f32;
    scale
}

fn physical_size(r: Rect) -> PhysicalSize<u32> {
    PhysicalSize::new(
        u32::try_from(r.width).unwrap_or(1),
        u32::try_from(r.height).unwrap_or(1),
    )
}
