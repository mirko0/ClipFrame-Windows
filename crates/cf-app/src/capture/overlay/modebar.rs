//! The mode bar at the bottom centre of the overlay: one button per capture mode, each with
//! a keycap showing its shortcut. Clicking a button does what its key does.
//!
//! Layout uses fixed sizes (no text measuring), so hit-testing works without a canvas.
//! Coordinates are the window's physical pixels; `s` is the monitor's UI scale.

use cf_core::capture_mode::CaptureMode;
use cf_render::{Canvas, Color, RectF, Stroke};

use super::keys::Command;

const BUTTON_W: f32 = 108.0;
const BUTTON_H: f32 = 34.0;
const GAP: f32 = 4.0;
const PAD: f32 = 6.0;
const BOTTOM: f32 = 28.0;
const KEYCAP: f32 = 20.0;

/// The buttons, left to right: keycap letter, label, and what a click does.
pub const ITEMS: [(&str, &str, Command); 7] = [
    ("F", "Free", Command::SetMode(CaptureMode::FreeSelect)),
    ("R", "Rectangle", Command::SetMode(CaptureMode::Rectangle)),
    ("W", "Window", Command::SetMode(CaptureMode::Window)),
    ("E", "Element", Command::SetMode(CaptureMode::Element)),
    ("C", "Page", Command::SetMode(CaptureMode::BrowserContent)),
    ("M", "Monitor", Command::SetMode(CaptureMode::Monitor)),
    ("A", "All screens", Command::CaptureAll),
];

/// The bar's panel and its buttons on a `screen`-sized window.
pub fn layout(screen: (f32, f32), s: f32) -> (RectF, Vec<RectF>) {
    #[allow(clippy::cast_precision_loss)] // seven buttons
    let count = ITEMS.len() as f32;
    let width = (PAD * 2.0 + BUTTON_W * count + GAP * (count - 1.0)) * s;
    let height = (PAD * 2.0 + BUTTON_H) * s;
    let panel = RectF::new(
        ((screen.0 - width) / 2.0).round(),
        (screen.1 - BOTTOM * s - height).round(),
        width.round(),
        height.round(),
    );
    let buttons = (0..ITEMS.len())
        .map(|i| {
            #[allow(clippy::cast_precision_loss)]
            let offset = (BUTTON_W + GAP) * i as f32;
            RectF::new(
                panel.x + (PAD + offset) * s,
                panel.y + PAD * s,
                BUTTON_W * s,
                BUTTON_H * s,
            )
        })
        .collect();
    (panel, buttons)
}

fn inside(r: RectF, (x, y): (f32, f32)) -> bool {
    x >= r.x && x < r.right() && y >= r.y && y < r.bottom()
}

/// Where on the bar a point is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spot {
    /// On button `n` of [`ITEMS`].
    Button(usize),
    /// On the bar, between buttons (a click there does nothing).
    Panel,
}

/// Whether `p` is on the bar, and on which button (clicks there never start a selection).
pub fn spot(screen: (f32, f32), s: f32, p: (f32, f32)) -> Option<Spot> {
    let (panel, buttons) = layout(screen, s);
    if !inside(panel, p) {
        return None;
    }
    Some(
        buttons
            .iter()
            .position(|r| inside(*r, p))
            .map_or(Spot::Panel, Spot::Button),
    )
}

/// The button under `p`, by index into [`ITEMS`].
pub fn hit(screen: (f32, f32), s: f32, p: (f32, f32)) -> Option<usize> {
    layout(screen, s).1.iter().position(|r| inside(*r, p))
}

/// Draws the bar; `note` (what to do now) goes in a pill above it.
pub fn paint(
    canvas: &Canvas<'_>,
    s: f32,
    mode: CaptureMode,
    pointer: Option<(f32, f32)>,
    note: &str,
) {
    let screen = canvas.size();
    let (panel, buttons) = layout(screen, s);
    canvas.fill_rounded(panel, 9.0 * s, panel_bg());
    let hover = pointer.and_then(|p| hit(screen, s, p));
    for (index, ((letter, label, command), rect)) in ITEMS.iter().zip(&buttons).enumerate() {
        let active = *command == Command::SetMode(mode);
        if active || hover == Some(index) {
            let fill = if active { active_bg() } else { hover_bg() };
            canvas.fill_rounded(*rect, 6.0 * s, fill);
        }
        button(canvas, *rect, letter, label, s);
    }
    if let Some(block) = canvas.text(note).filter(|_| !note.is_empty()) {
        let (pad_x, pad_y) = (10.0 * s, 4.0 * s);
        let size = (block.width + 2.0 * pad_x, block.height + 2.0 * pad_y);
        let at = RectF::new(
            (screen.0 - size.0) / 2.0,
            panel.y - 8.0 * s - size.1,
            size.0,
            size.1,
        );
        canvas.fill_rounded(at, 5.0 * s, panel_bg());
        canvas.draw_text(&block, at.x + pad_x, at.y + pad_y, muted());
    }
}

