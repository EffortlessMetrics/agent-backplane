// SPDX-License-Identifier: MIT OR Apache-2.0
//! In-memory run registry and event store for the daemon HTTP API.
//!
//! This module provides `RunRegistry` — a thread-safe, async-aware store
//! that tracks run lifecycle state and per-run event logs.

use abp_core::{AgentEvent, Receipt};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// RunPhase — lifecycle state for a single run
// ---------------------------------------------------------------------------

/// Lifecycle phase of a tracked run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunPhase {
    /// The run has been accepted but not yet started.
    Queued,
    /// The run is actively executing.
    Running,
    /// The run completed successfully.
    Completed,
    /// The run terminated with an error.
    Failed,
    /// The run was cancelled by a user request.
    Cancelled,
}

impl RunPhase {
    /// Returns `true` for terminal phases.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// Returns `true` if transitioning from `self` to `next` is valid.
    pub fn can_transition_to(self, next: RunPhase) -> bool {
        match self {
            Self::Queued => matches!(next, Self::Running | Self::Cancelled),
            Self::Running => matches!(next, Self::Completed | Self::Failed | Self::Cancelled),
            Self::Completed | Self::Failed | Self::Cancelled => false,
        }
    }
}

// ---------------------------------------------------------------------------
// RunRecord — full per-run tracking state
// ---------------------------------------------------------------------------

/// Complete record for a single tracked run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    /// Unique run identifier.
    pub id: Uuid,
    /// Target backend name.
    pub backend: String,
    /// Current lifecycle phase.
    pub phase: RunPhase,
    /// Timestamp when the run was created.
    pub created_at: DateTime<Utc>,
    /// Timestamp of the last status change.
    pub updated_at: DateTime<Utc>,
    /// Events collected during execution.
    pub events: Vec<AgentEvent>,
    /// Final receipt (only present when phase is `Completed`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<Receipt>,
    /// Error message (only present when phase is `Failed`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// RegistryError
// ---------------------------------------------------------------------------

/// Errors returned by [`RunRegistry`] operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// The run ID was not found in the registry.
    NotFound(Uuid),
    /// A run with this ID already exists.
    DuplicateId(Uuid),
    /// The requested state transition is invalid.
    InvalidTransition {
        /// Run identifier.
        run_id: Uuid,
        /// Current phase.
        from: RunPhase,
        /// Attempted target phase.
        to: RunPhase,
    },
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "run {id} not found"),
            Self::DuplicateId(id) => write!(f, "run {id} already exists"),
            Self::InvalidTransition { run_id, from, to } => {
                write!(f, "invalid transition for run {run_id}: {from:?} -> {to:?}")
            }
        }
    }
}

impl std::error::Error for RegistryError {}

// ---------------------------------------------------------------------------
// RunRegistry
// ---------------------------------------------------------------------------

/// Thread-safe in-memory registry that tracks run lifecycle and events.
#[derive(Clone, Default)]
pub struct RunRegistry {
    runs: Arc<RwLock<HashMap<Uuid, RunRecord>>>,
}

