#![allow(clippy::all)]
#![allow(unknown_lints)]
//! Comprehensive tests for `abp-retry` retry policies, backoff strategies,
//! circuit breaker state transitions, and combined resilience patterns.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use abp_retry::{
    retry_with_policy, CircuitBreaker, CircuitBreakerError, CircuitState, RetryPolicy,
};

// ===========================================================================
// 1. RetryPolicy — construction & field access
// ===========================================================================

#[test]
fn policy_new_stores_all_fields() {
    let p = RetryPolicy::new(
        7,
        Duration::from_millis(250),
        Duration::from_secs(30),
        1.5,
        true,
    );
    assert_eq!(p.max_retries, 7);
    assert_eq!(p.base_delay, Duration::from_millis(250));
    assert_eq!(p.max_delay, Duration::from_secs(30));
    assert!((p.backoff_multiplier - 1.5).abs() < f64::EPSILON);
    assert!(p.jitter);
}

#[test]
fn policy_default_values() {
    let p = RetryPolicy::default();
    assert_eq!(p.max_retries, 3);
    assert_eq!(p.base_delay, Duration::from_millis(100));
    assert_eq!(p.max_delay, Duration::from_secs(5));
    assert!((p.backoff_multiplier - 2.0).abs() < f64::EPSILON);
    assert!(p.jitter);
}

#[test]
fn policy_no_retry_zeroes_everything() {
    let p = RetryPolicy::no_retry();
    assert_eq!(p.max_retries, 0);
    assert_eq!(p.base_delay, Duration::ZERO);
    assert_eq!(p.max_delay, Duration::ZERO);
    assert!((p.backoff_multiplier - 1.0).abs() < f64::EPSILON);
    assert!(!p.jitter);
}

#[test]
fn policy_max_retries_one() {
    let p = RetryPolicy::new(
        1,
        Duration::from_millis(10),
        Duration::from_secs(1),
        2.0,
        false,
    );
    assert_eq!(p.max_retries, 1);
}

#[test]
fn policy_with_zero_base_delay() {
    let p = RetryPolicy::new(3, Duration::ZERO, Duration::from_secs(1), 2.0, false);
    assert_eq!(p.base_delay, Duration::ZERO);
    // delay_for_attempt should return zero since base is zero
    assert_eq!(p.delay_for_attempt(0), Duration::ZERO);
    assert_eq!(p.delay_for_attempt(5), Duration::ZERO);
}

#[test]
fn policy_with_very_large_max_retries() {
    let p = RetryPolicy::new(
        u32::MAX,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    assert_eq!(p.max_retries, u32::MAX);
}

#[test]
fn policy_with_multiplier_one_gives_fixed_delay() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(100),
        Duration::from_secs(60),
        1.0,
        false,
    );
    for attempt in 0..5 {
        assert_eq!(p.delay_for_attempt(attempt), Duration::from_millis(100));
    }
}

#[test]
fn policy_clone_is_independent() {
    let p1 = RetryPolicy::new(
        5,
        Duration::from_millis(200),
        Duration::from_secs(10),
        3.0,
        true,
    );
    let mut p2 = p1.clone();
    p2.max_retries = 10;
    assert_eq!(p1.max_retries, 5);
    assert_eq!(p2.max_retries, 10);
}

#[test]
fn policy_partial_eq() {
    let a = RetryPolicy::default();
    let b = RetryPolicy::default();
    assert_eq!(a, b);

    let c = RetryPolicy::no_retry();
    assert_ne!(a, c);
}

#[test]
fn policy_debug_contains_fields() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(200),
        Duration::from_secs(10),
        3.0,
        false,
    );
    let dbg = format!("{:?}", p);
    assert!(dbg.contains("max_retries"));
    assert!(dbg.contains("base_delay"));
    assert!(dbg.contains("backoff_multiplier"));
}

// ===========================================================================
// 2. Backoff strategies — fixed, exponential, capped
// ===========================================================================

#[test]
fn fixed_backoff_multiplier_one() {
    let p = RetryPolicy::new(
        10,
        Duration::from_millis(50),
        Duration::from_secs(60),
        1.0,
        false,
    );
    for i in 0..10 {
        assert_eq!(p.delay_for_attempt(i), Duration::from_millis(50));
    }
}

