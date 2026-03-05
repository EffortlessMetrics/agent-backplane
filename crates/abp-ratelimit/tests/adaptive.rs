//! Integration tests for `AdaptiveLimiter`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use abp_ratelimit::AdaptiveLimiter;

// ---------------------------------------------------------------------------
// Construction and accessors
// ---------------------------------------------------------------------------

#[test]
fn new_limiter_starts_at_base_rate() {
    let limiter = AdaptiveLimiter::new(50.0, 100);
    assert!((limiter.current_rate() - 50.0).abs() < f64::EPSILON);
    assert!((limiter.base_rate() - 50.0).abs() < f64::EPSILON);
}

#[test]
fn with_min_rate_builder() {
    let limiter = AdaptiveLimiter::new(100.0, 50).with_min_rate(10.0);
    // Force rate below min via errors
    for _ in 0..20 {
        limiter.record_response(Duration::from_millis(100), false);
    }
    assert!(limiter.current_rate() >= 10.0);
}

// ---------------------------------------------------------------------------
// Header-driven adaptation
// ---------------------------------------------------------------------------

#[test]
fn retry_after_blocks_then_expires() {
    let limiter = AdaptiveLimiter::new(100.0, 100);
    let mut headers = HashMap::new();
    headers.insert("retry-after".to_string(), "0".to_string());
    limiter.update_from_headers(&headers);
    // 0 seconds → already expired, should allow immediately
    assert!(limiter.try_acquire());
}

#[test]
fn remaining_and_reset_adapt_rate() {
    let limiter = AdaptiveLimiter::new(100.0, 50).with_min_rate(0.1);
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut headers = HashMap::new();
    headers.insert("x-ratelimit-remaining".to_string(), "20".to_string());
    headers.insert(
        "x-ratelimit-reset".to_string(),
        (now_unix + 10).to_string(),
    );
    limiter.update_from_headers(&headers);
    let rate = limiter.current_rate();
    // ~20 remaining / ~10s ≈ 2.0
    assert!(rate > 1.0 && rate < 4.0, "rate was {rate}");
}

#[test]
fn remaining_zero_drops_to_min_rate() {
    let limiter = AdaptiveLimiter::new(100.0, 50).with_min_rate(1.0);
    let mut headers = HashMap::new();
    headers.insert("x-ratelimit-remaining".to_string(), "0".to_string());
    limiter.update_from_headers(&headers);
    assert!((limiter.current_rate() - 1.0).abs() < f64::EPSILON);
}

#[test]
fn invalid_headers_leave_rate_unchanged() {
    let limiter = AdaptiveLimiter::new(42.0, 10);
    let mut headers = HashMap::new();
    headers.insert("retry-after".to_string(), "not-a-number".to_string());
    headers.insert("x-ratelimit-remaining".to_string(), "???".to_string());
    limiter.update_from_headers(&headers);
    assert!((limiter.current_rate() - 42.0).abs() < f64::EPSILON);
}

#[test]
fn empty_headers_no_change() {
    let limiter = AdaptiveLimiter::new(42.0, 10);
    limiter.update_from_headers(&HashMap::new());
    assert!((limiter.current_rate() - 42.0).abs() < f64::EPSILON);
}

// ---------------------------------------------------------------------------
// Error / success adaptation
// ---------------------------------------------------------------------------

#[test]
fn errors_halve_rate_repeatedly() {
    let limiter = AdaptiveLimiter::new(100.0, 50).with_min_rate(1.0);
    limiter.record_response(Duration::from_millis(100), false);
    assert!((limiter.current_rate() - 50.0).abs() < f64::EPSILON);
    limiter.record_response(Duration::from_millis(100), false);
    assert!((limiter.current_rate() - 25.0).abs() < f64::EPSILON);
}

#[test]
fn five_successes_trigger_recovery() {
    let limiter = AdaptiveLimiter::new(100.0, 50).with_min_rate(1.0);
    limiter.record_response(Duration::from_millis(100), false);
    let after_error = limiter.current_rate();
    for _ in 0..5 {
        limiter.record_response(Duration::from_millis(10), true);
    }
    assert!(limiter.current_rate() > after_error);
}

#[test]
fn rate_never_exceeds_base() {
    let limiter = AdaptiveLimiter::new(10.0, 50).with_min_rate(1.0);
    limiter.record_response(Duration::from_millis(100), false);
    for _ in 0..200 {
        limiter.record_response(Duration::from_millis(10), true);
    }
    assert!(limiter.current_rate() <= 10.0);
}

// ---------------------------------------------------------------------------
// Latency tracking
// ---------------------------------------------------------------------------

#[test]
fn avg_latency_none_initially() {
    let limiter = AdaptiveLimiter::new(10.0, 5);
    assert!(limiter.avg_latency().is_none());
}

#[test]
fn avg_latency_calculation() {
    let limiter = AdaptiveLimiter::new(10.0, 5);
    limiter.record_response(Duration::from_millis(100), true);
    limiter.record_response(Duration::from_millis(200), true);
    limiter.record_response(Duration::from_millis(300), true);
    let avg = limiter.avg_latency().unwrap();
    assert!(avg >= Duration::from_millis(190) && avg <= Duration::from_millis(210));
}

// ---------------------------------------------------------------------------
// Consecutive error tracking
// ---------------------------------------------------------------------------

#[test]
fn consecutive_errors_reset_on_success() {
    let limiter = AdaptiveLimiter::new(100.0, 50);
    limiter.record_response(Duration::from_millis(100), false);
    limiter.record_response(Duration::from_millis(100), false);
    assert_eq!(limiter.consecutive_errors(), 2);
    limiter.record_response(Duration::from_millis(100), true);
    assert_eq!(limiter.consecutive_errors(), 0);
}

// ---------------------------------------------------------------------------
// Reset
// ---------------------------------------------------------------------------

#[test]
fn reset_restores_all_state() {
    let limiter = AdaptiveLimiter::new(100.0, 50);
    limiter.record_response(Duration::from_millis(100), false);
    limiter.record_response(Duration::from_millis(100), false);
    limiter.reset();
    assert!((limiter.current_rate() - 100.0).abs() < f64::EPSILON);
    assert_eq!(limiter.consecutive_errors(), 0);
    assert!(limiter.avg_latency().is_none());
}

// ---------------------------------------------------------------------------
// should_throttle
// ---------------------------------------------------------------------------

#[test]
fn should_throttle_when_retry_after_active() {
    let limiter = AdaptiveLimiter::new(100.0, 100);
    let mut headers = HashMap::new();
    headers.insert("retry-after".to_string(), "60".to_string());
    limiter.update_from_headers(&headers);
    assert!(limiter.should_throttle());
}

// ---------------------------------------------------------------------------
// Concurrent access
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_record_response() {
    let limiter = Arc::new(AdaptiveLimiter::new(100.0, 50).with_min_rate(0.1));
    let mut handles = Vec::new();
    for i in 0..10 {
        let l = Arc::clone(&limiter);
        handles.push(tokio::spawn(async move {
            l.record_response(Duration::from_millis(10 * i), i % 2 == 0);
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    // Just verify no panics and rate is still positive
    assert!(limiter.current_rate() > 0.0);
}

// ---------------------------------------------------------------------------
// Clone shares state
// ---------------------------------------------------------------------------

#[test]
fn clone_shares_state() {
    let limiter = AdaptiveLimiter::new(10.0, 3);
    let clone = limiter.clone();
    limiter.try_acquire();
    limiter.try_acquire();
    limiter.try_acquire();
    assert!(!clone.try_acquire());
}
