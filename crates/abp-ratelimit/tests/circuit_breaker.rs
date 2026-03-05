//! Integration tests for `CircuitBreaker`.

use std::sync::Arc;
use std::time::Duration;

use abp_ratelimit::{CircuitBreaker, CircuitBreakerError, CircuitState};

// ---------------------------------------------------------------------------
// State transitions
// ---------------------------------------------------------------------------

#[test]
fn closed_to_open_to_half_open_to_closed() {
    let cb = CircuitBreaker::new(2, Duration::from_millis(20));

    assert_eq!(cb.state(), CircuitState::Closed);

    // Two failures → Open
    cb.record_failure();
    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Open);

    // Wait for timeout → HalfOpen
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(cb.state(), CircuitState::HalfOpen);

    // Success in HalfOpen → Closed
    cb.record_success();
    assert_eq!(cb.state(), CircuitState::Closed);
}

#[test]
fn half_open_failure_reopens() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(20));
    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Open);

    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(cb.state(), CircuitState::HalfOpen);

    // Failure in HalfOpen → back to Open
    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Open);
}

// ---------------------------------------------------------------------------
// call() integration
// ---------------------------------------------------------------------------

#[test]
fn call_propagates_success_value() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(60));
    let result: Result<i32, CircuitBreakerError<&str>> = cb.call(|| Ok(42));
    assert_eq!(result.unwrap(), 42);
}

#[test]
fn call_propagates_inner_error() {
    let cb = CircuitBreaker::new(3, Duration::from_secs(60));
    let result: Result<i32, CircuitBreakerError<String>> = cb.call(|| Err("boom".to_string()));
    match result {
        Err(CircuitBreakerError::Inner(msg)) => assert_eq!(msg, "boom"),
        other => panic!("expected Inner error, got {other:?}"),
    }
}

#[test]
fn open_circuit_rejects_without_calling() {
    let cb = CircuitBreaker::new(1, Duration::from_secs(60));
    cb.record_failure();

    let mut called = false;
    let result: Result<(), CircuitBreakerError<&str>> = cb.call(|| {
        called = true;
        Ok(())
    });
    assert!(
        !called,
        "function should not be invoked when circuit is open"
    );
    assert!(matches!(result, Err(CircuitBreakerError::Open)));
}

// ---------------------------------------------------------------------------
// Half-open successes builder
// ---------------------------------------------------------------------------

#[test]
fn multiple_half_open_successes_required() {
    let cb = CircuitBreaker::new(1, Duration::from_millis(20)).with_half_open_successes(3);
    cb.record_failure();
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(cb.state(), CircuitState::HalfOpen);

    cb.record_success();
    assert_eq!(cb.state(), CircuitState::HalfOpen);
    cb.record_success();
    assert_eq!(cb.state(), CircuitState::HalfOpen);
    cb.record_success();
    assert_eq!(cb.state(), CircuitState::Closed);
}

// ---------------------------------------------------------------------------
// Reset
// ---------------------------------------------------------------------------

#[test]
fn reset_from_open_returns_to_closed() {
    let cb = CircuitBreaker::new(1, Duration::from_secs(60));
    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Open);
    cb.reset();
    assert_eq!(cb.state(), CircuitState::Closed);
    assert_eq!(cb.failure_count(), 0);
}

// ---------------------------------------------------------------------------
// Concurrent usage
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_failures_trip_breaker() {
    let cb = Arc::new(CircuitBreaker::new(10, Duration::from_secs(60)));
    let mut handles = Vec::new();
    for _ in 0..10 {
        let c = Arc::clone(&cb);
        handles.push(tokio::spawn(async move {
            c.record_failure();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(cb.state(), CircuitState::Open);
}

// ---------------------------------------------------------------------------
// Error Display
// ---------------------------------------------------------------------------

#[test]
fn error_display_messages() {
    let open: CircuitBreakerError<String> = CircuitBreakerError::Open;
    let msg = format!("{open}");
    assert!(msg.contains("open"));

    let inner = CircuitBreakerError::Inner("timeout".to_string());
    let msg = format!("{inner}");
    assert!(msg.contains("timeout"));
}

// ---------------------------------------------------------------------------
// Default
// ---------------------------------------------------------------------------

#[test]
fn default_threshold_is_five() {
    let cb = CircuitBreaker::default();
    for _ in 0..4 {
        cb.record_failure();
    }
    assert_eq!(cb.state(), CircuitState::Closed);
    cb.record_failure();
    assert_eq!(cb.state(), CircuitState::Open);
}