#[test]
fn exponential_backoff_doubles() {
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
fn exponential_backoff_triples() {
    let p = RetryPolicy::new(
        4,
        Duration::from_millis(10),
        Duration::from_secs(60),
        3.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(0), Duration::from_millis(10));
    assert_eq!(p.delay_for_attempt(1), Duration::from_millis(30));
    assert_eq!(p.delay_for_attempt(2), Duration::from_millis(90));
    assert_eq!(p.delay_for_attempt(3), Duration::from_millis(270));
}

#[test]
fn backoff_capped_at_max_delay() {
    let p = RetryPolicy::new(
        10,
        Duration::from_secs(1),
        Duration::from_secs(5),
        10.0,
        false,
    );
    // attempt 0: 1s, attempt 1: 10s -> capped at 5s
    assert_eq!(p.delay_for_attempt(0), Duration::from_secs(1));
    assert_eq!(p.delay_for_attempt(1), Duration::from_secs(5));
    assert_eq!(p.delay_for_attempt(2), Duration::from_secs(5));
    assert_eq!(p.delay_for_attempt(5), Duration::from_secs(5));
}

#[test]
fn backoff_cap_reached_exactly() {
    // 100ms * 2^6 = 6400ms; max = 6400ms -> exactly at cap
    let p = RetryPolicy::new(
        10,
        Duration::from_millis(100),
        Duration::from_millis(6400),
        2.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(6), Duration::from_millis(6400));
}

#[test]
fn backoff_cap_exceeded() {
    let p = RetryPolicy::new(
        10,
        Duration::from_millis(100),
        Duration::from_millis(500),
        2.0,
        false,
    );
    // attempt 3: 100 * 8 = 800ms -> capped at 500ms
    assert_eq!(p.delay_for_attempt(3), Duration::from_millis(500));
}

#[test]
fn backoff_monotonically_nondecreasing_without_jitter() {
    let p = RetryPolicy::new(
        20,
        Duration::from_millis(10),
        Duration::from_secs(60),
        2.0,
        false,
    );
    let mut prev = Duration::ZERO;
    for i in 0..20 {
        let d = p.delay_for_attempt(i);
        assert!(d >= prev, "delay should be nondecreasing: attempt {i}");
        prev = d;
    }
}

#[test]
fn backoff_with_fractional_multiplier() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(1000),
        Duration::from_secs(60),
        1.5,
        false,
    );
    assert_eq!(p.delay_for_attempt(0), Duration::from_millis(1000));
    assert_eq!(p.delay_for_attempt(1), Duration::from_millis(1500));
    assert_eq!(p.delay_for_attempt(2), Duration::from_millis(2250));
}

#[test]
fn backoff_with_very_small_base_delay() {
    let p = RetryPolicy::new(
        5,
        Duration::from_nanos(1),
        Duration::from_secs(1),
        2.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(0), Duration::from_nanos(1));
    assert_eq!(p.delay_for_attempt(1), Duration::from_nanos(2));
}

// ===========================================================================
// 3. Jitter behaviour
// ===========================================================================

#[test]
fn jitter_delays_bounded_by_computed_max() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(100),
        Duration::from_secs(60),
        2.0,
        true,
    );
    for _ in 0..200 {
        let d = p.delay_for_attempt(2);
        // base * 2^2 = 400ms; with jitter in [0, 400ms]
        assert!(
            d <= Duration::from_millis(400),
            "jitter exceeded computed max: {:?}",
            d
        );
    }
}

#[test]
fn jitter_produces_variation_over_many_samples() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(100),
        Duration::from_secs(60),
        2.0,
        true,
    );
    let delays: Vec<Duration> = (0..50).map(|_| p.delay_for_attempt(1)).collect();
    let unique_count = {
        let mut sorted = delays.clone();
        sorted.sort();
        sorted.dedup();
        sorted.len()
    };
    assert!(
        unique_count > 1,
        "jitter should produce multiple distinct values"
    );
}

#[test]
fn no_jitter_is_deterministic_across_calls() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(100),
        Duration::from_secs(60),
        2.0,
        false,
    );
    let delays: Vec<Duration> = (0..20).map(|_| p.delay_for_attempt(2)).collect();
    assert!(delays.iter().all(|d| *d == Duration::from_millis(400)));
}

#[test]
fn jitter_bounded_at_max_delay_cap() {
    let p = RetryPolicy::new(
        10,
        Duration::from_secs(1),
        Duration::from_secs(5),
        10.0,
        true,
    );
    for _ in 0..200 {
        let d = p.delay_for_attempt(3);
        assert!(
            d <= Duration::from_secs(5),
            "jitter should respect max_delay cap"
        );
    }
}

#[test]
fn jitter_zero_base_is_still_zero() {
    let p = RetryPolicy::new(3, Duration::ZERO, Duration::from_secs(60), 2.0, true);
    for _ in 0..50 {
        assert_eq!(p.delay_for_attempt(0), Duration::ZERO);
    }
}

// ===========================================================================
// 4. retry_with_policy — async execution
// ===========================================================================

#[tokio::test]
async fn retry_immediate_success() {
    let p = RetryPolicy::default();
    let calls = Arc::new(AtomicU32::new(0));
    let c = calls.clone();
    let res = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok::<_, String>(42)
        }
    })
    .await;
    assert_eq!(res.unwrap(), 42);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn retry_recovers_after_two_failures() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let calls = Arc::new(AtomicU32::new(0));
    let c = calls.clone();
    let res = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                Err("transient")
            } else {
                Ok("recovered")
            }
        }
    })
    .await;
    assert_eq!(res.unwrap(), "recovered");
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn retry_exhausts_all_attempts_returns_last_error() {
    let p = RetryPolicy::new(
        4,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let calls = Arc::new(AtomicU32::new(0));
    let c = calls.clone();
    let res: Result<(), String> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            Err(format!("error-{}", n))
        }
    })
    .await;
    // 1 initial + 4 retries = 5 calls
    assert_eq!(calls.load(Ordering::SeqCst), 5);
    assert_eq!(res.unwrap_err(), "error-4");
}

