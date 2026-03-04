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

impl BackendHealth {
    /// Record a successful health check and update status to [`HealthStatus::Healthy`].
    pub fn record_success(&mut self, latency_ms: u64) {
        self.status = HealthStatus::Healthy;
        self.last_check = Some(Utc::now());
        self.latency_ms = Some(latency_ms);
        self.consecutive_failures = 0;
        self.error_rate = 0.0;
    }

    /// Record a failed health check, incrementing the consecutive failure count.
    ///
    /// Transitions status to [`HealthStatus::Degraded`] on the first failure
    /// and [`HealthStatus::Unhealthy`] after `unhealthy_threshold` consecutive failures.
    pub fn record_failure(&mut self, unhealthy_threshold: u32) {
        self.consecutive_failures += 1;
        self.last_check = Some(Utc::now());
        if self.consecutive_failures >= unhealthy_threshold {
            self.status = HealthStatus::Unhealthy;
            self.error_rate = 1.0;
        } else {
            self.status = HealthStatus::Degraded;
            self.error_rate = self.consecutive_failures as f64 / unhealthy_threshold as f64;
        }
    }

    /// Returns `true` if the backend is considered operational
    /// ([`HealthStatus::Healthy`] or [`HealthStatus::Degraded`]).
    #[must_use]
    pub fn is_operational(&self) -> bool {
        matches!(self.status, HealthStatus::Healthy | HealthStatus::Degraded)
    }
}
