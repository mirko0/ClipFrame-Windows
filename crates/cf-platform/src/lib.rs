//! Windows platform services for ClipFrame.
//!
//! Every `unsafe` Win32/WinRT call in the project lives in this crate, wrapped in a safe
//! function with Rust types. Each `unsafe` block carries a `// SAFETY:` comment.

#![warn(missing_docs)]

pub mod paths;
pub mod shell;
#[cfg(windows)]
pub mod single_instance;
