// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sidecar-side graceful shutdown handling.
//!
//! When the host requests shutdown the sidecar should finish in-flight
//! work and respond with a goodbye. This module provides
//! [`ShutdownHandler`] to track the sequence and enforce deadlines.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// ShutdownSignal
// ---------------------------------------------------------------------------

/// A shutdown signal received from the host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShutdownSignal {
    /// Reason the host is shutting the sidecar down.
    pub reason: String,
    /// Maximum time (ms) the sidecar is given to finish.
    pub deadline_ms: u64,
}

impl ShutdownSignal {
    /// Create a new signal.
    #[must_use]
    pub fn new(reason: impl Into<String>, deadline: Duration) -> Self {
        Self {
            reason: reason.into(),
            deadline_ms: deadline.as_millis() as u64,
        }
    }

    /// Deadline as a [`Duration`].
    #[must_use]
    pub fn deadline(&self) -> Duration {
        Duration::from_millis(self.deadline_ms)
    }
}

// ---------------------------------------------------------------------------
// ShutdownAck
// ---------------------------------------------------------------------------

/// Acknowledgement sent by the sidecar after handling a shutdown signal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShutdownAck {
    /// Whether the sidecar completed all in-flight work.
    pub clean: bool,
    /// Number of in-flight tasks that were completed.
    pub completed: u64,
    /// Number of in-flight tasks that were dropped.
    pub dropped: u64,
    /// Optional error description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ShutdownAck {
    /// A clean ack with zero in-flight work.
    #[must_use]
    pub fn clean() -> Self {
        Self {
            clean: true,
            completed: 0,
            dropped: 0,
            error: None,
        }
    }

    /// A partial ack — some work was dropped.
    #[must_use]
    pub fn partial(completed: u64, dropped: u64) -> Self {
        Self {
            clean: false,
            completed,
            dropped,
            error: None,
        }
    }

    /// Attach an error message.
    #[must_use]
    pub fn with_error(mut self, msg: impl Into<String>) -> Self {
        self.error = Some(msg.into());
        self
    }
}

// ---------------------------------------------------------------------------
// ShutdownPhase
// ---------------------------------------------------------------------------

/// The current phase of the sidecar's shutdown sequence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShutdownPhase {
    /// Running normally — no shutdown requested.
    Running,
    /// Shutdown requested — draining in-flight work.
    Draining,
    /// All work drained, ack sent.
    Complete,
    /// Deadline exceeded before draining finished.
    TimedOut,
}

// ---------------------------------------------------------------------------
// ShutdownHandler
// ---------------------------------------------------------------------------

/// Tracks a sidecar's shutdown lifecycle.
#[derive(Debug)]
pub struct ShutdownHandler {
    phase: ShutdownPhase,
    signal: Option<ShutdownSignal>,
    started_at: Option<Instant>,
    ack: Option<ShutdownAck>,
}

impl ShutdownHandler {
    /// Create a handler in the `Running` phase.
    #[must_use]
    pub fn new() -> Self {
        Self {
            phase: ShutdownPhase::Running,
            signal: None,
            started_at: None,
            ack: None,
        }
    }

    /// Current phase.
    #[must_use]
    pub fn phase(&self) -> &ShutdownPhase {
        &self.phase
    }

    /// Begin shutdown in response to a signal from the host.
    pub fn begin(&mut self, signal: ShutdownSignal) {
        self.phase = ShutdownPhase::Draining;
        self.signal = Some(signal);
        self.started_at = Some(Instant::now());
    }

    /// Complete the shutdown with an acknowledgement.
    pub fn complete(&mut self, ack: ShutdownAck) {
        self.phase = ShutdownPhase::Complete;
        self.ack = Some(ack);
    }

    /// Check whether the deadline has been exceeded.
    pub fn check_timeout(&mut self) -> bool {
        if let (Some(signal), Some(started)) = (&self.signal, self.started_at) {
            if started.elapsed() >= signal.deadline() && self.phase == ShutdownPhase::Draining {
                self.phase = ShutdownPhase::TimedOut;
                return true;
            }
        }
        false
    }

    /// Time remaining until the deadline, or zero.
    #[must_use]
    pub fn time_remaining(&self) -> Duration {
        match (&self.signal, self.started_at) {
            (Some(signal), Some(started)) => signal.deadline().saturating_sub(started.elapsed()),
            _ => Duration::ZERO,
        }
    }

    /// The acknowledgement, if sent.
    #[must_use]
    pub fn ack(&self) -> Option<&ShutdownAck> {
        self.ack.as_ref()
    }

    /// Returns `true` if a shutdown is in progress (draining or timed out).
    #[must_use]
    pub fn is_shutting_down(&self) -> bool {
        matches!(
            self.phase,
            ShutdownPhase::Draining | ShutdownPhase::TimedOut
        )
    }
}

impl Default for ShutdownHandler {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_initial_state() {
        let h = ShutdownHandler::new();
        assert_eq!(*h.phase(), ShutdownPhase::Running);
        assert!(!h.is_shutting_down());
        assert!(h.ack().is_none());
    }

    #[test]
    fn begin_shutdown() {
        let mut h = ShutdownHandler::new();
        h.begin(ShutdownSignal::new("test", Duration::from_secs(5)));
        assert_eq!(*h.phase(), ShutdownPhase::Draining);
        assert!(h.is_shutting_down());
    }

    #[test]
    fn complete_shutdown() {
        let mut h = ShutdownHandler::new();
        h.begin(ShutdownSignal::new("test", Duration::from_secs(5)));
        h.complete(ShutdownAck::clean());
        assert_eq!(*h.phase(), ShutdownPhase::Complete);
        assert!(h.ack().unwrap().clean);
    }

    #[test]
    fn partial_ack() {
        let ack = ShutdownAck::partial(3, 1);
        assert!(!ack.clean);
        assert_eq!(ack.completed, 3);
        assert_eq!(ack.dropped, 1);
    }

    #[test]
    fn ack_with_error() {
        let ack = ShutdownAck::clean().with_error("disk full");
        assert_eq!(ack.error.as_deref(), Some("disk full"));
    }

    #[test]
    fn time_remaining_without_signal() {
        let h = ShutdownHandler::new();
        assert_eq!(h.time_remaining(), Duration::ZERO);
    }

    #[test]
    fn time_remaining_with_signal() {
        let mut h = ShutdownHandler::new();
        h.begin(ShutdownSignal::new("test", Duration::from_secs(60)));
        assert!(h.time_remaining() > Duration::from_secs(59));
    }

    #[test]
    fn check_timeout_not_expired() {
        let mut h = ShutdownHandler::new();
        h.begin(ShutdownSignal::new("test", Duration::from_secs(60)));
        assert!(!h.check_timeout());
        assert_eq!(*h.phase(), ShutdownPhase::Draining);
    }

    #[test]
    fn shutdown_signal_serde() {
        let s = ShutdownSignal::new("normal", Duration::from_secs(30));
        let json = serde_json::to_string(&s).unwrap();
        let decoded: ShutdownSignal = serde_json::from_str(&json).unwrap();
        assert_eq!(s, decoded);
    }

    #[test]
    fn shutdown_ack_serde() {
        let ack = ShutdownAck::partial(5, 2).with_error("timeout");
        let json = serde_json::to_string(&ack).unwrap();
        let decoded: ShutdownAck = serde_json::from_str(&json).unwrap();
        assert_eq!(ack, decoded);
    }

    #[test]
    fn default_handler() {
        let h = ShutdownHandler::default();
        assert_eq!(*h.phase(), ShutdownPhase::Running);
    }
}
