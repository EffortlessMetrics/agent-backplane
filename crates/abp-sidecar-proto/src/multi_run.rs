// SPDX-License-Identifier: MIT OR Apache-2.0
//! Multi-run session tracking for concurrent work orders (v0.2 prep).
//!
//! V0.1 assumes one run at a time. This module provides [`RunSessionTracker`]
//! to manage multiple concurrent sessions, paving the way for v0.2
//! multiplexed operation.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// RunState
// ---------------------------------------------------------------------------

/// State of an individual run session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    /// The run has been accepted but not yet started execution.
    Pending,
    /// The run is actively streaming events.
    Active,
    /// The run completed with a final receipt.
    Completed,
    /// The run terminated with a fatal error.
    Failed,
    /// The run was cancelled by the host.
    Cancelled,
}

// ---------------------------------------------------------------------------
// RunSession
// ---------------------------------------------------------------------------

/// Metadata for a single in-flight run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunSession {
    /// The run identifier (matches `ref_id` in envelopes).
    pub run_id: String,
    /// Current state of the run.
    pub state: RunState,
    /// Number of events emitted so far.
    pub events_emitted: u64,
}

impl RunSession {
    /// Create a new session in the `Pending` state.
    #[must_use]
    pub fn new(run_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            state: RunState::Pending,
            events_emitted: 0,
        }
    }

    /// Transition to Active.
    pub fn activate(&mut self) {
        self.state = RunState::Active;
    }

    /// Record an emitted event.
    pub fn record_event(&mut self) {
        self.events_emitted += 1;
    }

    /// Mark as completed.
    pub fn complete(&mut self) {
        self.state = RunState::Completed;
    }

    /// Mark as failed.
    pub fn fail(&mut self) {
        self.state = RunState::Failed;
    }

    /// Mark as cancelled.
    pub fn cancel(&mut self) {
        self.state = RunState::Cancelled;
    }

    /// Returns `true` if the run has reached a terminal state.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            RunState::Completed | RunState::Failed | RunState::Cancelled
        )
    }
}

// ---------------------------------------------------------------------------
// RunSessionTracker
// ---------------------------------------------------------------------------

/// Tracks zero or more concurrent run sessions.
///
/// In v0.1 at most one session should be active. The tracker enforces a
/// configurable concurrency limit that defaults to 1.
#[derive(Debug)]
pub struct RunSessionTracker {
    sessions: BTreeMap<String, RunSession>,
    max_concurrent: usize,
}

impl RunSessionTracker {
    /// Create a tracker with the given concurrency limit.
    #[must_use]
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            sessions: BTreeMap::new(),
            max_concurrent,
        }
    }

    /// Try to register a new run. Returns `false` if the concurrency limit
    /// has been reached.
    pub fn register(&mut self, run_id: impl Into<String>) -> bool {
        let active = self.active_count();
        if active >= self.max_concurrent {
            return false;
        }
        let id = run_id.into();
        self.sessions.insert(id.clone(), RunSession::new(id));
        true
    }

    /// Get a reference to a session by run id.
    #[must_use]
    pub fn get(&self, run_id: &str) -> Option<&RunSession> {
        self.sessions.get(run_id)
    }

    /// Get a mutable reference to a session by run id.
    pub fn get_mut(&mut self, run_id: &str) -> Option<&mut RunSession> {
        self.sessions.get_mut(run_id)
    }

    /// Number of non-terminal sessions.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.sessions.values().filter(|s| !s.is_terminal()).count()
    }

    /// Total sessions tracked (including terminal).
    #[must_use]
    pub fn total_count(&self) -> usize {
        self.sessions.len()
    }

    /// Remove all terminal sessions, returning their ids.
    pub fn prune(&mut self) -> Vec<String> {
        let terminal: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.is_terminal())
            .map(|(id, _)| id.clone())
            .collect();
        for id in &terminal {
            self.sessions.remove(id);
        }
        terminal
    }

    /// The configured concurrency limit.
    #[must_use]
    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    /// Returns `true` if the concurrency limit allows another run.
    #[must_use]
    pub fn has_capacity(&self) -> bool {
        self.active_count() < self.max_concurrent
    }

    /// All run ids currently tracked.
    #[must_use]
    pub fn run_ids(&self) -> Vec<&str> {
        self.sessions.keys().map(String::as_str).collect()
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
        let mut s = RunSession::new("run-1");
        assert_eq!(s.state, RunState::Pending);
        assert!(!s.is_terminal());

        s.activate();
        assert_eq!(s.state, RunState::Active);
        assert!(!s.is_terminal());

        s.record_event();
        s.record_event();
        assert_eq!(s.events_emitted, 2);

        s.complete();
        assert_eq!(s.state, RunState::Completed);
        assert!(s.is_terminal());
    }

    #[test]
    fn session_fail() {
        let mut s = RunSession::new("run-2");
        s.activate();
        s.fail();
        assert!(s.is_terminal());
    }

    #[test]
    fn session_cancel() {
        let mut s = RunSession::new("run-3");
        s.activate();
        s.cancel();
        assert!(s.is_terminal());
    }

    #[test]
    fn tracker_concurrency_limit() {
        let mut t = RunSessionTracker::new(1);
        assert!(t.register("run-1"));
        assert!(!t.register("run-2")); // limit reached
        assert_eq!(t.active_count(), 1);
    }

    #[test]
    fn tracker_allows_after_completion() {
        let mut t = RunSessionTracker::new(1);
        t.register("run-1");
        t.get_mut("run-1").unwrap().activate();
        t.get_mut("run-1").unwrap().complete();
        assert!(t.has_capacity());
        assert!(t.register("run-2"));
    }

    #[test]
    fn tracker_multi_concurrent() {
        let mut t = RunSessionTracker::new(3);
        assert!(t.register("a"));
        assert!(t.register("b"));
        assert!(t.register("c"));
        assert!(!t.register("d"));
        assert_eq!(t.active_count(), 3);
    }

    #[test]
    fn tracker_prune_removes_terminal() {
        let mut t = RunSessionTracker::new(5);
        t.register("a");
        t.register("b");
        t.get_mut("a").unwrap().complete();
        let pruned = t.prune();
        assert_eq!(pruned, vec!["a"]);
        assert_eq!(t.total_count(), 1);
    }

    #[test]
    fn tracker_run_ids() {
        let mut t = RunSessionTracker::new(5);
        t.register("x");
        t.register("y");
        let ids = t.run_ids();
        assert!(ids.contains(&"x"));
        assert!(ids.contains(&"y"));
    }

    #[test]
    fn run_state_serde_roundtrip() {
        for state in [
            RunState::Pending,
            RunState::Active,
            RunState::Completed,
            RunState::Failed,
            RunState::Cancelled,
        ] {
            let json = serde_json::to_string(&state).unwrap();
            let decoded: RunState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, decoded);
        }
    }

    #[test]
    fn run_session_serde_roundtrip() {
        let mut s = RunSession::new("run-s");
        s.activate();
        s.record_event();
        let json = serde_json::to_string(&s).unwrap();
        let decoded: RunSession = serde_json::from_str(&json).unwrap();
        assert_eq!(s.run_id, decoded.run_id);
        assert_eq!(s.state, decoded.state);
        assert_eq!(s.events_emitted, decoded.events_emitted);
    }
}
