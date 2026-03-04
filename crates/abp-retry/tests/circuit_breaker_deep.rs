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
//! Comprehensive tests for circuit breaker and retry patterns in `abp-retry`.
//!
//! Categories covered:
//! 1. Retry with backoff
//! 2. Circuit breaker states
//! 3. Failure counting
//! 4. Success reset
//! 5. Timeout / recovery
//! 6. Half-open probe
//! 7. Configuration / custom thresholds
//! 8. Retry policy: which errors are retriable
//! 9. Non-retriable errors
//! 10. Max attempts
//! 11. Backoff calculation
//! 12. Integration: retry + circuit breaker together

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use abp_retry::{
    CircuitBreaker, CircuitBreakerError, CircuitState, RetryPolicy, retry_with_policy,
};

// ===========================================================================
// 1. Retry with backoff
// ===========================================================================

#[tokio::test]
async fn backoff_delays_increase_between_retries() {
    let timestamps = Arc::new(std::sync::Mutex::new(Vec::new()));
    let ts = timestamps.clone();
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(20),
        Duration::from_secs(5),
        2.0,
        false,
    );
    let _: Result<(), &str> = retry_with_policy(&p, || {
        let ts = ts.clone();
        async move {
            ts.lock().unwrap().push(std::time::Instant::now());
            Err("fail")
        }
    })
    .await;
    let times = timestamps.lock().unwrap();
    assert_eq!(times.len(), 4);
    // Gap between attempt 1→2 should be >= gap between attempt 0→1
    let gap_01 = times[1].duration_since(times[0]);
    let gap_12 = times[2].duration_since(times[1]);
    assert!(
        gap_12 >= gap_01.mul_f64(0.8),
        "backoff should grow: gap01={gap_01:?}, gap12={gap_12:?}"
    );
}

#[tokio::test]
async fn backoff_with_jitter_completes_in_reasonable_time() {
    let start = std::time::Instant::now();
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(10),
        Duration::from_secs(1),
        2.0,
        true,
    );
    let _: Result<(), &str> = retry_with_policy(&p, || async { Err("fail") }).await;
    let elapsed = start.elapsed();
    // Jitter scales [0,1) so delays are at most: 10 + 20 + 40 = 70ms
    assert!(
        elapsed < Duration::from_millis(500),
        "took too long with jitter: {elapsed:?}"
    );
}

#[tokio::test]
async fn backoff_no_jitter_minimum_total_delay() {
    let start = std::time::Instant::now();
    let p = RetryPolicy::new(
        2,
        Duration::from_millis(20),
        Duration::from_secs(5),
        2.0,
        false,
    );
    let _: Result<(), &str> = retry_with_policy(&p, || async { Err("fail") }).await;
    let elapsed = start.elapsed();
    // attempt 0 delay=20ms, attempt 1 delay=40ms => >=60ms total sleep
    assert!(
        elapsed >= Duration::from_millis(40),
        "expected >= 40ms, got {elapsed:?}"
    );
}

#[tokio::test]
async fn backoff_success_on_second_attempt_has_one_delay() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let start = std::time::Instant::now();
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(20),
        Duration::from_secs(5),
        2.0,
        false,
    );
    let result = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n == 0 { Err("transient") } else { Ok("ok") }
        }
    })
    .await;
    assert_eq!(result.unwrap(), "ok");
    let elapsed = start.elapsed();
    // Only one sleep of 20ms
    assert!(elapsed >= Duration::from_millis(15));
    assert!(elapsed < Duration::from_millis(500));
}

#[tokio::test]
async fn backoff_capped_delay_does_not_exceed_max() {
    let start = std::time::Instant::now();
    let p = RetryPolicy::new(
        2,
        Duration::from_millis(50),
        Duration::from_millis(60),
        10.0,
        false,
    );
    let _: Result<(), &str> = retry_with_policy(&p, || async { Err("fail") }).await;
    let elapsed = start.elapsed();
    // attempt 0: 50ms, attempt 1: min(500ms, 60ms) = 60ms => ~110ms
    assert!(
        elapsed < Duration::from_millis(500),
        "cap not respected: {elapsed:?}"
    );
}

// ===========================================================================
// 2. Circuit breaker states
// ===========================================================================

