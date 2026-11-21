//! The editor's toolbar: layout, hit-testing and painting (window pixels).

use cf_core::annotate::{Dash, EditorState, Style, Tool};
use cf_render::{Canvas, Color, LineDash, RectF, Stroke};

use super::command::{COLORS, Command};
use super::icons::{self, glyph, ink, label};
use super::render;

/// Toolbar height at 100 % scaling.
pub const HEIGHT: f32 = 48.0;
const BUTTON: f32 = 32.0;
const SWATCH: f32 = 22.0;
const WIDE: f32 = 50.0;
const GAP: f32 = 2.0;
const GROUP_GAP: f32 = 12.0;

const TOOLS: [(Tool, &str); 14] = [
    (Tool::Select, "Select · move (V)"),
    (Tool::Pen, "Pen (P)"),
    (Tool::Highlighter, "Highlighter (H)"),
    (Tool::Line, "Line (L)"),
    (Tool::Arrow, "Arrow (A)"),
    (Tool::Rect, "Rectangle (R)"),
    (Tool::Ellipse, "Ellipse (E)"),
    (Tool::Text, "Text (T)"),
    (Tool::Step, "Step number (N)"),
    (Tool::Redact, "Pixelate · solid when filled (X)"),
    (Tool::Blur, "Blur, not for secrets (B)"),
    (Tool::Spotlight, "Spotlight (S)"),
    (Tool::Magnify, "Magnify (M)"),
    (Tool::Crop, "Crop · Delete resets (C)"),
];

/// One clickable toolbar item.
#[derive(Debug, Clone, Copy)]
pub struct Button {
    pub rect: RectF,
    pub command: Command,
    pub label: &'static str,
}

/// Lays the buttons out left to right at UI `scale`.
pub fn layout(scale: f32) -> Vec<Button> {
    let mut row = Row::new(scale);
    for (tool, label) in TOOLS {
        row.push(BUTTON, Command::Tool(tool), label);
    }
    row.gap();
    for color in COLORS {
        row.push(SWATCH, Command::Color(color), "Colour (1–8)");
    }
    row.gap();
    row.push(BUTTON, Command::CycleWidth, "Thickness / strength ([ ])");
    row.push(BUTTON, Command::CycleDash, "Line style (D)");
    row.push(BUTTON, Command::ToggleFill, "Fill shapes (F)");
    row.push(WIDE, Command::CycleOpacity, "Opacity (O)");
    row.gap();
    row.push(BUTTON, Command::Undo, "Undo (Ctrl+Z)");
    row.push(BUTTON, Command::Redo, "Redo (Ctrl+Y)");
    row.gap();
    row.push(
        WIDE,
        Command::CopyText,
        "Copy the text in the image (Ctrl+Shift+T)",
    );
    row.push(
        WIDE,
        Command::HideSensitive,
        "Hide emails, IPs, numbers, keys (Ctrl+Shift+R)",
    );
    row.gap();
    row.push(WIDE, Command::Drag, "Drag into another app");
    row.push(WIDE, Command::Pin, "Pin on screen (Ctrl+P)");
    row.push(WIDE, Command::Copy, "Copy image (Ctrl+C)");
    row.push(WIDE, Command::Save, "Save as new file (Ctrl+S)");
    row.buttons
}

/// Total width the toolbar needs at `scale`.
pub fn min_width(scale: f32) -> f32 {
    layout(scale)
        .last()
        .map_or(0.0, |b| b.rect.right() + GROUP_GAP * scale)
}

/// The button under `(x, y)`.
pub fn hit(buttons: &[Button], (x, y): (f32, f32)) -> Option<usize> {
    buttons.iter().position(|b| {
        let r = b.rect;
        x >= r.x && x < r.right() && y >= r.y && y < r.bottom()
    })
}

struct Row {
    scale: f32,
    x: f32,
    buttons: Vec<Button>,
}

impl Row {
    fn new(scale: f32) -> Self {
        Self {
            scale,
            x: GROUP_GAP * scale,
            buttons: Vec::new(),
        }
    }

    fn push(&mut self, width: f32, command: Command, label: &'static str) {
        let (w, h) = (width * self.scale, width.min(BUTTON) * self.scale);
        let y = (HEIGHT * self.scale - h) / 2.0;
        self.buttons.push(Button {
            rect: RectF::new(self.x, y, w, h),
            command,
            label,
        });
        self.x += w + GAP * self.scale;
    }

    fn gap(&mut self) {
        self.x += (GROUP_GAP - GAP) * self.scale;
    }
}

/// What the toolbar shows as active, besides hover.
pub struct Look<'a> {
    pub state: &'a EditorState,
    pub hover: Option<usize>,
    pub scale: f32,
    /// A short message (e.g. "Copied") shown at the right end.
    pub status: Option<&'a str>,
}

