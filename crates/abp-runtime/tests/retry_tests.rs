#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for the retry / timeout module.

use abp_runtime::retry::{FallbackChain, RetryPolicy, TimeoutConfig};
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

// ── no_retry constructor ────────────────────────────────────────────────────

#[test]
fn no_retry_has_zero_max_retries() {
    assert_eq!(RetryPolicy::no_retry().max_retries, 0);
}

#[test]
fn no_retry_should_retry_is_always_false() {
    let p = RetryPolicy::no_retry();
    for attempt in 0..10 {
        assert!(!p.should_retry(attempt));
    }
}

#[test]
fn no_retry_delay_is_zero() {
    let p = RetryPolicy::no_retry();
    assert_eq!(p.delay_for(0), Duration::ZERO);
}

#[test]
fn no_retry_serde_roundtrip() {
    let p = RetryPolicy::no_retry();
    let json = serde_json::to_string(&p).unwrap();
    let p2: RetryPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}

// ── delay_for alias ─────────────────────────────────────────────────────────

#[test]
fn delay_for_equals_compute_delay() {
    let p = RetryPolicy::default();
    for attempt in 0..10 {
        assert_eq!(p.delay_for(attempt), p.compute_delay(attempt));
    }
}

// ── FallbackChain ───────────────────────────────────────────────────────────

#[test]
fn fallback_chain_iterates_in_order() {
    let mut chain = FallbackChain::new(vec!["alpha".into(), "beta".into(), "gamma".into()]);
    assert_eq!(chain.next_backend(), Some("alpha"));
    assert_eq!(chain.next_backend(), Some("beta"));
    assert_eq!(chain.next_backend(), Some("gamma"));
    assert_eq!(chain.next_backend(), None);
}

#[test]
fn fallback_chain_empty() {
    let mut chain = FallbackChain::new(vec![]);
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
    assert_eq!(chain.remaining(), 0);
    assert_eq!(chain.next_backend(), None);
}

#[test]
fn fallback_chain_exhausted_returns_none_repeatedly() {
    let mut chain = FallbackChain::new(vec!["only".into()]);
    assert_eq!(chain.next_backend(), Some("only"));
    assert_eq!(chain.next_backend(), None);
    assert_eq!(chain.next_backend(), None);
}

#[test]
fn fallback_chain_remaining_decrements() {
    let mut chain = FallbackChain::new(vec!["a".into(), "b".into(), "c".into()]);
    assert_eq!(chain.remaining(), 3);
    chain.next_backend();
    assert_eq!(chain.remaining(), 2);
    chain.next_backend();
    assert_eq!(chain.remaining(), 1);
    chain.next_backend();
    assert_eq!(chain.remaining(), 0);
}

#[test]
fn fallback_chain_len_is_total() {
    let chain = FallbackChain::new(vec!["a".into(), "b".into()]);
    assert_eq!(chain.len(), 2);
}

#[test]
fn fallback_chain_reset() {
    let mut chain = FallbackChain::new(vec!["x".into(), "y".into()]);
    chain.next_backend();
    chain.next_backend();
    assert_eq!(chain.remaining(), 0);

    chain.reset();
    assert_eq!(chain.remaining(), 2);
    assert_eq!(chain.next_backend(), Some("x"));
}

#[test]
fn fallback_chain_is_empty_false_with_backends() {
    let chain = FallbackChain::new(vec!["a".into()]);
    assert!(!chain.is_empty());
}

// ── RuntimeError retryability ───────────────────────────────────────────────

#[test]
fn backend_failed_is_retryable() {
    let err = abp_runtime::RuntimeError::BackendFailed(anyhow::anyhow!("connection reset"));
    assert!(err.is_retryable());
}

#[test]
fn workspace_failed_is_retryable() {
    let err = abp_runtime::RuntimeError::WorkspaceFailed(anyhow::anyhow!("disk full"));
    assert!(err.is_retryable());
}

#[test]
fn unknown_backend_is_not_retryable() {
    let err = abp_runtime::RuntimeError::UnknownBackend {
        name: "nope".into(),
    };
    assert!(!err.is_retryable());
}

#[test]
fn policy_failed_is_not_retryable() {
    let err = abp_runtime::RuntimeError::PolicyFailed(anyhow::anyhow!("bad glob"));
    assert!(!err.is_retryable());
}

#[test]
fn capability_check_failed_is_not_retryable() {
    let err = abp_runtime::RuntimeError::CapabilityCheckFailed("missing tool_use".into());
    assert!(!err.is_retryable());
}

#[test]
fn no_projection_match_is_not_retryable() {
    let err = abp_runtime::RuntimeError::NoProjectionMatch {
        reason: "no score".into(),
    };
    assert!(!err.is_retryable());
}