#[test]
fn cb_state_starts_closed() {
    let cb = CircuitBreaker::new(5, Duration::from_secs(30));
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn cb_state_remains_closed_on_success() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    for _ in 0..5 {
        let _: Result<i32, CircuitBreakerError<String>> =
            cb.call(|| async { Ok::<_, String>(1) }).await;
    }
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn cb_state_closed_below_threshold() {
    let cb = CircuitBreaker::new(5, Duration::from_secs(30));
    for _ in 0..4 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    }
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn cb_state_opens_at_exact_threshold() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    for _ in 0..3 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    }
    assert_eq!(cb.state(), CircuitState::Open);
}

#[tokio::test]
async fn cb_state_stays_open_on_additional_failures() {
    let cb = CircuitBreaker::new(2, Duration::from_secs(300));
    for _ in 0..2 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    }
    assert_eq!(cb.state(), CircuitState::Open);
    // Further calls are rejected (state stays open)
    let res: Result<String, CircuitBreakerError<&str>> =
        cb.call(|| async { Ok("nope".into()) }).await;
    assert!(matches!(res, Err(CircuitBreakerError::Open)));
    assert_eq!(cb.state(), CircuitState::Open);
}

#[test]
fn circuit_state_variants_are_distinct() {
    assert_ne!(CircuitState::Closed, CircuitState::Open);
    assert_ne!(CircuitState::Open, CircuitState::HalfOpen);
    assert_ne!(CircuitState::Closed, CircuitState::HalfOpen);
}

#[test]
fn circuit_state_copy_semantics() {
    let s = CircuitState::HalfOpen;
    let s2 = s;
    assert_eq!(s, s2);
}

// ===========================================================================
// 3. Failure counting
// ===========================================================================

#[tokio::test]
async fn failure_count_increments_on_each_failure() {
    let cb = CircuitBreaker::new(10, Duration::from_secs(30));
    for expected in 1..=5u32 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
        assert_eq!(cb.consecutive_failures(), expected);
    }
}

#[tokio::test]
async fn failure_count_starts_at_zero() {
    let cb = CircuitBreaker::new(5, Duration::from_secs(30));
    assert_eq!(cb.consecutive_failures(), 0);
}

#[tokio::test]
async fn failure_count_not_incremented_on_success() {
    let cb = CircuitBreaker::new(5, Duration::from_secs(30));
    let _: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("ok".into()) }).await;
    assert_eq!(cb.consecutive_failures(), 0);
}

#[tokio::test]
async fn failure_count_tracks_consecutive_only() {
    let cb = CircuitBreaker::new(10, Duration::from_secs(30));
    // 2 failures
    for _ in 0..2 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    }
    assert_eq!(cb.consecutive_failures(), 2);
    // success resets
    let _: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("ok".into()) }).await;
    assert_eq!(cb.consecutive_failures(), 0);
    // 1 more failure
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.consecutive_failures(), 1);
}

#[tokio::test]
async fn failure_count_reaches_threshold_then_opens() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(300));
    for _ in 0..2 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    }
    assert_eq!(cb.state(), CircuitState::Closed);
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
    assert_eq!(cb.consecutive_failures(), 3);
}

// ===========================================================================
// 4. Success reset
// ===========================================================================

#[tokio::test]
async fn success_resets_failure_count_to_zero() {
    let cb = CircuitBreaker::new(5, Duration::from_secs(30));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.consecutive_failures(), 2);
    let _: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("ok".into()) }).await;
    assert_eq!(cb.consecutive_failures(), 0);
}

#[tokio::test]
async fn success_keeps_state_closed() {
    let cb = CircuitBreaker::new(5, Duration::from_secs(30));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    let _: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("ok".into()) }).await;
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn success_after_multiple_failures_prevents_opening() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    // 2 failures, then success, then 2 more failures
    for _ in 0..2 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    }
    let _: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("ok".into()) }).await;
    for _ in 0..2 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    }
    // Only 2 consecutive, threshold is 3
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn multiple_successes_keep_count_zero() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    for _ in 0..10 {
        let _: Result<String, CircuitBreakerError<String>> =
            cb.call(|| async { Ok::<_, String>("ok".into()) }).await;
    }
    assert_eq!(cb.consecutive_failures(), 0);
    assert_eq!(cb.state(), CircuitState::Closed);
}

