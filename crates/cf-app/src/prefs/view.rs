//! Layout, hit-testing and drawing of the settings window (window pixels).

use cf_render::{Canvas, Color, LineDash, RectF, Stroke};

use super::model::{Kind, Page, Value};

/// Window size at 100 % scaling.
pub const WIDTH: f32 = 880.0;
pub const HEIGHT: f32 = 600.0;
const SIDEBAR: f32 = 200.0;
const PAD: f32 = 32.0;
const ROW_TOP: f32 = 96.0;
const SETUP_ROW_TOP: f32 = 200.0;
const ROW_HEIGHT: f32 = 80.0;
const CONTROL_W: f32 = 300.0;
const CONTROL_H: f32 = 36.0;
const NAV_W: f32 = 176.0;
const NAV_H: f32 = 38.0;
const BUTTON_W: f32 = 120.0;

/// Settings (sidebar of pages) or first-run setup (steps with Back / Next).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Settings,
    Setup,
}

/// Something clickable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Page(usize),
    Control(usize),
    ChoicePrev(usize),
    ChoiceNext(usize),
    Back,
    Next,
    CaptureNow,
}

/// Where everything is for `page` (index into the mode's pages) at `scale`.
pub fn layout(mode: Mode, pages: &[Page], page: usize, scale: f32) -> Vec<(RectF, Target)> {
    let mut out = Vec::new();
    if mode == Mode::Settings {
        nav_targets(pages.len(), scale, &mut out);
    }
    let top = if mode == Mode::Settings {
        ROW_TOP
    } else {
        SETUP_ROW_TOP
    };
    if let Some(current) = pages.get(page) {
        row_targets(current, top, scale, &mut out);
    }
    if mode == Mode::Setup {
        setup_targets(page, pages.len(), scale, &mut out);
    }
    out
}

fn nav_targets(count: usize, s: f32, out: &mut Vec<(RectF, Target)>) {
    for index in 0..count {
        #[allow(clippy::cast_precision_loss)] // a handful of pages
        let y = (24.0 + index as f32 * (NAV_H + 6.0)) * s;
        out.push((
            RectF::new(12.0 * s, y, NAV_W * s, NAV_H * s),
            Target::Page(index),
        ));
    }
}

fn row_targets(page: &Page, top: f32, s: f32, out: &mut Vec<(RectF, Target)>) {
    let right = WIDTH * s - PAD * s;
    for (index, row) in page.rows.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)] // a handful of rows
        let y = (top + index as f32 * ROW_HEIGHT + (ROW_HEIGHT - CONTROL_H) / 2.0 - 8.0) * s;
        let control = RectF::new(right - CONTROL_W * s, y, CONTROL_W * s, CONTROL_H * s);
        if let Kind::Choice(_) = row.field.kind() {
            let half = control.w / 2.0;
            let next = RectF {
                x: control.x + half,
                w: half,
                ..control
            };
            out.push((RectF { w: half, ..control }, Target::ChoicePrev(index)));
            out.push((next, Target::ChoiceNext(index)));
        } else {
            out.push((control, Target::Control(index)));
        }
    }
}

fn setup_targets(page: usize, count: usize, s: f32, out: &mut Vec<(RectF, Target)>) {
    let right = WIDTH * s - PAD * s;
    let next = RectF::new(
        right - BUTTON_W * s,
        HEIGHT * s - 64.0 * s,
        BUTTON_W * s,
        40.0 * s,
    );
    out.push((next, Target::Next));
    if page > 0 {
        let back = RectF {
            x: next.x - (BUTTON_W + 12.0) * s,
            ..next
        };
        out.push((back, Target::Back));
    }
    if page + 1 == count {
        let button = RectF::new(PAD * 1.5 * s, SETUP_ROW_TOP * s, 220.0 * s, 44.0 * s);
        out.push((button, Target::CaptureNow));
    }
}

/// The control rectangle of row `index` (for drawing).
pub fn control_rect(targets: &[(RectF, Target)], index: usize) -> Option<RectF> {
    let mut found: Option<RectF> = None;
    for (rect, target) in targets {
        match target {
            Target::Control(i) if *i == index => return Some(*rect),
            Target::ChoicePrev(i) if *i == index => found = Some(*rect),
            Target::ChoiceNext(i) if *i == index => {
                return found.map(|prev| RectF {
                    w: prev.w + rect.w,
                    ..prev
                });
            }
            _ => {}
        }
    }
    None
}

