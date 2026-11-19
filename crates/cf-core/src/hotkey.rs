//! Platform-independent hotkey descriptions such as `Ctrl+Shift+S`.
//!
//! Hotkeys are stored in the config as human-readable strings and parsed into [`Hotkey`].

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Modifier keys that must be held together with the main key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[allow(clippy::struct_excessive_bools)] // four independent flags is the natural model
pub struct Modifiers {
    /// Control key.
    pub ctrl: bool,
    /// Alt key.
    pub alt: bool,
    /// Shift key.
    pub shift: bool,
    /// Windows key.
    pub win: bool,
}

impl Modifiers {
    /// Returns `true` when no modifier is held.
    pub const fn is_empty(self) -> bool {
        !(self.ctrl || self.alt || self.shift || self.win)
    }
}

/// The main (non-modifier) key of a hotkey.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// An ASCII letter, stored uppercase (`'A'..='Z'`).
    Letter(char),
    /// A digit on the main keyboard row (`0..=9`).
    Digit(u8),
    /// A function key (`1..=24`).
    Function(u8),
    /// Print Screen.
    PrintScreen,
    /// Insert.
    Insert,
    /// Delete.
    Delete,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Pause / Break.
    Pause,
    /// Scroll Lock.
    ScrollLock,
    /// Space bar.
    Space,
}

/// Named keys: canonical label (also the W3C `code` name) and accepted aliases (lowercase).
const NAMED_KEYS: &[(Key, &str, &[&str])] = &[
    (
        Key::PrintScreen,
        "PrintScreen",
        &["prtsc", "prtscn", "print"],
    ),
    (Key::Insert, "Insert", &["ins"]),
    (Key::Delete, "Delete", &["del"]),
    (Key::Home, "Home", &[]),
    (Key::End, "End", &[]),
    (Key::PageUp, "PageUp", &["pgup"]),
    (Key::PageDown, "PageDown", &["pgdn"]),
    (Key::Pause, "Pause", &["break"]),
    (Key::ScrollLock, "ScrollLock", &["scrlk"]),
    (Key::Space, "Space", &[]),
];

impl Key {
    /// Parses a single key token, case-insensitively.
    fn parse(token: &str) -> Option<Self> {
        let lower = token.to_ascii_lowercase();
        if let Some(key) = parse_named(&lower) {
            return Some(key);
        }
        let mut chars = lower.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) if c.is_ascii_lowercase() => Some(Self::Letter(c.to_ascii_uppercase())),
            (Some(c), None) => c
                .to_digit(10)
                .and_then(|d| u8::try_from(d).ok())
                .map(Self::Digit),
            _ => parse_function(&lower),
        }
    }

    /// Keys that would hijack normal typing if bound without a modifier.
    const fn requires_modifier(self) -> bool {
        matches!(self, Self::Letter(_) | Self::Digit(_) | Self::Space)
    }

    /// The W3C UI Events `code` name, e.g. `KeyA`, `Digit1`, `F5`, `PrintScreen`.
    pub fn code_name(self) -> String {
        match self {
            Self::Letter(c) => format!("Key{c}"),
            Self::Digit(d) => format!("Digit{d}"),
            Self::Function(n) => format!("F{n}"),
            named => named_label(named).to_owned(),
        }
    }
}

fn parse_named(lower: &str) -> Option<Key> {
    NAMED_KEYS
        .iter()
        .find(|(_, name, aliases)| name.eq_ignore_ascii_case(lower) || aliases.contains(&lower))
        .map(|(key, ..)| *key)
}

fn named_label(key: Key) -> &'static str {
    NAMED_KEYS
        .iter()
        .find(|(k, ..)| *k == key)
        .map_or("?", |(_, name, _)| name)
}

fn parse_function(lower: &str) -> Option<Key> {
    let number: u8 = lower.strip_prefix('f')?.parse().ok()?;
    (1..=24).contains(&number).then_some(Key::Function(number))
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Letter(c) => write!(f, "{c}"),
            Self::Digit(d) => write!(f, "{d}"),
            Self::Function(n) => write!(f, "F{n}"),
            named => f.write_str(named_label(*named)),
        }
    }
}

