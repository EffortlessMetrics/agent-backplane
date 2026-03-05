// SPDX-License-Identifier: MIT OR Apache-2.0
//! Error recovery and reconnection logic for sidecar protocol sessions.
//!
//! When a transient I/O failure occurs the sidecar can attempt to
//! reconnect. This module provides [`RecoveryPolicy`] (configuration) and
//! [`RecoveryTracker`] (state) for managing reconnection attempts with
//! exponential backoff.

use serde::{Deserialize, Serialize};
use std::time::Duration;

// ---------------------------------------------------------------------------
// RecoveryPolicy
// ---------------------------------------------------------------------------

/// Configuration governing reconnection behaviour.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryPolicy {
    /// Maximum number of reconnection attempts (0 = no retries).
    pub max_attempts: u32,
    /// Base delay for exponential backoff.
    pub base_delay_ms: u64,
    /// Maximum delay cap.
    pub max_delay_ms: u64,
    /// Whether the sidecar should attempt to resume from the last event
    /// sequence number on reconnect.
    pub resume_from_last_seq: bool,
}

impl Default for RecoveryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 200,
            max_delay_ms: 10_000,
            resume_from_last_seq: true,
        }
    }
}

impl RecoveryPolicy {
    /// Compute the backoff delay for a given zero-indexed attempt.
    #[must_use]
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let exp = 2u64.saturating_pow(attempt);
        let delay_ms = self.base_delay_ms.saturating_mul(exp);
        Duration::from_millis(delay_ms.min(self.max_delay_ms))
    }
}

// ---------------------------------------------------------------------------
// RecoveryState
// ---------------------------------------------------------------------------

/// Current state of the recovery sequence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryState {
    /// Connected and operating normally.
    Connected,
    /// Currently attempting to reconnect.
    Reconnecting {
        /// Zero-indexed attempt number.
        attempt: u32,
    },
    /// Reconnection succeeded.
    Recovered {
        /// How many attempts it took.
        attempts_used: u32,
    },
    /// All attempts exhausted — giving up.
    Failed {
        /// Total attempts made.
        attempts_used: u32,
        /// Error from the last attempt.
        last_error: String,
    },
}

// ---------------------------------------------------------------------------
// RecoveryTracker
// ---------------------------------------------------------------------------

/// Tracks reconnection attempts for a single sidecar session.
#[derive(Debug)]
pub struct RecoveryTracker {
    policy: RecoveryPolicy,
    state: RecoveryState,
    current_attempt: u32,
    last_event_seq: Option<u64>,
}

impl RecoveryTracker {
    /// Create a new tracker with the given policy.
    #[must_use]
    pub fn new(policy: RecoveryPolicy) -> Self {
        Self {
            policy,
            state: RecoveryState::Connected,
            current_attempt: 0,
            last_event_seq: None,
        }
    }

    /// Current recovery state.
    #[must_use]
    pub fn state(&self) -> &RecoveryState {
        &self.state
    }

    /// Record that an event with the given sequence number was processed.
    pub fn record_event_seq(&mut self, seq: u64) {
        self.last_event_seq = Some(seq);
    }

    /// Last known event sequence number (for resumption).
    #[must_use]
    pub fn last_event_seq(&self) -> Option<u64> {
        self.last_event_seq
    }

    /// Signal a connection failure and begin recovery.
    ///
    /// Returns the delay the caller should wait before attempting
    /// reconnection, or `None` if all attempts are exhausted.
    pub fn on_disconnect(&mut self, error: impl Into<String>) -> Option<Duration> {
        if self.current_attempt >= self.policy.max_attempts {
            self.state = RecoveryState::Failed {
                attempts_used: self.current_attempt,
                last_error: error.into(),
            };
            return None;
        }

        let delay = self.policy.delay_for_attempt(self.current_attempt);
        self.state = RecoveryState::Reconnecting {
            attempt: self.current_attempt,
        };
        self.current_attempt += 1;
        Some(delay)
    }

    /// Signal that reconnection succeeded.
    pub fn on_reconnect(&mut self) {
        self.state = RecoveryState::Recovered {
            attempts_used: self.current_attempt,
        };
    }

    /// Reset to the connected state (e.g. after a successful run).
    pub fn reset(&mut self) {
        self.state = RecoveryState::Connected;
        self.current_attempt = 0;
        self.last_event_seq = None;
    }