pub fn hit(targets: &[(RectF, Target)], (x, y): (f32, f32)) -> Option<Target> {
    targets
        .iter()
        .find(|(r, _)| x >= r.x && x < r.right() && y >= r.y && y < r.bottom())
        .map(|(_, t)| *t)
}

/// What a row shows right now.
pub struct RowLook<'a> {
    pub label: &'a str,
    pub hint: &'a str,
    pub kind: Kind,
    pub value: &'a Value,
    /// Text being typed, or `Some("")` while recording a shortcut.
    pub editing: Option<&'a str>,
    pub hovered: bool,
}

pub fn background() -> Color {
    Color::rgba8(0x1E, 0x1E, 0x24, 0xFF)
}

fn panel() -> Color {
    Color::rgba8(0x26, 0x26, 0x2C, 0xFF)
}

fn field() -> Color {
    Color::rgba8(0x30, 0x30, 0x38, 0xFF)
}

fn field_hover() -> Color {
    Color::rgba8(0x3A, 0x3A, 0x44, 0xFF)
}

pub fn ink() -> Color {
    Color::rgba8(0xE6, 0xE6, 0xE6, 0xFF)
}

pub fn muted() -> Color {
    Color::rgba8(0x9A, 0x9A, 0xA6, 0xFF)
}

pub fn accent() -> Color {
    Color::rgba8(0x4F, 0xA3, 0xFF, 0xFF)
}

/// The sidebar (settings mode).
pub fn sidebar(
    canvas: &Canvas<'_>,
    pages: &[Page],
    targets: &[(RectF, Target)],
    look: (usize, Option<Target>, f32),
) {
    let (current, hover, s) = look;
    let (_, h) = canvas.size();
    canvas.fill_rect(RectF::new(0.0, 0.0, SIDEBAR * s, h), panel());
    for (rect, target) in targets {
        let Target::Page(index) = *target else {
            continue;
        };
        if index == current {
            canvas.fill_rounded(*rect, 6.0 * s, Color::rgba8(0x2F, 0x4F, 0x7A, 0xFF));
        } else if hover == Some(*target) {
            canvas.fill_rounded(*rect, 6.0 * s, field_hover());
        }
        if let Some(block) = canvas.text_sized(pages[index].title, 14.0 * s, true) {
            canvas.draw_text(
                &block,
                rect.x + 14.0 * s,
                rect.y + (rect.h - block.height) / 2.0,
                ink(),
            );
        }
    }
}

/// Page title and intro; returns nothing (rows start at a fixed offset).
pub fn heading(canvas: &Canvas<'_>, mode: Mode, title: &str, intro: &str, s: f32) {
    let left = content_left(mode, s);
    let width = WIDTH * s - left - PAD * s;
    let (size, y) = if mode == Mode::Settings {
        (22.0, 28.0)
    } else {
        (26.0, 44.0)
    };
    if let Some(block) = canvas.text_sized(title, size * s, true) {
        canvas.draw_text(&block, left, y * s, ink());
    }
    if !intro.is_empty() {
        if let Some(block) = canvas.text_wrapped(intro, 15.0 * s, false, width) {
            canvas.draw_text(&block, left, (y + 48.0) * s, muted());
        }
    }
}

pub fn content_left(mode: Mode, s: f32) -> f32 {
    match mode {
        Mode::Settings => (SIDEBAR + PAD) * s,
        Mode::Setup => PAD * 1.5 * s,
    }
}

/// Label, hint and control of one row.
pub fn row(canvas: &Canvas<'_>, mode: Mode, control: RectF, look: &RowLook<'_>, s: f32) {
    let left = content_left(mode, s);
    let text_width = control.x - left - 24.0 * s;
    let label_y = control.y + 2.0 * s;
    if let Some(block) = canvas.text_wrapped(look.label, 15.0 * s, true, text_width) {
        canvas.draw_text(&block, left, label_y, ink());
    }
    if !look.hint.is_empty() {
        if let Some(block) = canvas.text_wrapped(look.hint, 12.5 * s, false, text_width) {
            canvas.draw_text(&block, left, label_y + 22.0 * s, muted());
        }
    }
    match look.kind {
        Kind::Toggle => toggle(canvas, control, matches!(look.value, Value::Bool(true)), s),
        Kind::Choice(options) => choice(canvas, control, options, look, s),
        Kind::Hotkey { .. } | Kind::Text | Kind::Folder => boxed(canvas, control, look, s),
    }
}

