// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the health monitoring module.

use abp_host::health::{HealthMonitor, HealthReport, HealthStatus};
use std::time::Duration;

// ── HealthStatus ────────────────────────────────────────────────────

#[test]
fn health_status_variants_are_distinct() {
    let statuses = [
        HealthStatus::Healthy,
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        HealthStatus::Unhealthy {
            reason: "down".into(),
        },
        HealthStatus::Unknown,
    ];
    for (i, a) in statuses.iter().enumerate() {
        for (j, b) in statuses.iter().enumerate() {
            if i == j {
                assert_eq!(a, b);
            } else {
                assert_ne!(a, b);
            }
        }
    }
}

#[test]
fn health_status_serde_roundtrip() {
    let cases = vec![
        HealthStatus::Healthy,
        HealthStatus::Degraded {
            reason: "high latency".into(),
        },
        HealthStatus::Unhealthy {
            reason: "connection refused".into(),
        },
        HealthStatus::Unknown,
    ];
    for status in &cases {
        let json = serde_json::to_string(status).unwrap();
        let back: HealthStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, status);
    }
}

// ── HealthMonitor basics ────────────────────────────────────────────

#[test]
fn new_monitor_is_empty() {
    let mon = HealthMonitor::new();
    assert_eq!(mon.total_checks(), 0);
    assert!(!mon.all_healthy());
    assert!(mon.unhealthy_sidecars().is_empty());
}

#[test]
fn record_and_get_status() {
    let mut mon = HealthMonitor::new();
    mon.record_check(
        "node",
        HealthStatus::Healthy,
        Some(Duration::from_millis(42)),
    );

    let check = mon.get_status("node").expect("should exist");
    assert_eq!(check.name, "node");
    assert_eq!(check.status, HealthStatus::Healthy);
    assert_eq!(check.response_time, Some(Duration::from_millis(42)));
    assert_eq!(check.consecutive_failures, 0);
}

#[test]
fn get_status_missing_returns_none() {
    let mon = HealthMonitor::new();
    assert!(mon.get_status("nonexistent").is_none());
}

#[test]
fn all_healthy_with_mixed_statuses() {
    let mut mon = HealthMonitor::new();
    mon.record_check("a", HealthStatus::Healthy, None);
    assert!(mon.all_healthy());

    mon.record_check(
        "b",
        HealthStatus::Unhealthy {
            reason: "crash".into(),
        },
        None,
    );
    assert!(!mon.all_healthy());
}

#[test]
fn unhealthy_sidecars_filters_correctly() {
    let mut mon = HealthMonitor::new();
    mon.record_check("ok", HealthStatus::Healthy, None);
    mon.record_check(
        "bad",
        HealthStatus::Unhealthy {
            reason: "timeout".into(),
        },
        None,
    );
    mon.record_check(
        "slow",
        HealthStatus::Degraded {
            reason: "latency".into(),
        },
        None,
    );

    let unhealthy = mon.unhealthy_sidecars();
    assert_eq!(unhealthy.len(), 1);
    assert_eq!(unhealthy[0].name, "bad");
}

// ── Consecutive failures ────────────────────────────────────────────

#[test]
fn consecutive_failures_increment_and_reset() {
    let mut mon = HealthMonitor::new();

    mon.record_check(
        "s",
        HealthStatus::Unhealthy {
            reason: "err".into(),
        },
        None,
    );
    assert_eq!(mon.get_status("s").unwrap().consecutive_failures, 1);

    mon.record_check(
        "s",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        None,
    );
    assert_eq!(mon.get_status("s").unwrap().consecutive_failures, 2);

    // Recovery resets the counter.
    mon.record_check("s", HealthStatus::Healthy, None);
    assert_eq!(mon.get_status("s").unwrap().consecutive_failures, 0);
}

// ── Uptime percentage ───────────────────────────────────────────────

#[test]
fn uptime_percentage_no_history() {
    let mon = HealthMonitor::new();
    assert_eq!(mon.uptime_percentage("ghost"), 0.0);
}

#[test]
fn uptime_percentage_computed_from_history() {
    let mut mon = HealthMonitor::new();
    // 3 healthy, 1 unhealthy → 75%
    mon.record_check("x", HealthStatus::Healthy, None);
    mon.record_check("x", HealthStatus::Healthy, None);
    mon.record_check("x", HealthStatus::Healthy, None);
    mon.record_check(
        "x",
        HealthStatus::Unhealthy {
            reason: "fail".into(),
        },
        None,
    );

    let pct = mon.uptime_percentage("x");
    assert!((pct - 75.0).abs() < f64::EPSILON);
}

// ── HealthReport ────────────────────────────────────────────────────

#[test]
fn generate_report_empty_monitor() {
    let mon = HealthMonitor::new();
    let report = mon.generate_report();
    assert_eq!(report.overall, HealthStatus::Unknown);
    assert!(report.checks.is_empty());
}

#[test]
fn generate_report_overall_status() {
    let mut mon = HealthMonitor::new();
    mon.record_check("a", HealthStatus::Healthy, None);
    mon.record_check("b", HealthStatus::Healthy, None);

    let report = mon.generate_report();
    assert_eq!(report.overall, HealthStatus::Healthy);

    // Adding a degraded sidecar should degrade overall.
    mon.record_check(
        "c",
        HealthStatus::Degraded {
            reason: "lag".into(),
        },
        None,
    );
    let report = mon.generate_report();
    assert!(matches!(report.overall, HealthStatus::Degraded { .. }));

    // An unhealthy sidecar dominates.
    mon.record_check(
        "d",
        HealthStatus::Unhealthy {
            reason: "dead".into(),
        },
        None,
    );
    let report = mon.generate_report();
    assert!(matches!(report.overall, HealthStatus::Unhealthy { .. }));
}

#[test]
fn health_report_serde_roundtrip() {
    let mut mon = HealthMonitor::new();
    mon.record_check(
        "node",
        HealthStatus::Healthy,
        Some(Duration::from_millis(5)),
    );
    mon.record_check(
        "python",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
        None,
    );

    let report = mon.generate_report();
    let json = serde_json::to_string_pretty(&report).unwrap();
    let back: HealthReport = serde_json::from_str(&json).unwrap();

    assert_eq!(back.checks.len(), 2);
    assert!(matches!(back.overall, HealthStatus::Degraded { .. }));
}

// ── total_checks ────────────────────────────────────────────────────

#[test]
fn total_checks_counts_unique_sidecars() {
    let mut mon = HealthMonitor::new();
    mon.record_check("a", HealthStatus::Healthy, None);
    mon.record_check("b", HealthStatus::Healthy, None);
    // Updating "a" should not increase the count.
    mon.record_check("a", HealthStatus::Unknown, None);

    assert_eq!(mon.total_checks(), 2);
}
