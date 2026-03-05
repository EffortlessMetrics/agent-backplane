#![allow(dead_code, unused_imports)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Circuit breaker states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation — requests pass through.
    Closed,
    /// Circuit is tripped — all requests are rejected.
    Open,
    /// Tentatively allowing a single request to test recovery.
    HalfOpen,
}

/// Error returned when the circuit breaker rejects a call.
#[derive(Debug, Clone, thiserror::Error)]
pub enum CircuitBreakerError<E> {
    /// The circuit is open; the call was rejected without executing.
    #[error("circuit breaker is open")]
    Open,
    /// The inner call returned an error.
    #[error("inner call failed: {0}")]
    Inner(E),
}

/// Circuit breaker pattern implementation.
///
/// Tracks consecutive failures and trips open after a threshold, preventing
/// further calls until a timeout elapses. After the timeout the breaker
/// enters `HalfOpen`, allowing one probe request.
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    inner: Arc<Mutex<BreakerInner>>,
}

#[derive(Debug)]
struct BreakerInner {
    state: CircuitState,
    /// Number of consecutive failures.
    failure_count: u32,
    /// Threshold of consecutive failures to trip the breaker.
    failure_threshold: u32,
    /// How long to stay open before transitioning to half-open.
    reset_timeout: Duration,
    /// When the breaker was tripped open.
    opened_at: Option<Instant>,
    /// Count of successful calls since last reset.
    success_count: u32,
    /// Successes required in half-open to fully close.
    half_open_successes: u32,
}

