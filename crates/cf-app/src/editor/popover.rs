//! A small floating panel next to something on the image — selected text, a QR code or
//! barcode — with a short preview and a few buttons (window pixels).

use cf_core::annotate::Area;
use cf_render::{Canvas, Color, RectF};

use super::icons::ink;

const PAD: f32 = 8.0;
const GAP: f32 = 6.0;
const TITLE: f32 = 20.0;
const BUTTON_W: f32 = 104.0;
const BUTTON_H: f32 = 30.0;
const MIN_WIDTH: f32 = 240.0;
/// Characters of the preview shown before "…".
const MAX_CHARS: usize = 60;

/// What a popover button does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopAction {
    /// Copy this text to the clipboard (selected text, a code's content or link).
    Copy(String),
    /// Open this web link (already checked to be plain http/https).
    Open(String),
}

/// A popover anchored to an area of the image.
#[derive(Debug, Clone, PartialEq)]
pub struct Popover {
    /// Preview of what the buttons act on.
    pub title: String,
    pub actions: Vec<(&'static str, PopAction)>,
    /// What it points at, in image pixels.
    pub anchor: Area,
}

/// Where a popover and its buttons are, in window pixels.
pub struct Placed {
    pub panel: RectF,
    pub buttons: Vec<RectF>,
}

/// Places the panel under its anchor (`anchor` already in window pixels), or above when
/// there is no room below, kept inside the window.
pub fn place(popover: &Popover, anchor: RectF, window: (f32, f32), s: f32) -> Placed {
    #[allow(clippy::cast_precision_loss)] // a few buttons
    let count = popover.actions.len().max(1) as f32;
    // Wide enough for a readable preview even with a single button.
    let width = (PAD * 2.0 + BUTTON_W * count + GAP * (count - 1.0)).max(MIN_WIDTH) * s;
    let height = (PAD * 2.0 + TITLE + BUTTON_H) * s;
    let below = anchor.bottom() + GAP * s;
    let y = if below + height <= window.1 {
        below
    } else {
        (anchor.y - GAP * s - height).max(0.0)
    };
    let x = anchor.x.clamp(0.0, (window.0 - width).max(0.0));
    let panel = RectF::new(x, y, width, height);
    let buttons = (0..popover.actions.len())
        .map(|i| {
            #[allow(clippy::cast_precision_loss)]
            let offset = (BUTTON_W + GAP) * i as f32;
            RectF::new(
                x + (PAD + offset) * s,
                y + (PAD + TITLE) * s,
                BUTTON_W * s,
                BUTTON_H * s,
            )
        })
        .collect();
    Placed { panel, buttons }
}

/// Index of the button under `(x, y)`.
pub fn hit(placed: &Placed, (x, y): (f32, f32)) -> Option<usize> {
    placed
        .buttons
        .iter()
        .position(|r| x >= r.x && x < r.right() && y >= r.y && y < r.bottom())
}

/// Whether `(x, y)` is anywhere on the panel.
pub fn contains(placed: &Placed, (x, y): (f32, f32)) -> bool {
    let r = placed.panel;
    x >= r.x && x < r.right() && y >= r.y && y < r.bottom()
}

pub fn paint(
    canvas: &Canvas<'_>,
    popover: &Popover,
    placed: &Placed,
    hover: Option<usize>,
    s: f32,
) {
    canvas.fill_rounded(placed.panel, 8.0 * s, Color::rgba8(0x26, 0x26, 0x2C, 0xF5));
    let title = shorten(&popover.title);
    if let Some(block) = canvas.text_sized(&title, 12.5 * s, false) {
        let (x, y) = (placed.panel.x + PAD * s, placed.panel.y + PAD * s);
        canvas.push_clip(RectF::new(x, y, placed.panel.w - 2.0 * PAD * s, TITLE * s));
        canvas.draw_text(&block, x, y, Color::rgba8(0xA0, 0xA0, 0xAA, 0xFF));
        canvas.pop_clip();
    }
    for (index, ((label, _), rect)) in popover.actions.iter().zip(&placed.buttons).enumerate() {
        let fill = if hover == Some(index) {
            Color::rgba8(0x4A, 0x4A, 0x56, 0xFF)
        } else {
            Color::rgba8(0x3A, 0x3A, 0x44, 0xFF)
        };
        canvas.fill_rounded(*rect, 6.0 * s, fill);
        if let Some(block) = canvas.text(label) {
            let x = rect.x + (rect.w - block.width) / 2.0;
            let y = rect.y + (rect.h - block.height) / 2.0;
            canvas.draw_text(&block, x, y, ink());
        }
    }
}

/// The content on one line, cut to [`MAX_CHARS`].
pub fn shorten(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if flat.chars().count() <= MAX_CHARS {
        return flat;
    }
    let mut short: String = flat.chars().take(MAX_CHARS - 1).collect();
    short.push('…');
    short
}

#[cfg(test)]
mod tests {
    use cf_core::annotate::Vec2;

    use super::*;

    fn popover(actions: usize) -> Popover {
        Popover {
            title: "https://example.com".into(),
            actions: (0..actions)
                .map(|_| ("Copy", PopAction::Copy("x".into())))
                .collect(),
            anchor: Area::from_corners(Vec2::default(), Vec2::new(10.0, 10.0)),
        }
    }

    #[test]
    fn opens_below_or_above_and_stays_inside_the_window() {
        let window = (800.0, 600.0);
        let below = place(
            &popover(2),
            RectF::new(100.0, 100.0, 50.0, 20.0),
            window,
            1.0,
        );
        assert!(below.panel.y > 120.0, "below the anchor");
        let above = place(
            &popover(2),
            RectF::new(780.0, 580.0, 10.0, 10.0),
            window,
            1.0,
        );
        assert!(
            above.panel.bottom() <= 580.0,
            "above when there's no room below"
        );
        assert!(above.panel.right() <= 800.0, "kept inside horizontally");
        let first = above.buttons[0];
        assert_eq!(hit(&above, (first.x + 1.0, first.y + 1.0)), Some(0));
        assert!(contains(&above, (above.panel.x + 1.0, above.panel.y + 1.0)));
        assert_eq!(hit(&above, (0.0, 0.0)), None);
    }

    #[test]
    fn long_previews_are_shortened_to_one_line() {
        let long = format!("line1\nline2 {}", "x".repeat(100));
        let short = shorten(&long);
        assert_eq!(short.chars().count(), MAX_CHARS);
        assert!(short.ends_with('…') && !short.contains('\n'));
    }
}