#[tokio::test]
async fn retry_no_retry_single_attempt_success() {
    let p = RetryPolicy::no_retry();
    let res = retry_with_policy(&p, || async { Ok::<_, String>("once") }).await;
    assert_eq!(res.unwrap(), "once");
}

#[tokio::test]
async fn retry_no_retry_single_attempt_failure() {
    let p = RetryPolicy::no_retry();
    let calls = Arc::new(AtomicU32::new(0));
    let c = calls.clone();
    let res: Result<(), &str> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err("fail")
        }
    })
    .await;
    assert!(res.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn retry_succeeds_on_exact_last_attempt() {
    let max = 3;
    let p = RetryPolicy::new(
        max,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let calls = Arc::new(AtomicU32::new(0));
    let c = calls.clone();
    let res = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n < max {
                Err("not yet")
            } else {
                Ok("last-chance")
            }
        }
    })
    .await;
    assert_eq!(res.unwrap(), "last-chance");
    assert_eq!(calls.load(Ordering::SeqCst), max + 1);
}

#[tokio::test]
async fn retry_with_complex_return_type() {
    let p = RetryPolicy::new(
        2,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let calls = Arc::new(AtomicU32::new(0));
    let c = calls.clone();
    let res: Result<Vec<i32>, String> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n < 1 {
                Err("transient".to_string())
            } else {
                Ok(vec![1, 2, 3])
            }
        }
    })
    .await;
    assert_eq!(res.unwrap(), vec![1, 2, 3]);
}

#[tokio::test]
async fn retry_tracks_attempt_count_accurately() {
    let max_retries = 5;
    let p = RetryPolicy::new(
        max_retries,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let calls = Arc::new(AtomicU32::new(0));
    let c = calls.clone();
    let _: Result<(), &str> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err("always fail")
        }
    })
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), max_retries + 1);
}

#[tokio::test]
async fn retry_respects_backoff_delay() {
    // Use a measurable delay to verify timing
    let p = RetryPolicy::new(
        1,
        Duration::from_millis(50),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let start = Instant::now();
    let calls = Arc::new(AtomicU32::new(0));
    let c = calls.clone();
    let _: Result<(), &str> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err("fail")
        }
    })
    .await;
    let elapsed = start.elapsed();
    assert!(
        elapsed >= Duration::from_millis(40),
        "should have waited at least ~50ms between attempts, got {:?}",
        elapsed
    );
}

#[tokio::test]
async fn retry_with_unit_return() {
    let p = RetryPolicy::no_retry();
    let res: Result<(), String> = retry_with_policy(&p, || async { Ok(()) }).await;
    assert!(res.is_ok());
}

// ===========================================================================
// 5. Maximum retry limits
// ===========================================================================

#[tokio::test]
async fn max_retries_zero_means_single_attempt() {
    let p = RetryPolicy::new(
        0,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let calls = Arc::new(AtomicU32::new(0));
    let c = calls.clone();
    let _: Result<(), &str> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err("fail")
        }
    })
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn max_retries_one_means_two_attempts() {
    let p = RetryPolicy::new(
        1,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let calls = Arc::new(AtomicU32::new(0));
    let c = calls.clone();
    let _: Result<(), &str> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err("fail")
        }
    })
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn max_retries_ten_means_eleven_attempts() {
    let p = RetryPolicy::new(
        10,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let calls = Arc::new(AtomicU32::new(0));
    let c = calls.clone();
    let _: Result<(), &str> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err("fail")
        }
    })
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 11);
}

// ===========================================================================
// 6. CircuitBreaker — construction & getters
// ===========================================================================

#[test]
fn cb_starts_closed() {
    let cb = CircuitBreaker::new(5, Duration::from_secs(30));
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[test]
fn cb_initial_failure_count_zero() {
    let cb = CircuitBreaker::new(5, Duration::from_secs(30));
    assert_eq!(cb.consecutive_failures(), 0);
}

#[test]
fn cb_threshold_getter() {
    let cb = CircuitBreaker::new(7, Duration::from_secs(15));
    assert_eq!(cb.failure_threshold(), 7);
}

#[test]
fn cb_recovery_timeout_getter() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(60));
    assert_eq!(cb.recovery_timeout(), Duration::from_secs(60));
}

#[test]
fn cb_debug_output_contains_struct_name() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    let dbg = format!("{:?}", cb);
    assert!(dbg.contains("CircuitBreaker"));
    assert!(dbg.contains("failure_threshold"));
}

// ===========================================================================
// 7. CircuitBreaker — state transitions: closed → open → half-open → closed
// ===========================================================================

