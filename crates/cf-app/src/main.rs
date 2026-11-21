//! ClipFrame entry point. One executable, two roles:
//! - **tray** (no arguments): tray icon, hotkeys, settings; never touches the GPU.
//! - **capture helper** (`--capture-helper <request>`): overlay, GPU and delivery; started on
//!   demand by the tray and exits when idle.

// Release builds have no console window; debug builds keep one for log output.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod app;
mod capture;
mod editor;
mod helper;
mod helper_client;
mod hotkeys;
mod logging;
mod pin;
mod settings;
mod tray;
mod tray_icon_image;

use std::process::ExitCode;

use anyhow::Context as _;
use cf_core::capture_request::CaptureRequest;
use cf_platform::paths::AppPaths;
use cf_platform::single_instance::{InstanceError, SingleInstance};

/// Per-session mutex name; one tray per logged-in user.
const INSTANCE_NAME: &str = r"Local\ClipFrame.SingleInstance";

/// Which process this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Tray,
    Helper(CaptureRequest),
}

impl Role {
    fn from_args(mut args: impl Iterator<Item = String>) -> Self {
        match args.next() {
            Some(flag) if flag == helper::FLAG => {
                let request = args.next().and_then(|a| CaptureRequest::from_arg(&a));
                Self::Helper(request.unwrap_or(CaptureRequest::Interactive))
            }
            _ => Self::Tray,
        }
    }

    const fn log_prefix(self) -> &'static str {
        match self {
            Self::Tray => "clipframe",
            Self::Helper(_) => "clipframe-helper",
        }
    }
}

fn main() -> ExitCode {
    let role = Role::from_args(std::env::args().skip(1));
    let paths = match AppPaths::resolve() {
        Ok(paths) => paths,
        Err(err) => {
            eprintln!("ClipFrame: {err}");
            return ExitCode::FAILURE;
        }
    };
    let _log_guard = logging::init(&paths.log_dir, role.log_prefix())
        .inspect_err(|err| eprintln!("ClipFrame: file logging disabled: {err:#}"))
        .ok();
    logging::install_panic_hook();

    let result = match role {
        Role::Tray => run_tray(paths),
        Role::Helper(request) => helper::run(paths, request),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!("{err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run_tray(paths: AppPaths) -> anyhow::Result<()> {
    let _instance = match SingleInstance::acquire(INSTANCE_NAME) {
        Ok(instance) => instance,
        Err(InstanceError::AlreadyRunning) => {
            tracing::info!("another instance is already running; exiting");
            return Ok(());
        }
        Err(err) => return Err(err).context("checking for a running instance"),
    };
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        portable = paths.is_portable,
        config = %paths.config_file.display(),
        "starting ClipFrame"
    );
    app::run(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role(args: &[&str]) -> Role {
        Role::from_args(args.iter().map(ToString::to_string))
    }

    #[test]
    fn no_arguments_starts_the_tray() {
        assert_eq!(role(&[]), Role::Tray);
        assert_eq!(role(&["--unknown"]), Role::Tray);
    }

    #[test]
    fn helper_flag_starts_the_helper_with_its_request() {
        assert_eq!(
            role(&[helper::FLAG, "repeat-last"]),
            Role::Helper(CaptureRequest::RepeatLast)
        );
        assert_eq!(
            role(&[helper::FLAG]),
            Role::Helper(CaptureRequest::Interactive)
        );
    }
}
