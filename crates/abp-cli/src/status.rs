// SPDX-License-Identifier: MIT OR Apache-2.0
//! Status subcommand implementation.
//!
//! Displays runtime and daemon status information including registered
//! backends, configuration summary, and health.

#![allow(dead_code, unused_imports)]

use abp_core::CONTRACT_VERSION;
use abp_runtime::Runtime;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Health status of the runtime.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    /// The runtime is healthy and ready to accept work orders.
    Healthy,
    /// The runtime is running but some backends are unavailable.
    Degraded,
    /// The runtime or daemon is not running.
    Unavailable,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HealthStatus::Healthy => write!(f, "healthy"),
            HealthStatus::Degraded => write!(f, "degraded"),
            HealthStatus::Unavailable => write!(f, "unavailable"),
        }
    }
}

/// Collected status information about the runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusInfo {
    /// Contract version string.
    pub contract_version: String,
    /// Health status.
    pub health: HealthStatus,
    /// Names of registered backends.
    pub backends: Vec<String>,
    /// Number of registered backends.
    pub backend_count: usize,
    /// Whether a daemon is reachable (always `false` in CLI-only mode).
    pub daemon_running: bool,
    /// Number of active runs (always 0 in CLI-only mode; placeholder for daemon).
    pub active_runs: usize,
    /// Optional configuration summary.
    pub config_summary: Option<ConfigSummary>,
}

/// Summary of the active configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSummary {
    /// Default backend name, if configured.
    pub default_backend: Option<String>,
    /// Log level.
    pub log_level: Option<String>,
    /// Receipts directory.
    pub receipts_dir: Option<String>,
}

/// Gather status information about the runtime.
///
/// In CLI-only mode, this creates a transient runtime to enumerate backends
/// and reports the daemon as unavailable.
pub fn gather_status(config: &abp_config::BackplaneConfig) -> Result<StatusInfo> {
    let rt = Runtime::with_default_backends();
    let backends = rt.backend_names();
    let backend_count = backends.len();

    let health = if backend_count > 0 {
        HealthStatus::Healthy
    } else {
        HealthStatus::Degraded
    };

    let config_summary = ConfigSummary {
        default_backend: config.default_backend.clone(),
        log_level: config.log_level.clone(),
        receipts_dir: config.receipts_dir.clone(),
    };

    Ok(StatusInfo {
        contract_version: CONTRACT_VERSION.to_string(),
        health,
        backends,
        backend_count,
        daemon_running: false,
        active_runs: 0,
        config_summary: Some(config_summary),
    })
}

/// Print status information to stdout in a human-readable format.
pub fn print_status(info: &StatusInfo) -> Result<()> {
    println!("Agent Backplane Status");
    println!("======================");
    println!("contract: {}", info.contract_version);
    println!("health:   {}", info.health);
    println!("daemon:   {}", if info.daemon_running { "running" } else { "not running" });
    println!("active:   {} run(s)", info.active_runs);
    println!();
    println!("Backends ({}):", info.backend_count);
    for name in &info.backends {
        println!("  - {name}");
    }
    if let Some(ref summary) = info.config_summary {
        println!();
        println!("Config:");
        println!(
            "  default_backend: {}",
            summary.default_backend.as_deref().unwrap_or("<none>")
        );
        println!(
            "  log_level:       {}",
            summary.log_level.as_deref().unwrap_or("<none>")
        );
        println!(
            "  receipts_dir:    {}",
            summary.receipts_dir.as_deref().unwrap_or("<none>")
        );
    }
    Ok(())
}

/// Print status information as JSON.
pub fn print_status_json(info: &StatusInfo) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(info)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gather_status_with_defaults() {
        let config = abp_config::BackplaneConfig::default();
        let info = gather_status(&config).unwrap();
        assert_eq!(info.contract_version, CONTRACT_VERSION);
        assert!(!info.daemon_running);
        assert_eq!(info.active_runs, 0);
        // Default runtime has at least the "mock" backend.
        assert!(!info.backends.is_empty());
        assert_eq!(info.backend_count, info.backends.len());
    }

    #[test]
    fn gather_status_health_is_healthy_with_backends() {
        let config = abp_config::BackplaneConfig::default();
        let info = gather_status(&config).unwrap();
        assert_eq!(info.health, HealthStatus::Healthy);
    }

    #[test]
    fn status_info_serializes_to_json() {
        let config = abp_config::BackplaneConfig::default();
        let info = gather_status(&config).unwrap();
        let json = serde_json::to_string(&info).unwrap();
        let parsed: StatusInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.contract_version, info.contract_version);
        assert_eq!(parsed.backends, info.backends);
    }

    #[test]
    fn health_status_display() {
        assert_eq!(HealthStatus::Healthy.to_string(), "healthy");
        assert_eq!(HealthStatus::Degraded.to_string(), "degraded");
        assert_eq!(HealthStatus::Unavailable.to_string(), "unavailable");
    }

    #[test]
    fn config_summary_captures_fields() {
        let mut config = abp_config::BackplaneConfig::default();
        config.default_backend = Some("mock".into());
        config.log_level = Some("debug".into());
        config.receipts_dir = Some("/tmp/receipts".into());
        let info = gather_status(&config).unwrap();
        let summary = info.config_summary.unwrap();
        assert_eq!(summary.default_backend.as_deref(), Some("mock"));
        assert_eq!(summary.log_level.as_deref(), Some("debug"));
        assert_eq!(summary.receipts_dir.as_deref(), Some("/tmp/receipts"));
    }
}
