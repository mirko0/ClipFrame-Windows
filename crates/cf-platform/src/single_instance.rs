//! Ensures only one ClipFrame process runs per user session.

use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::HSTRING;

/// Holds a named mutex for as long as this value lives.
#[derive(Debug)]
pub struct SingleInstance {
    handle: HANDLE,
}

/// Why the single-instance lock could not be taken.
#[derive(Debug, thiserror::Error)]
pub enum InstanceError {
    /// Another process already holds the lock.
    #[error("ClipFrame is already running")]
    AlreadyRunning,
    /// The mutex could not be created.
    #[error("could not create the single-instance mutex")]
    Os(#[from] windows::core::Error),
}

impl SingleInstance {
    /// Takes the lock named `name` (use a `Local\` prefix for per-session scope).
    ///
    /// # Errors
    /// [`InstanceError::AlreadyRunning`] when another process holds it.
    pub fn acquire(name: &str) -> Result<Self, InstanceError> {
        let name = HSTRING::from(name);
        // SAFETY: `name` is a valid null-terminated wide string that outlives the call;
        // default security attributes (None) are allowed.
        let handle = unsafe { CreateMutexW(None, false, &name) }?;
        // SAFETY: GetLastError has no preconditions; it is read immediately after
        // CreateMutexW, which sets ERROR_ALREADY_EXISTS when the mutex existed.
        let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        let instance = Self { handle };
        if already_exists {
            return Err(InstanceError::AlreadyRunning); // `instance` drops and closes the handle
        }
        Ok(instance)
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        // SAFETY: `handle` came from a successful CreateMutexW and is closed exactly once.
        // A failure to close at shutdown is harmless, so the result is ignored.
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn second_acquire_fails_until_first_is_dropped() {
        let name = format!(r"Local\ClipFrame.Test.{}", std::process::id());

        let first = SingleInstance::acquire(&name).unwrap();
        assert!(matches!(
            SingleInstance::acquire(&name),
            Err(InstanceError::AlreadyRunning)
        ));

        drop(first);
        assert!(SingleInstance::acquire(&name).is_ok());
    }
}
