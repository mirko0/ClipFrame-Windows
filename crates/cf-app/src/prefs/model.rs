//! What the settings window and first-run setup show: pages of rows, each bound to one
//! setting. Pure (no drawing), so the bindings are unit-tested.

use std::path::{Path, PathBuf};

use cf_core::capture_mode::CaptureMode;
use cf_core::config::{Config, KeepGpuReady};
use cf_core::hotkey::Hotkey;

/// One setting the user can change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    CaptureHotkey,
    /// Win+Shift+S opens ClipFrame instead of the Snipping Tool.
    ReplaceSnipping,
    RepeatHotkey,
    StartMode,
    IncludeCursor,
    Delay,
    AutoSave,
    SaveDir,
    FileName,
    DateFolders,
    DeleteAfter,
    KeepRecent,
    CopyOnCapture,
    OpenEditor,
    UpdateOnEdit,
    Toast,
    SpotlightDim,
    /// Not stored in the config: the per-user Run key.
    StartWithWindows,
    KeepGpu,
}

/// How a field is edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Toggle,
    Choice(&'static [&'static str]),
    Hotkey { optional: bool },
    Text,
    Folder,
}

/// A field's current value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Bool(bool),
    Choice(usize),
    Hotkey(Option<Hotkey>),
    Text(String),
}

/// Facts outside the config that some fields show.
#[derive(Debug, Clone)]
pub struct Context {
    /// Whether ClipFrame starts with Windows.
    pub startup: bool,
    /// The captures folder used when none is configured.
    pub default_dir: PathBuf,
}

pub struct Row {
    pub field: Field,
    pub label: &'static str,
    pub hint: &'static str,
}

pub struct Page {
    pub title: &'static str,
    /// Shown under the title; `{hotkey}` becomes the capture shortcut.
    pub intro: &'static str,
    pub rows: &'static [Row],
}

const MODES: [CaptureMode; 7] = [
    CaptureMode::FreeSelect,
    CaptureMode::Rectangle,
    CaptureMode::Window,
    CaptureMode::Element,
    CaptureMode::BrowserContent,
    CaptureMode::Monitor,
    CaptureMode::AllMonitors,
];
const MODE_LABELS: [&str; 7] = [
    "Free select",
    "Rectangle",
    "Window",
    "UI element",
    "Browser page",
    "Monitor",
    "All monitors",
];
const DELAYS: [u8; 4] = [0, 3, 5, 10];
const DELAY_LABELS: [&str; 4] = ["Off", "3 seconds", "5 seconds", "10 seconds"];
const DELETE_DAYS: [u16; 4] = [0, 7, 30, 90];
const DELETE_LABELS: [&str; 4] = ["Never", "After 7 days", "After 30 days", "After 90 days"];
const RECENT: [u16; 4] = [0, 10, 20, 50];
const RECENT_LABELS: [&str; 4] = ["Off", "10", "20", "50"];
const DIMS: [u8; 3] = [35, 55, 75];
const DIM_LABELS: [&str; 3] = ["Light", "Medium", "Strong"];
const GPU: [KeepGpuReady; 3] = [
    KeepGpuReady::Never,
    KeepGpuReady::FiveMinutes,
    KeepGpuReady::Always,
];
const GPU_LABELS: [&str; 3] = ["Release right away", "Keep for 5 minutes", "Always keep"];

impl Field {
    pub const fn kind(self) -> Kind {
        match self {
            Self::CaptureHotkey => Kind::Hotkey { optional: false },
            Self::RepeatHotkey => Kind::Hotkey { optional: true },
            Self::StartMode => Kind::Choice(&MODE_LABELS),
            Self::Delay => Kind::Choice(&DELAY_LABELS),
            Self::KeepRecent => Kind::Choice(&RECENT_LABELS),
            Self::DeleteAfter => Kind::Choice(&DELETE_LABELS),
            Self::SpotlightDim => Kind::Choice(&DIM_LABELS),
            Self::KeepGpu => Kind::Choice(&GPU_LABELS),
            Self::SaveDir => Kind::Folder,
            Self::FileName => Kind::Text,
            Self::IncludeCursor
            | Self::AutoSave
            | Self::DateFolders
            | Self::CopyOnCapture
            | Self::OpenEditor
            | Self::UpdateOnEdit
            | Self::Toast
            | Self::ReplaceSnipping
            | Self::StartWithWindows => Kind::Toggle,
        }
    }

