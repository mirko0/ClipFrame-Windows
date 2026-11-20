//! How pointer and keyboard input change the document: the active tool, the style for new
//! annotations, the selection, and gestures in progress.

mod grow;

use self::grow::extend;
use super::area::Area;
use super::document::{Document, Edit};
use super::hit;
use super::shape::{Annotation, Rgba, Shape, Style, Vec2};

/// Crops smaller than this (a click) are ignored.
const MIN_CROP: f32 = 8.0;

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
    /// Highlighter (has its own style, yellow by default).
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
#[derive(Debug, Clone)]
pub struct EditorState {
    doc: Document,
    tool: Tool,
    style: Style,
    /// The highlighter's own style.
    marker: Style,
    /// Image size (crops are clamped to it; zero = unknown).
    canvas: Vec2,
    gesture: Option<Gesture>,
    selected: Option<usize>,
    draft: Option<TextDraft>,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            doc: Document::default(),
            tool: Tool::default(),
            style: Style::default(),
            marker: Style {
                color: Rgba::new(0xFD, 0xD8, 0x35, 0xFF),
                ..Style::default()
            },
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

    /// The style used for new annotations (the highlighter keeps its own).
    pub fn style(&self) -> Style {
        if self.tool == Tool::Highlighter {
            self.marker
        } else {
            self.style
        }
    }

    /// The crop to show: the one being dragged, else the document's.
    pub fn crop_preview(&self) -> Option<Area> {
        match &self.gesture {
            Some(Gesture::Cropping { from, to }) => Some(self.crop_area(*from, *to)),
            _ => self.doc.crop(),
        }
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
        if self.tool == Tool::Highlighter {
            self.marker = style;
            return;
        }
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
                    style: self.style(),
                },
                typing: false,
            }),
            Gesture::Cropping { .. } => None,
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

    /// The crop dragged from `from` to `to`, inside the image and on whole pixels.
    fn crop_area(&self, from: Vec2, to: Vec2) -> Area {
        let area = Area::from_corners(from, to);
        let area = if self.canvas.x > 0.0 {
            area.clamp_to(self.canvas)
        } else {
            area
        };
        area.round()
    }

    fn finish_crop(&mut self, from: Vec2, to: Vec2) {
        let area = self.crop_area(from, to);
        if area.width() < MIN_CROP || area.height() < MIN_CROP {
            return;
        }
        let whole = Area::from_corners(Vec2::default(), self.canvas);
        self.set_crop((area != whole).then_some(area));
    }

    fn set_crop(&mut self, crop: Option<Area>) {
        let before = self.doc.crop();
        if before != crop {
            self.doc.apply(Edit::Crop {
                before,
                after: crop,
            });
        }
    }
}

#[cfg(test)]
mod tests;
