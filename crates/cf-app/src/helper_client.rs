//! The tray side of the helper protocol: wake a running helper, or start one.

use std::io;
use std::process::Command;

use cf_core::capture_request::CaptureRequest;
use cf_platform::ipc;

use crate::helper;

/// Hands `request` to the capture helper, starting it if needed.
pub fn request(request: CaptureRequest) {
    if let ipc::Posted::Sent { pid, foreground } = ipc::post_request(request.code()) {
        if !foreground {
            tracing::warn!(pid, "could not grant the capture helper foreground rights");
        }
        tracing::debug!(?request, pid, "sent to the running capture helper");
        return;
    }
    match spawn(request) {
        Ok(pid) => {
            // The tray received the hotkey, so it may pass foreground rights on; without
            // them the overlay could open behind the active window, without keyboard focus.
            if !ipc::allow_foreground(pid) {
                tracing::warn!(pid, "could not grant the capture helper foreground rights");
            }
            tracing::debug!(?request, pid, "capture helper started");
        }
        Err(err) => tracing::error!("could not start the capture helper: {err}"),
    }
}

fn spawn(request: CaptureRequest) -> io::Result<u32> {
    let exe = std::env::current_exe()?;
    let child = Command::new(exe)
        .arg(helper::FLAG)
        .arg(request.as_arg())
        .spawn()?;
    Ok(child.id())
}
