//! Registers the configured global hotkeys and maps hotkey ids back to actions.

use cf_core::config::HotkeyConfig;
use cf_core::hotkey::{Hotkey, Modifiers};
use global_hotkey::GlobalHotKeyManager;
use global_hotkey::hotkey::{Code, HotKey, Modifiers as NativeModifiers};

use crate::actions::Action;

/// A hotkey that could not be registered (usually because another app owns it).
#[derive(Debug)]
pub struct RegistrationFailure {
    pub hotkey: Hotkey,
    pub action: Action,
    pub reason: String,
}

struct Binding {
    native: HotKey,
    action: Action,
}

/// Owns the OS hotkey registrations.
pub struct HotkeyService {
    manager: GlobalHotKeyManager,
    bindings: Vec<Binding>,
}

impl HotkeyService {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            manager: GlobalHotKeyManager::new()?,
            bindings: Vec::new(),
        })
    }

    /// Replaces all registrations with the hotkeys in `config`; returns those that failed.
    pub fn apply(&mut self, config: &HotkeyConfig) -> Vec<RegistrationFailure> {
        self.unregister_all();
        let wanted = [
            (Some(config.capture), Action::Capture),
            (config.repeat_last, Action::RepeatLast),
        ];
        wanted
            .into_iter()
            .filter_map(|(hotkey, action)| self.register(hotkey?, action).err())
            .collect()
    }

    pub fn action_for(&self, id: u32) -> Option<Action> {
        self.bindings
            .iter()
            .find(|b| b.native.id() == id)
            .map(|b| b.action)
    }

    fn register(&mut self, hotkey: Hotkey, action: Action) -> Result<(), RegistrationFailure> {
        let fail = |reason: String| RegistrationFailure {
            hotkey,
            action,
            reason,
        };
        let native = to_native(hotkey).map_err(fail)?;
        self.manager
            .register(native)
            .map_err(|err| fail(err.to_string()))?;
        self.bindings.push(Binding { native, action });
        Ok(())
    }

    fn unregister_all(&mut self) {
        for binding in self.bindings.drain(..) {
            if let Err(err) = self.manager.unregister(binding.native) {
                tracing::warn!(action = ?binding.action, "could not unregister hotkey: {err}");
            }
        }
    }
}

fn to_native(hotkey: Hotkey) -> Result<HotKey, String> {
    let code: Code = hotkey
        .key()
        .code_name()
        .parse()
        .map_err(|_| format!("key {} is not supported", hotkey.key()))?;
    Ok(HotKey::new(
        Some(to_native_modifiers(hotkey.modifiers())),
        code,
    ))
}

fn to_native_modifiers(modifiers: Modifiers) -> NativeModifiers {
    let pairs = [
        (modifiers.ctrl, NativeModifiers::CONTROL),
        (modifiers.alt, NativeModifiers::ALT),
        (modifiers.shift, NativeModifiers::SHIFT),
        (modifiers.win, NativeModifiers::META),
    ];
    pairs
        .into_iter()
        .filter(|(held, _)| *held)
        .fold(NativeModifiers::empty(), |acc, (_, m)| acc | m)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn every_supported_key_maps_to_a_native_code() {
        let keys = (b'A'..=b'Z')
            .map(|b| format!("Ctrl+{}", char::from(b)))
            .chain((0..=9).map(|d| format!("Ctrl+{d}")))
            .chain((1..=24).map(|n| format!("F{n}")))
            .chain(
                [
                    "PrintScreen",
                    "Insert",
                    "Delete",
                    "Home",
                    "End",
                    "PageUp",
                    "PageDown",
                ]
                .map(String::from),
            )
            .chain(["Pause", "ScrollLock", "Ctrl+Space"].map(String::from));
        for text in keys {
            let hotkey: Hotkey = text.parse().unwrap();
            assert!(to_native(hotkey).is_ok(), "{text} has no native mapping");
        }
    }

    #[test]
    fn modifiers_map_to_native_flags() {
        let hotkey: Hotkey = "Ctrl+Alt+Shift+Win+F1".parse().unwrap();
        let native = to_native_modifiers(hotkey.modifiers());
        assert_eq!(
            native,
            NativeModifiers::CONTROL
                | NativeModifiers::ALT
                | NativeModifiers::SHIFT
                | NativeModifiers::META
        );
    }
}
