//! The interactive selection rectangle of the capture overlay, as a pure state machine.
//!
//! The overlay feeds pointer and keyboard input in virtual-desktop pixels; this module decides
//! what the selection becomes. No windowing or rendering here, so all of it is unit-tested.

use crate::geometry::{Point, Rect};

/// A grab point on the selection border.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Handle {
    /// Top-left corner.
    TopLeft,
    /// Top edge.
    Top,
    /// Top-right corner.
    TopRight,
    /// Right edge.
    Right,
    /// Bottom-right corner.
    BottomRight,
    /// Bottom edge.
    Bottom,
    /// Bottom-left corner.
    BottomLeft,
    /// Left edge.
    Left,
}

/// What lies under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// A resize handle (corner or edge).
    Handle(Handle),
    /// Inside the selection (drag moves it).
    Inside,
    /// Outside, or there is no selection (drag starts a new one).
    Outside,
}

/// Modifier state while dragging.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DragModifiers {
    /// Constrain a new selection to a square (Shift).
    pub square: bool,
    /// Move the whole in-progress selection instead of resizing it (Space).
    pub reposition: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drag {
    Creating { anchor: Point },
    Moving { grab: Point, original: Rect },
    Resizing { handle: Handle, original: Rect },
}

/// The overlay's selection: a rectangle (possibly being dragged) clamped to `bounds`.
#[derive(Debug, Clone)]
pub struct Selection {
    bounds: Rect,
    rect: Option<Rect>,
    drag: Option<Drag>,
    last_pointer: Point,
}

impl Selection {
    /// An empty selection confined to `bounds` (the virtual desktop).
    pub const fn new(bounds: Rect) -> Self {
        Self {
            bounds,
            rect: None,
            drag: None,
            last_pointer: Point::new(0, 0),
        }
    }

    /// The current non-empty selection, including one still being dragged.
    pub fn rect(&self) -> Option<Rect> {
        self.rect.filter(|r| !r.is_empty())
    }

    /// Whether a drag is in progress.
    pub const fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Replaces the selection (e.g. with a whole monitor).
    pub fn set(&mut self, rect: Rect) {
        self.rect = rect.intersect(self.bounds);
        self.drag = None;
    }

    /// Removes the selection.
    pub fn clear(&mut self) {
        self.rect = None;
        self.drag = None;
    }

    /// What lies under `p`; `grab` is the handle tolerance in pixels.
    pub fn hit_test(&self, p: Point, grab: i32) -> Hit {
        let Some(r) = self.rect() else {
            return Hit::Outside;
        };
        let near = |edge: i32, v: i32| (v - edge).abs() <= grab;
        let within_x = p.x >= r.x - grab && p.x <= r.right() + grab;
        let within_y = p.y >= r.y - grab && p.y <= r.bottom() + grab;
        let left = near(r.x, p.x) && within_y;
        let right = near(r.right(), p.x) && within_y;
        let top = near(r.y, p.y) && within_x;
        let bottom = near(r.bottom(), p.y) && within_x;
        match (left, right, top, bottom) {
            (true, _, true, _) => Hit::Handle(Handle::TopLeft),
            (_, true, true, _) => Hit::Handle(Handle::TopRight),
            (true, _, _, true) => Hit::Handle(Handle::BottomLeft),
            (_, true, _, true) => Hit::Handle(Handle::BottomRight),
            (true, ..) => Hit::Handle(Handle::Left),
            (_, true, ..) => Hit::Handle(Handle::Right),
            (_, _, true, _) => Hit::Handle(Handle::Top),
            (.., true) => Hit::Handle(Handle::Bottom),
            _ if r.contains(p) => Hit::Inside,
            _ => Hit::Outside,
        }
    }

    /// Starts a drag at `p`: resize, move, or a new selection depending on what is hit.
    pub fn pointer_down(&mut self, p: Point, grab: i32) {
        self.last_pointer = p;
        self.drag = Some(match (self.hit_test(p, grab), self.rect()) {
            (Hit::Handle(handle), Some(original)) => Drag::Resizing { handle, original },
            (Hit::Inside, Some(original)) => Drag::Moving { grab: p, original },
            _ => {
                self.rect = None;
                Drag::Creating { anchor: p }
            }
        });
    }