    pub fn get(self, c: &Config, ctx: &Context) -> Value {
        match self {
            Self::CaptureHotkey => Value::Hotkey(Some(c.hotkeys.capture)),
            Self::RepeatHotkey => Value::Hotkey(c.hotkeys.repeat_last),
            Self::StartMode => Value::Choice(
                MODES
                    .iter()
                    .position(|m| *m == c.capture.default_mode)
                    .unwrap_or(0),
            ),
            Self::Delay => Value::Choice(nearest(&DELAYS, c.capture.delay_secs)),
            Self::KeepRecent => Value::Choice(nearest(&RECENT, c.history.keep)),
            Self::DeleteAfter => Value::Choice(nearest(&DELETE_DAYS, c.output.delete_after_days)),
            Self::ReplaceSnipping => Value::Bool(c.hotkeys.replace_snipping_tool),
            Self::SpotlightDim => Value::Choice(nearest(&DIMS, c.editor.spotlight_dim)),
            Self::KeepGpu => Value::Choice(
                GPU.iter()
                    .position(|g| *g == c.performance.keep_gpu_ready)
                    .unwrap_or(1),
            ),
            Self::SaveDir => Value::Text(
                c.output
                    .captures_dir(&ctx.default_dir)
                    .display()
                    .to_string(),
            ),
            Self::FileName => Value::Text(c.output.file_name.clone()),
            Self::IncludeCursor => Value::Bool(c.capture.include_cursor),
            Self::AutoSave => Value::Bool(c.output.auto_save),
            Self::DateFolders => Value::Bool(c.output.date_folders),
            Self::CopyOnCapture => Value::Bool(c.clipboard.copy_on_capture),
            Self::OpenEditor => Value::Bool(c.editor.open_after_capture),
            Self::UpdateOnEdit => Value::Bool(c.clipboard.update_on_edit),
            Self::Toast => Value::Bool(c.notifications.toast),
            Self::StartWithWindows => Value::Bool(ctx.startup),
        }
    }

    /// Stores `value`. `StartWithWindows` isn't part of the config; the caller applies it.
    ///
    /// # Errors
    /// A message for the user when the value can't be used.
    pub fn set(self, c: &mut Config, value: Value, ctx: &Context) -> Result<(), &'static str> {
        match (self, value) {
            (Self::CaptureHotkey, Value::Hotkey(Some(key))) => {
                if c.hotkeys.repeat_last == Some(key) {
                    return Err("That shortcut is already used for “Repeat last region”.");
                }
                c.hotkeys.capture = key;
            }
            (Self::CaptureHotkey, Value::Hotkey(None)) => {
                return Err("The capture shortcut can't be empty.");
            }
            (Self::RepeatHotkey, Value::Hotkey(key)) => {
                if key == Some(c.hotkeys.capture) {
                    return Err("That shortcut is already used for capturing.");
                }
                c.hotkeys.repeat_last = key;
            }
            (Self::StartMode, Value::Choice(i)) => c.capture.default_mode = *pick(&MODES, i),
            (Self::Delay, Value::Choice(i)) => c.capture.delay_secs = *pick(&DELAYS, i),
            (Self::KeepRecent, Value::Choice(i)) => c.history.keep = *pick(&RECENT, i),
            (Self::DeleteAfter, Value::Choice(i)) => {
                c.output.delete_after_days = *pick(&DELETE_DAYS, i);
            }
            (Self::ReplaceSnipping, Value::Bool(b)) => c.hotkeys.replace_snipping_tool = b,
            (Self::SpotlightDim, Value::Choice(i)) => c.editor.spotlight_dim = *pick(&DIMS, i),
            (Self::KeepGpu, Value::Choice(i)) => c.performance.keep_gpu_ready = *pick(&GPU, i),
            (Self::SaveDir, Value::Text(path)) => {
                let path = PathBuf::from(path.trim());
                c.output.save_dir = (path != ctx.default_dir).then_some(path);
            }
            (Self::FileName, Value::Text(text)) => c.output.file_name = text,
            (Self::IncludeCursor, Value::Bool(b)) => c.capture.include_cursor = b,
            (Self::AutoSave, Value::Bool(b)) => c.output.auto_save = b,
            (Self::DateFolders, Value::Bool(b)) => c.output.date_folders = b,
            (Self::CopyOnCapture, Value::Bool(b)) => c.clipboard.copy_on_capture = b,
            (Self::OpenEditor, Value::Bool(b)) => c.editor.open_after_capture = b,
            (Self::UpdateOnEdit, Value::Bool(b)) => c.clipboard.update_on_edit = b,
            (Self::Toast, Value::Bool(b)) => c.notifications.toast = b,
            (Self::StartWithWindows, Value::Bool(_)) => {}
            _ => return Err("Unexpected value for this setting."),
        }
        Ok(())
    }
}

