//! The editor's toolbar: layout, hit-testing and painting (window pixels).
//!
//! Tools come in groups (freehand, shapes, text, image effects), then the options of the
//! current tool (only those it uses), then undo/redo and the output actions. The options
//! group keeps the width of the fullest set, so nothing to its right moves when the tool
//! changes.

use cf_core::annotate::{Dash, EditorState, Style, Tool};
use cf_render::{Canvas, Color, LineDash, RectF, Stroke};

use super::command::Command;
use super::icons::{self, glyph, ink, label};
use super::render;

/// Toolbar height at 100 % scaling.
pub const HEIGHT: f32 = 48.0;
const BUTTON: f32 = 32.0;
const WIDE: f32 = 50.0;
const GAP: f32 = 2.0;
const GROUP_GAP: f32 = 14.0;

const TOOL_GROUPS: [&[(Tool, &str)]; 4] = [
    &[
        (Tool::Select, "Select · move (V)"),
        (Tool::Pen, "Pen (P)"),
        (Tool::Highlighter, "Highlighter (H)"),
    ],
    &[
        (Tool::Line, "Line (L)"),
        (Tool::Arrow, "Arrow (A)"),
        (Tool::Rect, "Rectangle (R)"),
        (Tool::Ellipse, "Ellipse (E)"),
    ],
    &[
        (Tool::Text, "Text (T) · drag the corner to resize"),
        (Tool::Step, "Step number (N)"),
    ],
    &[
        (Tool::Redact, "Pixelate · solid when filled (X)"),
        (Tool::Blur, "Blur, not for secrets (B)"),
        (Tool::Spotlight, "Spotlight (S)"),
        (Tool::Magnify, "Magnify (M)"),
        (Tool::Crop, "Crop · Delete resets (C)"),
    ],
];

/// Every tool option, in toolbar order.
const OPTIONS: [Item; 5] = [
    (BUTTON, Command::ColorMenu, "Colour (1–8)"),
    (BUTTON, Command::CycleWidth, "Thickness / strength ([ ])"),
    (BUTTON, Command::CycleDash, "Line style (D)"),
    (BUTTON, Command::ToggleFill, "Fill (F)"),
    (WIDE, Command::CycleOpacity, "Opacity (O)"),
];

/// One clickable toolbar item.
#[derive(Debug, Clone, Copy)]
pub struct Button {
    pub rect: RectF,
    pub command: Command,
    pub label: &'static str,
}

/// The buttons, and thin dividers between groups (`x`, row top).
pub struct Layout {
    pub buttons: Vec<Button>,
    pub dividers: Vec<(f32, f32)>,
}

