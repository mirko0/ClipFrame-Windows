//! Keyboard shortcuts inside the capture overlay.

use cf_core::capture_mode::CaptureMode;
use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// Nudge step with Shift held.
const BIG_STEP: i32 = 10;

/// What a key press asks the overlay to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Cancel,
    Confirm,
    /// Move the selection.
    Nudge(i32, i32),
    /// Resize the selection from its top-left corner.
    Grow(i32, i32),
    SetMode(CaptureMode),
    /// Capture every monitor immediately.
    CaptureAll,
    /// Copy the hex colour under the pointer.
    CopyColor,
    /// Space held: move the selection being drawn instead of resizing it.
    Reposition(bool),
}

/// Maps a key event to a command.
pub fn command_for(event: &KeyEvent, modifiers: ModifiersState) -> Option<Command> {
    if let Key::Named(NamedKey::Space) = event.logical_key {
        return Some(Command::Reposition(event.state == ElementState::Pressed));
    }
    if event.state != ElementState::Pressed {
        return None;
    }
    match &event.logical_key {
        Key::Named(named) => named_command(*named, modifiers),
        Key::Character(text) => character_command(text, modifiers),
        _ => None,
    }
}

fn named_command(key: NamedKey, modifiers: ModifiersState) -> Option<Command> {
    let step = if modifiers.shift_key() { BIG_STEP } else { 1 };
    let (dx, dy) = match key {
        NamedKey::Escape => return Some(Command::Cancel),
        NamedKey::Enter => return Some(Command::Confirm),
        NamedKey::ArrowLeft => (-step, 0),
        NamedKey::ArrowRight => (step, 0),
        NamedKey::ArrowUp => (0, -step),
        NamedKey::ArrowDown => (0, step),
        _ => return None,
    };
    Some(if modifiers.control_key() {
        Command::Grow(dx, dy)
    } else {
        Command::Nudge(dx, dy)
    })
}

fn character_command(text: &str, modifiers: ModifiersState) -> Option<Command> {
    let key = text.to_ascii_lowercase();
    if modifiers.control_key() {
        return (key == "c").then_some(Command::CopyColor);
    }
    if modifiers.alt_key() || modifiers.super_key() {
        return None;
    }
    match key.as_str() {
        "f" => Some(Command::SetMode(CaptureMode::FreeSelect)),
        "r" => Some(Command::SetMode(CaptureMode::Rectangle)),
        "m" => Some(Command::SetMode(CaptureMode::Monitor)),
        "a" => Some(Command::CaptureAll),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_nudge_and_shift_moves_further() {
        assert_eq!(
            named_command(NamedKey::ArrowLeft, ModifiersState::empty()),
            Some(Command::Nudge(-1, 0))
        );
        assert_eq!(
            named_command(NamedKey::ArrowDown, ModifiersState::SHIFT),
            Some(Command::Nudge(0, 10))
        );
    }

    #[test]
    fn ctrl_arrows_resize() {
        assert_eq!(
            named_command(NamedKey::ArrowRight, ModifiersState::CONTROL),
            Some(Command::Grow(1, 0))
        );
    }

    #[test]
    fn letters_switch_modes_case_insensitively() {
        assert_eq!(
            character_command("M", ModifiersState::SHIFT),
            Some(Command::SetMode(CaptureMode::Monitor))
        );
        assert_eq!(
            character_command("a", ModifiersState::empty()),
            Some(Command::CaptureAll)
        );
    }

    #[test]
    fn ctrl_c_copies_colour_and_other_ctrl_letters_do_nothing() {
        assert_eq!(
            character_command("c", ModifiersState::CONTROL),
            Some(Command::CopyColor)
        );
        assert_eq!(character_command("f", ModifiersState::CONTROL), None);
    }
}