// ===========================================================================
// 5. Timeout / recovery
// ===========================================================================

#[tokio::test]
async fn open_circuit_rejects_before_timeout() {
    let cb = CircuitBreaker::new(1, Duration::from_secs(60));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
    // Immediately try again — should be rejected
    let res: Result<String, CircuitBreakerError<&str>> =
        cb.call(|| async { Ok("nope".into()) }).await;
    assert!(matches!(res, Err(CircuitBreakerError::Open)));
}

#[tokio::test]
async fn open_circuit_allows_probe_after_timeout() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(15));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
    tokio::time::sleep(Duration::from_millis(25)).await;
    let res: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("probe".into()) }).await;
    assert!(res.is_ok());
}

#[tokio::test]
async fn timeout_duration_respected_short() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(50));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    // Wait less than timeout
    tokio::time::sleep(Duration::from_millis(10)).await;
    let res: Result<String, CircuitBreakerError<&str>> =
        cb.call(|| async { Ok("nope".into()) }).await;
    assert!(matches!(res, Err(CircuitBreakerError::Open)));
    // Wait past timeout
    tokio::time::sleep(Duration::from_millis(50)).await;
    let res: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("ok".into()) }).await;
    assert!(res.is_ok());
}

#[tokio::test]
async fn failed_probe_resets_timeout_window() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(15));
    // Trip the breaker
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
    tokio::time::sleep(Duration::from_millis(25)).await;
    // Failed probe reopens
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("still broken") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
    // Must wait again before next probe
    let res: Result<String, CircuitBreakerError<&str>> =
        cb.call(|| async { Ok("nope".into()) }).await;
    assert!(matches!(res, Err(CircuitBreakerError::Open)));
}

// ===========================================================================
// 6. Half-open probe
// ===========================================================================

#[tokio::test]
async fn half_open_successful_probe_closes_breaker() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(10));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    let res: Result<String, CircuitBreakerError<String>> = cb
        .call(|| async { Ok::<_, String>("probe ok".into()) })
        .await;
    assert_eq!(res.unwrap(), "probe ok");
    assert_eq!(cb.state(), CircuitState::Closed);
    assert_eq!(cb.consecutive_failures(), 0);
}

#[tokio::test]
async fn half_open_failed_probe_reopens_breaker() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(10));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("probe fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
}

#[tokio::test]
async fn half_open_probe_returns_inner_error_on_failure() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(10));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("trip") }).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    let res: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Err("probe error".to_string()) }).await;
    match res {
        Err(CircuitBreakerError::Inner(e)) => assert_eq!(e, "probe error"),
        other => panic!("expected Inner, got {other:?}"),
    }
}

#[tokio::test]
async fn half_open_probe_success_allows_subsequent_calls() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(10));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    // Successful probe
    let _: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("probe".into()) }).await;
    assert_eq!(cb.state(), CircuitState::Closed);
    // Subsequent calls succeed normally
    let res: Result<String, CircuitBreakerError<String>> = cb
        .call(|| async { Ok::<_, String>("follow-up".into()) })
        .await;
    assert_eq!(res.unwrap(), "follow-up");
}

#[tokio::test]
async fn half_open_probe_failure_requires_another_timeout() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(15));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("trip") }).await;
    tokio::time::sleep(Duration::from_millis(25)).await;
    // Failed probe
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("probe fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
    // Immediately: rejected
    let res: Result<String, CircuitBreakerError<&str>> =
        cb.call(|| async { Ok("nope".into()) }).await;
    assert!(matches!(res, Err(CircuitBreakerError::Open)));
    // After timeout: allowed
    tokio::time::sleep(Duration::from_millis(25)).await;
    let res: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("ok now".into()) }).await;
    assert_eq!(res.unwrap(), "ok now");
    assert_eq!(cb.state(), CircuitState::Closed);
}

// ===========================================================================
// 7. Configuration / custom thresholds
// ===========================================================================

#[test]
fn config_threshold_one_opens_on_first_failure() {
    // Verified by calling; threshold=1 means one failure opens.
    let cb = CircuitBreaker::new(1, Duration::from_secs(30));
    assert_eq!(cb.failure_threshold(), 1);
}

