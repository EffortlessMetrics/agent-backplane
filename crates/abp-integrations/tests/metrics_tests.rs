// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for backend metrics collection.

use abp_integrations::metrics::{BackendMetrics, MetricsRegistry, MetricsSnapshot};
use std::sync::Arc;

#[test]
fn new_metrics_are_zero() {
    let m = BackendMetrics::new();
    assert_eq!(m.total_runs(), 0);
    assert_eq!(m.success_rate(), 0.0);
    assert_eq!(m.average_duration_ms(), 0.0);
    assert_eq!(m.average_events_per_run(), 0.0);
}

#[test]
fn record_single_success() {
    let m = BackendMetrics::new();
    m.record_run(true, 5, 100);
    assert_eq!(m.total_runs(), 1);
    assert_eq!(m.success_rate(), 1.0);
    assert_eq!(m.average_duration_ms(), 100.0);
    assert_eq!(m.average_events_per_run(), 5.0);
}

#[test]
fn record_single_failure() {
    let m = BackendMetrics::new();
    m.record_run(false, 2, 50);
    assert_eq!(m.total_runs(), 1);
    assert_eq!(m.success_rate(), 0.0);
    assert_eq!(m.average_duration_ms(), 50.0);
    assert_eq!(m.average_events_per_run(), 2.0);
}

#[test]
fn mixed_success_and_failure() {
    let m = BackendMetrics::new();
    m.record_run(true, 10, 200);
    m.record_run(false, 4, 100);
    m.record_run(true, 6, 300);
    assert_eq!(m.total_runs(), 3);
    assert!((m.success_rate() - 2.0 / 3.0).abs() < 1e-10);
    assert!((m.average_duration_ms() - 200.0).abs() < 1e-10);
    assert!((m.average_events_per_run() - 20.0 / 3.0).abs() < 1e-10);
}

#[test]
fn reset_clears_all_counters() {
    let m = BackendMetrics::new();
    m.record_run(true, 10, 200);
    m.record_run(false, 5, 100);
    m.reset();
    assert_eq!(m.total_runs(), 0);
    assert_eq!(m.success_rate(), 0.0);
    assert_eq!(m.average_duration_ms(), 0.0);
    assert_eq!(m.average_events_per_run(), 0.0);
}

#[test]
fn snapshot_captures_current_state() {
    let m = BackendMetrics::new();
    m.record_run(true, 8, 400);
    m.record_run(false, 2, 200);
    let snap = m.snapshot();
    assert_eq!(snap.total_runs, 2);
    assert_eq!(snap.successful_runs, 1);
    assert_eq!(snap.failed_runs, 1);
    assert_eq!(snap.total_events, 10);
    assert_eq!(snap.total_duration_ms, 600);
    assert!((snap.success_rate - 0.5).abs() < 1e-10);
    assert!((snap.average_duration_ms - 300.0).abs() < 1e-10);
    assert!((snap.average_events_per_run - 5.0).abs() < 1e-10);
}

#[test]
fn snapshot_is_serializable() {
    let m = BackendMetrics::new();
    m.record_run(true, 3, 150);
    let snap = m.snapshot();
    let json = serde_json::to_string(&snap).expect("serialize");
    let deser: MetricsSnapshot = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deser.total_runs, snap.total_runs);
    assert_eq!(deser.successful_runs, snap.successful_runs);
    assert!((deser.success_rate - snap.success_rate).abs() < 1e-10);
}

#[test]
fn registry_get_or_create_returns_same_instance() {
    let reg = MetricsRegistry::new();
    let a = reg.get_or_create("mock");
    let b = reg.get_or_create("mock");
    // Same Arc allocation
    assert!(Arc::ptr_eq(&a, &b));
}

#[test]
fn registry_different_backends_are_independent() {
    let reg = MetricsRegistry::new();
    let mock = reg.get_or_create("mock");
    let sidecar = reg.get_or_create("sidecar");
    mock.record_run(true, 5, 100);
    assert_eq!(mock.total_runs(), 1);
    assert_eq!(sidecar.total_runs(), 0);
}

#[test]
fn registry_snapshot_all() {
    let reg = MetricsRegistry::new();
    reg.get_or_create("alpha").record_run(true, 1, 10);
    reg.get_or_create("beta").record_run(false, 2, 20);
    let snaps = reg.snapshot_all();
    assert_eq!(snaps.len(), 2);
    assert!(snaps.contains_key("alpha"));
    assert!(snaps.contains_key("beta"));
    assert_eq!(snaps["alpha"].successful_runs, 1);
    assert_eq!(snaps["beta"].failed_runs, 1);
}

#[test]
fn registry_snapshot_all_empty() {
    let reg = MetricsRegistry::new();
    let snaps = reg.snapshot_all();
    assert!(snaps.is_empty());
}

#[test]
fn concurrent_record_runs() {
    let m = Arc::new(BackendMetrics::new());
    let mut handles = Vec::new();
    for _ in 0..10 {
        let metrics = Arc::clone(&m);
        handles.push(std::thread::spawn(move || {
            for _ in 0..100 {
                metrics.record_run(true, 1, 10);
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(m.total_runs(), 1000);
    assert_eq!(m.success_rate(), 1.0);
}

#[test]
fn debug_impls_do_not_panic() {
    let m = BackendMetrics::new();
    m.record_run(true, 1, 10);
    let _ = format!("{m:?}");

    let reg = MetricsRegistry::new();
    reg.get_or_create("test");
    let _ = format!("{reg:?}");
}
