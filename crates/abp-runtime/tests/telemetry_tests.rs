// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for abp_runtime::telemetry.

use abp_runtime::telemetry::RunMetrics;
use std::sync::Arc;

#[test]
fn new_metrics_start_at_zero() {
    let m = RunMetrics::new();
    let s = m.snapshot();
    assert_eq!(s.total_runs, 0);
    assert_eq!(s.successful_runs, 0);
    assert_eq!(s.failed_runs, 0);
    assert_eq!(s.total_events, 0);
    assert_eq!(s.average_run_duration_ms, 0);
}

#[test]
fn record_success_increments_correctly() {
    let m = RunMetrics::new();
    m.record_run(100, true, 5);
    let s = m.snapshot();
    assert_eq!(s.total_runs, 1);
    assert_eq!(s.successful_runs, 1);
    assert_eq!(s.failed_runs, 0);
    assert_eq!(s.total_events, 5);
    assert_eq!(s.average_run_duration_ms, 100);
}

#[test]
fn record_failure_increments_correctly() {
    let m = RunMetrics::new();
    m.record_run(200, false, 3);
    let s = m.snapshot();
    assert_eq!(s.total_runs, 1);
    assert_eq!(s.successful_runs, 0);
    assert_eq!(s.failed_runs, 1);
    assert_eq!(s.total_events, 3);
    assert_eq!(s.average_run_duration_ms, 200);
}

#[test]
fn multiple_records_accumulate() {
    let m = RunMetrics::new();
    m.record_run(100, true, 2);
    m.record_run(200, false, 3);
    m.record_run(300, true, 5);
    let s = m.snapshot();
    assert_eq!(s.total_runs, 3);
    assert_eq!(s.successful_runs, 2);
    assert_eq!(s.failed_runs, 1);
    assert_eq!(s.total_events, 10);
    assert_eq!(s.average_run_duration_ms, 200); // (100+200+300)/3
}

#[test]
fn snapshot_returns_consistent_state() {
    let m = RunMetrics::new();
    m.record_run(50, true, 1);
    let s1 = m.snapshot();
    let s2 = m.snapshot();
    assert_eq!(s1.total_runs, s2.total_runs);
    assert_eq!(s1.successful_runs, s2.successful_runs);
    assert_eq!(s1.failed_runs, s2.failed_runs);
    assert_eq!(s1.total_events, s2.total_events);
    assert_eq!(s1.average_run_duration_ms, s2.average_run_duration_ms);
}

#[test]
fn thread_safety_concurrent_records() {
    let m = Arc::new(RunMetrics::new());
    let mut handles = Vec::new();
    for i in 0..10 {
        let m = Arc::clone(&m);
        handles.push(std::thread::spawn(move || {
            m.record_run(100, i % 2 == 0, 1);
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    let s = m.snapshot();
    assert_eq!(s.total_runs, 10);
    assert_eq!(s.successful_runs, 5);
    assert_eq!(s.failed_runs, 5);
    assert_eq!(s.total_events, 10);
    assert_eq!(s.average_run_duration_ms, 100);
}
