//! How pointer and keyboard input change the document: the active tool, the style for new
//! annotations, the selection, and gestures in progress.

use super::document::{Document, Edit};
use super::hit;
use super::shape::{Annotation, Shape, Style, Vec2};

/// Pen points closer than this to the previous one are skipped.
const PEN_MIN_STEP: f32 = 1.0;

/// The active editor tool.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tool {
    /// Select, move, restyle and delete annotations.
    Select,
    /// Freehand.
    #[default]
    Pen,
    /// Straight line.
    Line,
    /// Arrow.
    Arrow,
    /// Rectangle.
    Rect,
    /// Ellipse.
    Ellipse,
    /// Text.
    Text,
    /// Numbered step badge.
    Step,
}

#[derive(Debug, Clone)]
enum Gesture {
    Drawing(Shape),
    Moving {
        index: usize,
        grab: Vec2,
        before: Annotation,
        current: Annotation,
    },
}

/// Text being typed; becomes an annotation when committed.
#[derive(Debug, Clone, PartialEq)]
pub struct TextDraft {
    at: Vec2,
    text: String,
    style: Style,
    /// The existing text annotation being edited, if any.
    editing: Option<(usize, Annotation)>,
}

/// What to draw on top of the document while a gesture is in progress.
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    /// Index of the document item this replaces (hidden while previewing), if any.
    pub replaces: Option<usize>,
    /// The annotation as it currently looks.
    pub item: Annotation,
    /// Whether it is text being typed (draw a caret).
    pub typing: bool,
}

/// The editor's input state over a [`Document`].
#[derive(Debug, Clone, Default)]
pub struct EditorState {
    doc: Document,
    tool: Tool,
    style: Style,
    gesture: Option<Gesture>,
    selected: Option<usize>,
    draft: Option<TextDraft>,
}

impl EditorState {
    /// An empty document with the default tool and style.
    pub fn new() -> Self {
        Self::default()
    }

    /// The document.
    pub const fn document(&self) -> &Document {
        &self.doc
    }

    /// The active tool.
    pub const fn tool(&self) -> Tool {
        self.tool
    }

    /// The style used for new annotations.
    pub const fn style(&self) -> Style {
        self.style
    }

    /// The selected annotation (select tool).
    pub const fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Whether text is being typed (keys should go to the text).
    pub const fn is_typing(&self) -> bool {
        self.draft.is_some()
    }

    /// Switches tools, finishing any text being typed.
    pub fn set_tool(&mut self, tool: Tool) {
        self.commit_text();
        self.gesture = None;
        if tool != Tool::Select {
            self.selected = None;
        }
        self.tool = tool;
    }

    /// Changes the style for new annotations, and restyles the selection or the text being
    /// typed.
    pub fn set_style(&mut self, style: Style) {
        self.style = style;
        if let Some(draft) = &mut self.draft {
            draft.style = style;
        }
        let Some(index) = self.selected else { return };
        let Some(before) = self.doc.items().get(index).cloned() else {
            return;
        };
        if before.style != style {
            let after = Annotation {
                style,
                ..before.clone()
            };
            self.doc.apply(Edit::Replace {
                index,
                before,
                after,
            });
        }
    }

    /// Primary button pressed at `p`. `slop_scale` widens hit-testing when zoomed out.
    pub fn pointer_down(&mut self, p: Vec2, slop_scale: f32) {
        self.commit_text();
        match self.tool {
            Tool::Select => self.start_move(p, slop_scale),
            Tool::Text => self.start_text(p, slop_scale),
            Tool::Step => self.insert(Annotation {
                shape: Shape::Step { at: p },
                style: self.style,
            }),
            Tool::Pen => self.gesture = Some(Gesture::Drawing(Shape::Pen(vec![p]))),
            Tool::Line => self.start_drawing(Shape::Line { from: p, to: p }),
            Tool::Arrow => self.start_drawing(Shape::Arrow { from: p, to: p }),
            Tool::Rect => self.start_drawing(Shape::Rect { a: p, b: p }),
            Tool::Ellipse => self.start_drawing(Shape::Ellipse { a: p, b: p }),
        }
    }

    /// Pointer moved to `p` (with the button held or not).
    pub fn pointer_move(&mut self, p: Vec2) {
        match &mut self.gesture {
            Some(Gesture::Drawing(shape)) => extend(shape, p),
            Some(Gesture::Moving {
                grab,
                before,
                current,
                ..
            }) => {
                let mut moved = before.clone();
                moved.shape.translate(p.minus(*grab));
                *current = moved;
            }
            None => {}
        }
    }