impl CircuitBreaker {
    /// Create a new circuit breaker.
    ///
    /// - `failure_threshold`: consecutive failures before tripping open.
    /// - `reset_timeout`: how long to remain open before allowing a probe.
    pub fn new(failure_threshold: u32, reset_timeout: Duration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(BreakerInner {
                state: CircuitState::Closed,
                failure_count: 0,
                failure_threshold,
                reset_timeout,
                opened_at: None,
                success_count: 0,
                half_open_successes: 1,
            })),
        }
    }

    /// Set the number of successes required in half-open state to fully close.
    pub fn with_half_open_successes(self, n: u32) -> Self {
        {
            let mut inner = self.inner.lock().unwrap();
            inner.half_open_successes = n;
        }
        self
    }

    /// Return the current state, performing any time-based transitions.
    pub fn state(&self) -> CircuitState {
        let mut inner = self.inner.lock().unwrap();
        Self::maybe_transition(&mut inner);
        inner.state
    }

    /// Execute a fallible call through the breaker.
    ///
    /// If the circuit is open, the call is rejected immediately.
    /// If the call succeeds, the failure counter resets.
    /// If the call fails, the failure counter increments and the breaker
    /// may trip open.
    pub fn call<F, T, E>(&self, f: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: FnOnce() -> Result<T, E>,
    {
        // Check whether we can proceed
        {
            let mut inner = self.inner.lock().unwrap();
            Self::maybe_transition(&mut inner);
            match inner.state {
                CircuitState::Open => return Err(CircuitBreakerError::Open),
                CircuitState::HalfOpen | CircuitState::Closed => {}
            }
        }

        // Execute the call outside the lock
        match f() {
            Ok(val) => {
                self.record_success();
                Ok(val)
            }
            Err(e) => {
                self.record_failure();
                Err(CircuitBreakerError::Inner(e))
            }
        }
    }

    /// Manually record a success.
    pub fn record_success(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.failure_count = 0;
        inner.success_count += 1;
        if inner.state == CircuitState::HalfOpen && inner.success_count >= inner.half_open_successes
        {
            inner.state = CircuitState::Closed;
            inner.success_count = 0;
        }
    }

    /// Manually record a failure.
    pub fn record_failure(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.failure_count += 1;
        inner.success_count = 0;
        if inner.failure_count >= inner.failure_threshold {
            inner.state = CircuitState::Open;
            inner.opened_at = Some(Instant::now());
        }
    }

    /// Manually reset the breaker to closed state.
    pub fn reset(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.state = CircuitState::Closed;
        inner.failure_count = 0;
        inner.success_count = 0;
        inner.opened_at = None;
    }

    /// Return the consecutive failure count.
    pub fn failure_count(&self) -> u32 {
        let inner = self.inner.lock().unwrap();
        inner.failure_count
    }

    fn maybe_transition(inner: &mut BreakerInner) {
        if inner.state == CircuitState::Open {
            if let Some(opened) = inner.opened_at {
                if Instant::now().duration_since(opened) >= inner.reset_timeout {
                    inner.state = CircuitState::HalfOpen;
                    inner.success_count = 0;
                }
            }
        }
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(5, Duration::from_secs(30))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_closed() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(10));
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn trips_open_after_threshold() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(60));
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn success_resets_failure_count() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(60));
        cb.record_failure();
        cb.record_failure();
        cb.record_success();
        assert_eq!(cb.failure_count(), 0);
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn open_rejects_calls() {
        let cb = CircuitBreaker::new(1, Duration::from_secs(60));
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        let result: Result<(), CircuitBreakerError<&str>> = cb.call(|| Ok(()));
        assert!(matches!(result, Err(CircuitBreakerError::Open)));
    }

    #[test]
    fn half_open_after_timeout() {
        let cb = CircuitBreaker::new(1, Duration::from_millis(10));
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(cb.state(), CircuitState::HalfOpen);
    }

    #[test]
    fn half_open_success_closes() {
        let cb = CircuitBreaker::new(1, Duration::from_millis(10));
        cb.record_failure();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        let result: Result<i32, CircuitBreakerError<&str>> = cb.call(|| Ok(42));
        assert!(result.is_ok());
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn half_open_failure_reopens() {
        let cb = CircuitBreaker::new(1, Duration::from_millis(10));
        cb.record_failure();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        let result: Result<i32, CircuitBreakerError<&str>> = cb.call(|| Err("fail"));
        assert!(matches!(result, Err(CircuitBreakerError::Inner("fail"))));
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn call_success_path() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(10));
        let result: Result<i32, CircuitBreakerError<&str>> = cb.call(|| Ok(99));
        assert_eq!(result.unwrap(), 99);
        assert_eq!(cb.failure_count(), 0);
    }

    #[test]
    fn call_failure_increments() {
        let cb = CircuitBreaker::new(5, Duration::from_secs(10));
        let _: Result<i32, CircuitBreakerError<&str>> = cb.call(|| Err("oops"));
        assert_eq!(cb.failure_count(), 1);
    }

    #[test]
    fn manual_reset() {
        let cb = CircuitBreaker::new(1, Duration::from_secs(60));
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        cb.reset();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert_eq!(cb.failure_count(), 0);
    }

    #[test]
    fn default_config() {
        let cb = CircuitBreaker::default();
        assert_eq!(cb.state(), CircuitState::Closed);
        // Default threshold is 5
        for _ in 0..4 {
            cb.record_failure();
        }
        assert_eq!(cb.state(), CircuitState::Closed);
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn clone_shares_state() {
        let cb = CircuitBreaker::new(2, Duration::from_secs(60));
        let clone = cb.clone();
        cb.record_failure();
        cb.record_failure();
        assert_eq!(clone.state(), CircuitState::Open);
    }

    #[test]
    fn error_display() {
        let err: CircuitBreakerError<String> = CircuitBreakerError::Open;
        assert!(format!("{err}").contains("open"));
        let err2 = CircuitBreakerError::Inner("backend down".to_string());
        assert!(format!("{err2}").contains("backend down"));
    }

    #[test]
    fn with_half_open_successes() {
        let cb = CircuitBreaker::new(1, Duration::from_millis(10)).with_half_open_successes(2);
        cb.record_failure();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        cb.record_success();
        // Still half-open — need 2 successes
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn multiple_failures_in_call() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(60));
        for _ in 0..3 {
            let _: Result<(), CircuitBreakerError<&str>> = cb.call(|| Err("err"));
        }
        assert_eq!(cb.state(), CircuitState::Open);
    }
}
