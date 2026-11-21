//! How pointer and keyboard input change the document: the active tool, each tool's style,
//! the selection, and gestures in progress.

mod crop;
mod grow;
mod text;

use self::grow::extend;
pub use self::text::TextDraft;
use super::document::{Document, Edit};
use super::hit;
use super::shape::{Annotation, Rgba, Shape, Style, Vec2};

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
    /// Highlighter (yellow by default).
    Highlighter,
    /// Blur a region.
    Blur,
    /// Pixelate a region (solid box when the style is filled).
    Redact,
    /// Cut a hole in the spotlight dim layer.
    Spotlight,
    /// Magnified inset of a region.
    Magnify,
    /// Crop the image.
    Crop,
}

impl Tool {
    /// Every tool, in declaration order.
    pub const ALL: [Self; 14] = [
        Self::Select,
        Self::Pen,
        Self::Line,
        Self::Arrow,
        Self::Rect,
        Self::Ellipse,
        Self::Text,
        Self::Step,
        Self::Highlighter,
        Self::Blur,
        Self::Redact,
        Self::Spotlight,
        Self::Magnify,
        Self::Crop,
    ];

    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    /// The tool that draws `shape` (its style options apply to it).
    pub const fn for_shape(shape: &Shape) -> Self {
        match shape {
            Shape::Pen(_) => Self::Pen,
            Shape::Highlight(_) => Self::Highlighter,
            Shape::Line { .. } => Self::Line,
            Shape::Arrow { .. } => Self::Arrow,
            Shape::Rect { .. } => Self::Rect,
            Shape::Ellipse { .. } => Self::Ellipse,
            Shape::Text { .. } => Self::Text,
            Shape::Step { .. } => Self::Step,
            Shape::Blur { .. } => Self::Blur,
            Shape::Redact { .. } => Self::Redact,
            Shape::Spotlight { .. } => Self::Spotlight,
            Shape::Magnify { .. } => Self::Magnify,
        }
    }
}

#[derive(Debug, Clone)]
enum Gesture {
    Drawing(Shape),
    Cropping {
        from: Vec2,
        to: Vec2,
    },
    Moving {
        index: usize,
        grab: Vec2,
        before: Annotation,
        current: Annotation,
    },
    /// Dragging a text's corner handle: the distance from its top-left corner scales the
    /// font size.
    Resizing {
        index: usize,
        grab: Vec2,
        before: Annotation,
        current: Annotation,
    },
    /// Dragging the corner handle of the text being typed (it stays being typed).
    ResizingDraft {
        grab: Vec2,
        size: f32,
    },
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
#[derive(Debug, Clone)]
pub struct EditorState {
    doc: Document,
    tool: Tool,
    /// Each tool remembers its own style (indexed like [`Tool::ALL`]).
    styles: [Style; 14],
    /// Image size (crops are clamped to it; zero = unknown).
    canvas: Vec2,
    gesture: Option<Gesture>,
    selected: Option<usize>,
    draft: Option<TextDraft>,
}

impl Default for EditorState {
    fn default() -> Self {
        let mut styles = [Style::default(); 14];
        styles[Tool::Highlighter.index()].color = Rgba::new(0xFD, 0xD8, 0x35, 0xFF);
        Self {
            doc: Document::default(),
            tool: Tool::default(),
            styles,
            canvas: Vec2::default(),
            gesture: None,
            selected: None,
            draft: None,
        }
    }
}

impl EditorState {
    /// An empty document with the default tool and style.
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty document over an image of `width × height` pixels.
    pub fn with_canvas(width: f32, height: f32) -> Self {
        Self {
            canvas: Vec2::new(width, height),
            ..Self::default()
        }
    }

    /// The image size given to [`Self::with_canvas`].
    pub const fn canvas(&self) -> Vec2 {
        self.canvas
    }

    /// The document.
    pub const fn document(&self) -> &Document {
        &self.doc
    }

    /// The active tool.
    pub const fn tool(&self) -> Tool {
        self.tool
    }

    /// The tool whose options apply now: the active tool, or with the Select tool the tool
    /// that drew the selection.
    pub fn options_tool(&self) -> Tool {
        match (self.tool, self.selected_item()) {
            (Tool::Select, Some(item)) => Tool::for_shape(&item.shape),
            (tool, _) => tool,
        }
    }

    /// The style shown and changed by the style controls: the selection's (Select tool) or
    /// the active tool's.
    pub fn style(&self) -> Style {
        match (self.tool, self.selected_item()) {
            (Tool::Select, Some(item)) => item.style,
            (tool, _) => self.styles[tool.index()],
        }
    }

    /// The selected annotation (select tool).
    pub const fn selected(&self) -> Option<usize> {
        self.selected
    }

    fn selected_item(&self) -> Option<&Annotation> {
        self.selected.and_then(|i| self.doc.items().get(i))
    }

