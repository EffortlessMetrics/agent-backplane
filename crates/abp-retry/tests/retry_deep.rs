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
//! Comprehensive tests for `abp-retry` retry policies, backoff strategies, and circuit breakers.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use abp_retry::{
    CircuitBreaker, CircuitBreakerError, CircuitState, RetryPolicy, retry_with_policy,
};

// ===========================================================================
// 1. RetryPolicy construction
// ===========================================================================

#[test]
fn policy_default_max_retries_is_three() {
    assert_eq!(RetryPolicy::default().max_retries, 3);
}

#[test]
fn policy_default_base_delay_100ms() {
    assert_eq!(
        RetryPolicy::default().base_delay,
        Duration::from_millis(100)
    );
}

#[test]
fn policy_default_max_delay_5s() {
    assert_eq!(RetryPolicy::default().max_delay, Duration::from_secs(5));
}

#[test]
fn policy_default_backoff_multiplier_two() {
    let p = RetryPolicy::default();
    assert!((p.backoff_multiplier - 2.0).abs() < f64::EPSILON);
}

#[test]
fn policy_default_jitter_enabled() {
    assert!(RetryPolicy::default().jitter);
}

#[test]
fn policy_new_sets_all_fields() {
    let p = RetryPolicy::new(
        7,
        Duration::from_millis(250),
        Duration::from_secs(30),
        1.5,
        false,
    );
    assert_eq!(p.max_retries, 7);
    assert_eq!(p.base_delay, Duration::from_millis(250));
    assert_eq!(p.max_delay, Duration::from_secs(30));
    assert!((p.backoff_multiplier - 1.5).abs() < f64::EPSILON);
    assert!(!p.jitter);
}

#[test]
fn policy_no_retry_zero_retries() {
    assert_eq!(RetryPolicy::no_retry().max_retries, 0);
}

#[test]
fn policy_no_retry_zero_delays() {
    let p = RetryPolicy::no_retry();
    assert_eq!(p.base_delay, Duration::ZERO);
    assert_eq!(p.max_delay, Duration::ZERO);
}

#[test]
fn policy_no_retry_no_jitter() {
    assert!(!RetryPolicy::no_retry().jitter);
}

#[test]
fn policy_no_retry_multiplier_one() {
    let p = RetryPolicy::no_retry();
    assert!((p.backoff_multiplier - 1.0).abs() < f64::EPSILON);
}

#[test]
fn policy_clone_produces_equal_copy() {
    let p = RetryPolicy::default();
    assert_eq!(p, p.clone());
}

#[test]
fn policy_partial_eq_detects_difference() {
    let a = RetryPolicy::default();
    let b = RetryPolicy::no_retry();
    assert_ne!(a, b);
}

#[test]
fn policy_debug_contains_type_name() {
    let dbg = format!("{:?}", RetryPolicy::default());
    assert!(dbg.contains("RetryPolicy"));
}

#[test]
fn policy_new_large_retries() {
    let p = RetryPolicy::new(
        1000,
        Duration::from_nanos(1),
        Duration::from_secs(60),
        1.01,
        false,
    );
    assert_eq!(p.max_retries, 1000);
}

// ===========================================================================
// 2. Exponential backoff
// ===========================================================================

#[test]
fn exp_backoff_attempt_0_equals_base() {
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
fn exp_backoff_attempt_1_doubles() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(100),
        Duration::from_secs(60),
        2.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(1), Duration::from_millis(200));
}

#[test]
fn exp_backoff_attempt_2_quadruples() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(100),
        Duration::from_secs(60),
        2.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(2), Duration::from_millis(400));
}

#[test]
fn exp_backoff_attempt_3() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(100),
        Duration::from_secs(60),
        2.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(3), Duration::from_millis(800));
}