    /// Primary button released at `p`: finishes the gesture as one undoable edit.
    pub fn pointer_up(&mut self, p: Vec2) {
        self.pointer_move(p);
        match self.gesture.take() {
            Some(Gesture::Drawing(shape)) if !shape.is_degenerate() => {
                self.insert(Annotation {
                    shape,
                    style: self.style,
                });
            }
            Some(Gesture::Moving {
                index,
                before,
                current,
                ..
            }) if current != before => self.doc.apply(Edit::Replace {
                index,
                before,
                after: current,
            }),
            _ => {}
        }
    }

    /// Types `text` into the text being edited.
    pub fn type_text(&mut self, text: &str) {
        if let Some(draft) = &mut self.draft {
            draft.text.extend(text.chars().filter(|c| !c.is_control()));
        }
    }

    /// Starts a new line in the text being edited.
    pub fn new_line(&mut self) {
        if let Some(draft) = &mut self.draft {
            draft.text.push('\n');
        }
    }

    /// Deletes the last character of the text being edited.
    pub fn backspace(&mut self) {
        if let Some(draft) = &mut self.draft {
            draft.text.pop();
        }
    }

    /// Finishes the text being typed (empty text is dropped, or deletes the edited text).
    pub fn commit_text(&mut self) {
        let Some(draft) = self.draft.take() else {
            return;
        };
        let item = Annotation {
            shape: Shape::Text {
                at: draft.at,
                text: draft.text,
            },
            style: draft.style,
        };
        let keep = !item.shape.is_degenerate();
        match (draft.editing, keep) {
            (Some((index, before)), true) if before != item => self.doc.apply(Edit::Replace {
                index,
                before,
                after: item,
            }),
            (Some((index, before)), false) => self.doc.apply(Edit::Remove {
                index,
                item: before,
            }),
            (None, true) => self.insert(item),
            _ => {}
        }
    }

    /// Escape: finishes typing, else drops the selection. Returns `false` if there was
    /// nothing to finish (the caller may then close the editor).
    pub fn escape(&mut self) -> bool {
        if self.draft.is_some() {
            self.commit_text();
            true
        } else {
            self.gesture = None;
            self.selected.take().is_some()
        }
    }

    /// Deletes the selected annotation.
    pub fn delete_selected(&mut self) {
        let Some(index) = self.selected.take() else {
            return;
        };
        if let Some(item) = self.doc.items().get(index).cloned() {
            self.doc.apply(Edit::Remove { index, item });
        }
    }

    /// Undoes the latest change.
    pub fn undo(&mut self) -> bool {
        self.commit_text();
        self.gesture = None;
        self.selected = None;
        self.doc.undo()
    }

    /// Redoes the latest undone change.
    pub fn redo(&mut self) -> bool {
        self.commit_text();
        self.gesture = None;
        self.selected = None;
        self.doc.redo()
    }

    /// What to draw over the document for the gesture or text in progress.
    pub fn preview(&self) -> Option<Preview> {
        if let Some(draft) = &self.draft {
            return Some(Preview {
                replaces: draft.editing.as_ref().map(|(i, _)| *i),
                item: Annotation {
                    shape: Shape::Text {
                        at: draft.at,
                        text: draft.text.clone(),
                    },
                    style: draft.style,
                },
                typing: true,
            });
        }
        match self.gesture.as_ref()? {
            Gesture::Drawing(shape) => Some(Preview {
                replaces: None,
                item: Annotation {
                    shape: shape.clone(),
                    style: self.style,
                },
                typing: false,
            }),
            Gesture::Moving { index, current, .. } => Some(Preview {
                replaces: Some(*index),
                item: current.clone(),
                typing: false,
            }),
        }
    }

    fn start_drawing(&mut self, shape: Shape) {
        self.gesture = Some(Gesture::Drawing(shape));
    }

    fn start_move(&mut self, p: Vec2, slop_scale: f32) {
        self.selected = hit::topmost_at(self.doc.items(), p, slop_scale);
        if let Some(index) = self.selected {
            let item = self.doc.items()[index].clone();
            self.style = item.style;
            self.gesture = Some(Gesture::Moving {
                index,
                grab: p,
                before: item.clone(),
                current: item,
            });
        }
    }

    fn start_text(&mut self, p: Vec2, slop_scale: f32) {
        let existing = hit::topmost_at(self.doc.items(), p, slop_scale)
            .filter(|&i| matches!(self.doc.items()[i].shape, Shape::Text { .. }));
        self.draft = Some(match existing {
            Some(index) => {
                let item = self.doc.items()[index].clone();
                let Shape::Text { at, text } = &item.shape else {
                    unreachable!("filtered to text above");
                };
                self.style = item.style;
                TextDraft {
                    at: *at,
                    text: text.clone(),
                    style: item.style,
                    editing: Some((index, item)),
                }
            }
            None => TextDraft {
                at: p,
                text: String::new(),
                style: self.style,
                editing: None,
            },
        });
    }

    fn insert(&mut self, item: Annotation) {
        let index = self.doc.items().len();
        self.doc.apply(Edit::Insert { index, item });
    }
}

