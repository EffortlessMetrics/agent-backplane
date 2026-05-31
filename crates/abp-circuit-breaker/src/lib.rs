#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Standalone circuit breaker primitive for backend operations.

use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Possible states of a [`CircuitBreaker`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CircuitState {
    /// Normal operation — calls are allowed through.
    Closed,
    /// Too many failures — calls are rejected immediately.
    Open,
    /// Recovery probe — a single call is allowed to test the backend.
    HalfOpen,
}

/// Error type returned by [`CircuitBreaker::call`].
#[derive(Debug, thiserror::Error)]
pub enum CircuitBreakerError<E> {
    /// The circuit is open; the call was not attempted.
    #[error("circuit breaker is open")]
    Open,
    /// The underlying operation failed.
    #[error(transparent)]
    Inner(E),
}

/// Circuit breaker that prevents cascading failures by short-circuiting calls
/// to backends that exceed a failure threshold.
pub struct CircuitBreaker {
    failure_threshold: u32,
    recovery_timeout: Duration,
    consecutive_failures: AtomicU32,
    state: Mutex<CircuitState>,
    last_failure_time: Mutex<Option<Instant>>,
}

impl std::fmt::Debug for CircuitBreaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CircuitBreaker")
            .field("failure_threshold", &self.failure_threshold)
            .field("recovery_timeout", &self.recovery_timeout)
            .field(
                "consecutive_failures",
                &self.consecutive_failures.load(Ordering::SeqCst),
            )
            .field("state", &self.state.lock().unwrap())
            .finish()
    }
}

impl CircuitBreaker {
    /// Creates a new `CircuitBreaker`.
    pub fn new(failure_threshold: u32, recovery_timeout: Duration) -> Self {
        Self {
            failure_threshold,
            recovery_timeout,
            consecutive_failures: AtomicU32::new(0),
            state: Mutex::new(CircuitState::Closed),
            last_failure_time: Mutex::new(None),
        }
    }

    /// Returns the current state.
    pub fn state(&self) -> CircuitState {
        *self.state.lock().unwrap()
    }

    /// Returns the number of consecutive failures.
    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures.load(Ordering::SeqCst)
    }

    /// Returns the configured failure threshold.
    pub fn failure_threshold(&self) -> u32 {
        self.failure_threshold
    }

    /// Returns the configured recovery timeout.
    pub fn recovery_timeout(&self) -> Duration {
        self.recovery_timeout
    }

    /// Executes `f` through the circuit breaker.
    pub async fn call<F, Fut, T, E>(&self, f: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        {
            let mut state = self.state.lock().unwrap();
            match *state {
                CircuitState::Closed => {}
                CircuitState::Open => {
                    let last = self.last_failure_time.lock().unwrap();
                    if let Some(t) = *last {
                        if t.elapsed() >= self.recovery_timeout {
                            tracing::info!("circuit breaker transitioning to half-open");
                            *state = CircuitState::HalfOpen;
                        } else {
                            return Err(CircuitBreakerError::Open);
                        }
                    } else {
                        return Err(CircuitBreakerError::Open);
                    }
                }
                CircuitState::HalfOpen => {}
            }
        }

        match f().await {
            Ok(val) => {
                self.on_success();
                Ok(val)
            }
            Err(e) => {
                self.on_failure();
                Err(CircuitBreakerError::Inner(e))
            }
        }
    }

    fn on_success(&self) {
        self.consecutive_failures.store(0, Ordering::SeqCst);
        let mut state = self.state.lock().unwrap();
        if *state == CircuitState::HalfOpen {
            tracing::info!("circuit breaker closing after successful probe");
        }
        *state = CircuitState::Closed;
    }

    fn on_failure(&self) {
        let prev = self.consecutive_failures.fetch_add(1, Ordering::SeqCst);
        let count = prev + 1;

        let mut state = self.state.lock().unwrap();
        if *state == CircuitState::HalfOpen || count >= self.failure_threshold {
            tracing::warn!(
                count,
                threshold = self.failure_threshold,
                "circuit breaker opening"
            );
            *state = CircuitState::Open;
            let mut last = self.last_failure_time.lock().unwrap();
            *last = Some(Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn opens_at_threshold_and_rejects_calls() {
        let cb = CircuitBreaker::new(1, Duration::from_secs(30));
        let _: Result<(), CircuitBreakerError<()>> = cb.call(|| async { Err(()) }).await;

        let result: Result<(), CircuitBreakerError<()>> = cb.call(|| async { Ok(()) }).await;
        assert!(matches!(result, Err(CircuitBreakerError::Open)));
    }

    #[tokio::test]
    async fn transitions_to_half_open_then_closed_after_probe_success() {
        let cb = CircuitBreaker::new(1, Duration::from_millis(5));
        let _: Result<(), CircuitBreakerError<()>> = cb.call(|| async { Err(()) }).await;
        tokio::time::sleep(Duration::from_millis(15)).await;

        let result = cb.call(|| async { Ok::<_, ()>("ok") }).await;
        assert_eq!(result.unwrap(), "ok");
        assert_eq!(cb.state(), CircuitState::Closed);
        assert_eq!(cb.consecutive_failures(), 0);
    }

    #[test]
    fn circuit_state_serde_roundtrip() {
        let json = serde_json::to_string(&CircuitState::HalfOpen).unwrap();
        assert_eq!(json, "\"half_open\"");

        let state: CircuitState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, CircuitState::HalfOpen);
    }
}