/// A keycap with `letter`, then `label`.
fn button(canvas: &Canvas<'_>, rect: RectF, letter: &str, label: &str, s: f32) {
    let side = KEYCAP * s;
    let cap = RectF::new(
        (rect.x + 8.0 * s).round(),
        (rect.y + (rect.h - side) / 2.0).round(),
        side.round(),
        side.round(),
    );
    canvas.fill_rounded(cap, 4.0 * s, keycap_bg());
    let edge = Stroke {
        width: s.max(1.0),
        color: keycap_edge(),
        dash: cf_render::LineDash::Solid,
    };
    // The key's lower edge is a little thicker, like a physical key.
    canvas.stroke_line(
        (cap.x + 3.0 * s, cap.bottom()),
        (cap.right() - 3.0 * s, cap.bottom()),
        &Stroke {
            width: 2.0 * s,
            ..edge
        },
    );
    if let Some(block) = canvas.text_sized(letter, 11.5 * s, true) {
        let x = cap.x + (cap.w - block.width) / 2.0;
        let y = cap.y + (cap.h - block.height) / 2.0;
        canvas.draw_text(&block, x, y, white());
    }
    if let Some(block) = canvas.text(label) {
        let x = cap.right() + 8.0 * s;
        let y = rect.y + (rect.h - block.height) / 2.0;
        canvas.draw_text(&block, x, y, white());
    }
}

fn panel_bg() -> Color {
    Color::rgba8(18, 18, 22, 235)
}

fn hover_bg() -> Color {
    Color::rgba8(0x3A, 0x3A, 0x44, 0xFF)
}

fn active_bg() -> Color {
    Color::rgba8(0x2F, 0x4F, 0x7A, 0xFF)
}

fn keycap_bg() -> Color {
    Color::rgba8(0x4A, 0x4A, 0x56, 0xFF)
}

fn keycap_edge() -> Color {
    Color::rgba8(0x22, 0x22, 0x28, 0xFF)
}

fn white() -> Color {
    Color::rgba8(255, 255, 255, 255)
}

fn muted() -> Color {
    Color::rgba8(0xC8, 0xC8, 0xD0, 0xFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_sits_at_the_bottom_centre_and_buttons_are_hit() {
        let screen = (1920.0, 1080.0);
        let (panel, buttons) = layout(screen, 1.0);
        assert!((panel.x + panel.w / 2.0 - 960.0).abs() <= 1.0, "centred");
        assert!(panel.bottom() < 1080.0 && panel.y > 900.0, "at the bottom");
        assert_eq!(buttons.len(), ITEMS.len());
        for (i, r) in buttons.iter().enumerate() {
            assert_eq!(hit(screen, 1.0, (r.x + 2.0, r.y + 2.0)), Some(i));
        }
        assert_eq!(hit(screen, 1.0, (10.0, 10.0)), None);
        assert_eq!(
            spot(screen, 1.0, (panel.x + 1.0, panel.y + 1.0)),
            Some(Spot::Panel)
        );
        let first = buttons[0];
        assert_eq!(
            spot(screen, 1.0, (first.x + 1.0, first.y + 1.0)),
            Some(Spot::Button(0))
        );
        assert_eq!(spot(screen, 1.0, (10.0, 10.0)), None);
        // Fits a 1280-pixel-wide screen at 150 %.
        assert!(layout((1280.0, 720.0), 1.5).0.w <= 1280.0);
    }

    #[test]
    fn every_mode_key_has_a_button() {
        for mode in [
            CaptureMode::FreeSelect,
            CaptureMode::Rectangle,
            CaptureMode::Window,
            CaptureMode::Element,
            CaptureMode::BrowserContent,
            CaptureMode::Monitor,
        ] {
            assert!(ITEMS.iter().any(|(.., c)| *c == Command::SetMode(mode)));
        }
    }
}
