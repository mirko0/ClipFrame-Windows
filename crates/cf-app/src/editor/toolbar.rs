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

/// One item: its width at 100 % scaling, command and tooltip.
type Item = (f32, Command, &'static str);

/// The toolbar's groups, left to right. A group never splits across rows.
fn groups() -> Vec<Vec<Item>> {
    vec![
        TOOLS
            .iter()
            .map(|&(tool, label)| (BUTTON, Command::Tool(tool), label))
            .collect(),
        COLORS
            .iter()
            .map(|&c| (SWATCH, Command::Color(c), "Colour (1–8)"))
            .collect(),
        vec![
            (BUTTON, Command::CycleWidth, "Thickness / strength ([ ])"),
            (BUTTON, Command::CycleDash, "Line style (D)"),
            (BUTTON, Command::ToggleFill, "Fill shapes (F)"),
            (WIDE, Command::CycleOpacity, "Opacity (O)"),
        ],
        vec![
            (BUTTON, Command::Undo, "Undo (Ctrl+Z)"),
            (BUTTON, Command::Redo, "Redo (Ctrl+Y)"),
        ],
        vec![(
            WIDE,
            Command::HideSensitive,
            "Hide emails, IPs, numbers, keys (Ctrl+Shift+R)",
        )],
        vec![
            (WIDE, Command::Drag, "Drag into another app"),
            (WIDE, Command::Pin, "Pin on screen (Ctrl+P)"),
            (WIDE, Command::Copy, "Copy image (Ctrl+C)"),
            (WIDE, Command::Save, "Save as new file (Ctrl+S)"),
        ],
    ]
}

/// Width of a group at `scale`, without surrounding gaps.
fn group_width(group: &[Item], scale: f32) -> f32 {
    let items: f32 = group.iter().map(|(w, ..)| w).sum();
    #[allow(clippy::cast_precision_loss)] // a handful of items
    let gaps = group.len().saturating_sub(1) as f32 * GAP;
    (items + gaps) * scale
}

/// Lays the buttons out in rows that fit `width` window pixels at UI `scale`, wrapping whole
/// groups onto a new row when the window is too narrow (e.g. at 175 % scaling).
pub fn layout(scale: f32, width: f32) -> Vec<Button> {
    let mut buttons = Vec::new();
    let left = GROUP_GAP * scale;
    let (mut cursor, mut row) = (left, 0.0_f32);
    for group in groups() {
        let needed = group_width(&group, scale);
        if cursor > left && cursor + needed > width - left {
            row += 1.0;
            cursor = left;
        }
        for (size, command, label) in group {
            let (wide, tall) = (size * scale, size.min(BUTTON) * scale);
            let top = row * HEIGHT * scale + (HEIGHT * scale - tall) / 2.0;
            buttons.push(Button {
                rect: RectF::new(cursor, top, wide, tall),
                command,
                label,
            });
            cursor += wide + GAP * scale;
        }
        cursor += (GROUP_GAP - GAP) * scale;
    }
    buttons
}

/// Height of the toolbar (all its rows) for `buttons`.
pub fn height(buttons: &[Button], scale: f32) -> f32 {
    let rows = buttons
        .iter()
        .map(|b| (b.rect.y / (HEIGHT * scale)).floor())
        .fold(0.0_f32, f32::max);
    (rows + 1.0) * HEIGHT * scale
}

/// Width the toolbar needs to fit on one row at `scale`.
pub fn full_width(scale: f32) -> f32 {
    layout(scale, f32::MAX)
        .last()
        .map_or(0.0, |b| b.rect.right() + GROUP_GAP * scale)
}

/// The narrowest window that still shows every group (the widest group on its own row).
pub fn narrowest(scale: f32) -> f32 {
    groups()
        .iter()
        .map(|g| group_width(g, scale))
        .fold(0.0, f32::max)
        + 2.0 * GROUP_GAP * scale
}

/// The button under `(x, y)`.
pub fn hit(buttons: &[Button], (x, y): (f32, f32)) -> Option<usize> {
    buttons.iter().position(|b| {
        let r = b.rect;
        x >= r.x && x < r.right() && y >= r.y && y < r.bottom()
    })
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
    canvas.fill_rect(RectF::new(0.0, 0.0, w, height(buttons, s)), bar());
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
    // The note goes after the last button, on its row.
    if let (Some(block), Some(last)) = (note.and_then(|text| canvas.text(text)), buttons.last()) {
        let row_top = (last.rect.y / (HEIGHT * s)).floor() * HEIGHT * s;
        let y = row_top + (HEIGHT * s - block.height) / 2.0;
        canvas.draw_text(&block, last.rect.right() + GROUP_GAP * s, y, muted());
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
        let buttons = layout(1.5, f32::MAX);
        for pair in buttons.windows(2) {
            assert!(pair[0].rect.right() <= pair[1].rect.x);
        }
        let first = buttons[0].rect;
        let inside = (first.x + 1.0, first.y + 1.0);
        assert_eq!(hit(&buttons, inside), Some(0));
        assert_eq!(hit(&buttons, (0.0, 0.0)), None);
        assert!(
            (height(&buttons, 1.5) - HEIGHT * 1.5).abs() < 0.01,
            "one row"
        );
        // Fits a 1920-pixel-wide screen at 150 % scaling.
        assert!(
            full_width(1.5) <= 1920.0,
            "toolbar is {} px",
            full_width(1.5)
        );
    }

    #[test]
    fn narrow_windows_wrap_whole_groups_onto_more_rows() {
        let scale = 2.0; // 200 % on a 1920-pixel screen
        let width = 1920.0 * 0.9;
        let buttons = layout(scale, width);
        assert!(
            buttons.iter().all(|b| b.rect.right() <= width),
            "nothing runs off"
        );
        let rows = height(&buttons, scale) / (HEIGHT * scale);
        assert!(rows >= 2.0, "wraps at 200 %");
        // Each group stays on one row: the colour swatches share a top edge.
        let swatches: Vec<_> = buttons
            .iter()
            .filter(|b| matches!(b.command, Command::Color(_)))
            .map(|b| b.rect.y)
            .collect();
        assert!(swatches.windows(2).all(|w| (w[0] - w[1]).abs() < 0.01));
        let at_narrowest = layout(scale, narrowest(scale));
        assert!(
            at_narrowest
                .iter()
                .all(|b| b.rect.right() <= narrowest(scale))
        );
    }
}
