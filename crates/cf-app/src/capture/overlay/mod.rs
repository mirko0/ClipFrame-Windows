//! The capture overlay: frozen monitors, selection input, and the outcome of a capture.

mod keys;
mod paint;
mod window;

use std::time::{Duration, Instant};

use cf_core::capture_mode::CaptureMode;
use cf_core::geometry::{Point, Rect};
use cf_core::image::Frame;
use cf_core::selection::{DragModifiers, Handle, Hit, Selection};
use cf_render::Gpu;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::{CursorIcon, WindowId};

use self::keys::Command;
use self::paint::{LOUPE_PIXELS, Scene, Target};
use self::window::OverlayWindow;

/// Handle grab tolerance in points.
const GRAB_POINTS: f32 = 6.0;
/// Extra area repainted around the selection (label, handles) and pointer (loupe), in px.
const DAMAGE_MARGIN: i32 = 64;
const LOUPE_DAMAGE: i32 = LOUPE_PIXELS * 24 + 160;
const DOUBLE_CLICK: Duration = Duration::from_millis(450);

const HINT_DRAG: &str = "Drag to select · Esc cancel · F free · R rectangle · M monitor · A all monitors · Ctrl+C copy colour";
const HINT_ADJUST: &str =
    "Enter or double-click to capture · arrows move · Ctrl+arrows resize · Esc cancel";
const HINT_MONITOR: &str =
    "Click a monitor to capture it · F free select · A all monitors · Esc cancel";

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
}

impl Overlay {
    /// Creates one window per frame, paints them, then shows them all at once.
    pub fn open(
        event_loop: &ActiveEventLoop,
        gpu: &Gpu,
        frames: Vec<Frame>,
        bounds: Rect,
        options: OverlayOptions,
    ) -> anyhow::Result<Self> {
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
        };
        overlay.follow_monitor_under_pointer();
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
        let monitor_mode = self.options.mode == CaptureMode::Monitor;
        let hint = match (monitor_mode, adjusting, self.selection.is_dragging()) {
            (true, ..) => HINT_MONITOR,
            (_, true, _) => HINT_ADJUST,
            (_, _, true) => "",
            _ => HINT_DRAG,
        };
        Scene {
            selection: self.selection.rect(),
            show_handles: adjusting,
            pointer: self.pointer,
            show_loupe: !monitor_mode && dragging_new,
            pointer_color: self
                .pointer
                .and_then(|p| self.frames.iter().find_map(|f| f.pixel_at(p))),
            hint,
        }
    }

    // ---- input ---------------------------------------------------------------------------

    fn on_pointer_move(&mut self, index: usize, position: PhysicalPosition<f64>) {
        let before = self.damage();
        let monitor = self.windows[index].monitor;
        #[allow(clippy::cast_possible_truncation)] // window-relative pixels fit in i32
        let p = Point::new(
            monitor.x + position.x.floor() as i32,
            monitor.y + position.y.floor() as i32,
        );
        self.pointer = Some(p);
        if self.selection.is_dragging() {
            self.selection.pointer_move(p, self.drag_modifiers());
        }
        self.follow_monitor_under_pointer();
        self.update_cursor_icon(index, p);
        self.invalidate(before.union(self.damage()));
    }

    fn on_button(&mut self, index: usize, state: ElementState, button: MouseButton) -> Outcome {
        let Some(p) = self.pointer else {
            return Outcome::Continue;
        };
        let before = self.damage();
        let outcome = match (button, state) {
            (MouseButton::Left, ElementState::Pressed) => self.on_left_down(index, p),
            (MouseButton::Left, ElementState::Released) => self.on_left_up(p),
            (MouseButton::Right, ElementState::Pressed) => self.on_right_down(),
            _ => Outcome::Continue,
        };
        self.invalidate(before.union(self.damage()));
        outcome
    }

    fn on_left_down(&mut self, index: usize, p: Point) -> Outcome {
        if self.options.mode == CaptureMode::Monitor {
            return self
                .selection
                .rect()
                .map_or(Outcome::Continue, Outcome::Capture);
        }
        let now = Instant::now();
        let double_click = self.last_click.is_some_and(|t| now - t < DOUBLE_CLICK);
        self.last_click = Some(now);
        if let Some(rect) = self
            .selection
            .rect()
            .filter(|r| double_click && r.contains(p))
        {
            return Outcome::Capture(rect);
        }
        self.selection.pointer_down(p, self.grab_px(index));
        Outcome::Continue
    }

    fn on_left_up(&mut self, p: Point) -> Outcome {
        if !self.selection.is_dragging() {
            return Outcome::Continue;
        }
        match self.selection.pointer_up(p, self.drag_modifiers()) {
            Some(rect) if !self.options.confirm_with_enter => Outcome::Capture(rect),
            _ => Outcome::Continue,
        }
    }

    fn on_right_down(&mut self) -> Outcome {
        let has_selection = self.selection.rect().is_some() || self.selection.is_dragging();
        if has_selection && self.options.mode != CaptureMode::Monitor {
            self.selection.clear();
            Outcome::Continue
        } else {
            Outcome::Cancel
        }
    }

    fn on_command(&mut self, command: Command) -> Outcome {
        let before = self.damage();
        let outcome = match command {
            Command::Cancel => Outcome::Cancel,
            Command::Confirm => self
                .selection
                .rect()
                .map_or(Outcome::Continue, Outcome::Capture),
            Command::CaptureAll => Outcome::Capture(self.bounds),
            Command::Nudge(dx, dy) => self.edit(|s| s.nudge(dx, dy)),
            Command::Grow(dw, dh) => self.edit(|s| s.grow(dw, dh)),
            Command::SetMode(mode) => self.set_mode(mode),
            Command::CopyColor => self.copy_color(),
            Command::Reposition(held) => {
                self.reposition = held;
                Outcome::Continue
            }
        };
        self.invalidate(before.union(self.damage()));
        outcome
    }

    fn edit(&mut self, change: impl FnOnce(&mut Selection)) -> Outcome {
        if !self.selection.is_dragging() {
            change(&mut self.selection);
        }
        Outcome::Continue
    }

    fn set_mode(&mut self, mode: CaptureMode) -> Outcome {
        self.options.mode = mode;
        self.selection.clear();
        self.follow_monitor_under_pointer();
        self.invalidate(self.bounds);
        Outcome::Continue
    }

    fn copy_color(&self) -> Outcome {
        let Some([b, g, r, _]) = self.scene().pointer_color else {
            return Outcome::Continue;
        };
        let hex = format!("#{r:02X}{g:02X}{b:02X}");
        match cf_platform::clipboard::set_text(&hex) {
            Ok(()) => tracing::info!(%hex, "colour copied"),
            Err(err) => tracing::warn!("could not copy colour: {err}"),
        }
        Outcome::Continue
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

    fn update_cursor_icon(&self, index: usize, p: Point) {
        let icon = if self.options.mode == CaptureMode::Monitor {
            CursorIcon::Pointer
        } else if self.selection.is_dragging() || !self.options.confirm_with_enter {
            CursorIcon::Crosshair
        } else {
            match self.selection.hit_test(p, self.grab_px(index)) {
                Hit::Handle(handle) => resize_icon(handle),
                Hit::Inside => CursorIcon::Move,
                Hit::Outside => CursorIcon::Crosshair,
            }
        };
        self.windows[index].window.set_cursor(icon);
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
        let selection = self.selection.rect().map(|r| inflate(r, DAMAGE_MARGIN));
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
