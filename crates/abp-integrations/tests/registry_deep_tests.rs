#![allow(clippy::all)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for deepened backend registry: health tracking, discovery, pool,
//! selector strategies, and extended metrics.

use abp_core::Capability;
use abp_integrations::discovery::{BackendDiscovery, DiscoveryError};
use abp_integrations::health::{BackendHealthTracker, HealthStatus};
use abp_integrations::metrics::{
    BackendMetrics, ExtendedBackendMetrics, LatencyTracker, MetricsRegistry,
};
use abp_integrations::pool::{BackendPool, PoolConfig, PoolError};
use abp_integrations::selector::{
    BackendCandidate, BackendHealth, BackendSelector, SelectionStrategy,
};
use std::collections::BTreeMap;

// ===== helpers ==============================================================

fn cand(name: &str, caps: &[Capability], priority: u32) -> BackendCandidate {
    BackendCandidate {
        name: name.to_string(),
        capabilities: caps.to_vec(),
        priority,
        enabled: true,
        metadata: BTreeMap::new(),
    }
}

// ===== BackendHealthTracker =================================================

#[test]
fn tracker_empty_has_no_backends() {
    let t = BackendHealthTracker::new();
    assert_eq!(t.count(), 0);
    assert!(t.tracked_backends().is_empty());
}

#[test]
fn tracker_record_healthy_creates_entry() {
    let mut t = BackendHealthTracker::new();
    t.record_healthy("mock", Some(42));
    assert_eq!(t.count(), 1);
    assert!(t.is_healthy("mock"));
    let h = t.get("mock").unwrap();
    assert_eq!(h.consecutive_successes, 1);
    assert_eq!(h.consecutive_failures, 0);
    assert_eq!(h.last_response_time_ms, Some(42));
}

#[test]
fn tracker_record_degraded() {
    let mut t = BackendHealthTracker::new();
    t.record_degraded("slow", "high latency", Some(999));
    assert!(!t.is_healthy("slow"));
    let h = t.get("slow").unwrap();
    assert_eq!(h.consecutive_failures, 1);
    assert!(matches!(h.status, HealthStatus::Degraded { .. }));
}

#[test]
fn tracker_record_unhealthy() {
    let mut t = BackendHealthTracker::new();
    t.record_unhealthy("dead", "timeout");
    assert!(!t.is_healthy("dead"));
    let h = t.get("dead").unwrap();
    assert_eq!(h.consecutive_failures, 1);
    assert!(matches!(h.status, HealthStatus::Unhealthy { .. }));
}

#[test]
fn tracker_consecutive_failures_accumulate() {
    let mut t = BackendHealthTracker::new();
    t.record_unhealthy("flaky", "err1");
    t.record_unhealthy("flaky", "err2");
    t.record_unhealthy("flaky", "err3");
    assert_eq!(t.get("flaky").unwrap().consecutive_failures, 3);
    assert_eq!(t.get("flaky").unwrap().total_checks, 3);
}

#[test]
fn tracker_healthy_resets_failure_count() {
    let mut t = BackendHealthTracker::new();
    t.record_unhealthy("backend", "err");
    t.record_unhealthy("backend", "err");
    t.record_healthy("backend", None);
    let h = t.get("backend").unwrap();
    assert_eq!(h.consecutive_failures, 0);
    assert_eq!(h.consecutive_successes, 1);
    assert_eq!(h.total_checks, 3);
}

#[test]
fn tracker_unhealthy_backends_list() {
    let mut t = BackendHealthTracker::new();
    t.record_healthy("ok", None);
    t.record_unhealthy("bad1", "down");
    t.record_degraded("bad2", "slow", None);
    let bad = t.unhealthy_backends();
    assert_eq!(bad.len(), 2);
}

#[test]
fn tracker_remove_backend() {
    let mut t = BackendHealthTracker::new();
    t.record_healthy("a", None);
    assert!(t.remove("a"));
    assert_eq!(t.count(), 0);
    assert!(!t.remove("a"));
}

#[test]
fn tracker_clear() {
    let mut t = BackendHealthTracker::new();
    t.record_healthy("a", None);
    t.record_healthy("b", None);
    t.clear();
    assert_eq!(t.count(), 0);
}

