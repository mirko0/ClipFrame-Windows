//! Typing into text fields and recording shortcuts in the main window, and where the
//! window opens.

use cf_core::hotkey::{Hotkey, Key, Modifiers};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::KeyEvent;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key as LogicalKey, NamedKey, PhysicalKey};
use winit::window::{Window, WindowButtons};

use super::model::{Kind, Value};
use super::view::{self, Mode};
use super::{Focus, PrefsWindow};

impl PrefsWindow {
    pub(super) fn record_hotkey(&mut self, row: usize, event: &KeyEvent) {
        let Some(field) = self.field(row) else { return };
        let optional = matches!(field.kind(), Kind::Hotkey { optional: true });
        match &event.logical_key {
            LogicalKey::Named(NamedKey::Escape) => return,
            LogicalKey::Named(NamedKey::Backspace | NamedKey::Delete) if optional => {
                return self.apply(field, Value::Hotkey(None));
            }
            _ => {}
        }
        let PhysicalKey::Code(code) = event.physical_key else {
            self.focus = Some(Focus::Hotkey { row });
            return;
        };
        let Some(key) = Key::from_code_name(&format!("{code:?}")) else {
            // A modifier on its own (or an unusable key): keep listening.
            self.focus = Some(Focus::Hotkey { row });
            return;
        };
        let m = self.modifiers;
        let modifiers = Modifiers {
            ctrl: m.control_key(),
            alt: m.alt_key(),
            shift: m.shift_key(),
            win: m.super_key(),
        };
        match Hotkey::new(modifiers, key) {
            Ok(hotkey) => self.apply(field, Value::Hotkey(Some(hotkey))),
            Err(err) => {
                self.say(format!("{err}"));
                self.focus = Some(Focus::Hotkey { row });
            }
        }
    }

    pub(super) fn edit_text(&mut self, row: usize, mut text: String, event: &KeyEvent) {
        match &event.logical_key {
            LogicalKey::Named(NamedKey::Escape) => return,
            LogicalKey::Named(NamedKey::Enter) => {
                if let Some(field) = self.field(row) {
                    self.apply(field, Value::Text(text));
                }
                return;
            }
            LogicalKey::Named(NamedKey::Backspace) => {
                text.pop();
            }
            _ if !self.modifiers.control_key() => {
                if let Some(typed) = &event.text {
                    text.extend(typed.chars().filter(|c| !c.is_control()));
                }
            }
            _ => {}
        }
        self.focus = Some(Focus::Text { row, text });
    }

    /// Finishes text being typed (clicking elsewhere keeps what was typed).
    pub(super) fn commit_text(&mut self) {
        if let Some(Focus::Text { row, text }) = self.focus.take()
            && let Some(field) = self.field(row)
        {
            self.apply(field, Value::Text(text));
        }
    }
}

pub(super) fn scale_of(window: &Window) -> f32 {
    #[allow(clippy::cast_possible_truncation)] // scale factors are small
    let scale = window.scale_factor() as f32;
    scale
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // positive, screen-sized
fn to_px(v: f32) -> u32 {
    v.round().max(1.0) as u32
}

fn center_on_pointer_monitor(window: &Window, size: PhysicalSize<u32>) {
    let pointer = cf_platform::cursor::position();
    let monitors = cf_platform::monitors::enumerate().unwrap_or_default();
    let area = monitors
        .iter()
        .find(|m| pointer.is_some_and(|p| m.rect.contains(p)))
        .or_else(|| monitors.iter().find(|m| m.is_primary))
        .map(|m| m.work_area);
    if let Some(area) = area {
        let (w, h) = (
            i32::try_from(size.width).unwrap_or(0),
            i32::try_from(size.height).unwrap_or(0),
        );
        window.set_outer_position(PhysicalPosition::new(
            area.x + (area.width - w) / 2,
            area.y + (area.height - h) / 2,
        ));
    }
}

/// The (hidden) window, sized for the UI scale and centred on the pointer's monitor.
pub(super) fn create_window(
    event_loop: &ActiveEventLoop,
    mode: Mode,
) -> Result<Window, winit::error::OsError> {
    let title = match mode {
        Mode::Settings => "ClipFrame",
        Mode::Setup => "Set up ClipFrame",
    };
    let window = event_loop.create_window(
        Window::default_attributes()
            .with_title(title)
            .with_resizable(false)
            .with_enabled_buttons(WindowButtons::CLOSE | WindowButtons::MINIMIZE)
            .with_visible(false),
    )?;
    let scale = scale_of(&window);
    let size = PhysicalSize::new(to_px(view::WIDTH * scale), to_px(view::HEIGHT * scale));
    let _ = window.request_inner_size(size);
    center_on_pointer_monitor(&window, size);
    Ok(window)
}
