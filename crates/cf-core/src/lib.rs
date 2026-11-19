//! Platform-independent ClipFrame logic.
//!
//! This crate must never depend on Win32, GPU or windowing crates, so everything in it
//! can be unit-tested on any machine.

#![warn(missing_docs)]

pub mod capture_mode;
pub mod config;
pub mod hotkey;
