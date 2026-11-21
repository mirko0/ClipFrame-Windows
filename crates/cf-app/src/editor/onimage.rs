//! Using what was recognised in the image, with the Select tool: drag across text to select
//! it (Ctrl+C or the popover copies it), click a QR code or barcode to open or copy its
//! content. Annotations under the pointer still take priority (they get moved instead).

use cf_core::annotate::{Area, Tool, Vec2};
use cf_detect::link;
use cf_render::{Canvas, Color, LineDash, RectF, Stroke};
use winit::window::CursorIcon;

use super::popover::{self, Placed, PopAction, Popover};
use super::{Editor, rect_of};

/// Extra grab distance around words, in window pixels.
const WORD_SLOP: f32 = 3.0;

/// Text selection and popover state.
#[derive(Default)]
pub struct Pick {
    /// Selected words: anchor and focus, indices in reading order.
    selection: Option<(usize, usize)>,
    dragging: bool,
    popover: Option<Popover>,
    popover_hover: Option<usize>,
}

impl Editor {
    /// A press on the popover: runs a button, or just keeps the click. A press elsewhere
    /// closes it. Returns whether the press was used.
    pub(super) fn popover_press(&mut self, p: (f32, f32)) -> bool {
        let Some(placed) = self.placed_popover() else {
            return false;
        };
        if !popover::contains(&placed, p) {
            self.pick.popover = None;
            return false;
        }
        let action = popover::hit(&placed, p).and_then(|i| {
            let popover = self.pick.popover.as_ref()?;
            popover.actions.get(i).map(|(_, a)| a.clone())
        });
        if let Some(action) = action {
            self.pick.popover = None;
            self.run_pop_action(action);
        }
        true
    }

    /// A Select-tool press on the image at `p` (image pixels). Returns whether it was used
    /// for text or a code; `false` lets the editor select/move annotations as usual.
    pub(super) fn image_press(&mut self, p: Vec2, zoom: f32) -> bool {
        if self.state.tool() != Tool::Select || self.state.item_at(p, 1.0 / zoom).is_some() {
            self.pick.selection = None;
            return false;
        }
        if let Some(index) = self.code_at(p) {
            self.open_code_popover(index);
            return true;
        }
        let word = self
            .smart
            .text()
            .and_then(|t| t.word_at(p, WORD_SLOP / zoom));
        self.pick.selection = word.map(|w| (w, w));
        self.pick.dragging = word.is_some();
        word.is_some()
    }

    /// Extends a text selection being dragged.
    pub(super) fn image_drag(&mut self, p: Vec2) {
        if !self.pick.dragging {
            return;
        }
        let nearest = self.smart.text().and_then(|t| t.nearest_word(p));
        if let (Some((anchor, _)), Some(focus)) = (self.pick.selection, nearest) {
            self.pick.selection = Some((anchor, focus));
        }
    }

    /// Ends a text drag; offers to copy the selection. Returns whether a drag ended.
    pub(super) fn image_release(&mut self) -> bool {
        if !std::mem::take(&mut self.pick.dragging) {
            return false;
        }
        if let Some((text, anchor)) = self.selected_text() {
            self.pick.popover = Some(Popover {
                title: text.clone(),
                actions: vec![("Copy text", PopAction::Copy(text))],
                anchor,
            });
        }
        true
    }

    /// Ctrl+C with text selected copies the text. Returns whether it did.
    pub(super) fn copy_selection(&mut self) -> bool {
        let Some((text, _)) = self.selected_text() else {
            return false;
        };
        self.run_pop_action(PopAction::Copy(text));
        true
    }

    /// Esc: drops the popover and text selection first. Returns whether there was any.
    pub(super) fn clear_pick(&mut self) -> bool {
        let had = self.pick.popover.is_some() || self.pick.selection.is_some();
        self.pick = Pick::default();
        had
    }

    /// The pointer for the Select tool over text (I-beam) or a code (hand).
    pub(super) fn pick_cursor(&self, p: Vec2, zoom: f32) -> Option<CursorIcon> {
        if self.state.tool() != Tool::Select || self.state.item_at(p, 1.0 / zoom).is_some() {
            return None;
        }
        if self.code_at(p).is_some() {
            return Some(CursorIcon::Pointer);
        }
        let over_text = self
            .smart
            .text()
            .is_some_and(|t| t.word_at(p, WORD_SLOP / zoom).is_some());
        (over_text || self.pick.dragging).then_some(CursorIcon::Text)
    }

