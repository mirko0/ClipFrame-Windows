//! Pointer, wheel and keyboard input of the capture overlay.

use std::time::Instant;

use cf_core::capture_mode::CaptureMode;
use cf_core::geometry::Point;
use cf_core::selection::{Hit, Selection};
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, MouseScrollDelta};

use super::keys::Command;
use super::{DOUBLE_CLICK, Outcome, Overlay, is_pick_mode};

impl Overlay {
    pub(super) fn on_pointer_move(&mut self, index: usize, position: PhysicalPosition<f64>) {
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

    pub(super) fn on_wheel(&mut self, delta: MouseScrollDelta) {
        let up = match delta {
            MouseScrollDelta::LineDelta(_, y) => y > 0.0,
            MouseScrollDelta::PixelDelta(d) => d.y > 0.0,
        };
        if self.options.mode == CaptureMode::Element && self.targets.walk(up) {
            self.refresh_candidate();
        }
    }

    pub(super) fn on_button(
        &mut self,
        index: usize,
        state: ElementState,
        button: MouseButton,
    ) -> Outcome {
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

    pub(super) fn on_command(&mut self, command: Command) -> Outcome {
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
}
