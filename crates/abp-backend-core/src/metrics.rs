// SPDX-License-Identifier: MIT OR Apache-2.0
//! Backend metrics collection.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Accumulated metrics for a single backend.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BackendMetrics {
    /// Total number of runs attempted.
    pub total_runs: u64,
    /// Number of runs that completed successfully.
    pub successful_runs: u64,
    /// Number of runs that failed.
    pub failed_runs: u64,
    /// Cumulative latency of all runs in milliseconds.
    pub total_latency_ms: u64,
    /// Timestamp of the most recent run, if any.
    pub last_run_at: Option<DateTime<Utc>>,
}


impl BackendMetrics {
    /// Record a successful run with the given latency.
    pub fn record_success(&mut self, latency_ms: u64) {
        self.total_runs += 1;
        self.successful_runs += 1;
        self.total_latency_ms += latency_ms;
        self.last_run_at = Some(Utc::now());
    }

    /// Record a failed run with the given latency.
    pub fn record_failure(&mut self, latency_ms: u64) {
        self.total_runs += 1;
        self.failed_runs += 1;
        self.total_latency_ms += latency_ms;
        self.last_run_at = Some(Utc::now());
    }

    /// Average latency in milliseconds, or `None` if no runs have been recorded.
    #[must_use]
    pub fn average_latency_ms(&self) -> Option<f64> {
        if self.total_runs == 0 {
            None
        } else {
            Some(self.total_latency_ms as f64 / self.total_runs as f64)
        }
    }

    /// Success rate as a fraction in `[0.0, 1.0]`, or `None` if no runs.
    #[must_use]
    pub fn success_rate(&self) -> Option<f64> {
        if self.total_runs == 0 {
            None
        } else {
            Some(self.successful_runs as f64 / self.total_runs as f64)
        }
    }
}
