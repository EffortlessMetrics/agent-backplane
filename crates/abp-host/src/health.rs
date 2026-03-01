// SPDX-License-Identifier: MIT OR Apache-2.0
//! Health monitoring for sidecar processes.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

/// Health status of a sidecar process.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum HealthStatus {
    /// The sidecar is operating normally.
    Healthy,
    /// The sidecar is running but experiencing issues.
    Degraded {
        /// Description of the degradation.
        reason: String,
    },
    /// The sidecar is not functioning correctly.
    Unhealthy {
        /// Description of the failure.
        reason: String,
    },
    /// The sidecar's health has not been determined yet.
    Unknown,
}

/// Result of a single health check for a named sidecar.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthCheck {
    /// Name identifying the sidecar.
    pub name: String,
    /// Current health status.
    pub status: HealthStatus,
    /// When this check was last performed.
    pub last_checked: DateTime<Utc>,
    /// How long the check took, if measured.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "option_duration_millis"
    )]
    pub response_time: Option<Duration>,
    /// Number of consecutive failures recorded.
    pub consecutive_failures: u32,
}

/// Serde helper for `Option<Duration>` as milliseconds.
mod option_duration_millis {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(val: &Option<Duration>, ser: S) -> Result<S::Ok, S::Error> {
        match val {
            Some(d) => d.as_millis().serialize(ser),
            None => ser.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Option<Duration>, D::Error> {
        let opt: Option<u64> = Option::deserialize(de)?;
        Ok(opt.map(Duration::from_millis))
    }
}

/// Aggregated health report across all monitored sidecars.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthReport {
    /// Rolled-up status for the entire fleet.
    pub overall: HealthStatus,
    /// Individual check results.
    pub checks: Vec<HealthCheck>,
    /// When this report was generated.
    pub generated_at: DateTime<Utc>,
}

/// Monitors health of registered sidecar processes.
#[derive(Debug)]
pub struct HealthMonitor {
    checks: BTreeMap<String, HealthCheck>,
    history: BTreeMap<String, Vec<bool>>,
}

impl HealthMonitor {
    /// Create a new, empty health monitor.
    pub fn new() -> Self {
        Self {
            checks: BTreeMap::new(),
            history: BTreeMap::new(),
        }
    }

    /// Record the result of a health check for the named sidecar.
    pub fn record_check(
        &mut self,
        name: &str,
        status: HealthStatus,
        response_time: Option<Duration>,
    ) {
        let is_healthy = matches!(status, HealthStatus::Healthy);
        let consecutive_failures = if is_healthy {
            0
        } else {
            self.checks
                .get(name)
                .map_or(1, |prev| prev.consecutive_failures + 1)
        };

        self.checks.insert(
            name.to_string(),
            HealthCheck {
                name: name.to_string(),
                status,
                last_checked: Utc::now(),
                response_time,
                consecutive_failures,
            },
        );

        self.history
            .entry(name.to_string())
            .or_default()
            .push(is_healthy);
    }

    /// Get the latest health check for a sidecar by name.
    pub fn get_status(&self, name: &str) -> Option<&HealthCheck> {
        self.checks.get(name)
    }

    /// Returns `true` if every tracked sidecar is currently `Healthy`.
    pub fn all_healthy(&self) -> bool {
        !self.checks.is_empty()
            && self
                .checks
                .values()
                .all(|c| matches!(c.status, HealthStatus::Healthy))
    }

    /// Return references to all checks whose status is `Unhealthy`.
    pub fn unhealthy_sidecars(&self) -> Vec<&HealthCheck> {
        self.checks
            .values()
            .filter(|c| matches!(c.status, HealthStatus::Unhealthy { .. }))
            .collect()
    }

    /// Number of sidecars being tracked.
    pub fn total_checks(&self) -> usize {
        self.checks.len()
    }

    /// Percentage of historical checks that were healthy (0.0–100.0).
    ///
    /// Returns `0.0` if no history exists for the given name.
    pub fn uptime_percentage(&self, name: &str) -> f64 {
        match self.history.get(name) {
            Some(h) if !h.is_empty() => {
                let healthy = h.iter().filter(|&&ok| ok).count();
                (healthy as f64 / h.len() as f64) * 100.0
            }
            _ => 0.0,
        }
    }

    /// Generate a point-in-time report of all monitored sidecars.
    pub fn generate_report(&self) -> HealthReport {
        let checks: Vec<HealthCheck> = self.checks.values().cloned().collect();
        let overall = Self::compute_overall(&checks);
        HealthReport {
            overall,
            checks,
            generated_at: Utc::now(),
        }
    }

    /// Derive a single rolled-up status from a set of checks.
    fn compute_overall(checks: &[HealthCheck]) -> HealthStatus {
        if checks.is_empty() {
            return HealthStatus::Unknown;
        }
        let any_unhealthy = checks
            .iter()
            .any(|c| matches!(c.status, HealthStatus::Unhealthy { .. }));
        if any_unhealthy {
            return HealthStatus::Unhealthy {
                reason: "one or more sidecars unhealthy".into(),
            };
        }
        let any_degraded = checks
            .iter()
            .any(|c| matches!(c.status, HealthStatus::Degraded { .. }));
        if any_degraded {
            return HealthStatus::Degraded {
                reason: "one or more sidecars degraded".into(),
            };
        }
        let any_unknown = checks
            .iter()
            .any(|c| matches!(c.status, HealthStatus::Unknown));
        if any_unknown {
            return HealthStatus::Unknown;
        }
        HealthStatus::Healthy
    }
}

impl Default for HealthMonitor {
    fn default() -> Self {
        Self::new()
    }
}