#[tokio::test]
async fn cb_stays_closed_on_success() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    let _: Result<_, CircuitBreakerError<String>> = cb
        .call(|| async { Ok::<String, String>("ok".to_string()) })
        .await;
    assert_eq!(cb.state(), CircuitState::Closed);
    assert_eq!(cb.consecutive_failures(), 0);
}

#[tokio::test]
async fn cb_stays_closed_below_threshold() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("e1") }).await;
    assert_eq!(cb.state(), CircuitState::Closed);
    assert_eq!(cb.consecutive_failures(), 1);

    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("e2") }).await;
    assert_eq!(cb.state(), CircuitState::Closed);
    assert_eq!(cb.consecutive_failures(), 2);
}

#[tokio::test]
async fn cb_opens_at_threshold() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    for _ in 0..3 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    }
    assert_eq!(cb.state(), CircuitState::Open);
    assert_eq!(cb.consecutive_failures(), 3);
}

#[tokio::test]
async fn cb_opens_with_threshold_one() {
    let cb = CircuitBreaker::new(1, Duration::from_secs(30));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
}

#[tokio::test]
async fn cb_rejects_when_open() {
    let cb = CircuitBreaker::new(1, Duration::from_secs(300));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;

    let called = Arc::new(AtomicBool::new(false));
    let c = called.clone();
    let res: Result<String, CircuitBreakerError<String>> = cb
        .call(|| {
            let c = c.clone();
            async move {
                c.store(true, Ordering::SeqCst);
                Ok("should not reach".into())
            }
        })
        .await;
    assert!(matches!(res, Err(CircuitBreakerError::Open)));
    assert!(
        !called.load(Ordering::SeqCst),
        "function should not be called when open"
    );
}

#[tokio::test]
async fn cb_transitions_to_half_open_after_timeout() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(10));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);

    tokio::time::sleep(Duration::from_millis(20)).await;

    // Next call should transition to half-open internally, then succeed → closed
    let res: Result<String, CircuitBreakerError<String>> = cb
        .call(|| async { Ok::<_, String>("probe-ok".into()) })
        .await;
    assert_eq!(res.unwrap(), "probe-ok");
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn cb_half_open_success_closes() {
    let cb = CircuitBreaker::new(2, Duration::from_millis(10));
    for _ in 0..2 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    }
    assert_eq!(cb.state(), CircuitState::Open);

    tokio::time::sleep(Duration::from_millis(20)).await;

    let _: Result<String, CircuitBreakerError<String>> = cb
        .call(|| async { Ok::<String, String>("ok".to_string()) })
        .await;
    assert_eq!(cb.state(), CircuitState::Closed);
    assert_eq!(cb.consecutive_failures(), 0);
}

#[tokio::test]
async fn cb_half_open_failure_reopens() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(10));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);

    tokio::time::sleep(Duration::from_millis(20)).await;

    let _: Result<String, _> = cb
        .call(|| async { Err::<String, _>("still failing") })
        .await;
    assert_eq!(cb.state(), CircuitState::Open);
}

#[tokio::test]
async fn cb_full_cycle_closed_open_halfopen_closed() {
    let cb = CircuitBreaker::new(2, Duration::from_millis(10));

    // Phase 1: closed, accumulate failures
    for _ in 0..2 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    }
    assert_eq!(cb.state(), CircuitState::Open);

    // Phase 2: wait for recovery timeout
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Phase 3: half-open probe succeeds → closed
    let res: Result<String, CircuitBreakerError<String>> = cb
        .call(|| async { Ok::<_, String>("recovered".into()) })
        .await;
    assert_eq!(res.unwrap(), "recovered");
    assert_eq!(cb.state(), CircuitState::Closed);
    assert_eq!(cb.consecutive_failures(), 0);

    // Phase 4: can call again normally
    let res: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("normal".into()) }).await;
    assert_eq!(res.unwrap(), "normal");
}

#[tokio::test]
async fn cb_multiple_reopen_cycles() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(10));

    for _cycle in 0..3 {
        // Trip the breaker
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
        assert_eq!(cb.state(), CircuitState::Open);

        // Wait for recovery
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Probe fails → stays open
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("still bad") }).await;
        assert_eq!(cb.state(), CircuitState::Open);

        // Wait again
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Probe succeeds → closes
        let _: Result<String, CircuitBreakerError<String>> = cb
            .call(|| async { Ok::<String, String>("ok".to_string()) })
            .await;
        assert_eq!(cb.state(), CircuitState::Closed);
    }
}

// ===========================================================================
// 8. CircuitBreaker — failure counting and reset
// ===========================================================================

#[tokio::test]
async fn cb_failure_count_increments() {
    let cb = CircuitBreaker::new(10, Duration::from_secs(30));
    for expected in 1..=5 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
        assert_eq!(cb.consecutive_failures(), expected);
    }
}