    pub(super) fn popover_hover(&mut self, p: (f32, f32)) {
        self.pick.popover_hover = self
            .placed_popover()
            .and_then(|placed| popover::hit(&placed, p));
    }

    /// Selection highlights and code outlines, drawn in image pixels.
    pub(super) fn paint_pick(&self, canvas: &Canvas<'_>, zoom: f32) {
        if let (Some((a, b)), Some(text)) = (self.pick.selection, self.smart.text()) {
            for area in text.areas_between(a, b) {
                canvas.fill_rect(
                    rect_of(area).inflate(1.0 / zoom),
                    Color::rgba8(0x4F, 0xA3, 0xFF, 0x60),
                );
            }
        }
        if self.state.tool() == Tool::Select {
            let outline = Stroke {
                width: 1.5 / zoom,
                color: Color::rgba8(0x4F, 0xA3, 0xFF, 0xB0),
                dash: LineDash::Dashed,
            };
            for area in self.smart.codes().iter().filter_map(|c| c.area) {
                canvas.stroke_rect(rect_of(area), &outline);
            }
        }
    }

    /// The popover, drawn in window pixels.
    pub(super) fn paint_popover(&self, canvas: &Canvas<'_>) {
        if let (Some(popover), Some(placed)) = (&self.pick.popover, self.placed_popover()) {
            popover::paint(
                canvas,
                popover,
                &placed,
                self.pick.popover_hover,
                self.scale(),
            );
        }
    }

    fn code_at(&self, p: Vec2) -> Option<usize> {
        self.smart
            .codes()
            .iter()
            .position(|c| c.area.is_some_and(|a| a.contains(p)))
    }

    fn open_code_popover(&mut self, index: usize) {
        let Some(code) = self.smart.codes().get(index) else {
            return;
        };
        let actions = match link::web_url(&code.text) {
            Some(url) => vec![
                ("Open link", PopAction::Open(url.to_owned())),
                ("Copy link", PopAction::Copy(url.to_owned())),
            ],
            None => vec![("Copy value", PopAction::Copy(code.text.clone()))],
        };
        self.pick.selection = None;
        self.pick.popover = Some(Popover {
            title: code.text.clone(),
            actions,
            anchor: code.area.unwrap_or_else(|| self.whole()),
        });
    }

    /// The selected text and the area it covers.
    fn selected_text(&self) -> Option<(String, Area)> {
        let (a, b) = self.pick.selection?;
        let text = self.smart.text()?;
        let areas = text.areas_between(a, b);
        let first = *areas.first()?;
        let area = areas.iter().fold(first, |acc, r| Area {
            min: Vec2::new(acc.min.x.min(r.min.x), acc.min.y.min(r.min.y)),
            max: Vec2::new(acc.max.x.max(r.max.x), acc.max.y.max(r.max.y)),
        });
        Some((text.text_between(a, b), area))
    }

    fn run_pop_action(&mut self, action: PopAction) {
        match action {
            PopAction::Copy(text) => {
                let message = match cf_platform::clipboard::set_text(&text) {
                    Ok(()) => "Copied",
                    Err(_) => "The clipboard is busy",
                };
                self.say(message);
            }
            PopAction::Open(url) => {
                if let Err(err) = cf_platform::shell::open_url(&url) {
                    tracing::warn!("could not open the link: {err}");
                }
                self.say("Opened in the browser");
            }
        }
    }

    /// Where the popover is on screen: its anchor mapped through the view.
    fn placed_popover(&self) -> Option<Placed> {
        let popover = self.pick.popover.as_ref()?;
        let view = self.view();
        let anchor = rect_of(popover.anchor);
        let on_screen = RectF::new(
            anchor.x * view.zoom + view.offset.0,
            anchor.y * view.zoom + view.offset.1,
            anchor.w * view.zoom,
            anchor.h * view.zoom,
        );
        Some(popover::place(
            popover,
            on_screen,
            self.window_size(),
            self.scale(),
        ))
    }
}
