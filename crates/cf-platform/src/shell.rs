//! Opening folders and files in other applications.

use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

/// Opens `dir` in File Explorer, creating it first if needed.
///
/// # Errors
/// When the folder cannot be created or Explorer cannot be started.
pub fn open_folder(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    Command::new("explorer.exe").arg(dir).spawn().map(drop)
}

/// Opens `file` in Notepad.
///
/// Used for the settings file until the settings window exists; Notepad is always present,
/// whereas `.json` often has no associated editor.
///
/// # Errors
/// When Notepad cannot be started.
pub fn open_in_text_editor(file: &Path) -> io::Result<()> {
    Command::new("notepad.exe").arg(file).spawn().map(drop)
}