#[tokio::test]
async fn cb_success_resets_failure_count() {
    let cb = CircuitBreaker::new(10, Duration::from_secs(30));
    for _ in 0..5 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    }
    assert_eq!(cb.consecutive_failures(), 5);

    let _: Result<_, CircuitBreakerError<String>> = cb
        .call(|| async { Ok::<String, String>("ok".to_string()) })
        .await;
    assert_eq!(cb.consecutive_failures(), 0);
}

#[tokio::test]
async fn cb_interleaved_success_failure_resets() {
    let cb = CircuitBreaker::new(10, Duration::from_secs(30));

    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("f1") }).await;
    assert_eq!(cb.consecutive_failures(), 1);

    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("f2") }).await;
    assert_eq!(cb.consecutive_failures(), 2);

    // Success resets
    let _: Result<_, CircuitBreakerError<String>> = cb
        .call(|| async { Ok::<String, String>("ok".to_string()) })
        .await;
    assert_eq!(cb.consecutive_failures(), 0);

    // Failure starts from zero again
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("f3") }).await;
    assert_eq!(cb.consecutive_failures(), 1);
}

// ===========================================================================
// 9. CircuitBreakerError variants
// ===========================================================================

#[test]
fn cb_error_open_display() {
    let e: CircuitBreakerError<String> = CircuitBreakerError::Open;
    assert_eq!(e.to_string(), "circuit breaker is open");
}

#[test]
fn cb_error_inner_display() {
    let e: CircuitBreakerError<String> = CircuitBreakerError::Inner("connection refused".into());
    assert_eq!(e.to_string(), "connection refused");
}

#[test]
fn cb_error_open_debug() {
    let e: CircuitBreakerError<String> = CircuitBreakerError::Open;
    let dbg = format!("{:?}", e);
    assert!(dbg.contains("Open"));
}

#[test]
fn cb_error_inner_debug() {
    let e: CircuitBreakerError<i32> = CircuitBreakerError::Inner(42);
    let dbg = format!("{:?}", e);
    assert!(dbg.contains("Inner"));
    assert!(dbg.contains("42"));
}

#[tokio::test]
async fn cb_propagates_inner_error() {
    let cb = CircuitBreaker::new(10, Duration::from_secs(30));
    let res: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Err("my-error".to_string()) }).await;
    match res {
        Err(CircuitBreakerError::Inner(e)) => assert_eq!(e, "my-error"),
        other => panic!("expected Inner, got {:?}", other),
    }
}

#[tokio::test]
async fn cb_open_error_does_not_call_function() {
    let cb = CircuitBreaker::new(1, Duration::from_secs(300));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;

    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let _: Result<String, CircuitBreakerError<String>> = cb
        .call(|| {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok("nope".into())
            }
        })
        .await;
    assert_eq!(counter.load(Ordering::SeqCst), 0);
}

// ===========================================================================
// 10. CircuitState enum
// ===========================================================================

#[test]
fn circuit_state_clone_copy() {
    let s = CircuitState::Closed;
    let s2 = s;
    assert_eq!(s, s2);
}

#[test]
fn circuit_state_all_variants_debug() {
    for state in [
        CircuitState::Closed,
        CircuitState::Open,
        CircuitState::HalfOpen,
    ] {
        let dbg = format!("{:?}", state);
        assert!(!dbg.is_empty());
    }
}

#[test]
fn circuit_state_eq() {
    assert_eq!(CircuitState::Closed, CircuitState::Closed);
    assert_eq!(CircuitState::Open, CircuitState::Open);
    assert_eq!(CircuitState::HalfOpen, CircuitState::HalfOpen);
    assert_ne!(CircuitState::Closed, CircuitState::Open);
    assert_ne!(CircuitState::Open, CircuitState::HalfOpen);
    assert_ne!(CircuitState::Closed, CircuitState::HalfOpen);
}

// ===========================================================================
// 11. Serialization roundtrips
// ===========================================================================

#[test]
fn retry_policy_serde_roundtrip_default() {
    let p = RetryPolicy::default();
    let json = serde_json::to_string(&p).unwrap();
    let p2: RetryPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}

#[test]
fn retry_policy_serde_roundtrip_no_retry() {
    let p = RetryPolicy::no_retry();
    let json = serde_json::to_string(&p).unwrap();
    let p2: RetryPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}

#[test]
fn retry_policy_serde_roundtrip_custom() {
    let p = RetryPolicy::new(
        10,
        Duration::from_millis(500),
        Duration::from_secs(60),
        4.0,
        true,
    );
    let json = serde_json::to_string(&p).unwrap();
    let p2: RetryPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}

