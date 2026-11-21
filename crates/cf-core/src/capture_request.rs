//! What the tray asks the capture helper to do, and how it travels between processes:
//! as a command-line argument when the helper is started, or as a message code when it is
//! already running.

/// Most recent captures that can be reopened by index.
pub const MAX_RECENT: u8 = 50;
const RECENT_BASE: u32 = 100;
const RECENT_ARG: &str = "open-recent-";

/// A capture the user asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaptureRequest {
    /// Open the overlay in the configured mode.
    Interactive,
    /// Capture the previous region again, without the overlay.
    RepeatLast,
    /// Open the `n`-th most recent capture (0 = newest) in the editor.
    OpenRecent(u8),
    /// Open the settings window.
    Settings,
    /// Open the first-run setup.
    Onboarding,
    /// Exit now (before the app is uninstalled or updated).
    Quit,
}

impl CaptureRequest {
    /// Message code (never 0, so an empty message can't be mistaken for a request).
    pub fn code(self) -> u32 {
        match self {
            Self::Interactive => 1,
            Self::RepeatLast => 2,
            Self::Settings => 3,
            Self::Onboarding => 4,
            Self::Quit => 5,
            Self::OpenRecent(n) => RECENT_BASE + u32::from(n),
        }
    }

    /// The request for a message code.
    pub fn from_code(code: u32) -> Option<Self> {
        match code {
            1 => Some(Self::Interactive),
            2 => Some(Self::RepeatLast),
            3 => Some(Self::Settings),
            4 => Some(Self::Onboarding),
            5 => Some(Self::Quit),
            c if (RECENT_BASE..RECENT_BASE + u32::from(MAX_RECENT)).contains(&c) => {
                u8::try_from(c - RECENT_BASE).ok().map(Self::OpenRecent)
            }
            _ => None,
        }
    }

    /// Command-line spelling.
    pub fn as_arg(self) -> String {
        match self {
            Self::Interactive => "interactive".into(),
            Self::RepeatLast => "repeat-last".into(),
            Self::Settings => "settings".into(),
            Self::Onboarding => "onboarding".into(),
            Self::Quit => "quit".into(),
            Self::OpenRecent(n) => format!("{RECENT_ARG}{n}"),
        }
    }

    /// The request for a command-line argument.
    pub fn from_arg(arg: &str) -> Option<Self> {
        match arg {
            "interactive" => Some(Self::Interactive),
            "repeat-last" => Some(Self::RepeatLast),
            "settings" => Some(Self::Settings),
            "onboarding" => Some(Self::Onboarding),
            "quit" => Some(Self::Quit),
            _ => arg
                .strip_prefix(RECENT_ARG)?
                .parse::<u8>()
                .ok()
                .filter(|&n| n < MAX_RECENT)
                .map(Self::OpenRecent),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [CaptureRequest; 7] = [
        CaptureRequest::Interactive,
        CaptureRequest::Quit,
        CaptureRequest::RepeatLast,
        CaptureRequest::Settings,
        CaptureRequest::Onboarding,
        CaptureRequest::OpenRecent(0),
        CaptureRequest::OpenRecent(MAX_RECENT - 1),
    ];

    #[test]
    fn codes_and_args_round_trip() {
        for request in ALL {
            assert_eq!(CaptureRequest::from_code(request.code()), Some(request));
            assert_eq!(CaptureRequest::from_arg(&request.as_arg()), Some(request));
        }
    }

    #[test]
    fn unknown_values_are_rejected() {
        assert_eq!(CaptureRequest::from_code(0), None);
        assert_eq!(CaptureRequest::from_code(100 + u32::from(MAX_RECENT)), None);
        assert_eq!(CaptureRequest::from_arg("everything"), None);
        assert_eq!(CaptureRequest::from_arg("open-recent-50"), None);
        assert_eq!(CaptureRequest::from_arg("open-recent-x"), None);
    }
}