pub fn paint(canvas: &Canvas<'_>, buttons: &[Button], look: &Look<'_>) {
    let s = look.scale;
    let (w, _) = canvas.size();
    canvas.fill_rect(RectF::new(0.0, 0.0, w, HEIGHT * s), bar());
    for (index, button) in buttons.iter().enumerate() {
        let active = is_active(button.command, look.state);
        if active || look.hover == Some(index) {
            let shade = if active { selected() } else { hovered() };
            canvas.fill_rounded(button.rect, 6.0 * s, shade);
        }
        icon(canvas, button, look.state.style(), s);
    }
    let note = look
        .hover
        .and_then(|i| buttons.get(i))
        .map(|b| b.label)
        .or(look.status);
    if let Some(block) = note.and_then(|text| canvas.text(text)) {
        let x = buttons.last().map_or(0.0, |b| b.rect.right()) + GROUP_GAP * s;
        let y = (HEIGHT * s - block.height) / 2.0;
        canvas.draw_text(&block, x, y, muted());
    }
}

fn is_active(command: Command, state: &EditorState) -> bool {
    let style = state.style();
    match command {
        Command::Tool(tool) => state.tool() == tool,
        Command::Color(c) => (c.r, c.g, c.b) == (style.color.r, style.color.g, style.color.b),
        Command::ToggleFill => style.filled,
        Command::CycleDash => style.dash != Dash::Solid,
        _ => false,
    }
}

fn icon(canvas: &Canvas<'_>, button: &Button, style: Style, s: f32) {
    let r = button.rect;
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let half = 8.0 * s;
    let line = Stroke {
        width: 2.0 * s,
        color: ink(),
        dash: LineDash::Solid,
    };
    match button.command {
        Command::Tool(tool) => icons::tool(canvas, tool, (cx, cy), half, &line),
        Command::Color(color) => {
            let d = 16.0 * s;
            canvas.fill_ellipse(
                RectF::new(cx - d / 2.0, cy - d / 2.0, d, d),
                render::color(color),
            );
        }
        Command::CycleWidth => {
            let stroke = Stroke {
                width: style.width * s * 0.75,
                ..line
            };
            canvas.stroke_line((cx - half, cy), (cx + half, cy), &stroke);
        }
        Command::CycleDash => {
            // Solid shows a dashed sample (what the button switches to), so it doesn't look
            // like another thickness option.
            let dash = match style.dash {
                Dash::Solid | Dash::Dashed => LineDash::Dashed,
                Dash::Dotted => LineDash::Dotted,
            };
            canvas.stroke_line((cx - half, cy), (cx + half, cy), &Stroke { dash, ..line });
        }
        Command::ToggleFill => {
            let square = RectF::new(cx - half * 0.8, cy - half * 0.8, half * 1.6, half * 1.6);
            canvas.fill_rect(square, if style.filled { ink() } else { bar() });
            canvas.stroke_rect(square, &line);
        }
        Command::CycleOpacity => label(canvas, &opacity_label(style), (cx, cy)),
        Command::Undo => glyph(canvas, "↶", (cx, cy), s),
        Command::Redo => glyph(canvas, "↷", (cx, cy), s),
        Command::Copy => label(canvas, "Copy", (cx, cy)),
        Command::Save => label(canvas, "Save", (cx, cy)),
        Command::CopyText => label(canvas, "Text", (cx, cy)),
        Command::Pin => label(canvas, "Pin", (cx, cy)),
        Command::Drag => label(canvas, "Drag", (cx, cy)),
        Command::HideSensitive => label(canvas, "Hide", (cx, cy)),
        _ => {}
    }
}

fn opacity_label(style: Style) -> String {
    format!("{}%", u32::from(style.color.a) * 100 / 255)
}

fn bar() -> Color {
    Color::rgba8(0x26, 0x26, 0x2C, 0xFF)
}

fn hovered() -> Color {
    Color::rgba8(0x3A, 0x3A, 0x44, 0xFF)
}

fn selected() -> Color {
    Color::rgba8(0x2F, 0x4F, 0x7A, 0xFF)
}

fn muted() -> Color {
    Color::rgba8(0xA0, 0xA0, 0xAA, 0xFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_do_not_overlap_and_are_hit_by_position() {
        let buttons = layout(1.5);
        for pair in buttons.windows(2) {
            assert!(pair[0].rect.right() <= pair[1].rect.x);
        }
        let first = buttons[0].rect;
        let inside = (first.x + 1.0, first.y + 1.0);
        assert_eq!(hit(&buttons, inside), Some(0));
        assert_eq!(hit(&buttons, (0.0, 0.0)), None);
        assert!(min_width(1.0) > 600.0);
        // Fits a 1920-pixel-wide screen at 150 % scaling.
        assert!(min_width(1.5) <= 1920.0, "toolbar is {} px", min_width(1.5));
    }
}