#[test]
fn circuit_state_serde_closed() {
    let json = serde_json::to_string(&CircuitState::Closed).unwrap();
    assert_eq!(json, r#""closed""#);
    let s: CircuitState = serde_json::from_str(&json).unwrap();
    assert_eq!(s, CircuitState::Closed);
}

#[test]
fn circuit_state_serde_open() {
    let json = serde_json::to_string(&CircuitState::Open).unwrap();
    assert_eq!(json, r#""open""#);
    let s: CircuitState = serde_json::from_str(&json).unwrap();
    assert_eq!(s, CircuitState::Open);
}

#[test]
fn circuit_state_serde_half_open() {
    let json = serde_json::to_string(&CircuitState::HalfOpen).unwrap();
    assert_eq!(json, r#""half_open""#);
    let s: CircuitState = serde_json::from_str(&json).unwrap();
    assert_eq!(s, CircuitState::HalfOpen);
}

#[test]
fn retry_policy_serde_pretty_roundtrip() {
    let p = RetryPolicy::new(
        2,
        Duration::from_millis(100),
        Duration::from_secs(10),
        2.5,
        false,
    );
    let json = serde_json::to_string_pretty(&p).unwrap();
    let p2: RetryPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(p, p2);
}

// ===========================================================================
// 12. Thread safety markers
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

#[test]
fn circuit_breaker_error_is_send_when_inner_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<CircuitBreakerError<String>>();
}

// ===========================================================================
// 13. Error classification — retryable vs non-retryable patterns
// ===========================================================================

#[derive(Debug, Clone)]
enum AppError {
    Transient(String),
    Permanent(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Transient(msg) => write!(f, "transient: {}", msg),
            AppError::Permanent(msg) => write!(f, "permanent: {}", msg),
        }
    }
}

#[tokio::test]
async fn retry_transient_errors_eventually_succeed() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let calls = Arc::new(AtomicU32::new(0));
    let c = calls.clone();
    let res: Result<String, AppError> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n < 3 {
                Err(AppError::Transient("network timeout".into()))
            } else {
                Ok("success".into())
            }
        }
    })
    .await;
    assert!(res.is_ok());
}

#[tokio::test]
async fn retry_permanent_errors_exhaust_attempts() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let res: Result<String, AppError> = retry_with_policy(&p, || async {
        Err(AppError::Permanent("auth failed".into()))
    })
    .await;
    match res {
        Err(AppError::Permanent(msg)) => assert_eq!(msg, "auth failed"),
        other => panic!("expected Permanent error, got {:?}", other),
    }
}

#[tokio::test]
async fn retry_mixed_error_types() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let calls = Arc::new(AtomicU32::new(0));
    let c = calls.clone();
    let res: Result<String, AppError> = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            match n {
                0 => Err(AppError::Transient("timeout".into())),
                1 => Err(AppError::Transient("conn reset".into())),
                _ => Ok("ok".into()),
            }
        }
    })
    .await;
    assert_eq!(res.unwrap(), "ok");
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

// ===========================================================================
// 14. Combined retry + circuit breaker patterns
// ===========================================================================

#[tokio::test]
async fn retry_through_circuit_breaker_success() {
    let cb = Arc::new(CircuitBreaker::new(5, Duration::from_secs(30)));
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );

    let cb_ref = cb.clone();
    let res: Result<String, CircuitBreakerError<String>> = retry_with_policy(&p, || {
        let cb = cb_ref.clone();
        async move {
            cb.call(|| async { Ok::<String, String>("ok".to_string()) })
                .await
        }
    })
    .await;
    assert_eq!(res.unwrap(), "ok");
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn retry_through_circuit_breaker_eventual_success() {
    let cb = Arc::new(CircuitBreaker::new(10, Duration::from_secs(30)));
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let calls = Arc::new(AtomicU32::new(0));

    let cb_ref = cb.clone();
    let c = calls.clone();
    let res: Result<String, CircuitBreakerError<String>> = retry_with_policy(&p, || {
        let cb = cb_ref.clone();
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            cb.call(|| async move {
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
    assert_eq!(res.unwrap(), "recovered");
}

#[tokio::test]
async fn retry_stops_when_circuit_opens() {
    let cb = Arc::new(CircuitBreaker::new(2, Duration::from_secs(300)));
    let p = RetryPolicy::new(
        10,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let fn_calls = Arc::new(AtomicU32::new(0));

    let cb_ref = cb.clone();
    let c = fn_calls.clone();
    let res: Result<String, CircuitBreakerError<String>> = retry_with_policy(&p, || {
        let cb = cb_ref.clone();
        let c = c.clone();
        async move {
            cb.call(|| {
                let c = c.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Err::<String, _>("fail".to_string())
                }
            })
            .await
        }
    })
    .await;
    assert!(res.is_err());
    // The function should be called exactly 2 times (threshold) then circuit opens
    // and subsequent retries get CircuitBreakerError::Open without calling the function
    assert_eq!(fn_calls.load(Ordering::SeqCst), 2);
}

// ===========================================================================
// 15. Timing behaviour
// ===========================================================================

#[tokio::test]
async fn retry_no_delay_on_last_failed_attempt() {
    // With no_retry, should return immediately after single failure
    let p = RetryPolicy::no_retry();
    let start = Instant::now();
    let _: Result<(), &str> = retry_with_policy(&p, || async { Err("fail") }).await;
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(50),
        "no_retry should not delay"
    );
}

#[tokio::test]
async fn retry_exponential_delay_increases() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(20),
        Duration::from_secs(5),
        2.0,
        false,
    );
    let timestamps = Arc::new(std::sync::Mutex::new(Vec::new()));

    let ts = timestamps.clone();
    let _: Result<(), &str> = retry_with_policy(&p, || {
        let ts = ts.clone();
        async move {
            ts.lock().unwrap().push(Instant::now());
            Err("fail")
        }
    })
    .await;

    let ts = timestamps.lock().unwrap();
    assert_eq!(ts.len(), 4); // 1 initial + 3 retries

    // Check delays are roughly increasing
    if ts.len() >= 3 {
        let d01 = ts[1].duration_since(ts[0]);
        let d12 = ts[2].duration_since(ts[1]);
        // d12 should be roughly 2x d01 (within tolerance)
        assert!(
            d12 >= d01,
            "delay should increase: d01={:?}, d12={:?}",
            d01,
            d12
        );
    }
}

// ===========================================================================
// 16. Edge cases
// ===========================================================================

#[test]
fn delay_for_attempt_zero_with_high_multiplier() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(1),
        Duration::from_secs(60),
        100.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(0), Duration::from_millis(1));
}