fn nearest<T: Copy + Into<i64>>(options: &[T], value: T) -> usize {
    let value: i64 = value.into();
    options
        .iter()
        .enumerate()
        .min_by_key(|(_, o)| ((**o).into() - value).abs())
        .map_or(0, |(i, _)| i)
}

fn pick<T>(options: &[T], index: usize) -> &T {
    &options[index.min(options.len() - 1)]
}

/// The captures folder a path text stands for (for the folder picker's starting point).
pub fn folder_of(text: &str, default_dir: &Path) -> PathBuf {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        default_dir.to_path_buf()
    } else {
        PathBuf::from(trimmed)
    }
}

const fn row(field: Field, label: &'static str, hint: &'static str) -> Row {
    Row { field, label, hint }
}

/// The main window's pages (shown with a sidebar): home, then the settings.
pub const SETTINGS: [Page; 5] = [
    Page {
        title: "Home",
        intro: "Press {hotkey} anywhere to capture.",
        rows: &[],
    },
    Page {
        title: "Capture",
        intro: "",
        rows: &[
            row(
                Field::CaptureHotkey,
                "Capture shortcut",
                "Opens the capture overlay from anywhere.",
            ),
            row(
                Field::ReplaceSnipping,
                "Use Win+Shift+S",
                "Opens ClipFrame instead of the Snipping Tool, which then never starts.",
            ),
            row(
                Field::RepeatHotkey,
                "Repeat last region",
                "Captures the previous region again. Backspace clears.",
            ),
            row(
                Field::StartMode,
                "Start in",
                "How the overlay picks what to capture.",
            ),
            row(Field::IncludeCursor, "Include the mouse pointer", ""),
            row(Field::Delay, "Delay", "Wait before the screen is frozen."),
        ],
    },
    Page {
        title: "Saving",
        intro: "",
        rows: &[
            row(Field::AutoSave, "Save every capture", ""),
            row(Field::SaveDir, "Folder", ""),
            row(
                Field::FileName,
                "File name",
                "{date} {time} {app} {title} {n}",
            ),
            row(
                Field::DateFolders,
                "Monthly folders",
                "Save into a YYYY-MM subfolder.",
            ),
            row(
                Field::DeleteAfter,
                "Delete old captures",
                "Only captures ClipFrame saved.",
            ),
            row(
                Field::KeepRecent,
                "Recent captures",
                "Kept for the tray's Recent menu.",
            ),
        ],
    },
    Page {
        title: "After capture",
        intro: "",
        rows: &[
            row(Field::CopyOnCapture, "Copy to the clipboard", ""),
            row(Field::OpenEditor, "Open the editor", ""),
            row(
                Field::UpdateOnEdit,
                "Update the clipboard while editing",
                "",
            ),
            row(
                Field::Toast,
                "Show a notification",
                "When the editor doesn't open.",
            ),
            row(Field::SpotlightDim, "Spotlight dimming", ""),
        ],
    },
    Page {
        title: "System",
        intro: "",
        rows: &[
            row(Field::StartWithWindows, "Start with Windows", ""),
            row(
                Field::KeepGpu,
                "Keep graphics ready",
                "Faster next capture, or less memory.",
            ),
        ],
    },
];

/// The home page in [`SETTINGS`].
pub const HOME: usize = 0;
/// The first real settings page in [`SETTINGS`].
pub const FIRST_SETTINGS: usize = 1;

