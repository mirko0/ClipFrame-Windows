//! The capture overlay: frozen monitors, selection input, and the outcome of a capture.

mod hints;
mod input;
mod keys;
mod modebar;
mod paint;
mod picker;
mod targets;
mod window;

use std::time::{Duration, Instant};

use cf_core::capture_mode::CaptureMode;
use cf_core::geometry::{Point, Rect};
use cf_core::image::Frame;
use cf_core::selection::{DragModifiers, Handle, Hit, Selection};
use cf_render::Gpu;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::{CursorIcon, WindowId};

use self::paint::{LOUPE_PIXELS, Scene, Target};
pub use self::picker::Waker;
use self::targets::Targets;
use self::window::OverlayWindow;
use super::frozen::FrozenDesktop;

/// Handle grab tolerance in points.
const GRAB_POINTS: f32 = 6.0;
/// Extra area repainted around the selection (label, handles) and pointer (loupe), in px.
const DAMAGE_MARGIN: i32 = 64;
const LOUPE_DAMAGE: i32 = LOUPE_PIXELS * 24 + 160;
const DOUBLE_CLICK: Duration = Duration::from_millis(450);

/// What the overlay decided after an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Continue,
    Capture(Rect),
    Cancel,
}

/// Overlay behaviour options.
#[derive(Debug, Clone, Copy)]
pub struct OverlayOptions {
    pub mode: CaptureMode,
    pub confirm_with_enter: bool,
}

pub struct Overlay {
    frames: Vec<Frame>,
    windows: Vec<OverlayWindow>,
    selection: Selection,
    bounds: Rect,
    options: OverlayOptions,
    pointer: Option<Point>,
    modifiers: ModifiersState,
    reposition: bool,
    last_click: Option<Instant>,
    targets: Targets,
    /// What a click would pick (snapping modes), shown while nothing is selected.
    candidate: Option<Rect>,
}

impl Overlay {
    /// Creates one window per frame, paints them, then shows them all at once.
    pub fn open(
        event_loop: &ActiveEventLoop,
        gpu: &Gpu,
        desktop: FrozenDesktop,
        options: OverlayOptions,
        waker: Waker,
    ) -> anyhow::Result<Self> {
        let FrozenDesktop {
            frames,
            bounds,
            windows: snapshot,
        } = desktop;
        let windows = frames
            .iter()
            .map(|f| OverlayWindow::create(event_loop, gpu, f.rect, &f.image))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let mut overlay = Self {
            frames,
            windows,
            selection: Selection::new(bounds),
            bounds,
            options,
            pointer: cf_platform::cursor::position(),
            modifiers: ModifiersState::empty(),
            reposition: false,
            last_click: None,
            targets: Targets::new(snapshot, waker),
            candidate: None,
        };
        overlay.follow_monitor_under_pointer();
        overlay.update_candidate();
        for index in 0..overlay.windows.len() {
            overlay.render(gpu, index);
        }
        overlay.show();
        Ok(overlay)
    }

    /// Hands back the frozen frames (the windows are dropped).
    pub fn into_frames(self) -> Vec<Frame> {
        self.frames
    }

    pub fn owns(&self, id: WindowId) -> bool {
        self.windows.iter().any(|w| w.id() == id)
    }

