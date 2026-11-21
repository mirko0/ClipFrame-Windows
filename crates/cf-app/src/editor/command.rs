//! Editor commands (from keys and toolbar buttons) and the style presets they cycle.

use cf_core::annotate::{Rgba, Style, Tool};
use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// Colour presets (keys 1–8).
pub const COLORS: [Rgba; 8] = [
    Rgba::new(0xE5, 0x39, 0x35, 0xFF),
    Rgba::new(0xFB, 0x8C, 0x00, 0xFF),
    Rgba::new(0xFD, 0xD8, 0x35, 0xFF),
    Rgba::new(0x43, 0xA0, 0x47, 0xFF),
    Rgba::new(0x1E, 0x88, 0xE5, 0xFF),
    Rgba::new(0x8E, 0x24, 0xAA, 0xFF),
    Rgba::new(0x10, 0x10, 0x10, 0xFF),
    Rgba::new(0xFF, 0xFF, 0xFF, 0xFF),
];
/// Stroke widths (`[` and `]`).
pub const WIDTHS: [f32; 3] = [2.0, 4.0, 8.0];
/// Opacity steps (`O` cycles).
pub const OPACITIES: [u8; 3] = [255, 160, 80];

/// Something the user asked the editor to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    Tool(Tool),
    Color(Rgba),
    /// Thinner (`-1`) or thicker (`+1`).
    WidthStep(i8),
    /// Next thickness, wrapping around (toolbar button).
    CycleWidth,
    CycleDash,
    ToggleFill,
    CycleOpacity,
    Undo,
    Redo,
    Copy,
    Save,
    /// Copy the text in the image (OCR).
    CopyText,
    /// Pin the edited image on screen and close the editor.
    Pin,
    /// Drag the edited image into another app (toolbar only).
    Drag,
    /// Redact emails, IPs, long numbers and tokens found by OCR.
    HideSensitive,
    Delete,
    Escape,
    /// Text input: finish the text (`Enter`), new line (`Shift+Enter`) or erase.
    Commit,
    NewLine,
    Backspace,
}

impl Command {
    /// Applies a style command to `style`; `None` if this isn't a style command.
    pub fn restyle(self, style: Style) -> Option<Style> {
        Some(match self {
            Self::Color(color) => Style {
                color: color.with_alpha(style.color.a),
                ..style
            },
            Self::WidthStep(step) => Style {
                width: step_width(style.width, step),
                ..style
            },
            Self::CycleWidth => Style {
                width: next_width(style.width),
                ..style
            },
            Self::CycleDash => Style {
                dash: style.dash.next(),
                ..style
            },
            Self::ToggleFill => Style {
                filled: !style.filled,
                ..style
            },
            Self::CycleOpacity => Style {
                color: style.color.with_alpha(next_opacity(style.color.a)),
                ..style
            },
            _ => return None,
        })
    }
}

fn width_index(width: f32) -> usize {
    WIDTHS
        .iter()
        .position(|w| (w - width).abs() < f32::EPSILON)
        .unwrap_or(1)
}

fn next_width(width: f32) -> f32 {
    WIDTHS[(width_index(width) + 1) % WIDTHS.len()]
}

fn step_width(width: f32, step: i8) -> f32 {
    let index = width_index(width);
    let next = if step < 0 {
        index.saturating_sub(1)
    } else {
        (index + 1).min(WIDTHS.len() - 1)
    };
    WIDTHS[next]
}

fn next_opacity(alpha: u8) -> u8 {
    let index = OPACITIES.iter().position(|&a| a == alpha).unwrap_or(0);
    OPACITIES[(index + 1) % OPACITIES.len()]
}

/// Maps a key press to a command. While `typing`, plain keys go to the text instead.
pub fn for_key(event: &KeyEvent, modifiers: ModifiersState, typing: bool) -> Option<Command> {
    if event.state != ElementState::Pressed {
        return None;
    }
    if modifiers.control_key() {
        return ctrl_command(&event.logical_key, modifiers.shift_key());
    }
    match &event.logical_key {
        Key::Named(named) => named_command(*named, modifiers.shift_key(), typing),
        Key::Character(text) if !typing && !modifiers.alt_key() => letter_command(text),
        _ => None,
    }
}

fn ctrl_command(key: &Key, shift: bool) -> Option<Command> {
    let Key::Character(text) = key else {
        return None;
    };
    match (text.to_ascii_lowercase().as_str(), shift) {
        ("z", false) => Some(Command::Undo),
        ("z", true) | ("y", _) => Some(Command::Redo),
        // Ctrl+Shift+C is often taken globally (by other tools), so text is Ctrl+Shift+T.
        ("c", _) => Some(Command::Copy),
        ("t", true) => Some(Command::CopyText),
        ("r", true) => Some(Command::HideSensitive),
        ("s", _) => Some(Command::Save),
        ("p", false) => Some(Command::Pin),
        _ => None,
    }
}

