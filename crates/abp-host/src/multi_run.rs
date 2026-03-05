// SPDX-License-Identifier: MIT OR Apache-2.0
//! Host-side multi-run session tracking for concurrent work orders.
//!
//! Tracks multiple sidecar runs, dispatching events and receipts to the
//! correct run. Enforces a configurable concurrency limit.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// HostRunState
// ---------------------------------------------------------------------------

/// State of a run as tracked by the host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostRunState {
    /// The run envelope has been sent; waiting for events.
    Dispatched,
    /// Events are being received.
    Streaming,
    /// A final receipt was received.
    Completed,
    /// A fatal error was received.
    Failed,
    /// The host cancelled the run.
    Cancelled,
}

// ---------------------------------------------------------------------------
// HostRunSession
// ---------------------------------------------------------------------------

/// Metadata for a single run from the host's perspective.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HostRunSession {
    /// Run identifier.
    pub run_id: String,
    /// Current state.
    pub state: HostRunState,
    /// Events received so far.
    pub events_received: u64,
}

impl HostRunSession {
    /// Create a session in the `Dispatched` state.
    #[must_use]
    pub fn new(run_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            state: HostRunState::Dispatched,
            events_received: 0,
        }
    }

    /// Record an event reception.
    pub fn on_event(&mut self) {
        if self.state == HostRunState::Dispatched {
            self.state = HostRunState::Streaming;
        }
        self.events_received += 1;
    }

    /// Mark as completed (final receipt received).
    pub fn on_final(&mut self) {
        self.state = HostRunState::Completed;
    }

    /// Mark as failed (fatal received).
    pub fn on_fatal(&mut self) {
        self.state = HostRunState::Failed;
    }

    /// Mark as cancelled.
    pub fn on_cancel(&mut self) {
        self.state = HostRunState::Cancelled;
    }

    /// Whether the run is in a terminal state.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            HostRunState::Completed | HostRunState::Failed | HostRunState::Cancelled
        )
    }
}

// ---------------------------------------------------------------------------
// HostMultiRunTracker
// ---------------------------------------------------------------------------

/// Tracks multiple concurrent runs from the host's perspective.
#[derive(Debug)]
pub struct HostMultiRunTracker {
    sessions: BTreeMap<String, HostRunSession>,
    max_concurrent: usize,
}

impl HostMultiRunTracker {
    /// Create a tracker with the given concurrency limit.
    #[must_use]
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            sessions: BTreeMap::new(),
            max_concurrent,
        }
    }

    /// Dispatch a new run. Returns `false` if the limit is reached.
    pub fn dispatch(&mut self, run_id: impl Into<String>) -> bool {
        if self.active_count() >= self.max_concurrent {
            return false;
        }
        let id = run_id.into();
        self.sessions.insert(id.clone(), HostRunSession::new(id));
        true
    }

    /// Get a session.
    #[must_use]
    pub fn get(&self, run_id: &str) -> Option<&HostRunSession> {
        self.sessions.get(run_id)
    }

    /// Get a mutable session.
    pub fn get_mut(&mut self, run_id: &str) -> Option<&mut HostRunSession> {
        self.sessions.get_mut(run_id)
    }

    /// Number of active (non-terminal) runs.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.sessions.values().filter(|s| !s.is_terminal()).count()
    }

    /// Total tracked runs.
    #[must_use]
    pub fn total_count(&self) -> usize {
        self.sessions.len()
    }

    /// Remove terminal sessions.
    pub fn prune(&mut self) -> Vec<String> {
        let ids: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.is_terminal())
            .map(|(id, _)| id.clone())
            .collect();
        for id in &ids {
            self.sessions.remove(id);
        }
        ids
    }

    /// Whether another run can be dispatched.
    #[must_use]
    pub fn has_capacity(&self) -> bool {
        self.active_count() < self.max_concurrent
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_lifecycle() {
        let mut s = HostRunSession::new("r1");
        assert_eq!(s.state, HostRunState::Dispatched);

        s.on_event();
        assert_eq!(s.state, HostRunState::Streaming);
        assert_eq!(s.events_received, 1);

        s.on_final();
        assert!(s.is_terminal());
    }

    #[test]
    fn session_fatal() {
        let mut s = HostRunSession::new("r2");
        s.on_fatal();
        assert!(s.is_terminal());
    }

    #[test]
    fn session_cancel() {
        let mut s = HostRunSession::new("r3");
        s.on_cancel();
        assert!(s.is_terminal());
    }

    #[test]
    fn tracker_concurrency() {
        let mut t = HostMultiRunTracker::new(2);
        assert!(t.dispatch("a"));
        assert!(t.dispatch("b"));
        assert!(!t.dispatch("c"));
        assert_eq!(t.active_count(), 2);
    }

    #[test]
    fn tracker_prune() {
        let mut t = HostMultiRunTracker::new(5);
        t.dispatch("a");
        t.dispatch("b");
        t.get_mut("a").unwrap().on_final();
        let pruned = t.prune();
        assert_eq!(pruned, vec!["a"]);
        assert_eq!(t.total_count(), 1);
    }

    #[test]
    fn tracker_has_capacity_after_completion() {
        let mut t = HostMultiRunTracker::new(1);
        t.dispatch("a");
        assert!(!t.has_capacity());
        t.get_mut("a").unwrap().on_final();
        assert!(t.has_capacity());
    }

    #[test]
    fn run_state_serde() {
        for state in [
            HostRunState::Dispatched,
            HostRunState::Streaming,
            HostRunState::Completed,
            HostRunState::Failed,
            HostRunState::Cancelled,
        ] {
            let json = serde_json::to_string(&state).unwrap();
            let decoded: HostRunState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, decoded);
        }
    }

    #[test]
    fn session_serde_roundtrip() {
        let mut s = HostRunSession::new("rs");
        s.on_event();
        s.on_event();
        let json = serde_json::to_string(&s).unwrap();
        let decoded: HostRunSession = serde_json::from_str(&json).unwrap();
        assert_eq!(s.run_id, decoded.run_id);
        assert_eq!(s.events_received, decoded.events_received);
    }
}
