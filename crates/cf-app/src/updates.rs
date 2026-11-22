//! Installing and updating with Velopack.
//!
//! [`handle_install_hooks`] must run first in `main`: Velopack starts the app with special
//! arguments while installing, updating and uninstalling, and exits right after the hook.
//! Update checks run once at tray start-up on a worker thread (no timer, so idle stays at
//! zero wake-ups), only for installed copies and only when `updates.repo` is set; the tray
//! menu's "Check for updates" runs one on demand.

use std::thread;
use std::time::{Duration, Instant};

use cf_core::capture_request::CaptureRequest;
use cf_platform::ipc::{self, Channel};

use velopack::sources::GithubSource;
use velopack::{UpdateCheck, UpdateManager, VelopackApp, VelopackAsset};

/// Message code telling the tray to exit.
pub const QUIT_CODE: u32 = 2;
/// Message code: check for updates now and say what was found (the main window's button).
pub const CHECK_CODE: u32 = 6;
/// Longest wait for running copies to exit before files are replaced or removed.
const QUIT_WAIT: Duration = Duration::from_secs(5);

/// Runs Velopack's install / update / uninstall hooks; returns for a normal start.
pub fn handle_install_hooks() {
    VelopackApp::build()
        .set_app_user_model_id(cf_platform::toast::APP_ID)
        .on_before_update_fast_callback(|_| stop_running_copies())
        .on_before_uninstall_fast_callback(|_| {
            stop_running_copies();
            // Don't leave a "start with Windows" entry pointing at a deleted exe.
            let exe = std::env::current_exe().unwrap_or_default();
            let _ = cf_platform::startup::set_enabled(false, &exe);
        })
        .run();
}

/// Asks a running tray and capture helper to exit, and waits (briefly) until they have, so
/// their executable can be replaced or deleted. Velopack's own process check doesn't always
/// see them.
fn stop_running_copies() {
    let _ = ipc::post_request(Channel::Tray, QUIT_CODE);
    let _ = ipc::post_request(Channel::Helper, CaptureRequest::Quit.code());
    let deadline = Instant::now() + QUIT_WAIT;
    while (ipc::is_running(Channel::Tray) || ipc::is_running(Channel::Helper))
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(100));
    }
    // The message windows close first; give the processes a moment to finish exiting.
    thread::sleep(Duration::from_millis(500));
}

/// A downloaded update, ready to install on restart.
#[derive(Debug)]
pub struct Ready {
    pub version: String,
    asset: Box<VelopackAsset>,
}

/// How an update check ended.
#[derive(Debug)]
pub enum Outcome {
    /// A newer version was downloaded.
    Ready(Ready),
    /// This is the newest version.
    UpToDate,
    /// This copy wasn't installed by the installer (a dev build or portable copy).
    NotInstalled,
    /// The check or download failed (offline, GitHub unreachable, bad `updates.repo`, …).
    Failed(String),
}

/// Checks `repo` (a `https://github.com/owner/name` URL) for a newer release in the
/// background and downloads it; `on_done` runs on the worker thread with the outcome.
pub fn check(repo: &str, on_done: impl FnOnce(Outcome) + Send + 'static) {
    let Some(repo) = github_repo(repo) else {
        tracing::warn!(
            repo,
            "updates.repo must be a https://github.com/owner/name URL"
        );
        on_done(Outcome::Failed(
            "updates.repo must be a https://github.com/owner/name URL".into(),
        ));
        return;
    };
    let spawned = thread::Builder::new()
        .name("update-check".into())
        .spawn(move || on_done(run_check(&repo)));
    if let Err(err) = spawned {
        tracing::warn!("could not start the update check: {err}");
    }
}

fn run_check(repo: &str) -> Outcome {
    let manager = match manager(repo) {
        Ok(manager) => manager,
        Err(err) => {
            tracing::info!("update check skipped: {err:#}");
            return Outcome::NotInstalled;
        }
    };
    match download(&manager) {
        Ok(Some(ready)) => Outcome::Ready(ready),
        Ok(None) => {
            tracing::debug!("no update available");
            Outcome::UpToDate
        }
        Err(err) => {
            tracing::info!("update check failed: {err:#}");
            Outcome::Failed(format!("{err:#}"))
        }
    }
}

fn download(manager: &UpdateManager) -> anyhow::Result<Option<Ready>> {
    let UpdateCheck::UpdateAvailable(info) = manager.check_for_updates()? else {
        return Ok(None);
    };
    manager.download_updates(&info, None)?;
    let asset = Box::new(info.TargetFullRelease);
    tracing::info!(version = %asset.Version, "update downloaded");
    Ok(Some(Ready {
        version: asset.Version.clone(),
        asset,
    }))
}

/// Installs a downloaded update and restarts ClipFrame.
///
/// # Errors
/// When this copy isn't installed by Velopack or the updater can't start.
pub fn install_and_restart(repo: &str, ready: &Ready) -> anyhow::Result<()> {
    let repo = github_repo(repo).ok_or_else(|| anyhow::anyhow!("invalid updates.repo"))?;
    // Back to the tray quietly, like before the update.
    manager(&repo)?.apply_updates_and_restart_with_args(&*ready.asset, [crate::BACKGROUND_FLAG])?;
    Ok(())
}

/// Fails for copies not installed by Velopack (dev builds, portable mode).
fn manager(repo: &str) -> anyhow::Result<UpdateManager> {
    Ok(UpdateManager::new(
        GithubSource::new(repo, None, false),
        None,
        None,
    )?)
}

/// `repo` without a trailing slash when it's a plain GitHub repository URL.
/// (`GithubSource::new` panics on URLs it can't parse, so this is checked first.)
fn github_repo(repo: &str) -> Option<String> {
    let trimmed = repo.trim().trim_end_matches('/');
    let path = trimmed.strip_prefix("https://github.com/")?;
    let mut parts = path.split('/');
    let valid = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    let (owner, name) = (parts.next()?, parts.next()?);
    (valid(owner) && valid(name) && parts.next().is_none()).then(|| trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_github_repository_urls_are_accepted() {
        assert_eq!(
            github_repo("https://github.com/someone/ClipFrame/"),
            Some("https://github.com/someone/ClipFrame".into())
        );
        for bad in [
            "http://github.com/a/b",
            "https://github.com/a",
            "https://github.com/a/b/releases",
            "https://example.com/a/b",
            "https://github.com/a b/c",
            "",
        ] {
            assert_eq!(github_repo(bad), None, "{bad}");
        }
    }
}
