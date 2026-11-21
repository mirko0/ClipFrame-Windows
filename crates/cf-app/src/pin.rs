//! Pinned captures: borderless, always-on-top windows showing an image, placed where it was
//! captured. Drag to move, scroll to zoom, Ctrl+C to copy, Esc or double-click to close.
//! They live in the capture helper (they draw with its GPU device) and keep it running.

use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cf_core::image::Image;
use cf_render::{Canvas, Color, Gpu, GpuImage, RectF, WindowSurface};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::windows::WindowAttributesExtWindows as _;
use winit::window::{CursorIcon, Window, WindowId, WindowLevel};

use crate::capture::pipeline;

const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 4.0;
const ZOOM_STEP: f32 = 1.1;
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// An image to pin, with its top-left corner on the desktop.
#[derive(Debug)]
pub struct Pinned {
    pub image: Image,
    pub at: (i32, i32),
}

struct Pin {
    // Field order matters: the surface must be released before its window.
    surface: WindowSurface,
    picture: GpuImage,
    window: Window,
    image: Image,
    zoom: f32,
    last_press: Option<Instant>,
    modifiers: ModifiersState,
}

/// All pinned windows.
#[derive(Default)]
pub struct Pins {
    open: Vec<Pin>,
    jobs: Vec<JoinHandle<()>>,
}

impl Pins {
    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    pub fn open(&mut self, event_loop: &ActiveEventLoop, gpu: &Gpu, pinned: Pinned) {
        match Pin::open(event_loop, gpu, pinned) {
            Ok(pin) => {
                tracing::info!("capture pinned");
                self.open.push(pin);
            }
            Err(err) => tracing::error!("could not pin the capture: {err:#}"),
        }
    }

    /// Routes a window event; returns `false` if the window isn't a pin.
    pub fn handle(&mut self, gpu: &Gpu, id: WindowId, event: &WindowEvent) -> bool {
        let Some(index) = self.open.iter().position(|p| p.window.id() == id) else {
            return false;
        };
        let pin = &mut self.open[index];
        match pin.handle(gpu, event) {
            Reaction::Close => {
                self.open.remove(index);
                tracing::info!("pin closed");
            }
            Reaction::Copy => self.jobs.extend(pipeline::export(
                pin.image.clone(),
                true,
                None,
                std::path::PathBuf::new(),
            )),
            Reaction::None => {}
        }
        true
    }

    /// Clipboard copies started since the last call.
    pub fn take_jobs(&mut self) -> Vec<JoinHandle<()>> {
        std::mem::take(&mut self.jobs)
    }
}

enum Reaction {
    None,
    Copy,
    Close,
}

impl Pin {
    fn open(event_loop: &ActiveEventLoop, gpu: &Gpu, pinned: Pinned) -> anyhow::Result<Self> {
        let size = PhysicalSize::new(pinned.image.width().max(1), pinned.image.height().max(1));
        let attributes = Window::default_attributes()
            .with_title("ClipFrame pin")
            .with_decorations(false)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_position(PhysicalPosition::new(pinned.at.0, pinned.at.1))
            .with_inner_size(size)
            .with_skip_taskbar(true);
        let window = event_loop.create_window(attributes)?;
        if window.inner_size() != size {
            let _ = window.request_inner_size(size);
        }
        window.set_cursor(CursorIcon::Move);
        let actual = window.inner_size();
        let surface = WindowSurface::new(gpu, &window, (actual.width, actual.height), 13.0)?;
        let picture = GpuImage::upload(gpu, &pinned.image)?;
        window.focus_window();
        Ok(Self {
            surface,
            picture,
            window,
            image: pinned.image,
            zoom: 1.0,
            last_press: None,
            modifiers: ModifiersState::empty(),
        })
    }

