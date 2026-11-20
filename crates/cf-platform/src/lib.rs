//! Windows platform services for ClipFrame.
//!
//! Every `unsafe` Win32/WinRT call in the project lives in this crate, wrapped in a safe
//! function with Rust types. Each `unsafe` block carries a `// SAFETY:` comment.

#![warn(missing_docs)]

pub mod paths;
pub mod shell;

#[cfg(windows)]
pub mod clipboard;
#[cfg(windows)]
pub mod cursor;
#[cfg(windows)]
pub mod focus;
#[cfg(windows)]
pub mod ipc;
#[cfg(windows)]
pub mod monitors;
#[cfg(windows)]
pub mod screen_capture;
#[cfg(windows)]
pub mod single_instance;
#[cfg(windows)]
pub mod time;