    /// Updates the drag for a pointer at `p`.
    pub fn pointer_move(&mut self, p: Point, modifiers: DragModifiers) {
        let delta = (p.x - self.last_pointer.x, p.y - self.last_pointer.y);
        self.last_pointer = p;
        let Some(drag) = self.drag else { return };
        let rect = match drag {
            Drag::Creating { anchor } if modifiers.reposition => {
                let anchor = anchor.offset(delta.0, delta.1);
                self.drag = Some(Drag::Creating { anchor });
                self.creating_rect(anchor, p, modifiers.square)
            }
            Drag::Creating { anchor } => self.creating_rect(anchor, p, modifiers.square),
            Drag::Moving { grab, original } => Some(
                original
                    .translate(p.x - grab.x, p.y - grab.y)
                    .clamp_into(self.bounds),
            ),
            Drag::Resizing { handle, original } => {
                resize(original, handle, p).intersect(self.bounds)
            }
        };
        self.rect = rect;
    }

    /// Ends the drag. Returns the resulting selection; a click without movement yields `None`.
    pub fn pointer_up(&mut self, p: Point, modifiers: DragModifiers) -> Option<Rect> {
        self.pointer_move(p, modifiers);
        self.drag = None;
        if self.rect().is_none() {
            self.rect = None;
        }
        self.rect()
    }

    /// Moves the selection by `(dx, dy)`, staying inside the bounds.
    pub fn nudge(&mut self, dx: i32, dy: i32) {
        self.rect = self
            .rect()
            .map(|r| r.translate(dx, dy).clamp_into(self.bounds));
    }

    /// Grows or shrinks the selection from its top-left corner.
    pub fn grow(&mut self, dw: i32, dh: i32) {
        self.rect = self
            .rect()
            .and_then(|r| r.grow(dw, dh).intersect(self.bounds));
    }

    fn creating_rect(&self, anchor: Point, p: Point, square: bool) -> Option<Rect> {
        let rect = if square {
            Rect::square_from(anchor, p)
        } else {
            Rect::from_corners(anchor, p)
        };
        rect.intersect(self.bounds)
    }
}

