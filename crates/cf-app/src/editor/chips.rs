//! Chips for QR codes and barcodes found in the capture, stacked at the bottom-left of the
//! editor: the decoded content, *Copy*, and *Open* for plain web links (the link itself is
//! shown in the chip before anyone opens it).

use cf_detect::codes::Code;
use cf_detect::link;
use cf_render::{Canvas, Color, RectF};

use super::icons::{ink, label};

/// Most chips shown at once.
const MAX: usize = 3;
const HEIGHT: f32 = 30.0;
const GAP: f32 = 6.0;
const EDGE: f32 = 12.0;
const TEXT_WIDTH: f32 = 340.0;
const BUTTON: f32 = 54.0;
/// Characters of content shown before "…".
const MAX_CHARS: usize = 46;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipAction {
    Copy(usize),
    Open(usize),
}

#[derive(Debug, Clone, Copy)]
pub struct ChipButton {
    pub rect: RectF,
    pub action: ChipAction,
}

/// One chip: its background and buttons.
struct Chip {
    frame: RectF,
    buttons: Vec<ChipButton>,
}

fn layout(codes: &[Code], window_height: f32, s: f32) -> Vec<Chip> {
    codes
        .iter()
        .take(MAX)
        .enumerate()
        .map(|(index, code)| {
            #[allow(clippy::cast_precision_loss)] // at most MAX
            let y = window_height - EDGE * s - (HEIGHT + GAP) * s * (index + 1) as f32 + GAP * s;
            let mut x = EDGE * s + TEXT_WIDTH * s;
            let mut buttons = Vec::new();
            let mut push = |action| {
                let rect = RectF::new(x, y + 3.0 * s, BUTTON * s, (HEIGHT - 6.0) * s);
                buttons.push(ChipButton { rect, action });
                x += (BUTTON + 4.0) * s;
            };
            push(ChipAction::Copy(index));
            if link::web_url(&code.text).is_some() {
                push(ChipAction::Open(index));
            }
            let frame = RectF::new(EDGE * s, y, x - EDGE * s, HEIGHT * s);
            Chip { frame, buttons }
        })
        .collect()
}

/// Every chip button, for hit-testing.
pub fn buttons(codes: &[Code], window_height: f32, s: f32) -> Vec<ChipButton> {
    layout(codes, window_height, s)
        .into_iter()
        .flat_map(|chip| chip.buttons)
        .collect()
}

/// The chip button under `(x, y)`.
pub fn hit(buttons: &[ChipButton], (x, y): (f32, f32)) -> Option<ChipAction> {
    buttons
        .iter()
        .find(|b| x >= b.rect.x && x < b.rect.right() && y >= b.rect.y && y < b.rect.bottom())
        .map(|b| b.action)
}

pub fn paint(canvas: &Canvas<'_>, codes: &[Code], s: f32) {
    let (_, window_height) = canvas.size();
    for (chip, code) in layout(codes, window_height, s).iter().zip(codes) {
        canvas.fill_rounded(chip.frame, 8.0 * s, Color::rgba8(0x26, 0x26, 0x2C, 0xF2));
        let text = format!("{}  {}", kind(code), shorten(&code.text));
        if let Some(block) = canvas.text(&text) {
            let y = chip.frame.y + (chip.frame.h - block.height) / 2.0;
            canvas.draw_text(&block, chip.frame.x + 10.0 * s, y, ink());
        }
        for button in &chip.buttons {
            let r = button.rect;
            canvas.fill_rounded(r, 6.0 * s, Color::rgba8(0x3A, 0x3A, 0x44, 0xFF));
            let name = match button.action {
                ChipAction::Copy(_) => "Copy",
                ChipAction::Open(_) => "Open",
            };
            label(canvas, name, (r.x + r.w / 2.0, r.y + r.h / 2.0));
        }
    }
}

fn kind(code: &Code) -> &'static str {
    if code.format.to_ascii_lowercase().contains("qr") {
        "QR"
    } else {
        "Barcode"
    }
}

/// The content on one line, cut to [`MAX_CHARS`].
fn shorten(text: &str) -> String {
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
    use super::*;

    fn code(text: &str) -> Code {
        Code {
            format: "qrcode".into(),
            text: text.into(),
            area: None,
        }
    }

    #[test]
    fn only_web_links_get_an_open_button() {
        let codes = [code("https://example.com"), code("WIFI:S:home;;")];
        let all = buttons(&codes, 800.0, 1.0);
        let actions: Vec<_> = all.iter().map(|b| b.action).collect();
        assert_eq!(
            actions,
            [
                ChipAction::Copy(0),
                ChipAction::Open(0),
                ChipAction::Copy(1)
            ]
        );
        let open = all[1].rect;
        assert_eq!(
            hit(&all, (open.x + 1.0, open.y + 1.0)),
            Some(ChipAction::Open(0))
        );
        assert!(all[2].rect.y < all[0].rect.y, "later chips stack upwards");
    }

    #[test]
    fn long_content_is_shortened_to_one_line() {
        let long = format!("line1\nline2 {}", "x".repeat(100));
        let short = shorten(&long);
        assert_eq!(short.chars().count(), MAX_CHARS);
        assert!(short.ends_with('…') && !short.contains('\n'));
    }
}