fn named_command(key: NamedKey, shift: bool, typing: bool) -> Option<Command> {
    match key {
        NamedKey::Escape => Some(Command::Escape),
        NamedKey::Enter if typing => Some(if shift {
            Command::NewLine
        } else {
            Command::Commit
        }),
        NamedKey::Backspace if typing => Some(Command::Backspace),
        NamedKey::Delete | NamedKey::Backspace => Some(Command::Delete),
        _ => None,
    }
}

fn letter_command(text: &str) -> Option<Command> {
    let tool = |t| Some(Command::Tool(t));
    match text.to_ascii_lowercase().as_str() {
        "v" => tool(Tool::Select),
        "p" => tool(Tool::Pen),
        "l" => tool(Tool::Line),
        "a" => tool(Tool::Arrow),
        "r" => tool(Tool::Rect),
        "e" => tool(Tool::Ellipse),
        "t" => tool(Tool::Text),
        "n" => tool(Tool::Step),
        "h" => tool(Tool::Highlighter),
        "b" => tool(Tool::Blur),
        "x" => tool(Tool::Redact),
        "s" => tool(Tool::Spotlight),
        "m" => tool(Tool::Magnify),
        "c" => tool(Tool::Crop),
        "d" => Some(Command::CycleDash),
        "f" => Some(Command::ToggleFill),
        "o" => Some(Command::CycleOpacity),
        "[" => Some(Command::WidthStep(-1)),
        "]" => Some(Command::WidthStep(1)),
        digit => digit
            .parse::<usize>()
            .ok()
            .and_then(|n| COLORS.get(n.checked_sub(1)?))
            .map(|c| Command::Color(*c)),
    }
}

#[cfg(test)]
mod tests {
    use cf_core::annotate::Dash;

    use super::*;

    #[test]
    fn letters_pick_tools_and_digits_pick_colours() {
        assert_eq!(letter_command("A"), Some(Command::Tool(Tool::Arrow)));
        assert_eq!(letter_command("c"), Some(Command::Tool(Tool::Crop)));
        assert_eq!(letter_command("2"), Some(Command::Color(COLORS[1])));
        assert_eq!(letter_command("9"), None);
        assert_eq!(letter_command("0"), None);
    }

    #[test]
    fn ctrl_shortcuts() {
        let z = Key::Character("z".into());
        assert_eq!(ctrl_command(&z, false), Some(Command::Undo));
        assert_eq!(ctrl_command(&z, true), Some(Command::Redo));
        let c = Key::Character("C".into());
        assert_eq!(ctrl_command(&c, false), Some(Command::Copy));
        let t = Key::Character("T".into());
        assert_eq!(ctrl_command(&t, true), Some(Command::CopyText));
        assert_eq!(ctrl_command(&t, false), None);
    }

    #[test]
    fn enter_commits_text_and_shift_enter_breaks_lines() {
        assert_eq!(
            named_command(NamedKey::Enter, false, true),
            Some(Command::Commit)
        );
        assert_eq!(
            named_command(NamedKey::Enter, true, true),
            Some(Command::NewLine)
        );
        assert_eq!(named_command(NamedKey::Enter, false, false), None);
        assert_eq!(
            named_command(NamedKey::Backspace, false, false),
            Some(Command::Delete)
        );
    }

    #[test]
    fn style_commands_keep_opacity_and_clamp_widths() {
        let style = Style {
            color: COLORS[0].with_alpha(80),
            ..Style::default()
        };
        let blue = Command::Color(COLORS[4]).restyle(style).map(|s| s.color);
        assert_eq!(blue, Some(COLORS[4].with_alpha(80)));
        let thick = Command::WidthStep(1).restyle(Style::default());
        assert_eq!(thick.map(|s| s.width), Some(8.0));
        let thickest = thick.and_then(|s| Command::WidthStep(1).restyle(s));
        assert_eq!(thickest.map(|s| s.width), Some(8.0));
        let wrapped = thickest.and_then(|s| Command::CycleWidth.restyle(s));
        assert_eq!(wrapped.map(|s| s.width), Some(2.0));
        let dashed = Command::CycleDash.restyle(Style::default());
        assert_eq!(dashed.map(|s| s.dash), Some(Dash::Dashed));
        let faded = Command::CycleOpacity.restyle(Style::default());
        assert_eq!(faded.map(|s| s.color.a), Some(160));
        assert_eq!(Command::Undo.restyle(Style::default()), None);
    }
}