fn toggle(canvas: &Canvas<'_>, control: RectF, on: bool, s: f32) {
    let (w, h) = (46.0 * s, 24.0 * s);
    let pill = RectF::new(control.right() - w, control.y + (control.h - h) / 2.0, w, h);
    canvas.fill_rounded(pill, h / 2.0, if on { accent() } else { field_hover() });
    let knob = h - 8.0 * s;
    let x = if on {
        pill.right() - 4.0 * s - knob
    } else {
        pill.x + 4.0 * s
    };
    canvas.fill_ellipse(RectF::new(x, pill.y + 4.0 * s, knob, knob), ink());
    let text = if on { "On" } else { "Off" };
    if let Some(block) = canvas.text_sized(text, 13.0 * s, false) {
        canvas.draw_text(
            &block,
            pill.x - block.width - 10.0 * s,
            pill.y + (h - block.height) / 2.0,
            muted(),
        );
    }
}

fn choice(canvas: &Canvas<'_>, control: RectF, options: &[&str], look: &RowLook<'_>, s: f32) {
    canvas.fill_rounded(
        control,
        6.0 * s,
        if look.hovered { field_hover() } else { field() },
    );
    let index = match look.value {
        Value::Choice(i) => *i,
        _ => 0,
    };
    let text = options.get(index).copied().unwrap_or("");
    centered(canvas, text, control, 14.0 * s, ink());
    for (glyph, x) in [
        ("‹", control.x + 16.0 * s),
        ("›", control.right() - 16.0 * s),
    ] {
        if let Some(block) = canvas.text_sized(glyph, 20.0 * s, true) {
            let y = control.y + (control.h - block.height) / 2.0 - 1.0 * s;
            canvas.draw_text(&block, x - block.width / 2.0, y, muted());
        }
    }
}

fn boxed(canvas: &Canvas<'_>, control: RectF, look: &RowLook<'_>, s: f32) {
    let editing = look.editing.is_some();
    canvas.fill_rounded(
        control,
        6.0 * s,
        if look.hovered || editing {
            field_hover()
        } else {
            field()
        },
    );
    if editing {
        let frame = Stroke {
            width: 1.5 * s,
            color: accent(),
            dash: LineDash::Solid,
        };
        canvas.stroke_rect(control.inflate(-0.75 * s), &frame);
    }
    let inner = control.inflate(-12.0 * s);
    let (text, color) = display_text(look);
    let Some(block) = canvas.text_sized(&text, 14.0 * s, false) else {
        return;
    };
    let mut width = inner.w;
    if let Kind::Folder = look.kind {
        if let Some(change) = canvas.text_sized("Change…", 13.0 * s, true) {
            width -= change.width + 12.0 * s;
            let y = control.y + (control.h - change.height) / 2.0;
            canvas.draw_text(&change, inner.right() - change.width, y, accent());
        }
    }
    let y = control.y + (control.h - block.height) / 2.0;
    canvas.push_clip(RectF::new(inner.x, control.y, width, control.h));
    // Long text shows its end (the part being typed, or the folder's own name).
    let x = if block.width > width {
        inner.x + width - block.width
    } else {
        inner.x
    };
    canvas.draw_text(&block, x, y, color);
    if let (Some(_), Kind::Text) = (look.editing, look.kind) {
        let caret = (x + block.width + 1.0 * s).min(inner.x + width - 1.0 * s);
        let stroke = Stroke {
            width: 1.5 * s,
            color: accent(),
            dash: LineDash::Solid,
        };
        canvas.stroke_line((caret, y), (caret, y + block.height), &stroke);
    }
    canvas.pop_clip();
}

fn display_text(look: &RowLook<'_>) -> (String, Color) {
    match (look.kind, look.editing, look.value) {
        (Kind::Hotkey { optional }, Some(_), _) => {
            let hint = if optional {
                "Press a shortcut… (Backspace clears)"
            } else {
                "Press a shortcut… (Esc cancels)"
            };
            (hint.to_owned(), accent())
        }
        (Kind::Text, Some(text), _) => (text.to_owned(), ink()),
        (Kind::Hotkey { .. }, None, Value::Hotkey(Some(key))) => (key.to_string(), ink()),
        (Kind::Hotkey { .. }, None, Value::Hotkey(None)) => ("Not set".to_owned(), muted()),
        (_, _, Value::Text(text)) => (text.clone(), ink()),
        _ => (String::new(), ink()),
    }
}