#[test]
fn delay_for_attempt_high_number_capped() {
    let p = RetryPolicy::new(
        100,
        Duration::from_millis(1),
        Duration::from_secs(1),
        2.0,
        false,
    );
    // 2^30 would overflow millis, but should be capped at 1s
    let d = p.delay_for_attempt(30);
    assert_eq!(d, Duration::from_secs(1));
}

#[test]
fn policy_with_max_delay_less_than_base() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(500),
        Duration::from_millis(100),
        2.0,
        false,
    );
    // Even attempt 0: 500ms > max 100ms, should be capped
    let d = p.delay_for_attempt(0);
    assert_eq!(d, Duration::from_millis(100));
}

#[tokio::test]
async fn cb_threshold_two_exact_boundary() {
    let cb = CircuitBreaker::new(2, Duration::from_secs(30));
    // 1 failure: still closed
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("f1") }).await;
    assert_eq!(cb.state(), CircuitState::Closed);
    // 2nd failure: opens
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("f2") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
}

#[tokio::test]
async fn cb_success_between_failures_prevents_opening() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(30));
    // 2 failures
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("f1") }).await;
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("f2") }).await;
    assert_eq!(cb.consecutive_failures(), 2);

    // Success resets counter
    let _: Result<_, CircuitBreakerError<String>> = cb
        .call(|| async { Ok::<String, String>("ok".to_string()) })
        .await;
    assert_eq!(cb.consecutive_failures(), 0);

    // 2 more failures shouldn't open (need 3 consecutive)
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("f3") }).await;
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("f4") }).await;
    assert_eq!(cb.state(), CircuitState::Closed);
    assert_eq!(cb.consecutive_failures(), 2);
}

#[tokio::test]
async fn cb_open_rejects_immediately_before_timeout() {
    let cb = CircuitBreaker::new(1, Duration::from_secs(300));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;

    let start = Instant::now();
    let res: Result<String, CircuitBreakerError<String>> =
        cb.call(|| async { Ok::<_, String>("nope".into()) }).await;
    let elapsed = start.elapsed();

    assert!(matches!(res, Err(CircuitBreakerError::Open)));
    assert!(
        elapsed < Duration::from_millis(10),
        "open rejection should be near-instant"
    );
}

// ===========================================================================
// 17. Async function variety
// ===========================================================================

