//! The frame around text being typed or selected, with a corner handle that resizes the
//! text (its font size).

use cf_core::annotate::{Annotation, Shape, Vec2};
use cf_render::{Canvas, Color, LineDash, RectF, Stroke};

use super::{Editor, render};

/// Space between the text and its frame, in window pixels.
const PAD: f32 = 4.0;
/// Side of the resize handle, in window pixels.
const HANDLE: f32 = 10.0;

impl Editor {
    /// The text showing a frame: the text being typed, else the selected text (as it
    /// looks right now, also while it is being moved or resized).
    fn framed_text(&self) -> Option<Annotation> {
        if let Some(item) = self.state.typing_item() {
            return Some(item);
        }
        let index = self.state.selected()?;
        let item = self
            .state
            .preview()
            .filter(|p| p.replaces == Some(index))
            .map(|p| p.item)
            .or_else(|| self.state.document().items().get(index).cloned())?;
        matches!(item.shape, Shape::Text { .. }).then_some(item)
    }

    /// Draws the frame and handle (image pixels, view transform set). Returns `false` when
    /// there is no framed text. Remembers the frame for hit-testing the handle.
    pub(super) fn paint_text_frame(&self, canvas: &Canvas<'_>, zoom: f32) -> bool {
        let frame = self
            .framed_text()
            .and_then(|item| render::text_frame(canvas, &item))
            .map(|r| r.inflate(PAD / zoom));
        self.text_frame.set(frame);
        let Some(frame) = frame else {
            return false;
        };
        let line = Stroke {
            width: 1.5 / zoom,
            color: render::accent(),
            dash: LineDash::Solid,
        };
        canvas.stroke_rect(frame, &line);
        let handle = handle(frame, zoom);
        canvas.fill_rect(handle, Color::rgba8(0xFF, 0xFF, 0xFF, 0xFF));
        canvas.stroke_rect(handle, &line);
        true
    }

    /// Whether `p` (image pixels) is on the resize handle.
    pub(super) fn over_text_handle(&self, p: Vec2, zoom: f32) -> bool {
        self.text_frame.get().is_some_and(|frame| {
            let r = handle(frame, zoom).inflate(3.0 / zoom);
            p.x >= r.x && p.x < r.right() && p.y >= r.y && p.y < r.bottom()
        })
    }

    /// A press on the handle starts resizing. Returns whether it did.
    pub(super) fn text_handle_press(&mut self, p: Vec2, zoom: f32) -> bool {
        if !self.over_text_handle(p, zoom) {
            return false;
        }
        if self.state.is_typing() {
            return self.state.begin_typing_resize(p);
        }
        self.state
            .selected()
            .is_some_and(|index| self.state.begin_text_resize(index, p))
    }
}

/// The handle square, centred on the frame's bottom-right corner.
fn handle(frame: RectF, zoom: f32) -> RectF {
    let side = HANDLE / zoom;
    RectF::new(
        frame.right() - side / 2.0,
        frame.bottom() - side / 2.0,
        side,
        side,
    )
}