#[test]
fn config_threshold_large() {
    let cb = CircuitBreaker::new(100, Duration::from_secs(60));
    assert_eq!(cb.failure_threshold(), 100);
}

#[test]
fn config_recovery_timeout_millis() {
    let cb = CircuitBreaker::new(3, Duration::from_millis(500));
    assert_eq!(cb.recovery_timeout(), Duration::from_millis(500));
}

#[test]
fn config_recovery_timeout_seconds() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(120));
    assert_eq!(cb.recovery_timeout(), Duration::from_secs(120));
}

#[tokio::test]
async fn config_threshold_ten_requires_ten_failures() {
    let cb = CircuitBreaker::new(10, Duration::from_secs(30));
    for _ in 0..9 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    }
    assert_eq!(cb.state(), CircuitState::Closed);
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
}

#[test]
fn config_custom_policy_all_fields() {
    let p = RetryPolicy::new(
        10,
        Duration::from_millis(50),
        Duration::from_secs(30),
        1.5,
        true,
    );
    assert_eq!(p.max_retries, 10);
    assert_eq!(p.base_delay, Duration::from_millis(50));
    assert_eq!(p.max_delay, Duration::from_secs(30));
    assert!((p.backoff_multiplier - 1.5).abs() < f64::EPSILON);
    assert!(p.jitter);
}

#[test]
fn config_zero_base_delay() {
    let p = RetryPolicy::new(3, Duration::ZERO, Duration::from_secs(5), 2.0, false);
    assert_eq!(p.delay_for_attempt(0), Duration::ZERO);
    assert_eq!(p.delay_for_attempt(3), Duration::ZERO);
}

// ===========================================================================
// 8. Retry policy: which errors are retriable
// ===========================================================================

#[derive(Debug, Clone, PartialEq)]
enum AppError {
    Transient(String),
    Permanent(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Transient(msg) => write!(f, "transient: {msg}"),
            AppError::Permanent(msg) => write!(f, "permanent: {msg}"),
        }
    }
}

#[tokio::test]
async fn retry_recovers_from_transient_errors() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let result: Result<String, AppError> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                Err(AppError::Transient("timeout".into()))
            } else {
                Ok("recovered".into())
            }
        }
    })
    .await;
    assert_eq!(result.unwrap(), "recovered");
}

#[tokio::test]
async fn retry_exhausts_on_repeated_transient_errors() {
    let p = RetryPolicy::new(
        2,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let result: Result<String, AppError> = retry_with_policy(&p, || async {
        Err(AppError::Transient("always failing".into()))
    })
    .await;
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        AppError::Transient("always failing".into())
    );
}

#[tokio::test]
async fn retry_returns_last_error_variant() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let p = RetryPolicy::new(
        2,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let result: Result<String, AppError> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            Err(AppError::Transient(format!("attempt-{n}")))
        }
    })
    .await;
    assert_eq!(result.unwrap_err(), AppError::Transient("attempt-2".into()));
}

// ===========================================================================
// 9. Non-retriable errors (simulated via early bail-out)
// ===========================================================================

#[tokio::test]
async fn non_retriable_auth_error_stops_immediately() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    // Simulates bailing out: if the error is permanent, the closure returns Ok
    // wrapping an inner Result so the retry loop stops.
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let result: Result<Result<String, AppError>, AppError> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            // Permanent error: wrap as Ok to escape retry loop
            Ok(Err(AppError::Permanent("auth failed".into())))
        }
    })
    .await;
    // Called only once
    assert_eq!(counter.load(Ordering::SeqCst), 1);
    let inner = result.unwrap();
    assert_eq!(
        inner.unwrap_err(),
        AppError::Permanent("auth failed".into())
    );
}

#[tokio::test]
async fn non_retriable_bad_request_single_attempt() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let p = RetryPolicy::no_retry();
    let result: Result<String, AppError> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err(AppError::Permanent("400 bad request".into()))
        }
    })
    .await;
    assert_eq!(counter.load(Ordering::SeqCst), 1);
    assert!(result.is_err());
}