#[test]
fn tracker_last_check_is_populated() {
    let mut t = BackendHealthTracker::new();
    t.record_healthy("ts", Some(10));
    let h = t.get("ts").unwrap();
    assert!(!h.last_check.is_empty());
    assert!(h.last_check.contains('T'));
}

// ===== BackendDiscovery =====================================================

#[test]
fn discovery_empty() {
    let d = BackendDiscovery::new();
    assert!(d.is_empty());
    assert_eq!(d.count(), 0);
    assert_eq!(d.active_count(), 0);
}

#[test]
fn discovery_register_and_get() {
    let mut d = BackendDiscovery::new();
    d.register("mock", Some("mock backend"), BTreeMap::new())
        .unwrap();
    assert_eq!(d.count(), 1);
    assert!(d.contains("mock"));
    let reg = d.get("mock").unwrap();
    assert_eq!(reg.description.as_deref(), Some("mock backend"));
    assert!(reg.active);
}

#[test]
fn discovery_duplicate_register_fails() {
    let mut d = BackendDiscovery::new();
    d.register("x", None, BTreeMap::new()).unwrap();
    let err = d.register("x", None, BTreeMap::new()).unwrap_err();
    assert_eq!(err, DiscoveryError::AlreadyRegistered { name: "x".into() });
}

#[test]
fn discovery_unregister() {
    let mut d = BackendDiscovery::new();
    d.register("a", None, BTreeMap::new()).unwrap();
    let removed = d.unregister("a").unwrap();
    assert_eq!(removed.name, "a");
    assert!(d.is_empty());
}

#[test]
fn discovery_unregister_missing_fails() {
    let mut d = BackendDiscovery::new();
    let err = d.unregister("nope").unwrap_err();
    assert_eq!(
        err,
        DiscoveryError::NotFound {
            name: "nope".into()
        }
    );
}

#[test]
fn discovery_deactivate_and_activate() {
    let mut d = BackendDiscovery::new();
    d.register("b", None, BTreeMap::new()).unwrap();
    d.deactivate("b").unwrap();
    assert_eq!(d.active_count(), 0);
    assert_eq!(d.count(), 1);
    d.activate("b").unwrap();
    assert_eq!(d.active_count(), 1);
}

#[test]
fn discovery_deactivate_missing_fails() {
    let mut d = BackendDiscovery::new();
    assert!(d.deactivate("nope").is_err());
}

#[test]
fn discovery_list_active_filters() {
    let mut d = BackendDiscovery::new();
    d.register("a", None, BTreeMap::new()).unwrap();
    d.register("b", None, BTreeMap::new()).unwrap();
    d.deactivate("a").unwrap();
    assert_eq!(d.list_active().len(), 1);
    assert_eq!(d.list_all().len(), 2);
}

#[test]
fn discovery_names() {
    let mut d = BackendDiscovery::new();
    d.register("z", None, BTreeMap::new()).unwrap();
    d.register("a", None, BTreeMap::new()).unwrap();
    let names = d.names();
    assert_eq!(names, vec!["a", "z"]); // BTreeMap is sorted
}

#[test]
fn discovery_clear() {
    let mut d = BackendDiscovery::new();
    d.register("a", None, BTreeMap::new()).unwrap();
    d.register("b", None, BTreeMap::new()).unwrap();
    d.clear();
    assert!(d.is_empty());
}

#[test]
fn discovery_metadata() {
    let mut d = BackendDiscovery::new();
    let meta = BTreeMap::from([("region".to_string(), "us-east-1".to_string())]);
    d.register("cloud", None, meta).unwrap();
    let reg = d.get("cloud").unwrap();
    assert_eq!(reg.metadata.get("region").unwrap(), "us-east-1");
}

#[test]
fn discovery_error_display() {
    let e = DiscoveryError::AlreadyRegistered { name: "x".into() };
    assert!(format!("{e}").contains("already registered"));
    let e2 = DiscoveryError::NotFound { name: "y".into() };
    assert!(format!("{e2}").contains("not found"));
}

// ===== BackendPool ==========================================================

#[test]
fn pool_empty() {
    let p = BackendPool::new();
    assert!(p.is_empty());
    assert_eq!(p.backend_count(), 0);
}

#[test]
fn pool_register_creates_min_connections() {
    let mut p = BackendPool::new();
    p.register(
        "a",
        PoolConfig {
            min_connections: 3,
            max_connections: 10,
        },
    )
    .unwrap();
    assert_eq!(p.idle_count("a").unwrap(), 3);
    assert_eq!(p.active_count("a").unwrap(), 0);
    assert_eq!(p.total_count("a").unwrap(), 3);
}

