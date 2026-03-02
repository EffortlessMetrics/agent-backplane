// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for [`RunMetrics`] — concurrency, edge cases, and invariants.

use abp_runtime::telemetry::RunMetrics;
use std::sync::Arc;

// ── 1. Concurrent metric updates from 10 threads ───────────────────

#[test]
fn concurrent_updates_from_ten_threads() {
    let m = Arc::new(RunMetrics::new());
    let mut handles = Vec::new();
    for i in 0..10 {
        let m = Arc::clone(&m);
        handles.push(std::thread::spawn(move || {
            for j in 0..100 {
                m.record_run(10, (i + j) % 2 == 0, 1);
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    let s = m.snapshot();
    assert_eq!(s.total_runs, 1000);
    assert_eq!(s.successful_runs + s.failed_runs, 1000);
    assert_eq!(s.total_events, 1000);
    // All durations are 10, so average must be 10.
    assert_eq!(s.average_run_duration_ms, 10);
}

// ── 2. Snapshot consistency under load ──────────────────────────────

#[test]
fn snapshot_consistency_under_load() {
    let m = Arc::new(RunMetrics::new());
    let writer = {
        let m = Arc::clone(&m);
        std::thread::spawn(move || {
            for _ in 0..500 {
                m.record_run(20, true, 2);
            }
        })
    };
    // Take snapshots concurrently while writes happen.
    let mut snapshots = Vec::new();
    for _ in 0..50 {
        let s = m.snapshot();
        // Invariant: success + failure == total_runs
        assert_eq!(s.successful_runs + s.failed_runs, s.total_runs);
        snapshots.push(s);
    }
    writer.join().unwrap();

    // Snapshots must be monotonically non-decreasing in total_runs.
    for w in snapshots.windows(2) {
        assert!(w[1].total_runs >= w[0].total_runs);
    }

    let final_snap = m.snapshot();
    assert_eq!(final_snap.total_runs, 500);
    assert_eq!(final_snap.successful_runs, 500);
}

// ── 3. Duration tracking accuracy ───────────────────────────────────

#[test]
fn average_duration_computed_correctly() {
    let m = RunMetrics::new();
    m.record_run(100, true, 1);
    m.record_run(200, true, 1);
    m.record_run(300, true, 1);
    // Average: (100 + 200 + 300) / 3 = 200
    let s = m.snapshot();
    assert_eq!(s.average_run_duration_ms, 200);
}

#[test]
fn average_duration_integer_truncation() {
    let m = RunMetrics::new();
    m.record_run(10, true, 0);
    m.record_run(11, true, 0);
    // Average: 21 / 2 = 10 (integer division)
    let s = m.snapshot();
    assert_eq!(s.average_run_duration_ms, 10);
}

// ── 4. Large event counts (approaching u64 limits) ──────────────────

#[test]
fn large_event_counts_accumulate() {
    let m = RunMetrics::new();
    let large_count = u64::MAX / 2;
    m.record_run(1, true, large_count);
    m.record_run(1, true, 1);
    let s = m.snapshot();
    assert_eq!(s.total_events, large_count + 1);
}

// ── 5. Default trait gives zeroed metrics ───────────────────────────

#[test]
fn default_gives_zeroed_metrics() {
    let m = RunMetrics::default();
    let s = m.snapshot();
    assert_eq!(s.total_runs, 0);
    assert_eq!(s.successful_runs, 0);
    assert_eq!(s.failed_runs, 0);
    assert_eq!(s.total_events, 0);
    assert_eq!(s.average_run_duration_ms, 0);
}

// ── 6. Mixed success/failure runs ───────────────────────────────────

#[test]
fn mixed_success_failure_runs() {
    let m = RunMetrics::new();
    // 7 successes, 3 failures
    for i in 0..10 {
        m.record_run(50, i < 7, 3);
    }
    let s = m.snapshot();
    assert_eq!(s.total_runs, 10);
    assert_eq!(s.successful_runs, 7);
    assert_eq!(s.failed_runs, 3);
    assert_eq!(s.total_events, 30);
    assert_eq!(s.average_run_duration_ms, 50);
}

// ── 7. Zero-event runs ──────────────────────────────────────────────

#[test]
fn zero_event_runs() {
    let m = RunMetrics::new();
    m.record_run(10, true, 0);
    m.record_run(20, false, 0);
    let s = m.snapshot();
    assert_eq!(s.total_runs, 2);
    assert_eq!(s.total_events, 0);
}

// ── 8. Rapid sequential runs ────────────────────────────────────────

#[test]
fn rapid_sequential_runs() {
    let m = RunMetrics::new();
    for i in 0..1000u64 {
        m.record_run(i, true, 1);
    }
    let s = m.snapshot();
    assert_eq!(s.total_runs, 1000);
    assert_eq!(s.successful_runs, 1000);
    assert_eq!(s.total_events, 1000);
    // Sum 0..999 = 499500, average = 499500 / 1000 = 499
    assert_eq!(s.average_run_duration_ms, 499);
}

// ── 9. Zero-duration runs ───────────────────────────────────────────

#[test]
fn zero_duration_runs() {
    let m = RunMetrics::new();
    m.record_run(0, true, 5);
    m.record_run(0, false, 5);
    let s = m.snapshot();
    assert_eq!(s.total_runs, 2);
    assert_eq!(s.average_run_duration_ms, 0);
    assert_eq!(s.total_events, 10);
}

// ── 10. Snapshot serializable to JSON ───────────────────────────────

#[test]
fn snapshot_serializes_to_json() {
    let m = RunMetrics::new();
    m.record_run(42, true, 7);
    let s = m.snapshot();
    let json = serde_json::to_string(&s).expect("serialize snapshot");
    assert!(json.contains("\"total_runs\":1"));
    assert!(json.contains("\"total_events\":7"));
    assert!(json.contains("\"average_run_duration_ms\":42"));
}

// ── 11. All-failures run ────────────────────────────────────────────

#[test]
fn all_failures() {
    let m = RunMetrics::new();
    for _ in 0..5 {
        m.record_run(100, false, 2);
    }
    let s = m.snapshot();
    assert_eq!(s.total_runs, 5);
    assert_eq!(s.successful_runs, 0);
    assert_eq!(s.failed_runs, 5);
    assert_eq!(s.total_events, 10);
}

// ── 12. Concurrent readers and writers ──────────────────────────────

#[test]
fn concurrent_readers_and_writers() {
    let m = Arc::new(RunMetrics::new());

    let mut handles = Vec::new();
    // 5 writers
    for _ in 0..5 {
        let m = Arc::clone(&m);
        handles.push(std::thread::spawn(move || {
            for _ in 0..200 {
                m.record_run(10, true, 1);
            }
        }));
    }
    // 5 readers
    for _ in 0..5 {
        let m = Arc::clone(&m);
        handles.push(std::thread::spawn(move || {
            for _ in 0..200 {
                let s = m.snapshot();
                // During concurrent writes, individual atomics may be read between updates.
                // We can only assert each counter is non-negative and within expected range.
                assert!(s.total_runs <= 1000);
                assert!(s.successful_runs <= 1000);
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    let s = m.snapshot();
    assert_eq!(s.total_runs, 1000);
}