/// `original` with the edges named by `handle` moved to `p` (flipping if dragged past).
fn resize(original: Rect, handle: Handle, p: Point) -> Rect {
    let (mut left, mut top, mut right, mut bottom) =
        (original.x, original.y, original.right(), original.bottom());
    match handle {
        Handle::TopLeft => (left, top) = (p.x, p.y),
        Handle::Top => top = p.y,
        Handle::TopRight => (right, top) = (p.x, p.y),
        Handle::Right => right = p.x,
        Handle::BottomRight => (right, bottom) = (p.x, p.y),
        Handle::Bottom => bottom = p.y,
        Handle::BottomLeft => (left, bottom) = (p.x, p.y),
        Handle::Left => left = p.x,
    }
    Rect::from_edges(left, top, right, bottom)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOUNDS: Rect = Rect::new(-1920, 0, 4480, 1440);
    const NONE: DragModifiers = DragModifiers {
        square: false,
        reposition: false,
    };
    const GRAB: i32 = 6;

    fn drag(sel: &mut Selection, from: (i32, i32), to: (i32, i32)) -> Option<Rect> {
        sel.pointer_down(Point::new(from.0, from.1), GRAB);
        sel.pointer_up(Point::new(to.0, to.1), NONE)
    }

    fn selected(rect: Rect) -> Selection {
        let mut sel = Selection::new(BOUNDS);
        sel.set(rect);
        sel
    }

    #[test]
    fn dragging_creates_selection() {
        let mut sel = Selection::new(BOUNDS);
        assert_eq!(
            drag(&mut sel, (10, 10), (110, 60)),
            Some(Rect::new(10, 10, 100, 50))
        );
    }

    #[test]
    fn click_without_movement_selects_nothing() {
        let mut sel = Selection::new(BOUNDS);
        assert_eq!(drag(&mut sel, (10, 10), (10, 10)), None);
    }

    #[test]
    fn selection_can_span_monitors() {
        let mut sel = Selection::new(BOUNDS);
        assert_eq!(
            drag(&mut sel, (-100, 10), (100, 20)),
            Some(Rect::new(-100, 10, 200, 10))
        );
    }

    #[test]
    fn creation_is_clamped_to_desktop() {
        let mut sel = Selection::new(BOUNDS);
        let rect = drag(&mut sel, (2500, 1400), (9000, 9000)).unwrap_or_default();
        assert_eq!(rect, Rect::new(2500, 1400, 60, 40));
    }

    #[test]
    fn dragging_inside_moves_and_stays_in_bounds() {
        let mut sel = selected(Rect::new(0, 0, 100, 100));
        assert_eq!(
            drag(&mut sel, (50, 50), (60, -500)),
            Some(Rect::new(10, 0, 100, 100))
        );
    }

    #[test]
    fn dragging_corner_resizes_and_can_flip() {
        let mut sel = selected(Rect::new(100, 100, 100, 100));
        assert_eq!(
            drag(&mut sel, (200, 200), (50, 150)),
            Some(Rect::new(50, 100, 50, 50))
        );
    }

    #[test]
    fn dragging_edge_only_moves_that_edge() {
        let mut sel = selected(Rect::new(100, 100, 100, 100));
        assert_eq!(
            drag(&mut sel, (150, 100), (170, 80)),
            Some(Rect::new(100, 80, 100, 120))
        );
    }

    #[test]
    fn dragging_outside_starts_over() {
        let mut sel = selected(Rect::new(100, 100, 100, 100));
        assert_eq!(
            drag(&mut sel, (500, 500), (520, 530)),
            Some(Rect::new(500, 500, 20, 30))
        );
    }

    #[test]
    fn shift_constrains_to_square() {
        let mut sel = Selection::new(BOUNDS);
        sel.pointer_down(Point::new(0, 0), GRAB);
        let square = DragModifiers {
            square: true,
            ..NONE
        };
        assert_eq!(
            sel.pointer_up(Point::new(40, 90), square),
            Some(Rect::new(0, 0, 90, 90))
        );
    }

    #[test]
    fn space_repositions_selection_being_created() {
        let mut sel = Selection::new(BOUNDS);
        sel.pointer_down(Point::new(0, 0), GRAB);
        sel.pointer_move(Point::new(50, 50), NONE);
        let space = DragModifiers {
            reposition: true,
            ..NONE
        };
        sel.pointer_move(Point::new(80, 60), space);
        assert_eq!(
            sel.pointer_up(Point::new(80, 60), NONE),
            Some(Rect::new(30, 10, 50, 50))
        );
    }

    #[test]
    fn hit_test_finds_corners_edges_and_inside() {
        let sel = selected(Rect::new(100, 100, 100, 100));
        assert_eq!(
            sel.hit_test(Point::new(98, 103), GRAB),
            Hit::Handle(Handle::TopLeft)
        );
        assert_eq!(
            sel.hit_test(Point::new(150, 201), GRAB),
            Hit::Handle(Handle::Bottom)
        );
        assert_eq!(sel.hit_test(Point::new(150, 150), GRAB), Hit::Inside);
        assert_eq!(sel.hit_test(Point::new(300, 150), GRAB), Hit::Outside);
    }

    #[test]
    fn nudge_and_grow_respect_bounds() {
        let mut sel = selected(Rect::new(2550, 0, 10, 10));
        sel.nudge(100, -100);
        assert_eq!(sel.rect(), Some(Rect::new(2550, 0, 10, 10)));
        sel.grow(-50, 5);
        assert_eq!(sel.rect(), Some(Rect::new(2550, 0, 1, 15)));
    }
}
