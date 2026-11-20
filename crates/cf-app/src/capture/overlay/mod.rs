//! The capture overlay: frozen monitors, selection input, and the outcome of a capture.

mod hints;
mod keys;
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
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::{CursorIcon, WindowId};

use self::keys::Command;
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
        if !self.selection.is_dragging() {
            self.targets.hover(self.options.mode, p);
            self.update_candidate();
        }
        self.update_cursor_icon(index, p);
        self.invalidate(before.union(self.damage()));
    }

    fn on_wheel(&mut self, delta: MouseScrollDelta) {
        let up = match delta {
            MouseScrollDelta::LineDelta(_, y) => y > 0.0,
            MouseScrollDelta::PixelDelta(d) => d.y > 0.0,
        };
        if self.options.mode == CaptureMode::Element && self.targets.walk(up) {
            self.refresh_candidate();
        }
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
        let grab = self.grab_px(index);
        let on_selection = self.selection.hit_test(p, grab) != Hit::Outside;
        if is_pick_mode(self.options.mode) && !on_selection {
            return self.pick();
        }
        self.selection.pointer_down(p, grab);
        Outcome::Continue
    }

    /// Selects (or, without Enter confirmation, captures) the current candidate.
    fn pick(&mut self) -> Outcome {
        let Some(rect) = self.candidate.and_then(|r| r.intersect(self.bounds)) else {
            return Outcome::Continue;
        };
        if self.options.confirm_with_enter {
            self.selection.set(rect);
            Outcome::Continue
        } else {
            Outcome::Capture(rect)
        }
    }

    fn on_left_up(&mut self, p: Point) -> Outcome {
        if !self.selection.is_dragging() {
            return Outcome::Continue;
        }
        match self.selection.pointer_up(p, self.drag_modifiers()) {
            Some(rect) if !self.options.confirm_with_enter => Outcome::Capture(rect),
            // A click without dragging snaps to the window under the pointer.
            None if self.options.mode == CaptureMode::FreeSelect => self.pick(),
            _ => Outcome::Continue,
        }
    }

    fn on_right_down(&mut self) -> Outcome {
        let has_selection = self.selection.rect().is_some() || self.selection.is_dragging();
        if has_selection && self.options.mode != CaptureMode::Monitor {
            self.selection.clear();
            self.update_candidate();
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
        if let Some(p) = self.pointer {
            self.targets.hover(mode, p);
        }
        self.update_candidate();
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
