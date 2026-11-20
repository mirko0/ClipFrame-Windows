//! Deciding when an idle GPU device should be released.

use std::time::{Duration, Instant};

/// Tracks how long a resource has been idle against an optional timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeepAlive {
    timeout: Option<Duration>,
    idle_since: Option<Instant>,
}

impl KeepAlive {
    /// `timeout = None` keeps the resource forever.
    pub const fn new(timeout: Option<Duration>) -> Self {
        Self {
            timeout,
            idle_since: None,
        }
    }

    /// Changes the timeout (e.g. after a settings reload), keeping the idle start.
    pub const fn set_timeout(&mut self, timeout: Option<Duration>) {
        self.timeout = timeout;
    }

    /// The resource is in use; idle tracking stops.
    pub const fn mark_busy(&mut self) {
        self.idle_since = None;
    }

    /// The resource just became idle.
    pub const fn mark_idle(&mut self, now: Instant) {
        self.idle_since = Some(now);
    }

    /// Whether idle tracking is running.
    pub const fn is_idle(&self) -> bool {
        self.idle_since.is_some()
    }

    /// When the resource should be released, if it is idle and has a timeout.
    pub fn deadline(&self) -> Option<Instant> {
        Some(self.idle_since? + self.timeout?)
    }

    /// Whether the resource should be released now.
    pub fn is_expired(&self, now: Instant) -> bool {
        self.deadline().is_some_and(|deadline| now >= deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIVE_MIN: Duration = Duration::from_secs(300);

    #[test]
    fn busy_resource_never_expires() {
        let keep = KeepAlive::new(Some(Duration::ZERO));
        assert_eq!(keep.deadline(), None);
        assert!(!keep.is_expired(Instant::now()));
    }

    #[test]
    fn expires_after_timeout() {
        let start = Instant::now();
        let mut keep = KeepAlive::new(Some(FIVE_MIN));
        keep.mark_idle(start);
        assert!(!keep.is_expired(start + Duration::from_secs(299)));
        assert!(keep.is_expired(start + FIVE_MIN));
    }

    #[test]
    fn zero_timeout_expires_immediately() {
        let start = Instant::now();
        let mut keep = KeepAlive::new(Some(Duration::ZERO));
        keep.mark_idle(start);
        assert!(keep.is_expired(start));
    }

    #[test]
    fn no_timeout_never_expires() {
        let start = Instant::now();
        let mut keep = KeepAlive::new(None);
        keep.mark_idle(start);
        assert!(!keep.is_expired(start + Duration::from_secs(1_000_000)));
    }

    #[test]
    fn becoming_busy_cancels_expiry() {
        let start = Instant::now();
        let mut keep = KeepAlive::new(Some(FIVE_MIN));
        keep.mark_idle(start);
        keep.mark_busy();
        assert!(!keep.is_expired(start + FIVE_MIN * 2));
    }
}
