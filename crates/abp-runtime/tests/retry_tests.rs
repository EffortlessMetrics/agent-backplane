// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the retry / timeout module.

use abp_runtime::retry::{RetryPolicy, TimeoutConfig};
use std::time::Duration;

// ── Default policy values ───────────────────────────────────────────────────

#[test]
fn default_policy_values() {
    let p = RetryPolicy::default();
    assert_eq!(p.max_retries, 3);
    assert_eq!(p.initial_backoff, Duration::from_millis(100));
    assert_eq!(p.max_backoff, Duration::from_secs(5));
    assert!((p.backoff_multiplier - 2.0).abs() < f64::EPSILON);
}

// ── Custom policy via builder ───────────────────────────────────────────────

#[test]
fn custom_policy_via_builder() {
    let p = RetryPolicy::builder()
        .max_retries(5)
        .initial_backoff(Duration::from_millis(200))
        .max_backoff(Duration::from_secs(10))
        .backoff_multiplier(3.0)
        .build();

    assert_eq!(p.max_retries, 5);
    assert_eq!(p.initial_backoff, Duration::from_millis(200));
    assert_eq!(p.max_backoff, Duration::from_secs(10));
    assert!((p.backoff_multiplier - 3.0).abs() < f64::EPSILON);
}

// ── Backoff computation ─────────────────────────────────────────────────────

#[test]
fn backoff_first_attempt() {
    let p = RetryPolicy::builder()
        .initial_backoff(Duration::from_millis(100))
        .backoff_multiplier(2.0)
        .max_backoff(Duration::from_secs(60))
        .build();
    let delay = p.compute_delay(0);
    // 100ms * 2^0 = 100ms, jitter ±25% → [75, 125]ms
    assert!(delay >= Duration::from_millis(75) && delay <= Duration::from_millis(125));
}

#[test]
fn backoff_second_attempt() {
    let p = RetryPolicy::builder()
        .initial_backoff(Duration::from_millis(100))
        .backoff_multiplier(2.0)
        .max_backoff(Duration::from_secs(60))
        .build();
    let delay = p.compute_delay(1);
    // 100ms * 2^1 = 200ms, jitter ±25% → [150, 250]ms
    assert!(delay >= Duration::from_millis(150) && delay <= Duration::from_millis(250));
}

#[test]
fn backoff_third_attempt() {
    let p = RetryPolicy::builder()
        .initial_backoff(Duration::from_millis(100))
        .backoff_multiplier(2.0)
        .max_backoff(Duration::from_secs(60))
        .build();
    let delay = p.compute_delay(2);
    // 100ms * 2^2 = 400ms, jitter ±25% → [300, 500]ms
    assert!(delay >= Duration::from_millis(300) && delay <= Duration::from_millis(500));
}

// ── Max backoff cap ─────────────────────────────────────────────────────────

#[test]
fn backoff_capped_at_max() {
    let p = RetryPolicy::builder()
        .initial_backoff(Duration::from_secs(1))
        .backoff_multiplier(10.0)
        .max_backoff(Duration::from_secs(5))
        .build();
    // Attempt 3: 1s * 10^3 = 1000s → capped to 5s, then jitter ±25% → [3750, 5000]ms
    let delay = p.compute_delay(3);
    assert!(delay <= Duration::from_secs(5));
    assert!(delay >= Duration::from_millis(3750));
}

// ── Zero retries means no retry ─────────────────────────────────────────────

#[test]
fn zero_retries_no_retry() {
    let p = RetryPolicy::builder().max_retries(0).build();
    assert!(!p.should_retry(0));
    assert!(!p.should_retry(1));
}

// ── Timeout config defaults ─────────────────────────────────────────────────

#[test]
fn timeout_config_defaults() {
    let tc = TimeoutConfig::default();
    assert!(tc.run_timeout.is_none());
    assert!(tc.event_timeout.is_none());
}

// ── Backoff jitter stays within bounds ──────────────────────────────────────

#[test]
fn jitter_bounds_across_many_attempts() {
    let p = RetryPolicy::default(); // 100ms initial, 2x, 5s max
    for attempt in 0..50 {
        let delay = p.compute_delay(attempt);
        assert!(
            delay <= p.max_backoff,
            "attempt {attempt}: {delay:?} > max {:?}",
            p.max_backoff
        );
    }
}

// ── Serde roundtrip ─────────────────────────────────────────────────────────

#[test]
fn retry_policy_serde_roundtrip() {
    let p = RetryPolicy::builder()
        .max_retries(7)
        .initial_backoff(Duration::from_millis(250))
        .max_backoff(Duration::from_secs(30))
        .backoff_multiplier(1.5)
        .build();
    let json = serde_json::to_string(&p).unwrap();
    let p2: RetryPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}

#[test]
fn timeout_config_serde_roundtrip() {
    let tc = TimeoutConfig {
        run_timeout: Some(Duration::from_secs(60)),
        event_timeout: Some(Duration::from_millis(500)),
    };
    let json = serde_json::to_string(&tc).unwrap();
    let tc2: TimeoutConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(tc, tc2);
}

// ── Infinite retries ────────────────────────────────────────────────────────

#[test]
fn infinite_retries() {
    let p = RetryPolicy::builder().max_retries(u32::MAX).build();
    assert!(p.should_retry(0));
    assert!(p.should_retry(1_000_000));
    assert!(p.should_retry(u32::MAX - 1));
    assert!(!p.should_retry(u32::MAX));
}

// ── Various multiplier values ───────────────────────────────────────────────

#[test]
fn multiplier_one_gives_constant_backoff() {
    let p = RetryPolicy::builder()
        .initial_backoff(Duration::from_millis(500))
        .backoff_multiplier(1.0)
        .max_backoff(Duration::from_secs(60))
        .build();
    // All attempts should yield ~500ms (±25% jitter → [375, 625]).
    for attempt in 0..5 {
        let delay = p.compute_delay(attempt);
        assert!(
            delay >= Duration::from_millis(375) && delay <= Duration::from_millis(625),
            "attempt {attempt}: {delay:?}"
        );
    }
}

#[test]
fn multiplier_half_gives_decreasing_backoff() {
    let p = RetryPolicy::builder()
        .initial_backoff(Duration::from_millis(1000))
        .backoff_multiplier(0.5)
        .max_backoff(Duration::from_secs(60))
        .build();
    // attempt 0 → 1000ms base, attempt 2 → 250ms base
    let d0 = p.compute_delay(0);
    let d2 = p.compute_delay(2);
    // Even with jitter the second-attempt base is 4x smaller.
    assert!(d0 > d2, "d0={d0:?} should be > d2={d2:?}");
}