/// One item: its width at 100 % scaling, command and tooltip.
type Item = (f32, Command, &'static str);

/// Whether `tool` uses `option` (a command from [`OPTIONS`]).
pub fn uses(tool: Tool, option: Command) -> bool {
    use Tool::{
        Arrow, Blur, Crop, Ellipse, Highlighter, Line, Magnify, Pen, Rect, Redact, Select,
        Spotlight, Step, Text,
    };
    match option {
        Command::ColorMenu => !matches!(tool, Select | Redact | Blur | Spotlight | Crop),
        Command::CycleWidth => matches!(
            tool,
            Pen | Highlighter | Line | Arrow | Rect | Ellipse | Step | Redact | Blur | Magnify
        ),
        Command::CycleDash => matches!(tool, Pen | Line | Arrow | Rect | Ellipse | Magnify),
        Command::ToggleFill => matches!(tool, Rect | Ellipse | Redact),
        Command::CycleOpacity => matches!(
            tool,
            Pen | Highlighter | Line | Arrow | Rect | Ellipse | Text | Step
        ),
        _ => false,
    }
}

/// The toolbar's groups, left to right, with the options `tool` uses. A group never
/// splits across rows; the second value is the width (100 %) the group takes up.
fn groups(tool: Tool) -> Vec<(Vec<Item>, f32)> {
    let mut groups: Vec<Vec<Item>> = TOOL_GROUPS
        .iter()
        .map(|group| {
            group
                .iter()
                .map(|&(tool, label)| (BUTTON, Command::Tool(tool), label))
                .collect()
        })
        .collect();
    let options_index = groups.len();
    groups.push(
        OPTIONS
            .iter()
            .copied()
            .filter(|o| uses(tool, o.1))
            .collect(),
    );
    groups.push(vec![
        (BUTTON, Command::Undo, "Undo (Ctrl+Z)"),
        (BUTTON, Command::Redo, "Redo (Ctrl+Y)"),
    ]);
    groups.push(vec![(
        WIDE,
        Command::HideSensitive,
        "Hide emails, IPs, numbers, keys (Ctrl+Shift+R)",
    )]);
    groups.push(vec![
        (WIDE, Command::Drag, "Drag into another app"),
        (WIDE, Command::Pin, "Pin on screen (Ctrl+P)"),
        (WIDE, Command::Copy, "Copy image (Ctrl+C)"),
        (WIDE, Command::Save, "Save as new file (Ctrl+S)"),
    ]);
    groups
        .into_iter()
        .enumerate()
        .map(|(index, group)| {
            let width = if index == options_index {
                group_width(&OPTIONS)
            } else {
                group_width(&group)
            };
            (group, width)
        })
        .collect()
}

/// Width of a group at 100 %, without surrounding gaps.
fn group_width(group: &[Item]) -> f32 {
    let items: f32 = group.iter().map(|(w, ..)| w).sum();
    #[allow(clippy::cast_precision_loss)] // a handful of items
    let gaps = group.len().saturating_sub(1) as f32 * GAP;
    items + gaps
}

/// Lays the buttons out in rows that fit `width` window pixels at UI `scale`, wrapping whole
/// groups onto a new row when the window is too narrow (e.g. at 175 % scaling).
pub fn layout(scale: f32, width: f32, tool: Tool) -> Layout {
    let mut out = Layout {
        buttons: Vec::new(),
        dividers: Vec::new(),
    };
    let left = GROUP_GAP * scale;
    let (mut cursor, mut row) = (left, 0.0_f32);
    for (group, slot) in groups(tool) {
        let slot = slot * scale;
        if cursor > left && cursor + slot > width - left {
            row += 1.0;
            cursor = left;
        } else if cursor > left {
            out.dividers
                .push((cursor - GROUP_GAP * scale / 2.0, row * HEIGHT * scale));
        }
        let mut x = cursor;
        for (size, command, label) in group {
            let (wide, tall) = (size * scale, size.min(BUTTON) * scale);
            let top = row * HEIGHT * scale + (HEIGHT * scale - tall) / 2.0;
            out.buttons.push(Button {
                rect: RectF::new(x, top, wide, tall),
                command,
                label,
            });
            x += wide + GAP * scale;
        }
        cursor += slot + GROUP_GAP * scale;
    }
    out
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
    let groups = groups(Tool::Rect);
    let widths: f32 = groups.iter().map(|(_, w)| w).sum();
    #[allow(clippy::cast_precision_loss)] // a handful of groups
    let gaps = (groups.len() + 1) as f32 * GROUP_GAP;
    (widths + gaps) * scale
}

/// The narrowest window that still shows every group (the widest group on its own row).
pub fn narrowest(scale: f32) -> f32 {
    groups(Tool::Rect)
        .iter()
        .map(|(_, w)| *w)
        .fold(0.0, f32::max)
        * scale
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
    /// The colour menu is open.
    pub colors_open: bool,
}

pub fn paint(canvas: &Canvas<'_>, layout: &Layout, look: &Look<'_>) {
    let s = look.scale;
    let (w, _) = canvas.size();
    let buttons = &layout.buttons;
    canvas.fill_rect(RectF::new(0.0, 0.0, w, height(buttons, s)), bar());
    for &(x, top) in &layout.dividers {
        let tall = 22.0 * s;
        let y = top + (HEIGHT * s - tall) / 2.0;
        canvas.fill_rect(RectF::new(x - 0.5 * s, y, s.max(1.0), tall), divider());
    }
    for (index, button) in buttons.iter().enumerate() {
        let active = is_active(button.command, look);
        if active || look.hover == Some(index) {
            let shade = if active { selected() } else { hovered() };
            canvas.fill_rounded(button.rect, 6.0 * s, shade);
        }
        icon(canvas, button, look.state.style(), s);
    }
    // A status message goes after the last button, on its row.
    if let (Some(block), Some(last)) = (look.status.and_then(|t| canvas.text(t)), buttons.last()) {
        let row_top = (last.rect.y / (HEIGHT * s)).floor() * HEIGHT * s;
        let y = row_top + (HEIGHT * s - block.height) / 2.0;
        canvas.draw_text(&block, last.rect.right() + GROUP_GAP * s, y, muted());
    }
}

/// The hovered button's name in a small box under it.
pub fn paint_tooltip(canvas: &Canvas<'_>, button: &Button, s: f32) {
    let Some(block) = canvas.text(button.label) else {
        return;
    };
    let (pad_x, pad_y) = (8.0 * s, 5.0 * s);
    let size = (block.width + 2.0 * pad_x, block.height + 2.0 * pad_y);
    let (window_w, _) = canvas.size();
    let centre = button.rect.x + button.rect.w / 2.0;
    let x = (centre - size.0 / 2.0).clamp(4.0 * s, (window_w - size.0 - 4.0 * s).max(0.0));
    let y = button.rect.bottom() + 8.0 * s;
    canvas.fill_rounded(RectF::new(x, y, size.0, size.1), 5.0 * s, tooltip());
    canvas.draw_text(&block, x + pad_x, y + pad_y, ink());
}

fn is_active(command: Command, look: &Look<'_>) -> bool {
    let style = look.state.style();
    match command {
        Command::Tool(tool) => look.state.tool() == tool,
        Command::ColorMenu => look.colors_open,
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
        Command::ColorMenu => {
            let d = 18.0 * s;
            let dot = RectF::new(cx - d / 2.0, cy - d / 2.0, d, d);
            canvas.fill_ellipse(dot, render::color(style.color.with_alpha(255)));
            let ring = Stroke {
                width: 1.5 * s,
                color: muted(),
                ..line
            };
            canvas.stroke_ellipse(dot.inflate(1.5 * s), &ring);
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

pub fn bar() -> Color {
    Color::rgba8(0x26, 0x26, 0x2C, 0xFF)
}

pub fn hovered() -> Color {
    Color::rgba8(0x3A, 0x3A, 0x44, 0xFF)
}

fn selected() -> Color {
    Color::rgba8(0x2F, 0x4F, 0x7A, 0xFF)
}

pub fn muted() -> Color {
    Color::rgba8(0xA0, 0xA0, 0xAA, 0xFF)
}

fn divider() -> Color {
    Color::rgba8(0x44, 0x44, 0x4E, 0xFF)
}

fn tooltip() -> Color {
    Color::rgba8(0x10, 0x10, 0x14, 0xF0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_do_not_overlap_and_are_hit_by_position() {
        let buttons = layout(1.5, f32::MAX, Tool::Rect).buttons;
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
        let last = buttons.last().map_or(0.0, |b| b.rect.right());
        assert!(last <= full_width(1.5), "{last} > {}", full_width(1.5));
        // Fits a 1920-pixel-wide screen at 150 % scaling.
        assert!(
            full_width(1.5) <= 1920.0,
            "toolbar is {} px",
            full_width(1.5)
        );
    }

    #[test]
    fn options_follow_the_tool_and_nothing_else_moves() {
        let commands = |tool| -> Vec<Command> {
            layout(1.0, f32::MAX, tool)
                .buttons
                .iter()
                .map(|b| b.command)
                .collect()
        };
        assert!(commands(Tool::Rect).contains(&Command::ToggleFill));
        assert!(!commands(Tool::Arrow).contains(&Command::ToggleFill));
        assert!(!commands(Tool::Select).contains(&Command::ColorMenu));
        assert!(commands(Tool::Text).contains(&Command::ColorMenu));
        assert!(!commands(Tool::Text).contains(&Command::CycleDash));
        let save_x = |tool| {
            layout(1.0, f32::MAX, tool)
                .buttons
                .iter()
                .find(|b| b.command == Command::Save)
                .map(|b| b.rect.x)
        };
        assert_eq!(save_x(Tool::Select), save_x(Tool::Rect));
        assert_eq!(layout(1.0, f32::MAX, Tool::Rect).dividers.len(), 7);
    }

    #[test]
    fn narrow_windows_wrap_whole_groups_onto_more_rows() {
        let scale = 2.0; // 200 % on a 1920-pixel screen
        let width = 1920.0 * 0.6;
        let buttons = layout(scale, width, Tool::Rect).buttons;
        assert!(
            buttons.iter().all(|b| b.rect.right() <= width),
            "nothing runs off"
        );
        let rows = height(&buttons, scale) / (HEIGHT * scale);
        assert!(rows >= 2.0, "wraps");
        // Each group stays on one row: the shape tools share a top edge.
        let shapes: Vec<_> = buttons
            .iter()
            .filter(|b| {
                matches!(
                    b.command,
                    Command::Tool(Tool::Line | Tool::Arrow | Tool::Rect | Tool::Ellipse)
                )
            })
            .map(|b| b.rect.y)
            .collect();
        assert!(shapes.windows(2).all(|w| (w[0] - w[1]).abs() < 0.01));
        let at_narrowest = layout(scale, narrowest(scale), Tool::Rect).buttons;
        assert!(
            at_narrowest
                .iter()
                .all(|b| b.rect.right() <= narrowest(scale))
        );
    }
}