#[test]
fn exp_backoff_grows_monotonically() {
    let p = RetryPolicy::new(
        10,
        Duration::from_millis(50),
        Duration::from_secs(60),
        2.0,
        false,
    );
    let delays: Vec<Duration> = (0..6).map(|i| p.delay_for_attempt(i)).collect();
    for w in delays.windows(2) {
        assert!(
            w[1] >= w[0],
            "delay should not decrease: {:?} -> {:?}",
            w[0],
            w[1]
        );
    }
}

#[test]
fn exp_backoff_capped_at_max_delay() {
    let p = RetryPolicy::new(
        10,
        Duration::from_secs(1),
        Duration::from_secs(5),
        10.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(5), Duration::from_secs(5));
}

#[test]
fn exp_backoff_cap_applies_to_all_subsequent() {
    let p = RetryPolicy::new(
        10,
        Duration::from_secs(1),
        Duration::from_secs(5),
        10.0,
        false,
    );
    for i in 3..8 {
        assert_eq!(
            p.delay_for_attempt(i),
            Duration::from_secs(5),
            "attempt {i} should be capped"
        );
    }
}

#[test]
fn exp_backoff_multiplier_three() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(100),
        Duration::from_secs(600),
        3.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(0), Duration::from_millis(100));
    assert_eq!(p.delay_for_attempt(1), Duration::from_millis(300));
    assert_eq!(p.delay_for_attempt(2), Duration::from_millis(900));
    assert_eq!(p.delay_for_attempt(3), Duration::from_millis(2700));
}

// ===========================================================================
// 3. Linear backoff (multiplier = 1.0)
// ===========================================================================

#[test]
fn linear_backoff_constant_when_multiplier_one() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(200),
        Duration::from_secs(60),
        1.0,
        false,
    );
    for i in 0..5 {
        assert_eq!(
            p.delay_for_attempt(i),
            Duration::from_millis(200),
            "attempt {i} should equal base_delay with multiplier 1.0"
        );
    }
}

#[test]
fn linear_backoff_all_same_value() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(50),
        Duration::from_secs(10),
        1.0,
        false,
    );
    let d0 = p.delay_for_attempt(0);
    let d1 = p.delay_for_attempt(1);
    let d2 = p.delay_for_attempt(2);
    assert_eq!(d0, d1);
    assert_eq!(d1, d2);
}

// ===========================================================================
// 4. Constant backoff (same as linear with multiplier 1.0)
// ===========================================================================

#[test]
fn constant_backoff_fixed_delay() {
    let p = RetryPolicy::new(
        10,
        Duration::from_millis(500),
        Duration::from_secs(60),
        1.0,
        false,
    );
    let expected = Duration::from_millis(500);
    for attempt in 0..10 {
        assert_eq!(p.delay_for_attempt(attempt), expected);
    }
}

#[test]
fn constant_backoff_respects_max_delay() {
    let p = RetryPolicy::new(
        5,
        Duration::from_secs(10),
        Duration::from_secs(3),
        1.0,
        false,
    );
    // base_delay > max_delay => capped
    assert_eq!(p.delay_for_attempt(0), Duration::from_secs(3));
}

// ===========================================================================
// 5. Jitter
// ===========================================================================

#[test]
fn jitter_produces_varying_delays() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(100),
        Duration::from_secs(5),
        2.0,
        true,
    );
    let delays: Vec<Duration> = (0..30).map(|_| p.delay_for_attempt(1)).collect();
    let all_same = delays.windows(2).all(|w| w[0] == w[1]);
    assert!(
        !all_same,
        "jitter should produce varying delays across 30 samples"
    );
}

#[test]
fn jitter_delay_within_upper_bound() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(100),
        Duration::from_secs(5),
        2.0,
        true,
    );
    for _ in 0..200 {
        let d = p.delay_for_attempt(1);
        // base * 2^1 = 200ms; jitter scales [0, 200ms]
        assert!(
            d <= Duration::from_millis(200),
            "jitter delay {d:?} exceeds 200ms"
        );
    }
}

