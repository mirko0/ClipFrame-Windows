//! Things the overlay can snap a selection to: top-level windows and UI elements.
//!
//! The platform layer supplies raw rectangles (a z-ordered window list, the chain of UI
//! elements under a point); this module decides which one the pointer means.

use crate::geometry::{Point, Rect};

/// A top-level window at the moment the desktop was frozen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowInfo {
    /// Opaque platform handle (an `HWND` on Windows).
    pub id: usize,
    /// Visible bounds in virtual-desktop pixels.
    pub rect: Rect,
}

/// The visible top-level windows, topmost first.
#[derive(Debug, Clone, Default)]
pub struct WindowSnapshot {
    windows: Vec<WindowInfo>,
}

impl WindowSnapshot {
    /// A snapshot from windows listed topmost first; empty rectangles are dropped.
    pub fn new(top_first: Vec<WindowInfo>) -> Self {
        let windows = top_first
            .into_iter()
            .filter(|w| !w.rect.is_empty())
            .collect();
        Self { windows }
    }

    /// The topmost window containing `p`.
    pub fn at(&self, p: Point) -> Option<WindowInfo> {
        self.windows.iter().copied().find(|w| w.rect.contains(p))
    }

    /// Number of windows in the snapshot.
    pub fn len(&self) -> usize {
        self.windows.len()
    }

    /// Whether the snapshot holds no windows.
    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }
}

/// The nested UI elements under a point, innermost first, with a chosen nesting level
/// (the scroll wheel walks outward to parents and back inward).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementPath {
    rects: Vec<Rect>,
    level: usize,
}

impl ElementPath {
    /// Builds a path from element bounds listed innermost first. Each one is clipped to
    /// `clip` (its window, on screen); empty and repeated rectangles are dropped, since
    /// stepping to a parent should always change what is selected.
    pub fn new(innermost_first: impl IntoIterator<Item = Rect>, clip: Rect) -> Option<Self> {
        let mut rects: Vec<Rect> = Vec::new();
        for rect in innermost_first {
            let Some(rect) = rect.intersect(clip) else {
                continue;
            };
            if !rects.contains(&rect) {
                rects.push(rect);
            }
        }
        (!rects.is_empty()).then_some(Self { rects, level: 0 })
    }

    /// The element at the chosen level.
    pub fn current(&self) -> Rect {
        self.rects[self.level]
    }

    /// The innermost element (what the pointer is directly over).
    pub fn innermost(&self) -> Rect {
        self.rects[0]
    }

    /// The outermost element (normally the whole window).
    pub fn outermost(&self) -> Rect {
        self.rects[self.rects.len() - 1]
    }

    /// Steps to the parent element. Returns `false` at the outermost one.
    pub fn outer(&mut self) -> bool {
        let moved = self.level + 1 < self.rects.len();
        if moved {
            self.level += 1;
        }
        moved
    }

    /// Steps back to the child element. Returns `false` at the innermost one.
    pub fn inner(&mut self) -> bool {
        let moved = self.level > 0;
        if moved {
            self.level -= 1;
        }
        moved
    }

    /// Replaces this path with `next`, keeping the chosen level when the pointer is still
    /// over the same innermost element (so small pointer movements don't undo scrolling).
    pub fn replace(&mut self, next: Self) {
        let level = if next.innermost() == self.innermost() {
            self.level.min(next.rects.len() - 1)
        } else {
            0
        };
        *self = Self { level, ..next };
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    const SCREEN: Rect = Rect::new(0, 0, 1000, 1000);

    fn window(id: usize, x: i32, y: i32, w: i32, h: i32) -> WindowInfo {
        WindowInfo {
            id,
            rect: Rect::new(x, y, w, h),
        }
    }

    #[test]
    fn topmost_window_under_the_point_wins() {
        let snapshot = WindowSnapshot::new(vec![
            window(1, 100, 100, 100, 100),
            window(2, 0, 0, 500, 500),
        ]);
        assert_eq!(snapshot.at(Point::new(150, 150)).unwrap().id, 1);
        assert_eq!(snapshot.at(Point::new(300, 300)).unwrap().id, 2);
        assert_eq!(snapshot.at(Point::new(600, 600)), None);
    }

    #[test]
    fn empty_windows_are_ignored() {
        let snapshot = WindowSnapshot::new(vec![window(1, 0, 0, 0, 0), window(2, 0, 0, 9, 9)]);
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot.at(Point::new(0, 0)).unwrap().id, 2);
    }

    fn path() -> ElementPath {
        ElementPath::new(
            [
                Rect::new(10, 10, 5, 5),
                Rect::new(0, 0, 50, 50),
                Rect::new(0, 0, 50, 50), // same as its child: dropped
                Rect::new(0, 0, 100, 100),
            ],
            SCREEN,
        )
        .unwrap()
    }

    #[test]
    fn path_drops_duplicates_and_walks_both_ways() {
        let mut path = path();
        assert_eq!(path.current(), Rect::new(10, 10, 5, 5));
        assert!(path.outer());
        assert!(path.outer());
        assert_eq!(path.current(), Rect::new(0, 0, 100, 100));
        assert!(!path.outer());
        assert!(path.inner());
        assert_eq!(path.current(), Rect::new(0, 0, 50, 50));
        assert_eq!(path.outermost(), Rect::new(0, 0, 100, 100));
    }

    #[test]
    fn path_is_clipped_to_the_window() {
        let clip = Rect::new(0, 0, 40, 40);
        let path = ElementPath::new([Rect::new(500, 500, 9, 9), Rect::new(0, 0, 100, 100)], clip);
        assert_eq!(path.unwrap().current(), clip);
        assert_eq!(ElementPath::new([Rect::new(500, 500, 9, 9)], clip), None);
    }

    #[test]
    fn replacing_keeps_the_level_over_the_same_element() {
        let mut current = path();
        current.outer();
        current.replace(path());
        assert_eq!(current.current(), Rect::new(0, 0, 50, 50));

        let other = ElementPath::new([Rect::new(60, 60, 5, 5), Rect::new(0, 0, 100, 100)], SCREEN);
        current.replace(other.unwrap());
        assert_eq!(current.current(), Rect::new(60, 60, 5, 5));
    }
}