    /// The topmost annotation under `p` (`slop_scale` widens the grab area when zoomed out).
    pub fn item_at(&self, p: Vec2, slop_scale: f32) -> Option<usize> {
        hit::topmost_at(self.doc.items(), p, slop_scale)
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

    /// Changes the style: of the selection (Select tool, an undoable edit, also remembered
    /// for its tool), or of the active tool (and the text being typed).
    pub fn set_style(&mut self, style: Style) {
        if self.tool == Tool::Select {
            self.restyle_selection(style);
            return;
        }
        self.styles[self.tool.index()] = style;
        if let Some(draft) = &mut self.draft {
            draft.set_style(style);
        }
    }

    fn restyle_selection(&mut self, style: Style) {
        let Some(index) = self.selected else { return };
        let Some(before) = self.doc.items().get(index).cloned() else {
            return;
        };
        self.styles[Tool::for_shape(&before.shape).index()] = style;
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
                style: self.style(),
            }),
            Tool::Pen => self.start_drawing(Shape::Pen(vec![p])),
            Tool::Highlighter => self.start_drawing(Shape::Highlight(vec![p])),
            Tool::Line => self.start_drawing(Shape::Line { from: p, to: p }),
            Tool::Arrow => self.start_drawing(Shape::Arrow { from: p, to: p }),
            Tool::Rect => self.start_drawing(Shape::Rect { a: p, b: p }),
            Tool::Ellipse => self.start_drawing(Shape::Ellipse { a: p, b: p }),
            Tool::Blur => self.start_drawing(Shape::Blur { a: p, b: p }),
            Tool::Redact => self.start_drawing(Shape::Redact { a: p, b: p }),
            Tool::Spotlight => self.start_drawing(Shape::Spotlight { a: p, b: p }),
            Tool::Magnify => self.start_drawing(Shape::Magnify { a: p, b: p, at: p }),
            Tool::Crop => self.gesture = Some(Gesture::Cropping { from: p, to: p }),
        }
    }

    /// Pointer moved to `p` (with the button held or not).
    pub fn pointer_move(&mut self, p: Vec2) {
        let canvas = self.canvas;
        match &mut self.gesture {
            Some(Gesture::Drawing(shape)) => extend(shape, p, canvas),
            Some(Gesture::Cropping { to, .. }) => *to = p,
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
            Some(Gesture::Resizing {
                grab,
                before,
                current,
                ..
            }) => *current = text::resized(before, *grab, p),
            Some(Gesture::ResizingDraft { grab, size }) => {
                if let Some(draft) = &mut self.draft {
                    draft.resize(*size, *grab, p);
                }
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
                    style: self.style(),
                });
            }
            Some(Gesture::Cropping { from, to }) => self.finish_crop(from, to),
            Some(
                Gesture::Moving {
                    index,
                    before,
                    current,
                    ..
                }
                | Gesture::Resizing {
                    index,
                    before,
                    current,
                    ..
                },
            ) if current != before => self.doc.apply(Edit::Replace {
                index,
                before,
                after: current,
            }),
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

    /// Deletes the selected annotation; with the crop tool, removes the crop.
    pub fn delete_selected(&mut self) {
        if self.tool == Tool::Crop {
            self.set_crop(None);
            return;
        }
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
            return Some(draft.preview());
        }
        match self.gesture.as_ref()? {
            Gesture::Drawing(shape) => Some(Preview {
                replaces: None,
                item: Annotation {
                    shape: shape.clone(),
                    style: self.style(),
                },
                typing: false,
            }),
            Gesture::Cropping { .. } | Gesture::ResizingDraft { .. } => None,
            Gesture::Moving { index, current, .. } | Gesture::Resizing { index, current, .. } => {
                Some(Preview {
                    replaces: Some(*index),
                    item: current.clone(),
                    typing: false,
                })
            }
        }
    }

    fn start_drawing(&mut self, shape: Shape) {
        self.gesture = Some(Gesture::Drawing(shape));
    }

    fn start_move(&mut self, p: Vec2, slop_scale: f32) {
        self.selected = hit::topmost_at(self.doc.items(), p, slop_scale);
        if let Some(index) = self.selected {
            let item = self.doc.items()[index].clone();
            self.gesture = Some(Gesture::Moving {
                index,
                grab: p,
                before: item.clone(),
                current: item,
            });
        }
    }

    /// Adds `items` on top as one undoable step; returns how many were added.
    pub fn add_items(&mut self, items: Vec<Annotation>) -> usize {
        self.commit_text();
        let start = self.doc.items().len();
        let count = items.len();
        let edits = items
            .into_iter()
            .enumerate()
            .map(|(offset, item)| Edit::Insert {
                index: start + offset,
                item,
            })
            .collect::<Vec<_>>();
        if !edits.is_empty() {
            self.doc.apply(Edit::Batch(edits));
        }
        count
    }

    fn insert(&mut self, item: Annotation) {
        let index = self.doc.items().len();
        self.doc.apply(Edit::Insert { index, item });
    }
}

#[cfg(test)]
mod tests;