#[test]
fn jitter_delay_non_negative() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(100),
        Duration::from_secs(5),
        2.0,
        true,
    );
    for _ in 0..200 {
        let d = p.delay_for_attempt(0);
        assert!(d <= Duration::from_millis(100));
    }
}

#[test]
fn no_jitter_is_deterministic() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(100),
        Duration::from_secs(5),
        2.0,
        false,
    );
    let d1 = p.delay_for_attempt(2);
    let d2 = p.delay_for_attempt(2);
    assert_eq!(d1, d2);
}

#[test]
fn jitter_bounded_at_cap() {
    let p = RetryPolicy::new(
        10,
        Duration::from_secs(1),
        Duration::from_secs(5),
        10.0,
        true,
    );
    for _ in 0..200 {
        let d = p.delay_for_attempt(5);
        assert!(
            d <= Duration::from_secs(5),
            "jittered delay {d:?} exceeds max_delay"
        );
    }
}

#[test]
fn jitter_at_attempt_zero_bounded_by_base() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(500),
        Duration::from_secs(10),
        2.0,
        true,
    );
    for _ in 0..100 {
        let d = p.delay_for_attempt(0);
        assert!(d <= Duration::from_millis(500));
    }
}

// ===========================================================================
// 6. Max retries (via retry_with_policy)
// ===========================================================================

#[tokio::test]
async fn max_retries_zero_runs_once() {
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
async fn max_retries_one_runs_twice() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let p = RetryPolicy::new(
        1,
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
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn max_retries_five_runs_six_times() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let p = RetryPolicy::new(
        5,
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
    assert_eq!(counter.load(Ordering::SeqCst), 6);
}

#[tokio::test]
async fn retry_stops_on_first_success() {
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
            if n == 0 {
                Ok::<_, &str>("immediate")
            } else {
                Err("should not reach")
            }
        }
    })
    .await;
    assert_eq!(result.unwrap(), "immediate");
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn retry_succeeds_on_last_attempt() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let p = RetryPolicy::new(
        4,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let result = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n < 4 { Err("not yet") } else { Ok("last") }
        }
    })
    .await;
    assert_eq!(result.unwrap(), "last");
    assert_eq!(counter.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn retry_returns_last_error_on_exhaustion() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let p = RetryPolicy::new(
        2,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let result: Result<(), String> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            Err(format!("err-{n}"))
        }
    })
    .await;
    assert_eq!(result.unwrap_err(), "err-2");
}

// ===========================================================================
// 7. Timeout behavior (total elapsed time check)
// ===========================================================================

#[tokio::test]
async fn retry_with_tiny_delay_completes_quickly() {
    let start = std::time::Instant::now();
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let _: Result<(), &str> = retry_with_policy(&p, || async { Err("fail") }).await;
    let elapsed = start.elapsed();
    // 3 retries with 1ms delay each => should complete in well under 1 second
    assert!(
        elapsed < Duration::from_secs(1),
        "took too long: {elapsed:?}"
    );
}

#[tokio::test]
async fn retry_delay_is_respected() {
    let start = std::time::Instant::now();
    let p = RetryPolicy::new(
        2,
        Duration::from_millis(50),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let _: Result<(), &str> = retry_with_policy(&p, || async { Err("fail") }).await;
    let elapsed = start.elapsed();
    // At least 2 * 50ms = 100ms of sleeping
    assert!(
        elapsed >= Duration::from_millis(80),
        "expected >= 80ms, got {elapsed:?}"
    );
}

#[tokio::test]
async fn immediate_success_has_no_delay() {
    let start = std::time::Instant::now();
    let p = RetryPolicy::new(
        5,
        Duration::from_secs(10),
        Duration::from_secs(60),
        2.0,
        false,
    );
    let result = retry_with_policy(&p, || async { Ok::<_, &str>(42) }).await;
    assert_eq!(result.unwrap(), 42);
    assert!(start.elapsed() < Duration::from_millis(100));
}

// ===========================================================================
// 8. Circuit breaker states
// ===========================================================================

#[test]
fn cb_starts_closed() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[test]
fn cb_initial_failures_zero() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    assert_eq!(cb.consecutive_failures(), 0);
}