fn extend(shape: &mut Shape, p: Vec2) {
    match shape {
        Shape::Pen(points) => {
            if points
                .last()
                .is_none_or(|last| p.minus(*last).length() >= PEN_MIN_STEP)
            {
                points.push(p);
            }
        }
        Shape::Line { to, .. } | Shape::Arrow { to, .. } => *to = p,
        Shape::Rect { b, .. } | Shape::Ellipse { b, .. } => *b = p,
        Shape::Text { .. } | Shape::Step { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32) -> Vec2 {
        Vec2::new(x, y)
    }

    fn drag(state: &mut EditorState, from: Vec2, to: Vec2) {
        state.pointer_down(from, 1.0);
        state.pointer_move(v(f32::midpoint(from.x, to.x), f32::midpoint(from.y, to.y)));
        state.pointer_up(to);
    }

    #[test]
    fn drawing_a_rect_is_one_undoable_edit() {
        let mut s = EditorState::new();
        s.set_tool(Tool::Rect);
        drag(&mut s, v(10.0, 10.0), v(50.0, 40.0));
        assert_eq!(s.document().items().len(), 1);
        assert!(s.preview().is_none());
        assert!(s.undo());
        assert!(s.document().items().is_empty());
    }

    #[test]
    fn a_click_with_a_drawing_tool_adds_nothing() {
        let mut s = EditorState::new();
        s.set_tool(Tool::Arrow);
        drag(&mut s, v(10.0, 10.0), v(10.0, 10.0));
        assert!(s.document().items().is_empty());
    }

    #[test]
    fn pen_collects_points_and_previews_while_drawing() {
        let mut s = EditorState::new();
        s.pointer_down(v(0.0, 0.0), 1.0);
        s.pointer_move(v(0.5, 0.0)); // too close: skipped
        s.pointer_move(v(5.0, 0.0));
        let preview = s.preview().map(|p| p.item.shape);
        assert_eq!(preview, Some(Shape::Pen(vec![v(0.0, 0.0), v(5.0, 0.0)])));
        s.pointer_up(v(9.0, 0.0));
        assert_eq!(s.document().items().len(), 1);
    }

    #[test]
    fn select_tool_moves_and_deletes() {
        let mut s = EditorState::new();
        s.set_tool(Tool::Line);
        drag(&mut s, v(0.0, 0.0), v(100.0, 0.0));
        s.set_tool(Tool::Select);
        drag(&mut s, v(50.0, 1.0), v(50.0, 21.0));
        assert_eq!(s.selected(), Some(0));
        let moved = &s.document().items()[0].shape;
        assert_eq!(
            moved,
            &Shape::Line {
                from: v(0.0, 20.0),
                to: v(100.0, 20.0)
            }
        );
        s.delete_selected();
        assert!(s.document().items().is_empty());
        assert!(s.undo());
        assert_eq!(s.document().items().len(), 1);
    }

    #[test]
    fn restyling_the_selection_is_undoable() {
        let mut s = EditorState::new();
        s.set_tool(Tool::Step);
        s.pointer_down(v(10.0, 10.0), 1.0);
        s.set_tool(Tool::Select);
        s.pointer_down(v(10.0, 10.0), 1.0);
        s.pointer_up(v(10.0, 10.0));
        let wide = Style {
            width: 8.0,
            ..s.style()
        };
        s.set_style(wide);
        assert!((s.document().items()[0].style.width - 8.0).abs() < f32::EPSILON);
        s.undo();
        assert!((s.document().items()[0].style.width - 4.0).abs() < f32::EPSILON);
    }

    #[test]
    fn text_is_typed_committed_and_re_edited() {
        let mut s = EditorState::new();
        s.set_tool(Tool::Text);
        s.pointer_down(v(10.0, 10.0), 1.0);
        s.type_text("Hi");
        s.new_line();
        s.type_text("there\u{8}");
        assert!(s.preview().is_some_and(|p| p.typing));
        assert!(s.escape());
        let text = |s: &EditorState| match &s.document().items()[0].shape {
            Shape::Text { text, .. } => text.clone(),
            _ => String::new(),
        };
        assert_eq!(text(&s), "Hi\nthere");
        s.pointer_down(v(12.0, 12.0), 1.0); // click on it: edit
        s.backspace();
        s.commit_text();
        assert_eq!(text(&s), "Hi\nther");
        assert_eq!(s.document().items().len(), 1);
    }

    #[test]
    fn empty_text_is_dropped_and_escape_reports_nothing_to_do() {
        let mut s = EditorState::new();
        s.set_tool(Tool::Text);
        s.pointer_down(v(10.0, 10.0), 1.0);
        assert!(s.escape());
        assert!(s.document().items().is_empty());
        assert!(!s.escape());
    }
}
