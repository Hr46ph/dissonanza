//! App-level health-check layered on top of `core_paired`/`core_unpaired`, per CLAUDE.md §1:
//! that event pair is known to not always fire `core_unpaired` correctly on its own (see
//! `moo::handshake`'s pairing module for the sourced `node-roon-api` bug that motivates this).
//! [`Keepalive`] tracks when activity (any inbound MOO message) was last observed and reports
//! the connection stale once too much time has passed with none — the signal a connection state
//! machine can use to force `Unpaired`/`Disconnected` regardless of whether the event pair fired.
//!
//! Not wired into a connection state machine yet, so its items are unused outside their own
//! tests.
#![allow(dead_code)]

use std::time::{Duration, Instant};

/// Tracks the time activity was last observed and whether that makes the connection stale.
///
/// `now` is passed into every method rather than read internally via `Instant::now()`, so tests
/// can drive the clock deterministically without real sleeps; real callers just pass
/// `Instant::now()` at each call site.
#[derive(Debug)]
pub(crate) struct Keepalive {
    timeout: Duration,
    last_activity: Instant,
}

impl Keepalive {
    /// Starts tracking activity as of `now`, becoming stale after `timeout` passes with no
    /// further activity recorded.
    pub(crate) fn new(timeout: Duration, now: Instant) -> Self {
        Self {
            timeout,
            last_activity: now,
        }
    }

    /// Records that activity was observed at `now`, resetting the staleness clock.
    pub(crate) fn record_activity(&mut self, now: Instant) {
        self.last_activity = now;
    }

    /// Whether no activity has been observed for at least `timeout`, as of `now`.
    pub(crate) fn is_stale(&self, now: Instant) -> bool {
        now.duration_since(self.last_activity) >= self.timeout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn does_not_fire_while_activity_is_recent() {
        let now = Instant::now();
        let keepalive = Keepalive::new(Duration::from_secs(30), now);

        assert!(!keepalive.is_stale(now + Duration::from_secs(10)));
    }

    #[test]
    fn marks_stale_after_timeout_with_no_activity() {
        let now = Instant::now();
        let keepalive = Keepalive::new(Duration::from_secs(30), now);

        assert!(keepalive.is_stale(now + Duration::from_secs(30)));
        assert!(keepalive.is_stale(now + Duration::from_secs(60)));
    }

    #[test]
    fn resets_on_any_received_message() {
        let now = Instant::now();
        let mut keepalive = Keepalive::new(Duration::from_secs(30), now);

        let activity_at = now + Duration::from_secs(25);
        keepalive.record_activity(activity_at);

        assert!(!keepalive.is_stale(activity_at + Duration::from_secs(10)));
        assert!(keepalive.is_stale(activity_at + Duration::from_secs(30)));
    }
}