/// Why a hotkey string could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HotkeyParseError {
    /// The string was empty or whitespace.
    #[error("hotkey is empty")]
    Empty,
    /// A token is neither a modifier nor a supported key.
    #[error("unknown key or modifier `{0}`")]
    UnknownToken(String),
    /// The same modifier was given twice.
    #[error("modifier `{0}` appears more than once")]
    DuplicateModifier(String),
    /// Only modifiers were given.
    #[error("hotkey has no main key, only modifiers")]
    MissingKey,
    /// More than one non-modifier key was given.
    #[error("hotkey has more than one main key")]
    MultipleKeys,
    /// The key would interfere with typing unless combined with a modifier.
    #[error("`{0}` needs at least one modifier (Ctrl, Alt, Shift or Win)")]
    NeedsModifier(Key),
}

/// A validated global hotkey: zero or more modifiers plus exactly one main key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hotkey {
    modifiers: Modifiers,
    key: Key,
}

/// The default capture hotkey, `Alt+Shift+S`.
pub const DEFAULT_CAPTURE_HOTKEY: Hotkey = Hotkey {
    modifiers: Modifiers {
        ctrl: false,
        alt: true,
        shift: true,
        win: false,
    },
    key: Key::Letter('S'),
};

impl Hotkey {
    /// Creates a hotkey, rejecting typing keys that have no modifier.
    ///
    /// # Errors
    /// [`HotkeyParseError::NeedsModifier`] for letters, digits or space without a modifier.
    pub fn new(modifiers: Modifiers, key: Key) -> Result<Self, HotkeyParseError> {
        if modifiers.is_empty() && key.requires_modifier() {
            return Err(HotkeyParseError::NeedsModifier(key));
        }
        Ok(Self { modifiers, key })
    }

    /// The modifier keys.
    pub const fn modifiers(self) -> Modifiers {
        self.modifiers
    }

    /// The main key.
    pub const fn key(self) -> Key {
        self.key
    }
}

impl FromStr for Hotkey {
    type Err = HotkeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.trim().is_empty() {
            return Err(HotkeyParseError::Empty);
        }
        let mut modifiers = Modifiers::default();
        let mut key = None;
        for token in s.split('+').map(str::trim) {
            if apply_modifier(&mut modifiers, token)? {
                continue;
            }
            let parsed = Key::parse(token)
                .ok_or_else(|| HotkeyParseError::UnknownToken(token.to_owned()))?;
            if key.replace(parsed).is_some() {
                return Err(HotkeyParseError::MultipleKeys);
            }
        }
        Self::new(modifiers, key.ok_or(HotkeyParseError::MissingKey)?)
    }
}

/// Sets the modifier named by `token`. Returns `Ok(false)` when `token` is not a modifier.
fn apply_modifier(modifiers: &mut Modifiers, token: &str) -> Result<bool, HotkeyParseError> {
    let flag = match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => &mut modifiers.ctrl,
        "alt" => &mut modifiers.alt,
        "shift" => &mut modifiers.shift,
        "win" | "super" | "meta" => &mut modifiers.win,
        _ => return Ok(false),
    };
    if *flag {
        return Err(HotkeyParseError::DuplicateModifier(token.to_owned()));
    }
    *flag = true;
    Ok(true)
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = self.modifiers;
        let names = [
            (m.ctrl, "Ctrl"),
            (m.alt, "Alt"),
            (m.shift, "Shift"),
            (m.win, "Win"),
        ];
        for (_, name) in names.iter().filter(|(held, _)| *held) {
            write!(f, "{name}+")?;
        }
        write!(f, "{}", self.key)
    }
}