/// Back / Next / "Take a capture now" buttons and the step counter (setup mode).
pub fn setup_buttons(
    canvas: &Canvas<'_>,
    targets: &[(RectF, Target)],
    step: (usize, usize),
    hover: Option<Target>,
    s: f32,
) {
    let (index, count) = step;
    for (rect, target) in targets {
        let (text, primary) = match target {
            Target::Next if index + 1 == count => ("Finish", true),
            Target::Next => ("Next", true),
            Target::Back => ("Back", false),
            Target::CaptureNow => ("Take a capture now", true),
            _ => continue,
        };
        let hovered = hover == Some(*target);
        let fill = match (primary, hovered) {
            (true, false) => accent(),
            (true, true) => Color::rgba8(0x6F, 0xB6, 0xFF, 0xFF),
            (false, false) => field(),
            (false, true) => field_hover(),
        };
        canvas.fill_rounded(*rect, 8.0 * s, fill);
        let color = if primary {
            Color::rgba8(0x10, 0x14, 0x1C, 0xFF)
        } else {
            ink()
        };
        centered(canvas, text, *rect, 14.0 * s, color);
    }
    let label = format!("Step {} of {count}", index + 1);
    if let Some(block) = canvas.text_sized(&label, 13.0 * s, false) {
        canvas.draw_text(
            &block,
            PAD * 1.5 * s,
            HEIGHT * s - 64.0 * s + (40.0 * s - block.height) / 2.0,
            muted(),
        );
    }
}

/// A short message at the bottom of the content area.
pub fn status(canvas: &Canvas<'_>, mode: Mode, text: &str, s: f32) {
    if let Some(block) = canvas.text_sized(text, 13.0 * s, false) {
        let x = content_left(mode, s);
        let y = if mode == Mode::Settings {
            HEIGHT * s - 40.0 * s
        } else {
            HEIGHT * s - 100.0 * s
        };
        canvas.draw_text(&block, x, y, accent());
    }
}

fn centered(canvas: &Canvas<'_>, text: &str, rect: RectF, px: f32, color: Color) {
    if let Some(block) = canvas.text_sized(text, px, true) {
        let (x, y) = (
            rect.x + (rect.w - block.width) / 2.0,
            rect.y + (rect.h - block.height) / 2.0,
        );
        canvas.draw_text(&block, x, y, color);
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{SETTINGS, SETUP};
    use super::*;

    fn assert_apart(targets: &[(RectF, Target)], what: &str) {
        let pairs = targets
            .iter()
            .enumerate()
            .flat_map(|(i, (a, _))| targets[i + 1..].iter().map(move |(b, _)| (*a, *b)));
        for (a, b) in pairs {
            let apart =
                a.right() <= b.x || b.right() <= a.x || a.bottom() <= b.y || b.bottom() <= a.y;
            assert!(apart, "{what}: {a:?} overlaps {b:?}");
        }
    }

    #[test]
    fn targets_never_overlap_and_choice_halves_rejoin() {
        let modes = [(Mode::Settings, &SETTINGS[..]), (Mode::Setup, &SETUP[..])];
        let pages = modes
            .iter()
            .flat_map(|(mode, pages)| (0..pages.len()).map(move |page| (*mode, *pages, page)));
        for (mode, pages, page) in pages {
            let targets = layout(mode, pages, page, 1.5);
            assert_apart(&targets, &format!("{mode:?} page {page}"));
            assert!(
                targets
                    .iter()
                    .all(|(r, _)| r.right() <= WIDTH * 1.5 && r.bottom() <= HEIGHT * 1.5)
            );
            let widths =
                (0..pages[page].rows.len()).map(|row| control_rect(&targets, row).map(|r| r.w));
            assert!(
                widths
                    .into_iter()
                    .all(|w| w.is_some_and(|w| (w - CONTROL_W * 1.5).abs() < 0.01))
            );
        }
    }

    #[test]
    fn setup_has_back_only_after_the_first_step_and_capture_on_the_last() {
        let first = layout(Mode::Setup, &SETUP, 0, 1.0);
        assert!(first.iter().all(|(_, t)| *t != Target::Back));
        let last = layout(Mode::Setup, &SETUP, SETUP.len() - 1, 1.0);
        assert!(last.iter().any(|(_, t)| *t == Target::CaptureNow));
        assert_eq!(hit(&last, (0.0, 0.0)), None);
    }
}
