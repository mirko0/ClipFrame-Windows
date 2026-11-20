//! What the tray asks the capture helper to do, and how it travels between processes:
//! as a command-line argument when the helper is started, or as a message code when it is
//! already running.

/// A capture the user asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaptureRequest {
    /// Open the overlay in the configured mode.
    Interactive,
    /// Capture the previous region again, without the overlay.
    RepeatLast,
}

const ALL: [CaptureRequest; 2] = [CaptureRequest::Interactive, CaptureRequest::RepeatLast];

impl CaptureRequest {
    /// Message code (never 0, so an empty message can't be mistaken for a request).
    pub const fn code(self) -> u32 {
        match self {
            Self::Interactive => 1,
            Self::RepeatLast => 2,
        }
    }

    /// The request for a message code.
    pub fn from_code(code: u32) -> Option<Self> {
        ALL.into_iter().find(|r| r.code() == code)
    }

    /// Command-line spelling.
    pub const fn as_arg(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::RepeatLast => "repeat-last",
        }
    }

    /// The request for a command-line argument.
    pub fn from_arg(arg: &str) -> Option<Self> {
        ALL.into_iter().find(|r| r.as_arg() == arg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_and_args_round_trip() {
        for request in ALL {
            assert_eq!(CaptureRequest::from_code(request.code()), Some(request));
            assert_eq!(CaptureRequest::from_arg(request.as_arg()), Some(request));
        }
    }

    #[test]
    fn unknown_values_are_rejected() {
        assert_eq!(CaptureRequest::from_code(0), None);
        assert_eq!(CaptureRequest::from_arg("everything"), None);
    }
}