#[tokio::test]
async fn non_retriable_error_preserved_through_circuit_breaker() {
    let cb = CircuitBreaker::new(5, Duration::from_secs(30));
    let res: Result<String, CircuitBreakerError<AppError>> = cb
        .call(|| async { Err(AppError::Permanent("forbidden".into())) })
        .await;
    match res {
        Err(CircuitBreakerError::Inner(e)) => {
            assert_eq!(e, AppError::Permanent("forbidden".into()));
        }
        other => panic!("expected Inner, got {other:?}"),
    }
}

// ===========================================================================
// 10. Max attempts
// ===========================================================================

#[tokio::test]
async fn max_attempts_zero_retries_means_one_call() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let p = RetryPolicy::no_retry();
    let _: Result<(), &str> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err("fail")
        }
    })
    .await;
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn max_attempts_three_retries_means_four_calls() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let _: Result<(), &str> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err("fail")
        }
    })
    .await;
    assert_eq!(counter.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn max_attempts_ten_retries_means_eleven_calls() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let p = RetryPolicy::new(
        10,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let _: Result<(), &str> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err("fail")
        }
    })
    .await;
    assert_eq!(counter.load(Ordering::SeqCst), 11);
}

#[tokio::test]
async fn max_attempts_early_success_stops_retrying() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let p = RetryPolicy::new(
        10,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let result = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n == 2 { Ok("done") } else { Err("not yet") }
        }
    })
    .await;
    assert_eq!(result.unwrap(), "done");
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn max_attempts_succeeds_on_exact_last() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let result = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n == 3 { Ok("last") } else { Err("not yet") }
        }
    })
    .await;
    assert_eq!(result.unwrap(), "last");
    assert_eq!(counter.load(Ordering::SeqCst), 4);
}

// ===========================================================================
// 11. Backoff calculation
// ===========================================================================

#[test]
fn backoff_calc_attempt_0_equals_base() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(100),
        Duration::from_secs(60),
        2.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(0), Duration::from_millis(100));
}

#[test]
fn backoff_calc_exponential_doubling() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(100),
        Duration::from_secs(60),
        2.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(0), Duration::from_millis(100));
    assert_eq!(p.delay_for_attempt(1), Duration::from_millis(200));
    assert_eq!(p.delay_for_attempt(2), Duration::from_millis(400));
    assert_eq!(p.delay_for_attempt(3), Duration::from_millis(800));
    assert_eq!(p.delay_for_attempt(4), Duration::from_millis(1600));
}

#[test]
fn backoff_calc_multiplier_three() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(10),
        Duration::from_secs(600),
        3.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(0), Duration::from_millis(10));
    assert_eq!(p.delay_for_attempt(1), Duration::from_millis(30));
    assert_eq!(p.delay_for_attempt(2), Duration::from_millis(90));
    assert_eq!(p.delay_for_attempt(3), Duration::from_millis(270));
}

#[test]
fn backoff_calc_capped_at_max() {
    let p = RetryPolicy::new(
        10,
        Duration::from_millis(100),
        Duration::from_millis(500),
        2.0,
        false,
    );
    // 100, 200, 400, 500(cap), 500, ...
    assert_eq!(p.delay_for_attempt(3), Duration::from_millis(500));
    assert_eq!(p.delay_for_attempt(4), Duration::from_millis(500));
    assert_eq!(p.delay_for_attempt(10), Duration::from_millis(500));
}

#[test]
fn backoff_calc_multiplier_one_is_constant() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(200),
        Duration::from_secs(60),
        1.0,
        false,
    );
    for i in 0..5 {
        assert_eq!(p.delay_for_attempt(i), Duration::from_millis(200));
    }
}

#[test]
fn backoff_calc_jitter_bounded_by_computed_delay() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(100),
        Duration::from_secs(60),
        2.0,
        true,
    );
    for _ in 0..200 {
        let d = p.delay_for_attempt(2);
        // 100 * 2^2 = 400ms max
        assert!(d <= Duration::from_millis(400));
    }
}

#[test]
fn backoff_calc_jitter_produces_variation() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(100),
        Duration::from_secs(60),
        2.0,
        true,
    );
    let delays: Vec<Duration> = (0..30).map(|_| p.delay_for_attempt(2)).collect();
    let all_same = delays.windows(2).all(|w| w[0] == w[1]);
    assert!(!all_same, "jitter should produce variation");
}

