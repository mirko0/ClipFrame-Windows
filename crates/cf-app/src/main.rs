//! ClipFrame tray application entry point.

// Release builds have no console window; debug builds keep one for log output.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod app;
mod hotkeys;
mod logging;
mod tray;
mod tray_icon_image;

use std::process::ExitCode;

use anyhow::Context as _;
use cf_platform::paths::AppPaths;
use cf_platform::single_instance::{InstanceError, SingleInstance};

/// Per-session mutex name; one ClipFrame per logged-in user.
const INSTANCE_NAME: &str = r"Local\ClipFrame.SingleInstance";

fn main() -> ExitCode {
    let paths = match AppPaths::resolve() {
        Ok(paths) => paths,
        Err(err) => {
            eprintln!("ClipFrame: {err}");
            return ExitCode::FAILURE;
        }
    };
    let _log_guard = logging::init(&paths.log_dir)
        .inspect_err(|err| eprintln!("ClipFrame: file logging disabled: {err:#}"))
        .ok();
    logging::install_panic_hook();

    match run(paths) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!("{err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(paths: AppPaths) -> anyhow::Result<()> {
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
