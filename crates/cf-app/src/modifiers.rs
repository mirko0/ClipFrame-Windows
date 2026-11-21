//! Modifier keys for keyboard shortcuts, read from Windows at key time (window modifier
//! events can be missed, which made Ctrl+C act as a plain "C").

use winit::keyboard::ModifiersState;

/// The modifiers held for the key being handled.
pub fn held() -> ModifiersState {
    let held = cf_platform::keyboard::held();
    let mut state = ModifiersState::empty();
    state.set(ModifiersState::CONTROL, held.ctrl);
    state.set(ModifiersState::ALT, held.alt);
    state.set(ModifiersState::SHIFT, held.shift);
    state.set(ModifiersState::SUPER, held.win);
    state
}
