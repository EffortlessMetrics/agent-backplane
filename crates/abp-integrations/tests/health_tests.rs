// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the health-check infrastructure.

use abp_integrations::health::{HealthCheck, HealthChecker, HealthStatus, SystemHealth};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// HealthStatus
// ---------------------------------------------------------------------------

#[test]
fn health_status_healthy_equality() {
    assert_eq!(HealthStatus::Healthy, HealthStatus::Healthy);
}

#[test]
fn health_status_unknown_equality() {
    assert_eq!(HealthStatus::Unknown, HealthStatus::Unknown);
}

#[test]
fn health_status_degraded_equality() {
    let a = HealthStatus::Degraded {
        reason: "slow".into(),
    };
    let b = HealthStatus::Degraded {
        reason: "slow".into(),
    };
    assert_eq!(a, b);
}

#[test]
fn health_status_unhealthy_equality() {
    let a = HealthStatus::Unhealthy {
        reason: "down".into(),
    };
    let b = HealthStatus::Unhealthy {
        reason: "down".into(),
    };
    assert_eq!(a, b);
}

#[test]
fn health_status_different_variants_not_equal() {
    assert_ne!(HealthStatus::Healthy, HealthStatus::Unknown);
    assert_ne!(
        HealthStatus::Healthy,
        HealthStatus::Degraded { reason: "x".into() }
    );
}

#[test]
fn health_status_serde_roundtrip() {
    let cases = vec![
        HealthStatus::Healthy,
        HealthStatus::Unknown,
        HealthStatus::Degraded {
            reason: "high latency".into(),
        },
        HealthStatus::Unhealthy {
            reason: "timeout".into(),
        },
    ];
    for status in cases {
        let json = serde_json::to_string(&status).unwrap();
        let back: HealthStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, back);
    }
}

// ---------------------------------------------------------------------------
// HealthChecker — empty
// ---------------------------------------------------------------------------

#[test]
fn checker_new_is_empty() {
    let checker = HealthChecker::new();
    assert_eq!(checker.check_count(), 0);
    assert!(checker.checks().is_empty());
}

#[test]
fn checker_empty_is_healthy() {
    let checker = HealthChecker::new();
    assert!(checker.is_healthy());
    assert_eq!(checker.overall_status(), HealthStatus::Healthy);
}

// ---------------------------------------------------------------------------
// HealthChecker — single checks
// ---------------------------------------------------------------------------

#[test]
fn checker_add_healthy_check() {
    let mut checker = HealthChecker::new();
    checker.add_check("db", HealthStatus::Healthy);
    assert_eq!(checker.check_count(), 1);
    assert!(checker.is_healthy());
    assert_eq!(checker.checks()[0].name, "db");
}

#[test]
fn checker_add_degraded_check() {
    let mut checker = HealthChecker::new();
    checker.add_check(
        "cache",
        HealthStatus::Degraded {
            reason: "high miss rate".into(),
        },
    );
    assert!(!checker.is_healthy());
    assert_eq!(
        checker.overall_status(),
        HealthStatus::Degraded {
            reason: "high miss rate".into()
        }
    );
}

#[test]
fn checker_add_unhealthy_check() {
    let mut checker = HealthChecker::new();
    checker.add_check(
        "api",
        HealthStatus::Unhealthy {
            reason: "connection refused".into(),
        },
    );
    assert!(!checker.is_healthy());
    assert_eq!(
        checker.overall_status(),
        HealthStatus::Unhealthy {
            reason: "connection refused".into()
        }
    );
}

// ---------------------------------------------------------------------------
// HealthChecker — overall_status picks worst
// ---------------------------------------------------------------------------

#[test]
fn checker_overall_picks_worst_degraded() {
    let mut checker = HealthChecker::new();
    checker.add_check("a", HealthStatus::Healthy);
    checker.add_check(
        "b",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
    );
    assert_eq!(
        checker.overall_status(),
        HealthStatus::Degraded {
            reason: "slow".into()
        }
    );
}