    /// Returns `true` if recovery has failed permanently.
    #[must_use]
    pub fn is_failed(&self) -> bool {
        matches!(self.state, RecoveryState::Failed { .. })
    }

    /// Returns `true` if the session should attempt resumption on reconnect.
    #[must_use]
    pub fn should_resume(&self) -> bool {
        self.policy.resume_from_last_seq && self.last_event_seq.is_some()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_policy() -> RecoveryPolicy {
        RecoveryPolicy {
            max_attempts: 3,
            base_delay_ms: 100,
            max_delay_ms: 1000,
            resume_from_last_seq: true,
        }
    }

    #[test]
    fn tracker_initial_state() {
        let t = RecoveryTracker::new(test_policy());
        assert_eq!(*t.state(), RecoveryState::Connected);
        assert!(!t.is_failed());
        assert!(t.last_event_seq().is_none());
    }

    #[test]
    fn on_disconnect_returns_delay() {
        let mut t = RecoveryTracker::new(test_policy());
        let delay = t.on_disconnect("connection lost").unwrap();
        assert_eq!(delay, Duration::from_millis(100));
        assert!(matches!(
            t.state(),
            RecoveryState::Reconnecting { attempt: 0 }
        ));
    }

    #[test]
    fn backoff_increases() {
        let mut t = RecoveryTracker::new(test_policy());
        let d0 = t.on_disconnect("err").unwrap();
        let d1 = t.on_disconnect("err").unwrap();
        let d2 = t.on_disconnect("err").unwrap();
        assert!(d1 > d0);
        assert!(d2 > d1);
    }

    #[test]
    fn backoff_capped() {
        let policy = RecoveryPolicy {
            max_attempts: 10,
            base_delay_ms: 100,
            max_delay_ms: 500,
            resume_from_last_seq: false,
        };
        let mut t = RecoveryTracker::new(policy);
        for _ in 0..10 {
            if let Some(d) = t.on_disconnect("err") {
                assert!(d <= Duration::from_millis(500));
            }
        }
    }

    #[test]
    fn exhaustion_returns_none() {
        let mut t = RecoveryTracker::new(test_policy());
        t.on_disconnect("e1");
        t.on_disconnect("e2");
        t.on_disconnect("e3");
        assert!(t.on_disconnect("e4").is_none());
        assert!(t.is_failed());
    }

    #[test]
    fn on_reconnect_marks_recovered() {
        let mut t = RecoveryTracker::new(test_policy());
        t.on_disconnect("err");
        t.on_reconnect();
        assert!(matches!(t.state(), RecoveryState::Recovered { .. }));
    }

    #[test]
    fn reset_clears_state() {
        let mut t = RecoveryTracker::new(test_policy());
        t.record_event_seq(42);
        t.on_disconnect("err");
        t.reset();
        assert_eq!(*t.state(), RecoveryState::Connected);
        assert!(t.last_event_seq().is_none());
    }

    #[test]
    fn should_resume_with_seq() {
        let mut t = RecoveryTracker::new(test_policy());
        t.record_event_seq(10);
        assert!(t.should_resume());
    }

    #[test]
    fn should_resume_without_seq() {
        let t = RecoveryTracker::new(test_policy());
        assert!(!t.should_resume());
    }

    #[test]
    fn should_resume_disabled() {
        let mut policy = test_policy();
        policy.resume_from_last_seq = false;
        let mut t = RecoveryTracker::new(policy);
        t.record_event_seq(10);
        assert!(!t.should_resume());
    }

    #[test]
    fn policy_delay_for_attempt() {
        let p = test_policy();
        assert_eq!(p.delay_for_attempt(0), Duration::from_millis(100));
        assert_eq!(p.delay_for_attempt(1), Duration::from_millis(200));
        assert_eq!(p.delay_for_attempt(2), Duration::from_millis(400));
        // capped at 1000
        assert_eq!(p.delay_for_attempt(10), Duration::from_millis(1000));
    }

    #[test]
    fn policy_serde_roundtrip() {
        let p = test_policy();
        let json = serde_json::to_string(&p).unwrap();
        let decoded: RecoveryPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn default_policy() {
        let p = RecoveryPolicy::default();
        assert_eq!(p.max_attempts, 3);
        assert_eq!(p.base_delay_ms, 200);
        assert!(p.resume_from_last_seq);
    }
}