#[test]
fn cb_threshold_getter() {
    let cb = CircuitBreaker::new(5, Duration::from_secs(10));
    assert_eq!(cb.failure_threshold(), 5);
}

#[test]
fn cb_recovery_timeout_getter() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(42));
    assert_eq!(cb.recovery_timeout(), Duration::from_secs(42));
}

#[test]
fn circuit_state_closed_variant() {
    let s = CircuitState::Closed;
    assert_eq!(s, CircuitState::Closed);
}

#[test]
fn circuit_state_open_variant() {
    let s = CircuitState::Open;
    assert_eq!(s, CircuitState::Open);
}

#[test]
fn circuit_state_half_open_variant() {
    let s = CircuitState::HalfOpen;
    assert_eq!(s, CircuitState::HalfOpen);
}

#[test]
fn circuit_state_debug() {
    assert_eq!(format!("{:?}", CircuitState::Closed), "Closed");
    assert_eq!(format!("{:?}", CircuitState::Open), "Open");
    assert_eq!(format!("{:?}", CircuitState::HalfOpen), "HalfOpen");
}

#[test]
fn circuit_state_clone() {
    let s = CircuitState::Open;
    let s2 = s;
    assert_eq!(s, s2);
}

// ===========================================================================
// 9. Circuit breaker transitions
// ===========================================================================

#[tokio::test]
async fn cb_stays_closed_on_success() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    let _: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("ok".into()) }).await;
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn cb_stays_closed_below_threshold() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    for _ in 0..2 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    }
    assert_eq!(cb.consecutive_failures(), 2);
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn cb_opens_at_threshold() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    for _ in 0..3 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    }
    assert_eq!(cb.state(), CircuitState::Open);
}

#[tokio::test]
async fn cb_opens_at_threshold_one() {
    let cb = CircuitBreaker::new(1, Duration::from_secs(300));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
}

#[tokio::test]
async fn cb_rejects_when_open() {
    let cb = CircuitBreaker::new(1, Duration::from_secs(300));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    let res: Result<String, CircuitBreakerError<&str>> =
        cb.call(|| async { Ok("should not run".into()) }).await;
    assert!(matches!(res, Err(CircuitBreakerError::Open)));
}

#[tokio::test]
async fn cb_transitions_to_half_open_after_timeout() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(10));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
    tokio::time::sleep(Duration::from_millis(20)).await;
    // Next call transitions to half-open and executes probe
    let res: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("probe".into()) }).await;
    assert_eq!(res.unwrap(), "probe");
    // Successful probe closes the breaker
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn cb_closes_after_successful_probe() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(10));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    let _: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("ok".into()) }).await;
    assert_eq!(cb.state(), CircuitState::Closed);
    assert_eq!(cb.consecutive_failures(), 0);
}

#[tokio::test]
async fn cb_reopens_after_failed_probe() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(10));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("first") }).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("still broken") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
}

#[tokio::test]
async fn cb_success_resets_failure_count() {
    let cb = CircuitBreaker::new(5, Duration::from_secs(30));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.consecutive_failures(), 2);
    let _: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("ok".into()) }).await;
    assert_eq!(cb.consecutive_failures(), 0);
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn cb_failure_count_increments() {
    let cb = CircuitBreaker::new(10, Duration::from_secs(30));
    for i in 1..=5 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
        assert_eq!(cb.consecutive_failures(), i);
    }
}