#[tokio::test]
async fn retry_with_async_sleep_in_closure() {
    let p = RetryPolicy::new(
        2,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let calls = Arc::new(AtomicU32::new(0));
    let c = calls.clone();
    let res = retry_with_policy(&p, || {
        let c = c.clone();
        async move {
            tokio::time::sleep(Duration::from_millis(1)).await;
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n < 1 {
                Err("need more time")
            } else {
                Ok("done")
            }
        }
    })
    .await;
    assert_eq!(res.unwrap(), "done");
}

#[tokio::test]
async fn retry_closure_captures_shared_state() {
    let p = RetryPolicy::new(
        3,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let shared = Arc::new(AtomicU64::new(0));
    let s = shared.clone();
    let res = retry_with_policy(&p, || {
        let s = s.clone();
        async move {
            let val = s.fetch_add(10, Ordering::SeqCst);
            if val < 20 {
                Err("accumulating")
            } else {
                Ok(val + 10) // should be 30
            }
        }
    })
    .await;
    assert_eq!(res.unwrap(), 30);
    assert_eq!(shared.load(Ordering::SeqCst), 30);
}

#[tokio::test]
async fn retry_returns_complex_error_type() {
    #[derive(Debug)]
    struct DetailedError {
        code: u32,
        message: String,
    }

    let p = RetryPolicy::new(
        1,
        Duration::from_millis(1),
        Duration::from_secs(1),
        1.0,
        false,
    );
    let res: Result<(), DetailedError> = retry_with_policy(&p, || async {
        Err(DetailedError {
            code: 503,
            message: "service unavailable".into(),
        })
    })
    .await;
    let err = res.unwrap_err();
    assert_eq!(err.code, 503);
    assert_eq!(err.message, "service unavailable");
}

// ===========================================================================
// 18. Policy configuration variants
// ===========================================================================

#[test]
fn policy_very_small_delays() {
    let p = RetryPolicy::new(
        3,
        Duration::from_nanos(100),
        Duration::from_micros(10),
        2.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(0), Duration::from_nanos(100));
    assert_eq!(p.delay_for_attempt(1), Duration::from_nanos(200));
}

#[test]
fn policy_very_large_max_delay() {
    let p = RetryPolicy::new(
        3,
        Duration::from_secs(1),
        Duration::from_secs(3600),
        2.0,
        false,
    );
    assert_eq!(p.delay_for_attempt(0), Duration::from_secs(1));
    assert_eq!(p.delay_for_attempt(10), Duration::from_secs(1024));
}

#[test]
fn policy_multiplier_less_than_one_decreasing() {
    let p = RetryPolicy::new(
        5,
        Duration::from_millis(1000),
        Duration::from_secs(60),
        0.5,
        false,
    );
    // attempt 0: 1000ms, attempt 1: 500ms, attempt 2: 250ms
    assert_eq!(p.delay_for_attempt(0), Duration::from_millis(1000));
    assert_eq!(p.delay_for_attempt(1), Duration::from_millis(500));
    assert_eq!(p.delay_for_attempt(2), Duration::from_millis(250));
}

#[test]
fn policy_fields_are_public_and_settable() {
    let mut p = RetryPolicy::default();
    p.max_retries = 10;
    p.base_delay = Duration::from_secs(1);
    p.max_delay = Duration::from_secs(60);
    p.backoff_multiplier = 3.0;
    p.jitter = false;

    assert_eq!(p.max_retries, 10);
    assert_eq!(p.base_delay, Duration::from_secs(1));
    assert_eq!(p.max_delay, Duration::from_secs(60));
    assert!((p.backoff_multiplier - 3.0).abs() < f64::EPSILON);
    assert!(!p.jitter);
}

// ===========================================================================
// 19. CircuitBreaker with different thresholds
// ===========================================================================

#[tokio::test]
async fn cb_high_threshold_stays_closed_longer() {
    let cb = CircuitBreaker::new(10, Duration::from_secs(30));
    for _ in 0..9 {
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    }
    assert_eq!(cb.state(), CircuitState::Closed);
    assert_eq!(cb.consecutive_failures(), 9);

    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("last straw") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
}

#[tokio::test]
async fn cb_threshold_one_opens_immediately() {
    let cb = CircuitBreaker::new(1, Duration::from_secs(30));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("boom") }).await;
    assert_eq!(cb.state(), CircuitState::Open);
    assert_eq!(cb.consecutive_failures(), 1);
}

// ===========================================================================
// 20. CircuitBreaker recovery timeout variants
// ===========================================================================

#[tokio::test]
async fn cb_very_short_recovery_timeout() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(1));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);

    tokio::time::sleep(Duration::from_millis(10)).await;

    let res: Result<String, CircuitBreakerError<String>> = cb
        .call(|| async { Ok::<_, String>("quick recovery".into()) })
        .await;
    assert_eq!(res.unwrap(), "quick recovery");
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[tokio::test]
async fn cb_still_open_before_recovery_timeout() {
    let cb = CircuitBreaker::new(1, Duration::from_secs(60));
    let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
    assert_eq!(cb.state(), CircuitState::Open);

    // Don't wait for recovery
    let res: Result<String, CircuitBreakerError<String>> = cb
        .call(|| async { Ok::<_, String>("should not run".into()) })
        .await;
    assert!(matches!(res, Err(CircuitBreakerError::Open)));
}

// ===========================================================================
// 21. Concurrent access (basic)
// ===========================================================================

#[tokio::test]
async fn cb_shared_across_tasks() {
    let cb = Arc::new(CircuitBreaker::new(100, Duration::from_secs(30)));
    let mut handles = Vec::new();

    for _ in 0..10 {
        let cb = cb.clone();
        handles.push(tokio::spawn(async move {
            let _: Result<_, CircuitBreakerError<String>> = cb
                .call(|| async { Ok::<String, String>("ok".to_string()) })
                .await;
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    assert_eq!(cb.state(), CircuitState::Closed);
    assert_eq!(cb.consecutive_failures(), 0);
}

#[tokio::test]
async fn cb_concurrent_failures_trip_breaker() {
    let cb = Arc::new(CircuitBreaker::new(5, Duration::from_secs(300)));
    let mut handles = Vec::new();

    for _ in 0..10 {
        let cb = cb.clone();
        handles.push(tokio::spawn(async move {
            let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    assert_eq!(cb.state(), CircuitState::Open);
}