/// The first-run setup, one page per step.
pub const SETUP: [Page; 7] = [
    Page {
        title: "Welcome to ClipFrame",
        intro: "A fast screenshot tool that waits in the tray. These few steps set it up — \
                everything can be changed later in Settings.",
        rows: &[],
    },
    Page {
        title: "Your capture shortcut",
        intro: "Press it anywhere to start a capture. To change it, click the box and press \
                a new shortcut.",
        rows: &[row(Field::CaptureHotkey, "Capture shortcut", "")],
    },
    Page {
        title: "Where captures go",
        intro: "Captures are copied to the clipboard; saving a file as well is up to you.",
        rows: &[
            row(Field::AutoSave, "Save every capture", ""),
            row(Field::SaveDir, "Folder", ""),
            row(
                Field::FileName,
                "File name",
                "{date} {time} {app} {title} {n}",
            ),
        ],
    },
    Page {
        title: "Clipboard and editor",
        intro: "The editor opens after each capture for arrows, text, redaction and more.",
        rows: &[
            row(Field::CopyOnCapture, "Copy to the clipboard", ""),
            row(Field::OpenEditor, "Open the editor", ""),
            row(
                Field::UpdateOnEdit,
                "Update the clipboard while editing",
                "",
            ),
        ],
    },
    Page {
        title: "How a capture starts",
        intro: "Free select lets you drag any area, or click a window to take all of it. \
                You can switch modes with keys while capturing.",
        rows: &[row(Field::StartMode, "Start in", "")],
    },
    Page {
        title: "Start with Windows",
        intro: "So the shortcut works right after you sign in.",
        rows: &[row(Field::StartWithWindows, "Start with Windows", "")],
    },
    Page {
        title: "All set",
        intro: "Press {hotkey} to capture. ClipFrame waits in the tray — right-click its icon \
                for recent captures and settings.",
        rows: &[],
    },
];

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn ctx() -> Context {
        Context {
            startup: false,
            default_dir: PathBuf::from(r"C:\Pictures\ClipFrame"),
        }
    }

    #[test]
    fn every_row_round_trips_its_own_value() {
        let ctx = ctx();
        for page in SETTINGS.iter().chain(SETUP.iter()) {
            for row in page.rows {
                let mut config = Config::default();
                let value = row.field.get(&config, &ctx);
                row.field.set(&mut config, value.clone(), &ctx).unwrap();
                assert_eq!(row.field.get(&config, &ctx), value, "{:?}", row.field);
            }
        }
    }

    #[test]
    fn choices_map_to_config_values() {
        let (ctx, mut c) = (ctx(), Config::default());
        Field::Delay.set(&mut c, Value::Choice(3), &ctx).unwrap();
        assert_eq!(c.capture.delay_secs, 10);
        c.capture.delay_secs = 4; // hand-edited: shown as the nearest option
        assert_eq!(Field::Delay.get(&c, &ctx), Value::Choice(1));
        Field::StartMode
            .set(&mut c, Value::Choice(99), &ctx)
            .unwrap();
        assert_eq!(c.capture.default_mode, CaptureMode::AllMonitors);
    }

    #[test]
    fn clean_up_and_snipping_settings_map_to_the_config() {
        let (ctx, mut c) = (ctx(), Config::default());
        assert_eq!(
            Field::DeleteAfter.get(&c, &ctx),
            Value::Choice(0),
            "never by default"
        );
        Field::DeleteAfter
            .set(&mut c, Value::Choice(2), &ctx)
            .unwrap();
        assert_eq!(c.output.delete_after_days, 30);
        Field::ReplaceSnipping
            .set(&mut c, Value::Bool(true), &ctx)
            .unwrap();
        assert!(c.hotkeys.replace_snipping_tool);
        assert_eq!(SETTINGS[HOME].title, "Home");
        assert!(SETTINGS[HOME].rows.is_empty());
    }

    #[test]
    fn hotkeys_cannot_collide_or_be_empty() {
        let (ctx, mut c) = (ctx(), Config::default());
        let capture = c.hotkeys.capture;
        assert!(
            Field::RepeatHotkey
                .set(&mut c, Value::Hotkey(Some(capture)), &ctx)
                .is_err()
        );
        assert!(
            Field::CaptureHotkey
                .set(&mut c, Value::Hotkey(None), &ctx)
                .is_err()
        );
        let other: Hotkey = "Ctrl+Shift+R".parse().unwrap();
        Field::RepeatHotkey
            .set(&mut c, Value::Hotkey(Some(other)), &ctx)
            .unwrap();
        assert!(
            Field::CaptureHotkey
                .set(&mut c, Value::Hotkey(Some(other)), &ctx)
                .is_err()
        );
        Field::RepeatHotkey
            .set(&mut c, Value::Hotkey(None), &ctx)
            .unwrap();
        assert_eq!(c.hotkeys.repeat_last, None);
    }

    #[test]
    fn the_default_folder_is_stored_as_none() {
        let (ctx, mut c) = (ctx(), Config::default());
        Field::SaveDir
            .set(&mut c, Value::Text(r"D:\Shots ".into()), &ctx)
            .unwrap();
        assert_eq!(c.output.save_dir, Some(PathBuf::from(r"D:\Shots")));
        let default = ctx.default_dir.display().to_string();
        Field::SaveDir
            .set(&mut c, Value::Text(default), &ctx)
            .unwrap();
        assert_eq!(c.output.save_dir, None);
    }
}