#[tokio::test]
async fn cb_multiple_open_close_cycles() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(10));

    // Cycle 1: fail → open → wait → probe success → closed
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
    tokio::time::sleep(Duration::from_millis(20)).await;
    let _: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("ok".into()) }).await;
    assert_eq!(cb.state(), CircuitState::Closed);

    // Cycle 2: fail → open → wait → probe success → closed
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail again") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
    tokio::time::sleep(Duration::from_millis(20)).await;
    let _: Result<String, CircuitBreakerError<String>> = cb
        .call(|| async { Ok::<_, String>("recovered".into()) })
        .await;
    assert_eq!(cb.state(), CircuitState::Closed);
}

// ===========================================================================
// 10. Should retry / custom decision logic
// ===========================================================================

#[tokio::test]
async fn retry_with_transient_error() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let result = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n < 3 {
                Err("transient")
            } else {
                Ok("recovered")
            }
        }
    })
    .await;
    assert_eq!(result.unwrap(), "recovered");
    assert_eq!(counter.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn retry_closure_called_fresh_each_time() {
    let states = Arc::new(std::sync::Mutex::new(Vec::new()));
    let s = states.clone();
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let _: Result<(), &str> = retry_with_policy(&p, || {
        let s = s.clone();
        async move {
            s.lock().unwrap().push("called");
            Err("fail")
        }
    })
    .await;
    assert_eq!(states.lock().unwrap().len(), 4); // 1 initial + 3 retries
}

#[tokio::test]
async fn retry_with_different_error_types() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let result: Result<i32, String> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            Err(format!("error type {n}"))
        }
    })
    .await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "error type 3");
}

// ===========================================================================
// 11. Retry context: track attempt number
// ===========================================================================

