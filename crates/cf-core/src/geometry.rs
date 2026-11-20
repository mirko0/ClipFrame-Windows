//! Integer geometry in **physical pixels on the virtual desktop** (the coordinate space that
//! spans every monitor; the primary monitor's top-left is `(0, 0)`, others may be negative).

/// A point in physical virtual-desktop pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Point {
    /// Horizontal position.
    pub x: i32,
    /// Vertical position.
    pub y: i32,
}

impl Point {
    /// Creates a point.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// The point moved by `(dx, dy)`.
    #[must_use]
    pub const fn offset(self, dx: i32, dy: i32) -> Self {
        Self::new(self.x.saturating_add(dx), self.y.saturating_add(dy))
    }
}

/// An axis-aligned rectangle in physical virtual-desktop pixels.
///
/// Covers `x..x + width` and `y..y + height`. Width and height are never negative.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct Rect {
    /// Left edge (inclusive).
    pub x: i32,
    /// Top edge (inclusive).
    pub y: i32,
    /// Width in pixels.
    pub width: i32,
    /// Height in pixels.
    pub height: i32,
}

impl Rect {
    /// Creates a rectangle; negative sizes are clamped to zero.
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        let width = if width < 0 { 0 } else { width };
        let height = if height < 0 { 0 } else { height };
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The rectangle spanned by two corners, in any order.
    pub fn from_corners(a: Point, b: Point) -> Self {
        Self::from_edges(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y))
    }

    /// The rectangle with the given edges (`right`/`bottom` exclusive), normalised if swapped.
    pub fn from_edges(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        let (l, r) = (left.min(right), left.max(right));
        let (t, b) = (top.min(bottom), top.max(bottom));
        Self::new(l, t, r - l, b - t)
    }

    /// A square anchored at `anchor` extending towards `towards`, sized by the longer axis.
    pub fn square_from(anchor: Point, towards: Point) -> Self {
        let dx = towards.x - anchor.x;
        let dy = towards.y - anchor.y;
        let side = dx.abs().max(dy.abs());
        let corner = Point::new(
            anchor.x + side * dx.signum_or_one(),
            anchor.y + side * dy.signum_or_one(),
        );
        Self::from_corners(anchor, corner)
    }

    /// Right edge (exclusive).
    pub const fn right(self) -> i32 {
        self.x + self.width
    }

    /// Bottom edge (exclusive).
    pub const fn bottom(self) -> i32 {
        self.y + self.height
    }

    /// Top-left corner.
    pub const fn origin(self) -> Point {
        Point::new(self.x, self.y)
    }

    /// Whether the rectangle covers no pixels.
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Whether `p` lies inside.
    pub const fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    /// The overlapping area, or `None` when the rectangles don't overlap.
    pub fn intersect(self, other: Self) -> Option<Self> {
        let r = Self::from_edges(
            self.x.max(other.x),
            self.y.max(other.y),
            self.right().min(other.right()),
            self.bottom().min(other.bottom()),
        );
        let overlaps = self.x.max(other.x) < self.right().min(other.right())
            && self.y.max(other.y) < self.bottom().min(other.bottom());
        overlaps.then_some(r)
    }

    /// The smallest rectangle containing both.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self::from_edges(
            self.x.min(other.x),
            self.y.min(other.y),
            self.right().max(other.right()),
            self.bottom().max(other.bottom()),
        )
    }

    /// The rectangle moved by `(dx, dy)`.
    #[must_use]
    pub const fn translate(self, dx: i32, dy: i32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    /// Moves (never resizes, unless larger than `bounds`) the rectangle to lie inside `bounds`.
    #[must_use]
    pub fn clamp_into(self, bounds: Self) -> Self {
        let width = self.width.min(bounds.width);
        let height = self.height.min(bounds.height);
        let x = self.x.clamp(bounds.x, bounds.right() - width);
        let y = self.y.clamp(bounds.y, bounds.bottom() - height);
        Self::new(x, y, width, height)
    }

    /// The rectangle grown by `(dw, dh)` from its top-left; shrinking stops at 1×1.
    #[must_use]
    pub fn grow(self, dw: i32, dh: i32) -> Self {
        Self::new(
            self.x,
            self.y,
            (self.width + dw).max(1),
            (self.height + dh).max(1),
        )
    }

    /// Number of pixels covered.
    pub fn area(self) -> i64 {
        i64::from(self.width) * i64::from(self.height)
    }
}

trait SignumOrOne {
    fn signum_or_one(self) -> Self;
}

impl SignumOrOne for i32 {
    fn signum_or_one(self) -> Self {
        if self < 0 { -1 } else { 1 }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn from_corners_normalises_any_drag_direction() {
        let r = Rect::from_corners(Point::new(50, 40), Point::new(10, 20));
        assert_eq!(r, Rect::new(10, 20, 40, 20));
    }

    #[test]
    fn negative_sizes_clamp_to_zero() {
        assert!(Rect::new(0, 0, -5, 10).is_empty());
    }

    #[test]
    fn contains_is_exclusive_at_right_and_bottom() {
        let r = Rect::new(0, 0, 10, 10);
        assert!(r.contains(Point::new(9, 9)));
        assert!(!r.contains(Point::new(10, 5)));
    }

    #[test]
    fn intersect_of_adjacent_monitors_is_none() {
        let left = Rect::new(-1920, 0, 1920, 1080);
        let right = Rect::new(0, 0, 2560, 1440);
        assert_eq!(left.intersect(right), None);
        assert_eq!(left.union(right), Rect::new(-1920, 0, 4480, 1440));
    }

    #[test]
    fn intersect_returns_overlap() {
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(50, 60, 100, 100);
        assert_eq!(a.intersect(b), Some(Rect::new(50, 60, 50, 40)));
    }

    #[test]
    fn clamp_into_moves_without_resizing() {
        let bounds = Rect::new(0, 0, 100, 100);
        assert_eq!(
            Rect::new(90, -5, 20, 20).clamp_into(bounds),
            Rect::new(80, 0, 20, 20)
        );
    }

    #[test]
    fn square_from_follows_drag_direction() {
        let sq = Rect::square_from(Point::new(100, 100), Point::new(70, 120));
        assert_eq!(sq, Rect::new(70, 100, 30, 30));
    }

    #[test]
    fn grow_never_collapses() {
        assert_eq!(Rect::new(0, 0, 3, 3).grow(-10, -10), Rect::new(0, 0, 1, 1));
    }

    fn any_rect() -> impl Strategy<Value = Rect> {
        (-5000..5000, -5000..5000, 0..4000, 0..4000).prop_map(|(x, y, w, h)| Rect::new(x, y, w, h))
    }

    proptest! {
        #[test]
        fn intersection_is_inside_both(a in any_rect(), b in any_rect()) {
            if let Some(i) = a.intersect(b) {
                prop_assert!(!i.is_empty());
                prop_assert_eq!(a.union(i), a);
                prop_assert_eq!(b.union(i), b);
            }
        }

        #[test]
        fn clamped_rect_lies_inside_bounds(r in any_rect(), b in any_rect()) {
            prop_assume!(!b.is_empty());
            let c = r.clamp_into(b);
            prop_assert_eq!(b.union(c), b);
        }

        #[test]
        fn square_is_square(ax in -3000..3000, ay in -3000..3000, bx in -3000..3000, by in -3000..3000) {
            let sq = Rect::square_from(Point::new(ax, ay), Point::new(bx, by));
            prop_assert_eq!(sq.width, sq.height);
        }
    }
}
