// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sidecar lifecycle state machine — tracks and enforces valid state transitions.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::{Duration, Instant};

/// Lifecycle state of a sidecar process.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    /// The sidecar has not been initialized yet.
    Uninitialized,
    /// The sidecar is starting up.
    Starting,
    /// The sidecar has completed its handshake and is ready to accept work.
    Ready,
    /// The sidecar is actively processing a work order.
    Running,
    /// The sidecar is shutting down gracefully.
    Stopping,
    /// The sidecar has stopped.
    Stopped,
    /// The sidecar encountered an unrecoverable error.
    Failed,
}

impl fmt::Display for LifecycleState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Uninitialized => "uninitialized",
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        };
        f.write_str(s)
    }
}

/// Record of a single lifecycle state transition.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LifecycleTransition {
    /// State before the transition.
    pub from: LifecycleState,
    /// State after the transition.
    pub to: LifecycleState,
    /// ISO-8601 timestamp of when the transition occurred.
    pub timestamp: String,
    /// Optional human-readable reason for the transition.
    pub reason: Option<String>,
}

/// Errors produced by [`LifecycleManager`] when a transition is invalid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifecycleError {
    /// The requested transition is not allowed by the state machine.
    InvalidTransition {
        /// Current state.
        from: LifecycleState,
        /// Requested target state.
        to: LifecycleState,
    },
    /// The manager is already in the requested state.
    AlreadyInState(LifecycleState),
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransition { from, to } => {
                write!(f, "invalid lifecycle transition from {from} to {to}")
            }
            Self::AlreadyInState(s) => write!(f, "already in state {s}"),
        }
    }
}

impl std::error::Error for LifecycleError {}

/// Manages sidecar lifecycle state and enforces valid transitions.
#[derive(Debug)]
pub struct LifecycleManager {
    state: LifecycleState,
    history: Vec<LifecycleTransition>,
    ready_since: Option<Instant>,
}

impl LifecycleManager {
    /// Create a new manager in the [`LifecycleState::Uninitialized`] state.
    pub fn new() -> Self {
        Self {
            state: LifecycleState::Uninitialized,
            history: Vec::new(),
            ready_since: None,
        }
    }

    /// Current lifecycle state.
    pub fn state(&self) -> &LifecycleState {
        &self.state
    }

    /// Attempt to transition to a new state.
    ///
    /// Returns an error if the transition is not allowed or if the manager is
    /// already in the target state.
    pub fn transition(
        &mut self,
        to: LifecycleState,
        reason: Option<String>,
    ) -> Result<(), LifecycleError> {
        if self.state == to {
            return Err(LifecycleError::AlreadyInState(to));
        }
        if !self.can_transition(&to) {
            return Err(LifecycleError::InvalidTransition {
                from: self.state.clone(),
                to,
            });
        }

        let from = self.state.clone();
        self.state = to.clone();

        if to == LifecycleState::Ready && self.ready_since.is_none() {
            self.ready_since = Some(Instant::now());
        }

        self.history.push(LifecycleTransition {
            from,
            to,
            timestamp: chrono::Utc::now().to_rfc3339(),
            reason,
        });

        Ok(())
    }

    /// Returns `true` if transitioning from the current state to `to` is valid.
    pub fn can_transition(&self, to: &LifecycleState) -> bool {
        // Transition to Failed is always allowed (from any state).
        if *to == LifecycleState::Failed {
            return true;
        }

        matches!(
            (&self.state, to),
            (LifecycleState::Uninitialized, LifecycleState::Starting)
                | (LifecycleState::Starting, LifecycleState::Ready)
                | (LifecycleState::Ready, LifecycleState::Running)
                | (LifecycleState::Ready, LifecycleState::Stopping)
                | (LifecycleState::Running, LifecycleState::Ready)
                | (LifecycleState::Running, LifecycleState::Stopping)
                | (LifecycleState::Stopping, LifecycleState::Stopped)
        )
    }

    /// Full history of state transitions.
    pub fn history(&self) -> &[LifecycleTransition] {
        &self.history
    }

    /// Time elapsed since the manager first entered the [`LifecycleState::Ready`] state.
    ///
    /// Returns `None` if the manager has never been `Ready`.
    pub fn uptime(&self) -> Option<Duration> {
        self.ready_since.map(|t| t.elapsed())
    }
}

impl Default for LifecycleManager {
    fn default() -> Self {
        Self::new()
    }
}
