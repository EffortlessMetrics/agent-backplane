// SPDX-License-Identifier: MIT OR Apache-2.0
//! Telemetry and metrics collection for runtime runs.

use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

/// Atomic run-level metrics that can be shared across threads.
pub struct RunMetrics {
    total_runs: AtomicU64,
    successful_runs: AtomicU64,
    failed_runs: AtomicU64,
    total_events: AtomicU64,
    /// Cumulative duration used to compute the running average.
    cumulative_duration_ms: AtomicU64,
    average_run_duration_ms: AtomicU64,
}

impl RunMetrics {
    /// Create a new, zero-initialised metrics collector.
    #[must_use]
    pub fn new() -> Self {
        Self {
            total_runs: AtomicU64::new(0),
            successful_runs: AtomicU64::new(0),
            failed_runs: AtomicU64::new(0),
            total_events: AtomicU64::new(0),
            cumulative_duration_ms: AtomicU64::new(0),
            average_run_duration_ms: AtomicU64::new(0),
        }
    }

    /// Record the outcome of a single run.
    pub fn record_run(&self, duration_ms: u64, success: bool, event_count: u64) {
        let total = self.total_runs.fetch_add(1, Relaxed) + 1;
        if success {
            self.successful_runs.fetch_add(1, Relaxed);
        } else {
            self.failed_runs.fetch_add(1, Relaxed);
        }
        self.total_events.fetch_add(event_count, Relaxed);
        let cumulative = self.cumulative_duration_ms.fetch_add(duration_ms, Relaxed) + duration_ms;
        self.average_run_duration_ms
            .store(cumulative / total, Relaxed);
    }

    /// Take a point-in-time snapshot of the current metric values.
    #[must_use]
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            total_runs: self.total_runs.load(Relaxed),
            successful_runs: self.successful_runs.load(Relaxed),
            failed_runs: self.failed_runs.load(Relaxed),
            total_events: self.total_events.load(Relaxed),
            average_run_duration_ms: self.average_run_duration_ms.load(Relaxed),
        }
    }
}

impl Default for RunMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Non-atomic, serialisable snapshot of [`RunMetrics`].
#[derive(Debug, Clone, Serialize)]
pub struct MetricsSnapshot {
    /// Total number of runs recorded.
    pub total_runs: u64,
    /// Number of runs that completed successfully.
    pub successful_runs: u64,
    /// Number of runs that failed.
    pub failed_runs: u64,
    /// Cumulative number of [`AgentEvent`](abp_core::AgentEvent)s across all runs.
    pub total_events: u64,
    /// Running average of run duration in milliseconds.
    pub average_run_duration_ms: u64,
}