#[test]
fn pool_checkout_returns_idle() {
    let mut p = BackendPool::new();
    p.register("b", PoolConfig::default()).unwrap();
    let id = p.checkout("b").unwrap();
    assert_eq!(p.active_count("b").unwrap(), 1);
    assert_eq!(p.idle_count("b").unwrap(), 0);
    p.checkin("b", id).unwrap();
    assert_eq!(p.active_count("b").unwrap(), 0);
    assert_eq!(p.idle_count("b").unwrap(), 1);
}

#[test]
fn pool_checkout_grows_to_max() {
    let mut p = BackendPool::new();
    p.register(
        "c",
        PoolConfig {
            min_connections: 0,
            max_connections: 3,
        },
    )
    .unwrap();
    let _id1 = p.checkout("c").unwrap();
    let _id2 = p.checkout("c").unwrap();
    let _id3 = p.checkout("c").unwrap();
    assert_eq!(p.total_count("c").unwrap(), 3);
    let err = p.checkout("c").unwrap_err();
    assert_eq!(err, PoolError::Exhausted { name: "c".into() });
}

#[test]
fn pool_checkout_missing_backend() {
    let mut p = BackendPool::new();
    let err = p.checkout("nope").unwrap_err();
    assert_eq!(
        err,
        PoolError::BackendNotFound {
            name: "nope".into()
        }
    );
}

#[test]
fn pool_checkin_unknown_connection() {
    let mut p = BackendPool::new();
    p.register("d", PoolConfig::default()).unwrap();
    let err = p.checkin("d", 999).unwrap_err();
    assert!(matches!(err, PoolError::ConnectionNotFound { .. }));
}

#[test]
fn pool_duplicate_register() {
    let mut p = BackendPool::new();
    p.register("e", PoolConfig::default()).unwrap();
    let err = p.register("e", PoolConfig::default()).unwrap_err();
    assert_eq!(err, PoolError::AlreadyRegistered { name: "e".into() });
}

#[test]
fn pool_unregister() {
    let mut p = BackendPool::new();
    p.register("f", PoolConfig::default()).unwrap();
    p.unregister("f").unwrap();
    assert!(p.is_empty());
}

#[test]
fn pool_unregister_missing() {
    let mut p = BackendPool::new();
    assert!(p.unregister("nope").is_err());
}

#[test]
fn pool_status() {
    let mut p = BackendPool::new();
    p.register(
        "g",
        PoolConfig {
            min_connections: 2,
            max_connections: 5,
        },
    )
    .unwrap();
    let _id = p.checkout("g").unwrap();
    let s = p.status("g").unwrap();
    assert_eq!(s.active, 1);
    assert_eq!(s.idle, 1);
    assert_eq!(s.total, 2);
    assert_eq!(s.config.max_connections, 5);
}

#[test]
fn pool_status_all() {
    let mut p = BackendPool::new();
    p.register("x", PoolConfig::default()).unwrap();
    p.register("y", PoolConfig::default()).unwrap();
    let all = p.status_all();
    assert_eq!(all.len(), 2);
}

#[test]
fn pool_backend_names() {
    let mut p = BackendPool::new();
    p.register("z", PoolConfig::default()).unwrap();
    p.register("a", PoolConfig::default()).unwrap();
    let names = p.backend_names();
    assert_eq!(names, vec!["a", "z"]); // BTreeMap sorted
}

#[test]
fn pool_error_display() {
    let e = PoolError::Exhausted { name: "x".into() };
    assert!(format!("{e}").contains("exhausted"));
}

// ===== Selector — new strategies ============================================

#[test]
fn weighted_picks_highest_priority() {
    let mut sel = BackendSelector::new(SelectionStrategy::Weighted);
    sel.add_candidate(cand("low", &[Capability::Streaming], 1));
    sel.add_candidate(cand("high", &[Capability::Streaming], 100));
    let picked = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(picked.name, "high");
}

#[test]
fn weighted_respects_capabilities() {
    let mut sel = BackendSelector::new(SelectionStrategy::Weighted);
    sel.add_candidate(cand("a", &[Capability::ToolRead], 100));
    sel.add_candidate(cand("b", &[Capability::Streaming], 1));
    let picked = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(picked.name, "b");
}