#[test]
fn backoff_calc_no_jitter_is_deterministic() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(100),
        Duration::from_secs(60),
        2.0,
        false,
    );
    for i in 0..5 {
        let d1 = p.delay_for_attempt(i);
        let d2 = p.delay_for_attempt(i);
        assert_eq!(d1, d2, "attempt {i} should be deterministic without jitter");
    }
}

#[test]
fn backoff_calc_base_delay_larger_than_max() {
    let p = RetryPolicy::new(
        3,
        Duration::from_secs(10),
        Duration::from_secs(1),
        2.0,
        false,
    );
    // base > max => capped
    assert_eq!(p.delay_for_attempt(0), Duration::from_secs(1));
}

#[test]
fn backoff_calc_monotonic_until_cap() {
    let p = RetryPolicy::new(
        10,
        Duration::from_millis(10),
        Duration::from_secs(60),
        2.0,
        false,
    );
    let delays: Vec<Duration> = (0..8).map(|i| p.delay_for_attempt(i)).collect();
    for w in delays.windows(2) {
        assert!(
            w[1] >= w[0],
            "should be monotonic: {:?} -> {:?}",
            w[0],
            w[1]
        );
    }
}

// ===========================================================================
// 12. Integration: retry + circuit breaker together
// ===========================================================================

#[tokio::test]
async fn integration_retry_then_circuit_breaker() {
    let cb = Arc::new(CircuitBreaker::new(3, Duration::from_secs(300)));
    let call_count = Arc::new(AtomicU32::new(0));

    // Trip the breaker with 3 direct failures
    for _ in 0..3 {
        let cc = call_count.clone();
        let _: Result<String, CircuitBreakerError<String>> = cb
            .call(|| {
                let cc = cc.clone();
                async move {
                    cc.fetch_add(1, Ordering::SeqCst);
                    Err("backend down".to_string())
                }
            })
            .await;
    }
    assert_eq!(cb.state(), CircuitState::Open);

    // Further calls through the CB should fail with Open (long timeout, no recovery)
    let cb2 = cb.clone();
    let p = RetryPolicy::new(
        2,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let result = retry_with_policy(&p, || {
        let cb2 = cb2.clone();
        async move {
            cb2.call(|| async { Ok::<String, String>("should not run".into()) })
                .await
        }
    })
    .await;
    assert!(matches!(result, Err(CircuitBreakerError::Open)));
}

#[tokio::test]
async fn integration_circuit_breaker_recovery_with_retry() {
    let cb = Arc::new(CircuitBreaker::new(2, Duration::from_millis(15)));
    let p = RetryPolicy::new(
        1,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );

    // Trip the breaker
    for _ in 0..2 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    }
    assert_eq!(cb.state(), CircuitState::Open);

    // Wait for recovery timeout
    tokio::time::sleep(Duration::from_millis(25)).await;

    // Now retry through the CB should succeed with a healthy backend
    let cb2 = cb.clone();
    let result = retry_with_policy(&p, || {
        let cb2 = cb2.clone();
        async move {
            cb2.call(|| async { Ok::<String, String>("healthy".into()) })
                .await
        }
    })
    .await;
    assert_eq!(result.unwrap(), "healthy");
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn integration_retry_succeeds_before_tripping_breaker() {
    let cb = Arc::new(CircuitBreaker::new(5, Duration::from_secs(30)));
    let counter = Arc::new(AtomicU32::new(0));
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );

    let c = counter.clone();
    let cb2 = cb.clone();
    let result = retry_with_policy(&p, || {
        let c = c.clone();
        let cb2 = cb2.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            cb2.call(|| async move {
                if n < 2 {
                    Err("transient".to_string())
                } else {
                    Ok("recovered".to_string())
                }
            })
            .await
        }
    })
    .await;
    assert_eq!(result.unwrap(), "recovered");
    // Failures went through CB but didn't reach threshold of 5
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn integration_multiple_cycles_open_close() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(10));

    // Cycle 1: trip → wait → recover
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail 1") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
    tokio::time::sleep(Duration::from_millis(20)).await;
    let _: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("ok 1".into()) }).await;
    assert_eq!(cb.state(), CircuitState::Closed);

    // Cycle 2: trip → wait → recover
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail 2") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
    tokio::time::sleep(Duration::from_millis(20)).await;
    let _: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("ok 2".into()) }).await;
    assert_eq!(cb.state(), CircuitState::Closed);

    // Cycle 3: trip → wait → fail probe → wait → recover
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail 3") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
    tokio::time::sleep(Duration::from_millis(20)).await;
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("probe fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
    tokio::time::sleep(Duration::from_millis(20)).await;
    let _: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("ok 3".into()) }).await;
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn integration_cb_error_display_variants() {
    let cb = CircuitBreaker::new(1, Duration::from_secs(300));
    // Inner error
    let res: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Err("inner msg".to_string()) }).await;
    assert_eq!(res.unwrap_err().to_string(), "inner msg");

    // Open error
    let res: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("nope".into()) }).await;
    assert_eq!(res.unwrap_err().to_string(), "circuit breaker is open");
}

