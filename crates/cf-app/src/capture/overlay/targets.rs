//! What a click would pick in the snapping modes: a window, a UI element, or a browser's
//! page area. Windows come from the snapshot taken with the frozen desktop (instant);
//! elements and documents come from the background [`Picker`].

use std::collections::HashMap;

use cf_core::capture_mode::CaptureMode;
use cf_core::geometry::{Point, Rect};
use cf_core::targets::{ElementPath, WindowInfo, WindowSnapshot};

use super::picker::{Answer, Picker, Query, Waker};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Document {
    Pending,
    Known(Option<Rect>),
}

pub struct Targets {
    windows: WindowSnapshot,
    waker: Waker,
    picker: Option<Picker>,
    picker_failed: bool,
    element: Option<(WindowInfo, ElementPath)>,
    documents: HashMap<usize, Document>,
}

impl Targets {
    pub fn new(windows: WindowSnapshot, waker: Waker) -> Self {
        Self {
            windows,
            waker,
            picker: None,
            picker_failed: false,
            element: None,
            documents: HashMap::new(),
        }
    }

    /// Whether `mode` snaps to things under the pointer.
    pub const fn snaps(mode: CaptureMode) -> bool {
        matches!(
            mode,
            CaptureMode::FreeSelect
                | CaptureMode::Window
                | CaptureMode::Element
                | CaptureMode::BrowserContent
        )
    }

    /// The pointer moved to `p`: start any lookup the mode needs.
    pub fn hover(&mut self, mode: CaptureMode, p: Point) {
        let Some(window) = self.windows.at(p) else {
            return;
        };
        match mode {
            CaptureMode::Element => self.ask(Query::Element { window, point: p }),
            CaptureMode::BrowserContent if !self.documents.contains_key(&window.id) => {
                self.documents.insert(window.id, Document::Pending);
                self.ask(Query::Document { window });
            }
            _ => {}
        }
    }

    /// What a click at `p` would pick, or `None` over bare desktop / in other modes. Until a
    /// lookup has answered, the whole window stands in.
    pub fn candidate(&self, mode: CaptureMode, p: Point) -> Option<Rect> {
        let window = self.windows.at(p)?;
        let rect = match mode {
            CaptureMode::FreeSelect | CaptureMode::Window => window.rect,
            CaptureMode::Element => self
                .element
                .as_ref()
                .filter(|(w, _)| w.id == window.id)
                .map_or(window.rect, |(_, path)| path.current()),
            CaptureMode::BrowserContent => match self.documents.get(&window.id) {
                Some(Document::Known(Some(page))) => *page,
                _ => window.rect,
            },
            _ => return None,
        };
        Some(rect)
    }

    /// Takes in finished lookups. Returns whether any arrived.
    pub fn poll(&mut self) -> bool {
        let Some(picker) = &self.picker else {
            return false;
        };
        let answers: Vec<Answer> = picker.answers().collect();
        for answer in &answers {
            self.apply(answer);
        }
        !answers.is_empty()
    }

    /// Scroll wheel in element mode: `outward` selects the parent, otherwise the child.
    pub fn walk(&mut self, outward: bool) -> bool {
        self.element.as_mut().is_some_and(
            |(_, path)| {
                if outward { path.outer() } else { path.inner() }
            },
        )
    }

    fn apply(&mut self, answer: &Answer) {
        match answer {
            Answer::Element { window, rects } => {
                let Some(path) = ElementPath::new(rects.iter().copied(), window.rect) else {
                    return;
                };
                match &mut self.element {
                    Some((w, current)) if w.id == window.id => current.replace(path),
                    _ => self.element = Some((*window, path)),
                }
            }
            Answer::Document { window, rect } => {
                let page = rect.and_then(|r| r.intersect(window.rect));
                self.documents.insert(window.id, Document::Known(page));
            }
        }
    }

    fn ask(&mut self, query: Query) {
        if self.picker.is_none() && !self.picker_failed {
            match Picker::spawn(self.waker.clone()) {
                Ok(picker) => self.picker = Some(picker),
                Err(err) => {
                    tracing::warn!("could not start the element picker: {err}");
                    self.picker_failed = true;
                }
            }
        }
        if let Some(picker) = &self.picker {
            picker.ask(query);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn targets() -> Targets {
        let windows = WindowSnapshot::new(vec![
            WindowInfo {
                id: 1,
                rect: Rect::new(100, 100, 200, 200),
            },
            WindowInfo {
                id: 2,
                rect: Rect::new(0, 0, 1000, 800),
            },
        ]);
        Targets::new(windows, Arc::new(|| {}))
    }

    #[test]
    fn window_modes_pick_the_topmost_window() {
        let t = targets();
        let p = Point::new(150, 150);
        assert_eq!(
            t.candidate(CaptureMode::Window, p),
            Some(Rect::new(100, 100, 200, 200))
        );
        assert_eq!(
            t.candidate(CaptureMode::FreeSelect, Point::new(500, 500)),
            Some(Rect::new(0, 0, 1000, 800))
        );
        assert_eq!(t.candidate(CaptureMode::Rectangle, p), None);
        assert_eq!(
            t.candidate(CaptureMode::Window, Point::new(5000, 5000)),
            None
        );
    }

    #[test]
    fn element_answers_replace_the_window_and_scroll_walks_outward() {
        let mut t = targets();
        let p = Point::new(150, 150);
        let window = t.windows.at(p).map(|w| (w.id, w.rect));
        assert_eq!(window, Some((1, Rect::new(100, 100, 200, 200))));
        t.apply(&Answer::Element {
            window: WindowInfo {
                id: 1,
                rect: Rect::new(100, 100, 200, 200),
            },
            rects: vec![Rect::new(140, 140, 20, 20), Rect::new(100, 100, 200, 200)],
        });
        assert_eq!(
            t.candidate(CaptureMode::Element, p),
            Some(Rect::new(140, 140, 20, 20))
        );
        assert!(t.walk(true));
        assert_eq!(
            t.candidate(CaptureMode::Element, p),
            Some(Rect::new(100, 100, 200, 200))
        );
        assert!(!t.walk(true));
    }

    #[test]
    fn browser_content_uses_the_document_once_known() {
        let mut t = targets();
        let p = Point::new(500, 500);
        assert_eq!(
            t.candidate(CaptureMode::BrowserContent, p),
            Some(Rect::new(0, 0, 1000, 800))
        );
        t.apply(&Answer::Document {
            window: WindowInfo {
                id: 2,
                rect: Rect::new(0, 0, 1000, 800),
            },
            rect: Some(Rect::new(0, 90, 1000, 2000)),
        });
        assert_eq!(
            t.candidate(CaptureMode::BrowserContent, p),
            Some(Rect::new(0, 90, 1000, 710))
        );
    }
}