#[tokio::test]
async fn retry_attempt_numbers_sequential() {
    let attempts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let a = attempts.clone();
    let p = RetryPolicy::new(
        4,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let _: Result<(), &str> = retry_with_policy(&p, || {
        let a = a.clone();
        async move {
            let mut v = a.lock().unwrap();
            let idx = v.len() as u32;
            v.push(idx);
            Err("fail")
        }
    })
    .await;
    let recorded = attempts.lock().unwrap().clone();
    assert_eq!(recorded, vec![0, 1, 2, 3, 4]);
}

#[tokio::test]
async fn retry_total_elapsed_reasonable() {
    let start = std::time::Instant::now();
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(10),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let _: Result<(), &str> = retry_with_policy(&p, || async { Err("fail") }).await;
    let elapsed = start.elapsed();
    // 3 sleeps of ~10ms = ~30ms minimum
    assert!(elapsed >= Duration::from_millis(20));
    assert!(elapsed < Duration::from_secs(2));
}

// ===========================================================================
// 12. Serde roundtrip
// ===========================================================================

#[test]
fn serde_retry_policy_default_roundtrip() {
    let p = RetryPolicy::default();
    let json = serde_json::to_string(&p).unwrap();
    let p2: RetryPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}

#[test]
fn serde_retry_policy_no_retry_roundtrip() {
    let p = RetryPolicy::no_retry();
    let json = serde_json::to_string(&p).unwrap();
    let p2: RetryPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}

#[test]
fn serde_retry_policy_custom_roundtrip() {
    let p = RetryPolicy::new(
        10,
        Duration::from_millis(500),
        Duration::from_secs(30),
        1.5,
        false,
    );
    let json = serde_json::to_string_pretty(&p).unwrap();
    let p2: RetryPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}

#[test]
fn serde_circuit_state_closed() {
    let json = serde_json::to_string(&CircuitState::Closed).unwrap();
    assert_eq!(json, r#""closed""#);
    let s: CircuitState = serde_json::from_str(&json).unwrap();
    assert_eq!(s, CircuitState::Closed);
}

#[test]
fn serde_circuit_state_open() {
    let json = serde_json::to_string(&CircuitState::Open).unwrap();
    assert_eq!(json, r#""open""#);
    let s: CircuitState = serde_json::from_str(&json).unwrap();
    assert_eq!(s, CircuitState::Open);
}

#[test]
fn serde_circuit_state_half_open_snake_case() {
    let json = serde_json::to_string(&CircuitState::HalfOpen).unwrap();
    assert_eq!(json, r#""half_open""#);
    let s: CircuitState = serde_json::from_str(&json).unwrap();
    assert_eq!(s, CircuitState::HalfOpen);
}

#[test]
fn serde_circuit_state_all_variants_roundtrip() {
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
fn serde_retry_policy_json_contains_expected_fields() {
    let p = RetryPolicy::default();
    let json = serde_json::to_string(&p).unwrap();
    assert!(json.contains("max_retries"));
    assert!(json.contains("base_delay"));
    assert!(json.contains("max_delay"));
    assert!(json.contains("backoff_multiplier"));
    assert!(json.contains("jitter"));
}

#[test]
fn serde_retry_policy_deserialize_from_manual_json() {
    let json = r#"{
        "max_retries": 5,
        "base_delay": {"secs": 0, "nanos": 200000000},
        "max_delay": {"secs": 10, "nanos": 0},
        "backoff_multiplier": 3.0,
        "jitter": true
    }"#;
    let p: RetryPolicy = serde_json::from_str(json).unwrap();
    assert_eq!(p.max_retries, 5);
    assert_eq!(p.base_delay, Duration::from_millis(200));
    assert_eq!(p.max_delay, Duration::from_secs(10));
    assert!((p.backoff_multiplier - 3.0).abs() < f64::EPSILON);
    assert!(p.jitter);
}

// ===========================================================================
// 13. Edge cases
// ===========================================================================

#[test]
fn edge_zero_base_delay_no_jitter() {
    let p = RetryPolicy::new(3, Duration::ZERO, Duration::from_secs(5), 2.0, false);
    assert_eq!(p.delay_for_attempt(0), Duration::ZERO);
    assert_eq!(p.delay_for_attempt(5), Duration::ZERO);
}

#[test]
fn edge_zero_max_delay() {
    let p = RetryPolicy::new(3, Duration::from_millis(100), Duration::ZERO, 2.0, false);
    assert_eq!(p.delay_for_attempt(0), Duration::ZERO);
}

#[test]
fn edge_multiplier_zero() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(100),
        Duration::from_secs(5),
        0.0,
        false,
    );
    // 100ms * 0^0 = 100ms * 1 = 100ms for attempt 0
    assert_eq!(p.delay_for_attempt(0), Duration::from_millis(100));
    // 100ms * 0^1 = 0ms for attempt 1
    assert_eq!(p.delay_for_attempt(1), Duration::ZERO);
}

#[test]
fn edge_very_large_multiplier() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(1),
        Duration::from_secs(5),
        1000.0,
        false,
    );
    // Should be capped at max_delay
    assert_eq!(p.delay_for_attempt(2), Duration::from_secs(5));
}

#[tokio::test]
async fn edge_immediate_success_no_retry_needed() {
    let p = RetryPolicy::new(
        100,
        Duration::from_secs(60),
        Duration::from_secs(600),
        2.0,
        false,
    );
    let result = retry_with_policy(&p, || async { Ok::<_, &str>("instant") }).await;
    assert_eq!(result.unwrap(), "instant");
}

#[tokio::test]
async fn edge_zero_retries_success() {
    let p = RetryPolicy::no_retry();
    let result = retry_with_policy(&p, || async { Ok::<_, &str>(99) }).await;
    assert_eq!(result.unwrap(), 99);
}

#[tokio::test]
async fn edge_zero_retries_failure() {
    let p = RetryPolicy::no_retry();
    let result: Result<i32, &str> = retry_with_policy(&p, || async { Err("nope") }).await;
    assert_eq!(result.unwrap_err(), "nope");
}