#[test]
fn least_latency_picks_lowest() {
    let mut sel = BackendSelector::new(SelectionStrategy::LeastLatency);
    sel.add_candidate(cand("slow", &[Capability::Streaming], 1));
    sel.add_candidate(cand("fast", &[Capability::Streaming], 1));
    sel.set_latency("slow", 500);
    sel.set_latency("fast", 10);
    let picked = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(picked.name, "fast");
}

#[test]
fn least_latency_unknown_treated_as_max() {
    let mut sel = BackendSelector::new(SelectionStrategy::LeastLatency);
    sel.add_candidate(cand("known", &[Capability::Streaming], 1));
    sel.add_candidate(cand("unknown", &[Capability::Streaming], 1));
    sel.set_latency("known", 100);
    // "unknown" has no latency set → treated as u64::MAX
    let picked = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(picked.name, "known");
}

#[test]
fn explicit_selects_named_backend() {
    let mut sel = BackendSelector::new(SelectionStrategy::Explicit {
        backend: "target".into(),
    });
    sel.add_candidate(cand("other", &[Capability::Streaming], 1));
    sel.add_candidate(cand("target", &[Capability::Streaming], 2));
    let picked = sel.select(&[Capability::Streaming]).unwrap();
    assert_eq!(picked.name, "target");
}

#[test]
fn explicit_returns_none_when_missing() {
    let mut sel = BackendSelector::new(SelectionStrategy::Explicit {
        backend: "gone".into(),
    });
    sel.add_candidate(cand("other", &[Capability::Streaming], 1));
    assert!(sel.select(&[Capability::Streaming]).is_none());
}

#[test]
fn explicit_returns_none_when_incapable() {
    let mut sel = BackendSelector::new(SelectionStrategy::Explicit {
        backend: "target".into(),
    });
    sel.add_candidate(cand("target", &[Capability::ToolRead], 1));
    assert!(sel.select(&[Capability::Streaming]).is_none());
}

#[test]
fn selector_set_and_get_latency() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.set_latency("a", 42);
    assert_eq!(sel.get_latency("a"), Some(42));
    assert_eq!(sel.get_latency("b"), None);
}

#[test]
fn selector_remove_candidate() {
    let mut sel = BackendSelector::new(SelectionStrategy::FirstMatch);
    sel.add_candidate(cand("a", &[Capability::Streaming], 1));
    sel.add_candidate(cand("b", &[Capability::Streaming], 2));
    sel.set_health("a", BackendHealth::Up);
    sel.set_latency("a", 10);
    assert!(sel.remove_candidate("a"));
    assert_eq!(sel.candidate_count(), 1);
    assert!(sel.get_health("a").is_none());
    assert_eq!(sel.get_latency("a"), None);
    assert!(!sel.remove_candidate("a")); // already gone
}

#[test]
fn selector_strategy_serde_weighted() {
    let s = SelectionStrategy::Weighted;
    let json = serde_json::to_string(&s).unwrap();
    let back: SelectionStrategy = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, SelectionStrategy::Weighted));
}

#[test]
fn selector_strategy_serde_least_latency() {
    let s = SelectionStrategy::LeastLatency;
    let json = serde_json::to_string(&s).unwrap();
    let back: SelectionStrategy = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, SelectionStrategy::LeastLatency));
}

