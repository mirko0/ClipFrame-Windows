//! Platform-independent ClipFrame logic.
//!
//! This crate must never depend on Win32, GPU or windowing crates, so everything in it
//! can be unit-tested on any machine.

#![warn(missing_docs)]

pub mod annotate;
pub mod capture_mode;
pub mod capture_request;
pub mod config;
pub mod encode;
pub mod geometry;
pub mod history;
pub mod hotkey;
pub mod image;
pub mod naming;
pub mod ocr;
pub mod output;
pub mod retention;
pub mod selection;
pub mod state;
pub mod targets;