#[test]
fn checker_overall_picks_worst_unhealthy() {
    let mut checker = HealthChecker::new();
    checker.add_check("a", HealthStatus::Healthy);
    checker.add_check(
        "b",
        HealthStatus::Degraded {
            reason: "slow".into(),
        },
    );
    checker.add_check(
        "c",
        HealthStatus::Unhealthy {
            reason: "dead".into(),
        },
    );
    assert_eq!(
        checker.overall_status(),
        HealthStatus::Unhealthy {
            reason: "dead".into()
        }
    );
}

#[test]
fn checker_overall_unknown_worse_than_healthy() {
    let mut checker = HealthChecker::new();
    checker.add_check("a", HealthStatus::Healthy);
    checker.add_check("b", HealthStatus::Unknown);
    assert_eq!(checker.overall_status(), HealthStatus::Unknown);
}

// ---------------------------------------------------------------------------
// HealthChecker — unhealthy_checks
// ---------------------------------------------------------------------------

#[test]
fn checker_unhealthy_checks_filters() {
    let mut checker = HealthChecker::new();
    checker.add_check("ok1", HealthStatus::Healthy);
    checker.add_check(
        "bad1",
        HealthStatus::Unhealthy {
            reason: "err".into(),
        },
    );
    checker.add_check("ok2", HealthStatus::Healthy);
    checker.add_check(
        "bad2",
        HealthStatus::Degraded {
            reason: "lag".into(),
        },
    );

    let bad = checker.unhealthy_checks();
    assert_eq!(bad.len(), 2);
    assert_eq!(bad[0].name, "bad1");
    assert_eq!(bad[1].name, "bad2");
}

// ---------------------------------------------------------------------------
// HealthChecker — clear
// ---------------------------------------------------------------------------

#[test]
fn checker_clear_resets() {
    let mut checker = HealthChecker::new();
    checker.add_check("a", HealthStatus::Healthy);
    checker.add_check("b", HealthStatus::Unhealthy { reason: "x".into() });
    assert_eq!(checker.check_count(), 2);

    checker.clear();
    assert_eq!(checker.check_count(), 0);
    assert!(checker.is_healthy());
}

// ---------------------------------------------------------------------------
// HealthCheck fields
// ---------------------------------------------------------------------------

#[test]
fn health_check_checked_at_populated() {
    let mut checker = HealthChecker::new();
    checker.add_check("ts", HealthStatus::Healthy);
    let check = &checker.checks()[0];
    assert!(!check.checked_at.is_empty());
    // Should be a valid RFC-3339 timestamp.
    assert!(check.checked_at.contains('T'));
}

#[test]
fn health_check_serde_roundtrip() {
    let check = HealthCheck {
        name: "db".into(),
        status: HealthStatus::Healthy,
        checked_at: "2025-01-01T00:00:00Z".into(),
        response_time_ms: Some(42),
        details: BTreeMap::from([("host".into(), "localhost".into())]),
    };
    let json = serde_json::to_string(&check).unwrap();
    let back: HealthCheck = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "db");
    assert_eq!(back.response_time_ms, Some(42));
    assert_eq!(back.details.get("host").unwrap(), "localhost");
}

// ---------------------------------------------------------------------------
// SystemHealth serde
// ---------------------------------------------------------------------------

#[test]
fn system_health_serde_roundtrip() {
    let sh = SystemHealth {
        backends: vec![HealthCheck {
            name: "mock".into(),
            status: HealthStatus::Healthy,
            checked_at: "2025-01-01T00:00:00Z".into(),
            response_time_ms: None,
            details: BTreeMap::new(),
        }],
        overall: HealthStatus::Healthy,
        uptime_seconds: 3600,
        version: "0.1.0".into(),
    };
    let json = serde_json::to_string(&sh).unwrap();
    let back: SystemHealth = serde_json::from_str(&json).unwrap();
    assert_eq!(back.uptime_seconds, 3600);
    assert_eq!(back.version, "0.1.0");
    assert_eq!(back.backends.len(), 1);
    assert_eq!(back.overall, HealthStatus::Healthy);
}
