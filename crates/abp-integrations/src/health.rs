// SPDX-License-Identifier: MIT OR Apache-2.0
//! Health check infrastructure for backend monitoring.
//!
//! Provides [`HealthStatus`], [`HealthCheck`], [`HealthChecker`], and
//! [`SystemHealth`] for tracking the operational state of backends.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Operational status of a single health check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum HealthStatus {
    /// The component is operating normally.
    Healthy,
    /// The component is operational but experiencing issues.
    Degraded {
        /// Human-readable explanation of the degradation.
        reason: String,
    },
    /// The component is not operational.
    Unhealthy {
        /// Human-readable explanation of the failure.
        reason: String,
    },
    /// The component's status has not been determined.
    Unknown,
}

/// Result of a single health check.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthCheck {
    /// Name identifying what was checked.
    pub name: String,
    /// Observed status.
    pub status: HealthStatus,
    /// ISO-8601 timestamp of when the check was performed.
    pub checked_at: String,
    /// Round-trip time of the check, if measured.
    pub response_time_ms: Option<u64>,
    /// Arbitrary key-value metadata about the check.
    pub details: BTreeMap<String, String>,
}

/// Accumulates [`HealthCheck`] results and derives an overall status.
#[derive(Debug, Default)]
pub struct HealthChecker {
    checks: Vec<HealthCheck>,
}

impl HealthChecker {
    /// Create a new, empty health checker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a check result with the given name and status.
    pub fn add_check(&mut self, name: &str, status: HealthStatus) {
        self.checks.push(HealthCheck {
            name: name.to_string(),
            status,
            checked_at: Utc::now().to_rfc3339(),
            response_time_ms: None,
            details: BTreeMap::new(),
        });
    }

    /// Return all recorded checks.
    #[must_use]
    pub fn checks(&self) -> &[HealthCheck] {
        &self.checks
    }

    /// Derive the worst status across all checks.
    ///
    /// Returns [`HealthStatus::Healthy`] when no checks have been recorded.
    #[must_use]
    pub fn overall_status(&self) -> HealthStatus {
        if self.checks.is_empty() {
            return HealthStatus::Healthy;
        }

        let mut worst = 0u8; // 0=healthy, 1=unknown, 2=degraded, 3=unhealthy
        let mut worst_reason = String::new();

        for check in &self.checks {
            let severity = severity(&check.status);
            if severity > worst {
                worst = severity;
                worst_reason = reason_of(&check.status);
            }
        }

        match worst {
            3 => HealthStatus::Unhealthy {
                reason: worst_reason,
            },
            2 => HealthStatus::Degraded {
                reason: worst_reason,
            },
            1 => HealthStatus::Unknown,
            _ => HealthStatus::Healthy,
        }
    }

    /// Returns `true` when all recorded checks are [`HealthStatus::Healthy`].
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.overall_status() == HealthStatus::Healthy
    }

    /// Return references to all checks whose status is not [`HealthStatus::Healthy`].
    #[must_use]
    pub fn unhealthy_checks(&self) -> Vec<&HealthCheck> {
        self.checks
            .iter()
            .filter(|c| c.status != HealthStatus::Healthy)
            .collect()
    }

    /// Remove all recorded checks.
    pub fn clear(&mut self) {
        self.checks.clear();
    }

    /// Number of recorded checks.
    #[must_use]
    pub fn check_count(&self) -> usize {
        self.checks.len()
    }
}

/// Aggregate system health report.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SystemHealth {
    /// Per-backend health checks.
    pub backends: Vec<HealthCheck>,
    /// Overall derived status.
    pub overall: HealthStatus,
    /// Seconds since the system started.
    pub uptime_seconds: u64,
    /// Software version string.
    pub version: String,
}

// --- helpers ----------------------------------------------------------------

fn severity(status: &HealthStatus) -> u8 {
    match status {
        HealthStatus::Healthy => 0,
        HealthStatus::Unknown => 1,
        HealthStatus::Degraded { .. } => 2,
        HealthStatus::Unhealthy { .. } => 3,
    }
}

fn reason_of(status: &HealthStatus) -> String {
    match status {
        HealthStatus::Degraded { reason } | HealthStatus::Unhealthy { reason } => reason.clone(),
        _ => String::new(),
    }
}