    fn handle(&mut self, gpu: &Gpu, event: &WindowEvent) -> Reaction {
        match event {
            WindowEvent::RedrawRequested => self.render(gpu),
            WindowEvent::Resized(size) => {
                if let Err(err) = self.surface.resize((size.width, size.height)) {
                    tracing::warn!("could not resize the pin: {err:#}");
                }
                self.window.request_redraw();
            }
            WindowEvent::CloseRequested => return Reaction::Close,
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => return self.on_press(),
            WindowEvent::MouseWheel { delta, .. } => self.on_wheel(*delta),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                return match &event.logical_key {
                    Key::Named(NamedKey::Escape) => Reaction::Close,
                    Key::Character(c)
                        if self.modifiers.control_key() && c.eq_ignore_ascii_case("c") =>
                    {
                        Reaction::Copy
                    }
                    _ => Reaction::None,
                };
            }
            _ => {}
        }
        Reaction::None
    }

    /// A double-click closes; otherwise the press starts moving the window.
    fn on_press(&mut self) -> Reaction {
        let now = Instant::now();
        if self.last_press.is_some_and(|t| now - t < DOUBLE_CLICK) {
            return Reaction::Close;
        }
        self.last_press = Some(now);
        if let Err(err) = self.window.drag_window() {
            tracing::debug!("could not move the pin: {err}");
        }
        Reaction::None
    }

    fn on_wheel(&mut self, delta: MouseScrollDelta) {
        let steps = match delta {
            MouseScrollDelta::LineDelta(_, y) => y,
            #[allow(clippy::cast_possible_truncation)] // wheel deltas are small
            MouseScrollDelta::PixelDelta(p) => (p.y / 120.0) as f32,
        };
        let zoom = zoomed(self.zoom, steps);
        if (zoom - self.zoom).abs() < f32::EPSILON {
            return;
        }
        self.zoom = zoom;
        let size = scaled_size((self.image.width(), self.image.height()), zoom);
        let _ = self.window.request_inner_size(size);
    }

    fn render(&self, gpu: &Gpu) {
        let result = self.surface.draw(gpu, |canvas| self.paint(canvas));
        if let Err(err) = result {
            tracing::warn!("pin frame failed: {err:#}");
        }
    }

    fn paint(&self, canvas: &Canvas<'_>) {
        let (w, h) = canvas.size();
        let area = RectF::new(0.0, 0.0, w, h);
        let exact = (self.zoom - 1.0).abs() < f32::EPSILON;
        canvas.draw_image(&self.picture, area, None, !exact);
        // A thin frame tells the pin apart from the screen underneath.
        canvas.stroke_outside(
            area.inflate(-1.0),
            1.0,
            Color::rgba8(0x4F, 0xA3, 0xFF, 0xC0),
        );
    }
}

/// The zoom after `steps` wheel notches (positive = larger), within limits.
fn zoomed(zoom: f32, steps: f32) -> f32 {
    (zoom * ZOOM_STEP.powf(steps)).clamp(MIN_ZOOM, MAX_ZOOM)
}

fn scaled_size((w, h): (u32, u32), zoom: f32) -> PhysicalSize<u32> {
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let scale = |v: u32| ((v as f32 * zoom).round() as u32).max(16);
    PhysicalSize::new(scale(w), scale(h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_steps_are_multiplicative_and_bounded() {
        assert!((zoomed(1.0, 1.0) - 1.1).abs() < 1e-5);
        assert!((zoomed(1.0, -1.0) - 1.0 / 1.1).abs() < 0.01);
        assert!((zoomed(3.9, 5.0) - MAX_ZOOM).abs() < f32::EPSILON);
        assert!((zoomed(0.11, -5.0) - MIN_ZOOM).abs() < f32::EPSILON);
    }

    #[test]
    fn scaled_sizes_round_and_stay_grabbable() {
        assert_eq!(scaled_size((200, 100), 1.5), PhysicalSize::new(300, 150));
        assert_eq!(scaled_size((50, 20), 0.1), PhysicalSize::new(16, 16));
    }
}
