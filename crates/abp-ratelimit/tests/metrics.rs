//! Integration tests for `RateLimitMetrics`.

use std::sync::Arc;
use std::time::Duration;

use abp_ratelimit::{MetricsSnapshot, RateLimitMetrics};

// ---------------------------------------------------------------------------
// Basic recording
// ---------------------------------------------------------------------------

#[test]
fn fresh_snapshot_is_zeroed() {
    let m = RateLimitMetrics::new(Duration::from_secs(10));
    let s = m.snapshot();
    assert_eq!(s.total_requests, 0);
    assert_eq!(s.total_throttled, 0);
    assert_eq!(s.queue_depth, 0);
    assert_eq!(s.avg_wait, Duration::ZERO);
    assert_eq!(s.max_wait, Duration::ZERO);
    assert!(s.requests_per_sec.abs() < f64::EPSILON);
}

#[test]
fn request_and_throttle_counts() {
    let m = RateLimitMetrics::new(Duration::from_secs(10));
    for _ in 0..7 {
        m.record_request();
    }
    for _ in 0..3 {
        m.record_throttle();
    }
    let s = m.snapshot();
    assert_eq!(s.total_requests, 7);
    assert_eq!(s.total_throttled, 3);
}

// ---------------------------------------------------------------------------
// Queue depth
// ---------------------------------------------------------------------------

#[test]
fn queue_depth_inc_dec() {
    let m = RateLimitMetrics::new(Duration::from_secs(10));
    m.inc_queue_depth();
    m.inc_queue_depth();
    m.inc_queue_depth();
    assert_eq!(m.snapshot().queue_depth, 3);
    m.dec_queue_depth();
    assert_eq!(m.snapshot().queue_depth, 2);
}

#[test]
fn queue_depth_saturates_at_zero() {
    let m = RateLimitMetrics::new(Duration::from_secs(10));
    m.dec_queue_depth();
    m.dec_queue_depth();
    assert_eq!(m.snapshot().queue_depth, 0);
}

// ---------------------------------------------------------------------------
// Wait time stats
// ---------------------------------------------------------------------------

#[test]
fn avg_and_max_wait() {
    let m = RateLimitMetrics::new(Duration::from_secs(10));
    m.record_wait(Duration::from_millis(10));
    m.record_wait(Duration::from_millis(30));
    m.record_wait(Duration::from_millis(50));
    let s = m.snapshot();
    // avg = (10+30+50)/3 = 30
    assert_eq!(s.avg_wait, Duration::from_millis(30));
    assert_eq!(s.max_wait, Duration::from_millis(50));
}

#[test]
fn max_wait_tracks_peak() {
    let m = RateLimitMetrics::new(Duration::from_secs(10));
    m.record_wait(Duration::from_millis(500));
    m.record_wait(Duration::from_millis(100));
    m.record_wait(Duration::from_millis(200));
    assert_eq!(m.snapshot().max_wait, Duration::from_millis(500));
}

// ---------------------------------------------------------------------------
// Requests per second
// ---------------------------------------------------------------------------

#[test]
fn rps_over_window() {
    let m = RateLimitMetrics::new(Duration::from_secs(5));
    for _ in 0..10 {
        m.record_request();
    }
    let rps = m.snapshot().requests_per_sec;
    // 10 events in a 5-second window → ~2.0 rps
    assert!(rps > 1.5 && rps < 2.5, "rps was {rps}");
}

// ---------------------------------------------------------------------------
// Reset
// ---------------------------------------------------------------------------

#[test]
fn reset_clears_everything() {
    let m = RateLimitMetrics::new(Duration::from_secs(10));
    m.record_request();
    m.record_throttle();
    m.inc_queue_depth();
    m.record_wait(Duration::from_millis(100));
    m.reset();
    let s = m.snapshot();
    assert_eq!(s.total_requests, 0);
    assert_eq!(s.total_throttled, 0);
    assert_eq!(s.queue_depth, 0);
    assert_eq!(s.max_wait, Duration::ZERO);
}

// ---------------------------------------------------------------------------
// Default
// ---------------------------------------------------------------------------

#[test]
fn default_metrics_work() {
    let m = RateLimitMetrics::default();
    m.record_request();
    assert_eq!(m.snapshot().total_requests, 1);
}

// ---------------------------------------------------------------------------
// Clone shares state
// ---------------------------------------------------------------------------

#[test]
fn clone_shares_state() {
    let m = RateLimitMetrics::new(Duration::from_secs(10));
    let c = m.clone();
    m.record_request();
    m.record_request();
    assert_eq!(c.snapshot().total_requests, 2);
}

// ---------------------------------------------------------------------------
// MetricsSnapshot PartialEq
// ---------------------------------------------------------------------------

#[test]
fn snapshot_equality() {
    let a = MetricsSnapshot {
        total_requests: 1,
        total_throttled: 2,
        requests_per_sec: 3.0,
        queue_depth: 4,
        avg_wait: Duration::from_millis(5),
        max_wait: Duration::from_millis(6),
    };
    let b = a.clone();
    assert_eq!(a, b);
}

// ---------------------------------------------------------------------------
// Concurrent access
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_recording() {
    let m = Arc::new(RateLimitMetrics::new(Duration::from_secs(10)));
    let mut handles = Vec::new();
    for _ in 0..10 {
        let mc = Arc::clone(&m);
        handles.push(tokio::spawn(async move {
            for _ in 0..100 {
                mc.record_request();
            }
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(m.snapshot().total_requests, 1000);
}