    pub fn handle_event(&mut self, gpu: &Gpu, id: WindowId, event: WindowEvent) -> Outcome {
        let Some(index) = self.windows.iter().position(|w| w.id() == id) else {
            return Outcome::Continue;
        };
        match event {
            WindowEvent::RedrawRequested => self.render(gpu, index),
            WindowEvent::Resized(size) => self.windows[index].resize(size),
            WindowEvent::ScaleFactorChanged { .. } => self.windows[index].keep_monitor_size(),
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::CursorMoved { position, .. } => self.on_pointer_move(index, position),
            WindowEvent::MouseInput { state, button, .. } => {
                return self.on_button(index, state, button);
            }
            WindowEvent::MouseWheel { delta, .. } => self.on_wheel(delta),
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(command) = keys::command_for(&event, self.modifiers) {
                    return self.on_command(command);
                }
            }
            WindowEvent::CloseRequested => return Outcome::Cancel,
            _ => {}
        }
        Outcome::Continue
    }

    fn show(&self) {
        for w in &self.windows {
            w.window.set_visible(true);
        }
        let focus = self.window_at(self.pointer).or(self.windows.first());
        if let Some(w) = focus {
            let focused = cf_platform::focus::bring_to_front(&w.window);
            tracing::debug!(focused, "overlay focus requested");
            if !focused {
                tracing::warn!("overlay could not take keyboard focus (Windows foreground lock)");
            }
        }
    }

    fn render(&self, gpu: &Gpu, index: usize) {
        let scene = self.scene();
        let w = &self.windows[index];
        let target = Target {
            monitor: w.monitor,
            scale: w.scale(),
            frozen: &w.frozen,
        };
        w.render(gpu, |canvas| paint::paint(canvas, &target, &scene));
    }

    fn scene(&self) -> Scene {
        let dragging_new = self.selection.is_dragging() || self.selection.rect().is_none();
        let adjusting =
            self.options.confirm_with_enter && self.selection.rect().is_some() && !dragging_new;
        let mode = self.options.mode;
        let free = matches!(mode, CaptureMode::FreeSelect | CaptureMode::Rectangle);
        Scene {
            selection: self.shown_rect(),
            show_handles: adjusting,
            pointer: self.pointer,
            show_loupe: free && dragging_new,
            pointer_color: self
                .pointer
                .and_then(|p| self.frames.iter().find_map(|f| f.pixel_at(p))),
            hint: hints::hint(mode, adjusting, self.selection.is_dragging()),
            mode,
            show_bar: !self.selection.is_dragging(),
        }
    }

    /// The selection, or else what a click would pick.
    fn shown_rect(&self) -> Option<Rect> {
        let idle = !self.selection.is_dragging() || self.selection.rect().is_none();
        self.selection.rect().or(self.candidate.filter(|_| idle))
    }

    /// Applies finished element/document lookups from the picker thread.
    pub fn on_targets_ready(&mut self) {
        if self.targets.poll() {
            self.refresh_candidate();
        }
    }

    // ---- helpers -------------------------------------------------------------------------

    /// In monitor mode the selection is the monitor under the pointer.
    fn follow_monitor_under_pointer(&mut self) {
        if self.options.mode != CaptureMode::Monitor {
            return;
        }
        if let Some(w) = self.window_at(self.pointer) {
            let monitor = w.monitor;
            if self.selection.rect() != Some(monitor) {
                self.selection.set(monitor);
            }
        }
    }

    fn update_candidate(&mut self) {
        let mode = self.options.mode;
        self.candidate = self.pointer.filter(|_| Targets::snaps(mode)).and_then(|p| {
            self.targets
                .candidate(mode, p)
                .or_else(|| self.window_at(Some(p)).map(|w| w.monitor))
        });
    }

    /// Recomputes the candidate after a lookup or scroll, repainting what changed.
    fn refresh_candidate(&mut self) {
        let before = self.damage();
        self.update_candidate();
        self.invalidate(before.union(self.damage()));
    }

    fn update_cursor_icon(&self, index: usize, p: Point) {
        if self.on_mode_bar(index, p).is_some() {
            self.windows[index].window.set_cursor(CursorIcon::Pointer);
            return;
        }
        let mode = self.options.mode;
        let idle = if is_pick_mode(mode) {
            CursorIcon::Pointer
        } else {
            CursorIcon::Crosshair
        };
        let icon = if mode == CaptureMode::Monitor {
            CursorIcon::Pointer
        } else if self.selection.is_dragging() || !self.options.confirm_with_enter {
            idle
        } else {
            match self.selection.hit_test(p, self.grab_px(index)) {
                Hit::Handle(handle) => resize_icon(handle),
                Hit::Inside => CursorIcon::Move,
                Hit::Outside => idle,
            }
        };
        self.windows[index].window.set_cursor(icon);
    }

    /// Where `p` is on window `index`'s mode bar, if on it. The bar is hidden while dragging.
    fn on_mode_bar(&self, index: usize, p: Point) -> Option<modebar::Spot> {
        if self.selection.is_dragging() {
            return None;
        }
        let w = &self.windows[index];
        let size = w.window.inner_size();
        #[allow(clippy::cast_precision_loss)] // window sizes and local offsets are small
        let (screen, local) = (
            (size.width as f32, size.height as f32),
            ((p.x - w.monitor.x) as f32, (p.y - w.monitor.y) as f32),
        );
        let s = w.scale();
        modebar::spot(screen, s, local)
    }

    fn drag_modifiers(&self) -> DragModifiers {
        DragModifiers {
            square: self.modifiers.shift_key(),
            reposition: self.reposition,
        }
    }

    fn grab_px(&self, index: usize) -> i32 {
        #[allow(clippy::cast_possible_truncation)] // a few pixels
        let px = (GRAB_POINTS * self.windows[index].scale()).round() as i32;
        px
    }

    fn window_at(&self, p: Option<Point>) -> Option<&OverlayWindow> {
        let p = p?;
        self.windows.iter().find(|w| w.monitor.contains(p))
    }

    /// The area whose appearance depends on the current state.
    fn damage(&self) -> Rect {
        let selection = self.shown_rect().map(|r| inflate(r, DAMAGE_MARGIN));
        let loupe = self
            .pointer
            .map(|p| inflate(Rect::new(p.x, p.y, 1, 1), LOUPE_DAMAGE));
        let hint = self.window_at(self.pointer).map(|w| w.monitor);
        [selection, loupe, hint]
            .into_iter()
            .flatten()
            .reduce(Rect::union)
            .unwrap_or_default()
    }

    /// Requests a repaint of every window that overlaps `area`.
    fn invalidate(&self, area: Rect) {
        for w in self
            .windows
            .iter()
            .filter(|w| w.monitor.intersect(area).is_some())
        {
            w.window.request_redraw();
        }
    }
}

/// Modes where a click picks something instead of starting a drag.
const fn is_pick_mode(mode: CaptureMode) -> bool {
    matches!(
        mode,
        CaptureMode::Window | CaptureMode::Element | CaptureMode::BrowserContent
    )
}

fn inflate(r: Rect, by: i32) -> Rect {
    Rect::new(r.x - by, r.y - by, r.width + 2 * by, r.height + 2 * by)
}

fn resize_icon(handle: Handle) -> CursorIcon {
    match handle {
        Handle::TopLeft | Handle::BottomRight => CursorIcon::NwseResize,
        Handle::TopRight | Handle::BottomLeft => CursorIcon::NeswResize,
        Handle::Top | Handle::Bottom => CursorIcon::NsResize,
        Handle::Left | Handle::Right => CursorIcon::EwResize,
    }
}