#[test]
fn edge_very_small_base_delay() {
    let p = RetryPolicy::new(
        5,
        Duration::from_nanos(1),
        Duration::from_secs(5),
        2.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(0), Duration::from_nanos(1));
    assert_eq!(p.delay_for_attempt(1), Duration::from_nanos(2));
}

#[test]
fn edge_base_delay_equals_max_delay() {
    let p = RetryPolicy::new(
        3,
        Duration::from_secs(5),
        Duration::from_secs(5),
        2.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(0), Duration::from_secs(5));
    assert_eq!(p.delay_for_attempt(1), Duration::from_secs(5));
}

// ===========================================================================
// CircuitBreakerError display & variants
// ===========================================================================

#[test]
fn cb_error_open_display() {
    let e: CircuitBreakerError<String> = CircuitBreakerError::Open;
    assert_eq!(e.to_string(), "circuit breaker is open");
}

#[test]
fn cb_error_inner_display() {
    let e: CircuitBreakerError<String> = CircuitBreakerError::Inner("something broke".into());
    assert_eq!(e.to_string(), "something broke");
}

#[test]
fn cb_error_debug_open() {
    let e: CircuitBreakerError<String> = CircuitBreakerError::Open;
    let dbg = format!("{e:?}");
    assert!(dbg.contains("Open"));
}

#[test]
fn cb_error_debug_inner() {
    let e: CircuitBreakerError<String> = CircuitBreakerError::Inner("x".into());
    let dbg = format!("{e:?}");
    assert!(dbg.contains("Inner"));
}

#[tokio::test]
async fn cb_inner_error_preserves_value() {
    let cb = CircuitBreaker::new(5, Duration::from_secs(30));
    let res: Result<String, CircuitBreakerError<String>> = cb
        .call(|| async { Err("detailed error".to_string()) })
        .await;
    match res {
        Err(CircuitBreakerError::Inner(e)) => assert_eq!(e, "detailed error"),
        other => panic!("expected Inner, got {other:?}"),
    }
}

// ===========================================================================
// Thread safety
// ===========================================================================

#[test]
fn retry_policy_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<RetryPolicy>();
}

#[test]
fn retry_policy_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<RetryPolicy>();
}

#[test]
fn circuit_breaker_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<CircuitBreaker>();
}

#[test]
fn circuit_breaker_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<CircuitBreaker>();
}

#[test]
fn circuit_state_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CircuitState>();
}

// ===========================================================================
// Circuit breaker debug
// ===========================================================================

#[test]
fn cb_debug_contains_type_name() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    let dbg = format!("{cb:?}");
    assert!(dbg.contains("CircuitBreaker"));
}

#[test]
fn cb_debug_contains_threshold() {
    let cb = CircuitBreaker::new(7, Duration::from_secs(30));
    let dbg = format!("{cb:?}");
    assert!(dbg.contains("7"));
}

// ===========================================================================
// Additional integration-style tests
// ===========================================================================

#[tokio::test]
async fn retry_with_exponential_backoff_timing() {
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
    // attempt 0 delay: 20ms, attempt 1 delay: 40ms => total ~60ms
    assert!(
        elapsed >= Duration::from_millis(40),
        "expected >= 40ms, got {elapsed:?}"
    );
}

#[tokio::test]
async fn cb_call_returns_ok_value() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    let res: Result<i32, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>(42) }).await;
    assert_eq!(res.unwrap(), 42);
}

#[tokio::test]
async fn cb_threshold_two_needs_two_failures() {
    let cb = CircuitBreaker::new(2, Duration::from_secs(300));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail 1") }).await;
    assert_eq!(cb.state(), CircuitState::Closed);
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail 2") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
}

#[tokio::test]
async fn cb_success_between_failures_prevents_opening() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.consecutive_failures(), 2);
    // Success resets the counter
    let _: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("ok".into()) }).await;
    assert_eq!(cb.consecutive_failures(), 0);
    // Two more failures should NOT open (need 3 consecutive)
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.state(), CircuitState::Closed);
}