impl RunRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new run in `Queued` phase. Returns the assigned run ID.
    pub async fn create_run(&self, run_id: Uuid, backend: String) -> Result<Uuid, RegistryError> {
        let mut guard = self.runs.write().await;
        if guard.contains_key(&run_id) {
            return Err(RegistryError::DuplicateId(run_id));
        }
        let now = Utc::now();
        guard.insert(
            run_id,
            RunRecord {
                id: run_id,
                backend,
                phase: RunPhase::Queued,
                created_at: now,
                updated_at: now,
                events: Vec::new(),
                receipt: None,
                error: None,
            },
        );
        Ok(run_id)
    }

    /// Transition a run to a new phase.
    pub async fn transition(&self, run_id: Uuid, to: RunPhase) -> Result<RunPhase, RegistryError> {
        let mut guard = self.runs.write().await;
        let record = guard
            .get_mut(&run_id)
            .ok_or(RegistryError::NotFound(run_id))?;
        if !record.phase.can_transition_to(to) {
            return Err(RegistryError::InvalidTransition {
                run_id,
                from: record.phase,
                to,
            });
        }
        record.phase = to;
        record.updated_at = Utc::now();
        Ok(to)
    }

    /// Append an event to a run's event log.
    pub async fn push_event(
        &self,
        run_id: Uuid,
        event: AgentEvent,
    ) -> Result<usize, RegistryError> {
        let mut guard = self.runs.write().await;
        let record = guard
            .get_mut(&run_id)
            .ok_or(RegistryError::NotFound(run_id))?;
        record.events.push(event);
        Ok(record.events.len())
    }

    /// Complete a run with its receipt.
    pub async fn complete(&self, run_id: Uuid, receipt: Receipt) -> Result<(), RegistryError> {
        let mut guard = self.runs.write().await;
        let record = guard
            .get_mut(&run_id)
            .ok_or(RegistryError::NotFound(run_id))?;
        if !record.phase.can_transition_to(RunPhase::Completed) {
            return Err(RegistryError::InvalidTransition {
                run_id,
                from: record.phase,
                to: RunPhase::Completed,
            });
        }
        record.phase = RunPhase::Completed;
        record.receipt = Some(receipt);
        record.updated_at = Utc::now();
        Ok(())
    }

    /// Fail a run with an error message.
    pub async fn fail(&self, run_id: Uuid, error: String) -> Result<(), RegistryError> {
        let mut guard = self.runs.write().await;
        let record = guard
            .get_mut(&run_id)
            .ok_or(RegistryError::NotFound(run_id))?;
        if !record.phase.can_transition_to(RunPhase::Failed) {
            return Err(RegistryError::InvalidTransition {
                run_id,
                from: record.phase,
                to: RunPhase::Failed,
            });
        }
        record.phase = RunPhase::Failed;
        record.error = Some(error);
        record.updated_at = Utc::now();
        Ok(())
    }

    /// Cancel a run.
    pub async fn cancel(&self, run_id: Uuid) -> Result<(), RegistryError> {
        let mut guard = self.runs.write().await;
        let record = guard
            .get_mut(&run_id)
            .ok_or(RegistryError::NotFound(run_id))?;
        if !record.phase.can_transition_to(RunPhase::Cancelled) {
            return Err(RegistryError::InvalidTransition {
                run_id,
                from: record.phase,
                to: RunPhase::Cancelled,
            });
        }
        record.phase = RunPhase::Cancelled;
        record.updated_at = Utc::now();
        Ok(())
    }

    /// Get a snapshot of a run record.
    pub async fn get(&self, run_id: Uuid) -> Option<RunRecord> {
        self.runs.read().await.get(&run_id).cloned()
    }

    /// Get the events for a run.
    pub async fn events(&self, run_id: Uuid) -> Result<Vec<AgentEvent>, RegistryError> {
        let guard = self.runs.read().await;
        let record = guard.get(&run_id).ok_or(RegistryError::NotFound(run_id))?;
        Ok(record.events.clone())
    }

    /// List all run IDs.
    pub async fn list_ids(&self) -> Vec<Uuid> {
        self.runs.read().await.keys().copied().collect()
    }

    /// List all run records.
    pub async fn list_all(&self) -> Vec<RunRecord> {
        self.runs.read().await.values().cloned().collect()
    }

    /// Count runs by phase.
    pub async fn count_by_phase(&self, phase: RunPhase) -> usize {
        self.runs
            .read()
            .await
            .values()
            .filter(|r| r.phase == phase)
            .count()
    }

    /// Total number of tracked runs.
    pub async fn len(&self) -> usize {
        self.runs.read().await.len()
    }

    /// Whether the registry is empty.
    pub async fn is_empty(&self) -> bool {
        self.runs.read().await.is_empty()
    }

    /// Remove a run in a terminal phase. Returns the record if removed.
    pub async fn remove(&self, run_id: Uuid) -> Result<RunRecord, RegistryError> {
        let mut guard = self.runs.write().await;
        let record = guard.get(&run_id).ok_or(RegistryError::NotFound(run_id))?;
        if !record.phase.is_terminal() {
            return Err(RegistryError::InvalidTransition {
                run_id,
                from: record.phase,
                to: record.phase,
            });
        }
        Ok(guard.remove(&run_id).unwrap())
    }
}

// ---------------------------------------------------------------------------
// BackendRegistry
// ---------------------------------------------------------------------------

/// Simple in-memory backend name registry.
#[derive(Clone, Default)]
pub struct BackendList {
    names: Arc<RwLock<Vec<String>>>,
}

impl BackendList {
    /// Create an empty backend list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a backend list pre-populated with the given names.
    pub fn from_names(names: Vec<String>) -> Self {
        Self {
            names: Arc::new(RwLock::new(names)),
        }
    }

    /// Register a backend name.
    pub async fn register(&self, name: String) {
        let mut guard = self.names.write().await;
        if !guard.contains(&name) {
            guard.push(name);
        }
    }

    /// Return a snapshot of registered names.
    pub async fn list(&self) -> Vec<String> {
        self.names.read().await.clone()
    }

    /// Check if a backend name is registered.
    pub async fn contains(&self, name: &str) -> bool {
        self.names.read().await.iter().any(|n| n == name)
    }

    /// Number of registered backends.
    pub async fn len(&self) -> usize {
        self.names.read().await.len()
    }

    /// Whether the list is empty.
    pub async fn is_empty(&self) -> bool {
        self.names.read().await.is_empty()
    }
}

// ---------------------------------------------------------------------------
// ServerState — shared state for DaemonServer
// ---------------------------------------------------------------------------

/// Shared application state for [`DaemonServer`](crate::server::DaemonServer).
///
/// Wraps a [`BackendList`] and [`RunRegistry`] together with a start time
/// for uptime tracking. Designed to be wrapped in [`Arc`] and passed as
/// Axum router state.
#[derive(Clone)]
pub struct ServerState {
    /// Registered backend names.
    pub backends: BackendList,
    /// Registry of tracked runs.
    pub registry: RunRegistry,
    /// Instant the server was created (for uptime calculation).
    pub start_time: std::time::Instant,
}

impl ServerState {
    /// Create a new server state pre-populated with the given backend names.
    pub fn new(backend_names: Vec<String>) -> Self {
        Self {
            backends: BackendList::from_names(backend_names),
            registry: RunRegistry::new(),
            start_time: std::time::Instant::now(),
        }
    }

    /// Server uptime in whole seconds since creation.
    pub fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }
}

impl Default for ServerState {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}