impl Serialize for Hotkey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Hotkey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use proptest::prelude::*;

    use super::*;

    fn parse(s: &str) -> Result<Hotkey, HotkeyParseError> {
        s.parse()
    }

    #[test]
    fn parses_modifiers_and_letter() {
        let hotkey = parse("Ctrl+Shift+S").unwrap();
        assert!(hotkey.modifiers().ctrl && hotkey.modifiers().shift);
        assert!(!hotkey.modifiers().alt && !hotkey.modifiers().win);
        assert_eq!(hotkey.key(), Key::Letter('S'));
    }

    #[test]
    fn parsing_is_case_and_whitespace_insensitive() {
        assert_eq!(
            parse(" ctrl + shift + s ").unwrap(),
            parse("Ctrl+Shift+S").unwrap()
        );
    }

    #[test]
    fn accepts_key_aliases() {
        assert_eq!(parse("PrtSc").unwrap().key(), Key::PrintScreen);
        assert_eq!(parse("Ctrl+PgDn").unwrap().key(), Key::PageDown);
        assert_eq!(
            parse("Control+Super+Del").unwrap().to_string(),
            "Ctrl+Win+Delete"
        );
    }

    #[test]
    fn displays_modifiers_in_canonical_order() {
        assert_eq!(
            parse("Win+Shift+Alt+Ctrl+F12").unwrap().to_string(),
            "Ctrl+Alt+Shift+Win+F12"
        );
    }

    #[test]
    fn allows_print_screen_and_function_keys_without_modifier() {
        assert!(parse("PrintScreen").is_ok());
        assert!(parse("F9").is_ok());
    }

    #[test]
    fn rejects_typing_keys_without_modifier() {
        assert_eq!(
            parse("S"),
            Err(HotkeyParseError::NeedsModifier(Key::Letter('S')))
        );
        assert_eq!(
            parse("5"),
            Err(HotkeyParseError::NeedsModifier(Key::Digit(5)))
        );
    }

    #[test]
    fn reports_malformed_input() {
        assert_eq!(parse(""), Err(HotkeyParseError::Empty));
        assert_eq!(parse("Ctrl+Shift"), Err(HotkeyParseError::MissingKey));
        assert_eq!(parse("Ctrl+A+B"), Err(HotkeyParseError::MultipleKeys));
        assert_eq!(
            parse("Ctrl+ctrl+A"),
            Err(HotkeyParseError::DuplicateModifier("ctrl".into()))
        );
        assert_eq!(
            parse("Ctrl+F25"),
            Err(HotkeyParseError::UnknownToken("F25".into()))
        );
        assert_eq!(
            parse("Ctrl++"),
            Err(HotkeyParseError::UnknownToken(String::new()))
        );
    }

    #[test]
    fn code_names_follow_w3c_ui_events() {
        assert_eq!(Key::Letter('Q').code_name(), "KeyQ");
        assert_eq!(Key::Digit(0).code_name(), "Digit0");
        assert_eq!(Key::Function(24).code_name(), "F24");
        assert_eq!(Key::PrintScreen.code_name(), "PrintScreen");
    }

    #[test]
    fn serde_uses_display_string() {
        let json = serde_json::to_string(&DEFAULT_CAPTURE_HOTKEY).unwrap();
        assert_eq!(json, "\"Alt+Shift+S\"");
        assert_eq!(
            serde_json::from_str::<Hotkey>(&json).unwrap(),
            DEFAULT_CAPTURE_HOTKEY
        );
    }

    #[test]
    fn deserializing_invalid_string_fails() {
        assert!(serde_json::from_str::<Hotkey>("\"Ctrl+Nope\"").is_err());
    }

    fn any_key() -> impl Strategy<Value = Key> {
        prop_oneof![
            (b'A'..=b'Z').prop_map(|b| Key::Letter(char::from(b))),
            (0u8..=9).prop_map(Key::Digit),
            (1u8..=24).prop_map(Key::Function),
            proptest::sample::select(NAMED_KEYS.iter().map(|(k, ..)| *k).collect::<Vec<_>>()),
        ]
    }

    fn any_modifiers() -> impl Strategy<Value = Modifiers> {
        any::<[bool; 4]>().prop_map(|[ctrl, alt, shift, win]| Modifiers {
            ctrl,
            alt,
            shift,
            win,
        })
    }

    proptest! {
        #[test]
        fn display_then_parse_round_trips(modifiers in any_modifiers(), key in any_key()) {
            if let Ok(hotkey) = Hotkey::new(modifiers, key) {
                prop_assert_eq!(parse(&hotkey.to_string()), Ok(hotkey));
            }
        }
    }
}
