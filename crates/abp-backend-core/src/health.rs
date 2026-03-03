// SPDX-License-Identifier: MIT OR Apache-2.0
//! Backend health tracking types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Simplified health status for a backend.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    /// The backend is operating normally.
    Healthy,
    /// The backend is operational but experiencing issues.
    Degraded,
    /// The backend is not operational.
    Unhealthy,
    /// The backend's status has not been determined.
    #[default]
    Unknown,
}

/// Health snapshot for a single backend.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackendHealth {
    /// Current health status.
    pub status: HealthStatus,
    /// Timestamp of the last health check.
    pub last_check: Option<DateTime<Utc>>,
    /// Round-trip latency of the last check in milliseconds.
    pub latency_ms: Option<u64>,
    /// Observed error rate (0.0 = no errors, 1.0 = all errors).
    pub error_rate: f64,
    /// Number of consecutive failures.
    pub consecutive_failures: u32,
}

impl Default for BackendHealth {
    fn default() -> Self {
        Self {
            status: HealthStatus::Unknown,
            last_check: None,
            latency_ms: None,
            error_rate: 0.0,
            consecutive_failures: 0,
        }
    }
}
