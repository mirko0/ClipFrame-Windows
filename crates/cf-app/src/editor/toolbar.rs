//! The editor's toolbar: layout, hit-testing and painting (window pixels).

use cf_core::annotate::{Annotation, Dash, EditorState, Shape, Style, Tool, Vec2};
use cf_render::{Canvas, Color, LineDash, RectF, Stroke};

use super::command::{COLORS, Command, WIDTHS};
use super::render;

/// Toolbar height at 100 % scaling.
pub const HEIGHT: f32 = 48.0;
const BUTTON: f32 = 34.0;
const SWATCH: f32 = 26.0;
const WIDE: f32 = 54.0;
const GAP: f32 = 4.0;
const GROUP_GAP: f32 = 14.0;

const TOOLS: [(Tool, &str); 8] = [
    (Tool::Select, "Select · move (V)"),
    (Tool::Pen, "Pen (P)"),
    (Tool::Line, "Line (L)"),
    (Tool::Arrow, "Arrow (A)"),
    (Tool::Rect, "Rectangle (R)"),
    (Tool::Ellipse, "Ellipse (E)"),
    (Tool::Text, "Text (T)"),
    (Tool::Step, "Step number (N)"),
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
    for width in WIDTHS {
        row.push(BUTTON, Command::Width(width), "Thickness ([ ])");
    }
    row.push(BUTTON, Command::CycleDash, "Line style (D)");
    row.push(BUTTON, Command::ToggleFill, "Fill shapes (F)");
    row.push(WIDE, Command::CycleOpacity, "Opacity (O)");
    row.gap();
    row.push(BUTTON, Command::Undo, "Undo (Ctrl+Z)");
    row.push(BUTTON, Command::Redo, "Redo (Ctrl+Y)");
    row.gap();
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
        Command::Width(width) => (width - style.width).abs() < f32::EPSILON,
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
        Command::Tool(tool) => tool_icon(canvas, tool, (cx, cy), half, &line),
        Command::Color(color) => {
            let d = 16.0 * s;
            canvas.fill_ellipse(
                RectF::new(cx - d / 2.0, cy - d / 2.0, d, d),
                render::color(color),
            );
        }
        Command::Width(width) => {
            let stroke = Stroke {
                width: width * s * 0.75,
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
        _ => {}
    }
}

fn tool_icon(canvas: &Canvas<'_>, tool: Tool, (cx, cy): (f32, f32), half: f32, line: &Stroke) {
    let at = |dx: f32, dy: f32| Vec2::new(cx + dx * half, cy + dy * half);
    let shape = match tool {
        Tool::Select => {
            let pointer = [(-0.6, -0.9), (0.7, 0.3), (0.05, 0.35), (-0.3, 0.95)]
                .map(|(dx, dy)| (cx + dx * half, cy + dy * half));
            canvas.fill_polygon(&pointer, line.color);
            return;
        }
        Tool::Text => return label(canvas, "T", (cx, cy)),
        Tool::Step => {
            let badge = RectF::new(cx - half, cy - half, half * 2.0, half * 2.0);
            canvas.stroke_ellipse(badge, line);
            return label(canvas, "1", (cx, cy));
        }
        Tool::Pen => Shape::Pen(vec![
            at(-0.9, 0.5),
            at(-0.3, -0.5),
            at(0.3, 0.5),
            at(0.9, -0.5),
        ]),
        Tool::Line => Shape::Line {
            from: at(-0.9, 0.9),
            to: at(0.9, -0.9),
        },
        Tool::Arrow => Shape::Arrow {
            from: at(-0.9, 0.9),
            to: at(0.9, -0.9),
        },
        Tool::Rect => Shape::Rect {
            a: at(-0.9, -0.7),
            b: at(0.9, 0.7),
        },
        Tool::Ellipse => Shape::Ellipse {
            a: at(-0.9, -0.7),
            b: at(0.9, 0.7),
        },
    };
    let style = Style {
        color: cf_core::annotate::Rgba::new(0xE6, 0xE6, 0xE6, 0xFF),
        width: line.width,
        dash: Dash::Solid,
        filled: false,
    };
    render::annotation(canvas, &Annotation { shape, style }, 1);
}

/// A symbol drawn larger than UI text.
fn glyph(canvas: &Canvas<'_>, text: &str, (cx, cy): (f32, f32), s: f32) {
    if let Some(block) = canvas.text_sized(text, 20.0 * s, true) {
        let (x, y) = (cx - block.width / 2.0, cy - block.height / 2.0);
        canvas.draw_text(&block, x, y, ink());
    }
}

fn label(canvas: &Canvas<'_>, text: &str, (cx, cy): (f32, f32)) {
    if let Some(block) = canvas.text(text) {
        canvas.draw_text(
            &block,
            cx - block.width / 2.0,
            cy - block.height / 2.0,
            ink(),
        );
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

fn ink() -> Color {
    Color::rgba8(0xE6, 0xE6, 0xE6, 0xFF)
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
    }
}