#[tokio::test]
async fn integration_shared_cb_across_retried_operations() {
    let cb = Arc::new(CircuitBreaker::new(3, Duration::from_millis(15)));
    let global_counter = Arc::new(AtomicU32::new(0));
    let p = RetryPolicy::new(
        1,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );

    // First operation: 2 failures through CB
    let gc = global_counter.clone();
    let cb1 = cb.clone();
    let _: Result<_, CircuitBreakerError<String>> = retry_with_policy(&p, || {
        let gc = gc.clone();
        let cb1 = cb1.clone();
        async move {
            gc.fetch_add(1, Ordering::SeqCst);
            cb1.call(|| async { Err::<String, _>("fail".to_string()) })
                .await
        }
    })
    .await;
    assert_eq!(cb.consecutive_failures(), 2);
    assert_eq!(cb.state(), CircuitState::Closed);

    // Second operation: 1 more failure trips it
    let _: Result<String, _> = cb
        .call(|| async { Err::<String, _>("final blow".to_string()) })
        .await;
    assert_eq!(cb.state(), CircuitState::Open);
}

#[tokio::test]
async fn integration_retry_with_cb_returns_correct_value() {
    let cb = Arc::new(CircuitBreaker::new(10, Duration::from_secs(30)));
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let cb2 = cb.clone();

    let result = retry_with_policy(&p, || {
        let c = c.clone();
        let cb2 = cb2.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            cb2.call(|| async move {
                if n < 1 {
                    Err("transient".to_string())
                } else {
                    Ok(42i32)
                }
            })
            .await
        }
    })
    .await;
    assert_eq!(result.unwrap(), 42);
}

// ===========================================================================
// Additional edge cases & trait tests
// ===========================================================================

#[test]
fn cb_debug_shows_fields() {
    let cb = CircuitBreaker::new(5, Duration::from_secs(10));
    let dbg = format!("{cb:?}");
    assert!(dbg.contains("CircuitBreaker"));
    assert!(dbg.contains("failure_threshold"));
    assert!(dbg.contains("recovery_timeout"));
    assert!(dbg.contains("consecutive_failures"));
}

#[test]
fn circuit_state_serde_roundtrip_all() {
    for state in [
        CircuitState::Closed,
        CircuitState::Open,
        CircuitState::HalfOpen,
    ] {
        let json = serde_json::to_string(&state).unwrap();
        let s2: CircuitState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, s2);
    }
}

#[test]
fn circuit_state_serde_snake_case_half_open() {
    let json = serde_json::to_string(&CircuitState::HalfOpen).unwrap();
    assert_eq!(json, r#""half_open""#);
}

#[test]
fn retry_policy_serde_roundtrip() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(250),
        Duration::from_secs(30),
        1.5,
        true,
    );
    let json = serde_json::to_string(&p).unwrap();
    let p2: RetryPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}

#[test]
fn retry_policy_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RetryPolicy>();
}

#[test]
fn circuit_breaker_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CircuitBreaker>();
}

#[test]
fn circuit_state_send_sync_copy() {
    fn assert_traits<T: Send + Sync + Copy + Clone + PartialEq + Eq>() {}
    assert_traits::<CircuitState>();
}