#[test]
fn selector_strategy_serde_explicit() {
    let s = SelectionStrategy::Explicit {
        backend: "mock".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: SelectionStrategy = serde_json::from_str(&json).unwrap();
    match back {
        SelectionStrategy::Explicit { backend } => assert_eq!(backend, "mock"),
        _ => panic!("wrong variant"),
    }
}

// ===== LatencyTracker =======================================================

#[test]
fn latency_tracker_empty() {
    let t = LatencyTracker::new();
    assert_eq!(t.count(), 0);
    assert_eq!(t.min(), 0);
    assert_eq!(t.max(), 0);
    assert_eq!(t.mean(), 0.0);
    assert_eq!(t.p50(), 0);
    assert_eq!(t.p99(), 0);
}

#[test]
fn latency_tracker_single_sample() {
    let mut t = LatencyTracker::new();
    t.record(100);
    assert_eq!(t.count(), 1);
    assert_eq!(t.min(), 100);
    assert_eq!(t.max(), 100);
    assert_eq!(t.mean(), 100.0);
    assert_eq!(t.p50(), 100);
    assert_eq!(t.p99(), 100);
}

#[test]
fn latency_tracker_percentiles() {
    let mut t = LatencyTracker::new();
    for i in 1..=100 {
        t.record(i);
    }
    assert_eq!(t.min(), 1);
    assert_eq!(t.max(), 100);
    assert!(t.p50() >= 49 && t.p50() <= 51);
    assert!(t.p90() >= 89 && t.p90() <= 91);
    assert!(t.p95() >= 94 && t.p95() <= 96);
    assert!(t.p99() >= 98 && t.p99() <= 100);
}

#[test]
fn latency_tracker_reset() {
    let mut t = LatencyTracker::new();
    t.record(10);
    t.record(20);
    t.reset();
    assert_eq!(t.count(), 0);
    assert_eq!(t.mean(), 0.0);
}

#[test]
fn latency_tracker_snapshot() {
    let mut t = LatencyTracker::new();
    t.record(10);
    t.record(20);
    t.record(30);
    let snap = t.snapshot();
    assert_eq!(snap.count, 3);
    assert_eq!(snap.min_ms, 10);
    assert_eq!(snap.max_ms, 30);
    assert!((snap.mean_ms - 20.0).abs() < 1e-10);
}

#[test]
fn latency_snapshot_serde_roundtrip() {
    let mut t = LatencyTracker::new();
    t.record(50);
    let snap = t.snapshot();
    let json = serde_json::to_string(&snap).unwrap();
    let back: abp_integrations::metrics::LatencySnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(back.count, snap.count);
    assert_eq!(back.p50_ms, snap.p50_ms);
}

// ===== ExtendedBackendMetrics ===============================================

#[test]
fn extended_metrics_new_is_zero() {
    let m = ExtendedBackendMetrics::new();
    assert_eq!(m.core.total_runs(), 0);
    assert!(m.error_counts().is_empty());
}

#[test]
fn extended_metrics_record_success() {
    let m = ExtendedBackendMetrics::new();
    m.record_success(5, 100);
    assert_eq!(m.core.total_runs(), 1);
    assert_eq!(m.core.success_rate(), 1.0);
    let lat = m.latency.lock().unwrap();
    assert_eq!(lat.count(), 1);
    assert_eq!(lat.mean(), 100.0);
}

#[test]
fn extended_metrics_record_failure() {
    let m = ExtendedBackendMetrics::new();
    m.record_failure(2, 50, "timeout");
    assert_eq!(m.core.total_runs(), 1);
    assert_eq!(m.core.success_rate(), 0.0);
    let errs = m.error_counts();
    assert_eq!(errs.get("timeout"), Some(&1));
}

#[test]
fn extended_metrics_multiple_error_kinds() {
    let m = ExtendedBackendMetrics::new();
    m.record_failure(1, 10, "timeout");
    m.record_failure(1, 20, "timeout");
    m.record_failure(1, 30, "rate_limit");
    let errs = m.error_counts();
    assert_eq!(errs.get("timeout"), Some(&2));
    assert_eq!(errs.get("rate_limit"), Some(&1));
}

#[test]
fn extended_metrics_snapshot() {
    let m = ExtendedBackendMetrics::new();
    m.record_success(5, 100);
    m.record_failure(2, 200, "err");
    let snap = m.snapshot();
    assert_eq!(snap.core.total_runs, 2);
    assert_eq!(snap.latency.count, 2);
    assert_eq!(snap.error_counts.get("err"), Some(&1));
}

#[test]
fn extended_metrics_reset() {
    let m = ExtendedBackendMetrics::new();
    m.record_success(5, 100);
    m.record_failure(2, 200, "err");
    m.reset();
    assert_eq!(m.core.total_runs(), 0);
    assert!(m.error_counts().is_empty());
    assert_eq!(m.latency.lock().unwrap().count(), 0);
}

#[test]
fn extended_metrics_snapshot_serde() {
    let m = ExtendedBackendMetrics::new();
    m.record_success(1, 50);
    let snap = m.snapshot();
    let json = serde_json::to_string(&snap).unwrap();
    let back: abp_integrations::metrics::ExtendedMetricsSnapshot =
        serde_json::from_str(&json).unwrap();
    assert_eq!(back.core.total_runs, 1);
}
