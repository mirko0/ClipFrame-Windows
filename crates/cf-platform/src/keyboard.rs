//! Which modifier keys are held, straight from Windows.
//!
//! Tracking modifiers through window events can go stale (for example when Ctrl was pressed
//! before the window had focus), which turned Ctrl+C into a plain "C". `GetKeyState` reports
//! the state that belongs to the key message being handled, so it is always right there.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};

/// Modifier keys held down.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // four independent keys, not a state machine
pub struct Held {
    /// Either Ctrl key.
    pub ctrl: bool,
    /// Either Alt key.
    pub alt: bool,
    /// Either Shift key.
    pub shift: bool,
    /// Either Windows key.
    pub win: bool,
}

/// The modifiers held for the input message currently being processed on this thread.
pub fn held() -> Held {
    Held {
        ctrl: down(VK_CONTROL),
        alt: down(VK_MENU),
        shift: down(VK_SHIFT),
        win: down(VK_LWIN) || down(VK_RWIN),
    }
}

fn down(key: VIRTUAL_KEY) -> bool {
    // SAFETY: no preconditions; the high bit of the result means "down".
    let state = unsafe { GetKeyState(i32::from(key.0)) };
    state < 0
}
