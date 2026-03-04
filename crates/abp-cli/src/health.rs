// SPDX-License-Identifier: MIT OR Apache-2.0
//! Health subcommand implementation.
//!
//! Checks health of all configured backends by probing their identity
//! and capabilities.

use abp_core::CONTRACT_VERSION;
use abp_runtime::Runtime;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Health status for an individual backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackendHealthStatus {
    /// Backend is registered and reachable.
    Ok,
    /// Backend is registered but degraded.
    Degraded,
    /// Backend is not available.
    Unavailable,
}

impl std::fmt::Display for BackendHealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ok => write!(f, "ok"),
            Self::Degraded => write!(f, "degraded"),
            Self::Unavailable => write!(f, "unavailable"),
        }
    }
}

/// Health probe result for one backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendProbe {
    /// Backend name.
    pub name: String,
    /// Health status.
    pub status: BackendHealthStatus,
    /// Number of capabilities reported by the backend.
    pub capability_count: usize,
    /// Optional message (e.g. error reason).
    pub message: Option<String>,
}

/// Aggregate health report for all backends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    /// Contract version.
    pub contract_version: String,
    /// Overall health status.
    pub overall: BackendHealthStatus,
    /// Individual backend probes.
    pub backends: Vec<BackendProbe>,
}

/// Probe all registered backends and build a health report.
pub fn check_health(config: &abp_config::BackplaneConfig) -> Result<HealthReport> {
    let rt = build_runtime(config);
    let names = rt.backend_names();
    let mut probes = Vec::new();

    for name in &names {
        let probe = match rt.backend(name) {
            Some(b) => {
                let caps = b.capabilities();
                BackendProbe {
                    name: name.clone(),
                    status: BackendHealthStatus::Ok,
                    capability_count: caps.len(),
                    message: None,
                }
            }
            None => BackendProbe {
                name: name.clone(),
                status: BackendHealthStatus::Unavailable,
                capability_count: 0,
                message: Some("backend not found".into()),
            },
        };
        probes.push(probe);
    }

    let overall = if probes.is_empty() {
        BackendHealthStatus::Unavailable
    } else if probes.iter().all(|p| p.status == BackendHealthStatus::Ok) {
        BackendHealthStatus::Ok
    } else if probes.iter().any(|p| p.status == BackendHealthStatus::Ok) {
        BackendHealthStatus::Degraded
    } else {
        BackendHealthStatus::Unavailable
    };

    Ok(HealthReport {
        contract_version: CONTRACT_VERSION.to_string(),
        overall,
        backends: probes,
    })
}

/// Print the health report in human-readable format.
pub fn print_health(report: &HealthReport) -> Result<()> {
    println!("Health Check ({})", report.contract_version);
    println!("overall: {}", report.overall);
    println!();
    for probe in &report.backends {
        let msg = probe.message.as_deref().unwrap_or("");
        println!(
            "  {:<20} status={:<12} capabilities={}  {}",
            probe.name, probe.status, probe.capability_count, msg
        );
    }
    Ok(())
}

/// Print the health report as JSON.
pub fn print_health_json(report: &HealthReport) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(report)?);
    Ok(())
}

fn build_runtime(config: &abp_config::BackplaneConfig) -> Runtime {
    let mut rt = Runtime::with_default_backends();
    for (name, entry) in &config.backends {
        match entry {
            abp_config::BackendEntry::Mock {} => {
                rt.register_backend(name, abp_integrations::MockBackend);
            }
            abp_config::BackendEntry::Sidecar { command, args, .. } => {
                let mut spec = abp_host::SidecarSpec::new(command);
                spec.args = args.clone();
                rt.register_backend(name, abp_integrations::SidecarBackend::new(spec));
            }
        }
    }
    rt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_health_default_config() {
        let config = abp_config::BackplaneConfig::default();
        let report = check_health(&config).unwrap();
        assert_eq!(report.contract_version, CONTRACT_VERSION);
        assert!(!report.backends.is_empty());
        assert_eq!(report.overall, BackendHealthStatus::Ok);
    }

    #[test]
    fn health_report_serializes_to_json() {
        let config = abp_config::BackplaneConfig::default();
        let report = check_health(&config).unwrap();
        let json = serde_json::to_string(&report).unwrap();
        let parsed: HealthReport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.overall, report.overall);
        assert_eq!(parsed.backends.len(), report.backends.len());
    }

    #[test]
    fn backend_health_status_display() {
        assert_eq!(BackendHealthStatus::Ok.to_string(), "ok");
        assert_eq!(BackendHealthStatus::Degraded.to_string(), "degraded");
        assert_eq!(BackendHealthStatus::Unavailable.to_string(), "unavailable");
    }

    #[test]
    fn health_with_mock_backend_in_config() {
        let mut config = abp_config::BackplaneConfig::default();
        config
            .backends
            .insert("test-mock".into(), abp_config::BackendEntry::Mock {});
        let report = check_health(&config).unwrap();
        assert!(report.backends.iter().any(|p| p.name == "test-mock"));
    }

    #[test]
    fn health_probe_structure() {
        let probe = BackendProbe {
            name: "test".into(),
            status: BackendHealthStatus::Ok,
            capability_count: 5,
            message: None,
        };
        assert_eq!(probe.name, "test");
        assert_eq!(probe.capability_count, 5);
        assert!(probe.message.is_none());
    }
}
