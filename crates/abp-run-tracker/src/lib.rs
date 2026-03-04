#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Run lifecycle tracking for Agent Backplane execution services.

use abp_core::Receipt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Status of a tracked run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RunStatus {
    /// The run is queued but not yet started.
    Pending,
    /// The run is currently executing.
    Running,
    /// The run completed successfully with a receipt.
    Completed {
        /// The final receipt.
        receipt: Box<Receipt>,
    },
    /// The run failed with an error.
    Failed {
        /// Error description.
        error: String,
    },
    /// The run was cancelled by a user request.
    Cancelled,
}

/// Tracks active and finished runs with their current status.
#[derive(Clone, Default)]
pub struct RunTracker {
    runs: Arc<RwLock<HashMap<Uuid, RunStatus>>>,
}

impl RunTracker {
    /// Create an empty run tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark a run as running. Errors if the run is already tracked.
    pub async fn start_run(&self, run_id: Uuid) -> anyhow::Result<()> {
        let mut guard = self.runs.write().await;
        if guard.contains_key(&run_id) {
            anyhow::bail!("run {run_id} is already tracked");
        }
        guard.insert(run_id, RunStatus::Running);
        Ok(())
    }

    /// Transition a run to completed with its receipt. Errors if the run is
    /// not currently tracked.
    pub async fn complete_run(&self, run_id: Uuid, receipt: Receipt) -> anyhow::Result<()> {
        let mut guard = self.runs.write().await;
        if !guard.contains_key(&run_id) {
            anyhow::bail!("run {run_id} is not tracked");
        }
        guard.insert(
            run_id,
            RunStatus::Completed {
                receipt: Box::new(receipt),
            },
        );
        Ok(())
    }

    /// Transition a run to failed with an error message. Errors if the run is
    /// not currently tracked.
    pub async fn fail_run(&self, run_id: Uuid, error: String) -> anyhow::Result<()> {
        let mut guard = self.runs.write().await;
        if !guard.contains_key(&run_id) {
            anyhow::bail!("run {run_id} is not tracked");
        }
        guard.insert(run_id, RunStatus::Failed { error });
        Ok(())
    }

    /// Cancel a run. Only pending or running runs can be cancelled.
    pub async fn cancel_run(&self, run_id: Uuid) -> anyhow::Result<()> {
        let mut guard = self.runs.write().await;
        match guard.get(&run_id) {
            None => anyhow::bail!("run {run_id} is not tracked"),
            Some(RunStatus::Pending) | Some(RunStatus::Running) => {
                guard.insert(run_id, RunStatus::Cancelled);
                Ok(())
            }
            Some(_) => anyhow::bail!("run {run_id} is already in a terminal state"),
        }
    }

    /// Return the current status of a run, or `None` if not tracked.
    pub async fn get_run_status(&self, run_id: Uuid) -> Option<RunStatus> {
        self.runs.read().await.get(&run_id).cloned()
    }

    /// Remove a completed or failed run from the tracker. Returns the removed
    /// status, or an error if the run is still running or not found.
    pub async fn remove_run(&self, run_id: Uuid) -> Result<RunStatus, &'static str> {
        let mut guard = self.runs.write().await;
        match guard.get(&run_id) {
            None => Err("not found"),
            Some(RunStatus::Running) | Some(RunStatus::Pending) => Err("conflict"),
            Some(RunStatus::Completed { .. })
            | Some(RunStatus::Failed { .. })
            | Some(RunStatus::Cancelled) => Ok(guard.remove(&run_id).expect("state checked above")),
        }
    }

    /// Return all tracked runs and their statuses.
    pub async fn list_runs(&self) -> Vec<(Uuid, RunStatus)> {
        self.runs
            .read()
            .await
            .iter()
            .map(|(id, s)| (*id, s.clone()))
            .collect()
    }
}

/// Aggregate run metrics exposed by status endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMetrics {
    /// Total number of runs tracked.
    pub total_runs: usize,
    /// Number of currently running tasks.
    pub running: usize,
    /// Number of completed runs.
    pub completed: usize,
    /// Number of failed runs.
    pub failed: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tracks_failure_lifecycle() {
        let tracker = RunTracker::new();
        let run_id = Uuid::new_v4();

        tracker.start_run(run_id).await.expect("start");
        tracker
            .fail_run(run_id, "boom".to_string())
            .await
            .expect("fail");

        let status = tracker.get_run_status(run_id).await;
        assert!(matches!(status, Some(RunStatus::Failed { .. })));
    }
}
