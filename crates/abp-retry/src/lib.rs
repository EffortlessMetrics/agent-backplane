#![warn(missing_docs)]
//! Retry and circuit breaker middleware for Agent Backplane backend calls.
//!
//! This crate provides two core resilience primitives:
//!
//! - [`RetryPolicy`]: Configurable retry logic with exponential backoff and optional jitter.
//! - [`CircuitBreaker`]: Prevents cascading failures by short-circuiting calls to unhealthy backends.
//!
//! # Examples
//!
//! ## Retry with default policy
//!
//! ```rust
//! use abp_retry::{RetryPolicy, retry_with_policy};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let policy = RetryPolicy::default();
//! let result = retry_with_policy(&policy, || async {
//!     Ok::<_, String>("success".to_string())
//! }).await;
//! assert!(result.is_ok());
//! # Ok(())
//! # }
//! ```
//!
//! ## Circuit breaker
//!
//! ```rust
//! use abp_retry::CircuitBreaker;
//! use std::time::Duration;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let cb = CircuitBreaker::new(3, Duration::from_secs(30));
//! let result = cb.call(|| async {
//!     Ok::<_, String>("healthy".to_string())
//! }).await;
//! assert!(result.is_ok());
//! # Ok(())
//! # }
//! ```

use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Policy controlling retry behavior for fallible operations.
///
/// Supports exponential backoff with optional jitter to prevent thundering-herd effects.
///
/// # Examples
///
/// ```rust
/// use abp_retry::RetryPolicy;
///
/// let policy = RetryPolicy::default();
/// assert_eq!(policy.max_retries, 3);
/// assert!(policy.jitter);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts after the initial call.
    pub max_retries: u32,
    /// Base delay between retries. Subsequent delays are multiplied by `backoff_multiplier`.
    pub base_delay: Duration,
    /// Upper bound on the computed delay.
    pub max_delay: Duration,
    /// Factor by which the delay increases after each retry.
    pub backoff_multiplier: f64,
    /// Whether to add random jitter to the delay to avoid thundering-herd effects.
    pub jitter: bool,
}

impl Default for RetryPolicy {
    /// Creates a default retry policy: 3 retries, 100 ms base, 5 s max, 2.0 multiplier, jitter on.
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(5),
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }
}

impl RetryPolicy {
    /// Creates a new `RetryPolicy` with the given parameters.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use abp_retry::RetryPolicy;
    /// use std::time::Duration;
    ///
    /// let policy = RetryPolicy::new(5, Duration::from_millis(200), Duration::from_secs(10), 3.0, false);
    /// assert_eq!(policy.max_retries, 5);
    /// assert_eq!(policy.backoff_multiplier, 3.0);
    /// ```
    pub fn new(
        max_retries: u32,
        base_delay: Duration,
        max_delay: Duration,
        backoff_multiplier: f64,
        jitter: bool,
    ) -> Self {
        Self {
            max_retries,
            base_delay,
            max_delay,
            backoff_multiplier,
            jitter,
        }
    }

    /// Creates a policy that never retries.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use abp_retry::RetryPolicy;
    ///
    /// let policy = RetryPolicy::no_retry();
    /// assert_eq!(policy.max_retries, 0);
    /// ```
    pub fn no_retry() -> Self {
        Self {
            max_retries: 0,
            base_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
            backoff_multiplier: 1.0,
            jitter: false,
        }
    }

    /// Computes the delay for a given attempt number (0-indexed).
    ///
    /// The delay grows exponentially: `base_delay * backoff_multiplier^attempt`, capped at `max_delay`.
    /// If `jitter` is enabled, the returned delay is randomly scaled between 0% and 100%
    /// of the computed value.
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let base_nanos = self.base_delay.as_nanos() as f64;
        let multiplied = base_nanos * self.backoff_multiplier.powi(attempt as i32);
        let capped = multiplied.min(self.max_delay.as_nanos() as f64);

        let nanos = if self.jitter {
            let jitter_factor = pseudo_random_factor();
            capped * jitter_factor
        } else {
            capped
        };

        Duration::from_nanos(nanos as u64)
    }
}

/// Executes an async closure with retry logic according to the given [`RetryPolicy`].
///
/// The closure `f` is called up to `policy.max_retries + 1` times. If all attempts fail,
/// the last error is returned.
///
/// # Examples
///
/// ```rust
/// use abp_retry::{RetryPolicy, retry_with_policy};
///
/// # #[tokio::main]
/// # async fn main() {
/// let policy = RetryPolicy::no_retry();
/// let result = retry_with_policy(&policy, || async {
///     Ok::<_, String>(42)
/// }).await;
/// assert_eq!(result.unwrap(), 42);
/// # }
/// ```
pub async fn retry_with_policy<F, Fut, T, E>(policy: &RetryPolicy, f: F) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let mut last_err: Option<E> = None;

    for attempt in 0..=policy.max_retries {
        match f().await {
            Ok(val) => return Ok(val),
            Err(e) => {
                tracing::warn!(attempt, max = policy.max_retries, "retry attempt failed");
                last_err = Some(e);

                if attempt < policy.max_retries {
                    let delay = policy.delay_for_attempt(attempt);
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    Err(last_err.expect("at least one attempt must have been made"))
}

// ---------------------------------------------------------------------------
// Circuit Breaker
// ---------------------------------------------------------------------------

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

/// A circuit breaker that prevents cascading failures by short-circuiting calls
/// to backends that have exceeded a failure threshold.
///
/// After `failure_threshold` consecutive failures the breaker opens and rejects
/// all calls for `recovery_timeout`. After the timeout it enters a half-open state
/// where a single probe call is allowed through — success closes the breaker,
/// failure reopens it.
///
/// # Thread Safety
///
/// `CircuitBreaker` is `Send + Sync` and safe to share across tasks.
///
/// # Examples
///
/// ```rust
/// use abp_retry::CircuitBreaker;
/// use std::time::Duration;
///
/// # #[tokio::main]
/// # async fn main() {
/// let cb = CircuitBreaker::new(2, Duration::from_secs(5));
/// let res: Result<String, abp_retry::CircuitBreakerError<String>> =
/// cb.call(|| async { Ok::<_, String>("ok".to_string()) }).await;
/// assert!(res.is_ok());
/// # }
/// ```
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
    ///
    /// * `failure_threshold` — number of consecutive failures before the breaker opens.
    /// * `recovery_timeout` — how long the breaker stays open before allowing a probe.
    pub fn new(failure_threshold: u32, recovery_timeout: Duration) -> Self {
        Self {
            failure_threshold,
            recovery_timeout,
            consecutive_failures: AtomicU32::new(0),
            state: Mutex::new(CircuitState::Closed),
            last_failure_time: Mutex::new(None),
        }
    }

    /// Returns the current state of the circuit breaker.
    pub fn state(&self) -> CircuitState {
        *self.state.lock().unwrap()
    }

    /// Returns the number of consecutive failures recorded so far.
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
    ///
    /// * **Closed** — the call proceeds normally. On failure the failure counter increments;
    ///   once the threshold is reached the breaker opens.
    /// * **Open** — if the recovery timeout has elapsed the state transitions to half-open and
    ///   a probe call is allowed; otherwise returns [`CircuitBreakerError::Open`].
    /// * **HalfOpen** — a single call is allowed. Success closes the breaker; failure reopens it.
    pub async fn call<F, Fut, T, E>(&self, f: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        // Determine whether we may proceed.
        {
            let mut state = self.state.lock().unwrap();
            match *state {
                CircuitState::Closed => { /* allowed */ }
                CircuitState::Open => {
                    let last = self.last_failure_time.lock().unwrap();
                    if let Some(t) = *last {
                        if t.elapsed() >= self.recovery_timeout {
                            tracing::info!("circuit breaker transitioning to half-open");
                            *state = CircuitState::HalfOpen;
                            // fall through to allow the probe
                        } else {
                            return Err(CircuitBreakerError::Open);
                        }
                    } else {
                        return Err(CircuitBreakerError::Open);
                    }
                }
                CircuitState::HalfOpen => { /* probe allowed */ }
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Simple pseudo-random factor in `[0.0, 1.0)` using thread-local state so we
/// don't pull in the `rand` crate.
fn pseudo_random_factor() -> f64 {
    use std::cell::Cell;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    thread_local! {
        static SEED: Cell<u64> = Cell::new({
            let mut h = DefaultHasher::new();
            std::thread::current().id().hash(&mut h);
            // Mix in the address of a stack variable for additional entropy.
            let x = 0u8;
            (&x as *const u8 as u64).hash(&mut h);
            h.finish()
        });
    }

    SEED.with(|cell| {
        // xorshift64
        let mut s = cell.get();
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        cell.set(s);
        (s as f64) / (u64::MAX as f64)
    })
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    // -----------------------------------------------------------------------
    // RetryPolicy construction & defaults
    // -----------------------------------------------------------------------

    #[test]
    fn default_policy_max_retries() {
        let p = RetryPolicy::default();
        assert_eq!(p.max_retries, 3);
    }

    #[test]
    fn default_policy_base_delay() {
        let p = RetryPolicy::default();
        assert_eq!(p.base_delay, Duration::from_millis(100));
    }

    #[test]
    fn default_policy_max_delay() {
        let p = RetryPolicy::default();
        assert_eq!(p.max_delay, Duration::from_secs(5));
    }

    #[test]
    fn default_policy_backoff_multiplier() {
        let p = RetryPolicy::default();
        assert!((p.backoff_multiplier - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn default_policy_jitter_enabled() {
        let p = RetryPolicy::default();
        assert!(p.jitter);
    }

    #[test]
    fn no_retry_policy_max_retries() {
        let p = RetryPolicy::no_retry();
        assert_eq!(p.max_retries, 0);
    }

    #[test]
    fn no_retry_policy_base_delay_zero() {
        let p = RetryPolicy::no_retry();
        assert_eq!(p.base_delay, Duration::ZERO);
    }

    #[test]
    fn no_retry_policy_jitter_disabled() {
        let p = RetryPolicy::no_retry();
        assert!(!p.jitter);
    }

    #[test]
    fn custom_policy_construction() {
        let p = RetryPolicy::new(
            5,
            Duration::from_millis(200),
            Duration::from_secs(10),
            3.0,
            false,
        );
        assert_eq!(p.max_retries, 5);
        assert_eq!(p.base_delay, Duration::from_millis(200));
        assert_eq!(p.max_delay, Duration::from_secs(10));
        assert!((p.backoff_multiplier - 3.0).abs() < f64::EPSILON);
        assert!(!p.jitter);
    }

    #[test]
    fn policy_clone_equality() {
        let p = RetryPolicy::default();
        let p2 = p.clone();
        assert_eq!(p, p2);
    }

    #[test]
    fn policy_debug_impl() {
        let p = RetryPolicy::default();
        let dbg = format!("{p:?}");
        assert!(dbg.contains("RetryPolicy"));
    }

    // -----------------------------------------------------------------------
    // Backoff delay calculation
    // -----------------------------------------------------------------------

    #[test]
    fn delay_attempt_zero_no_jitter() {
        let p = RetryPolicy::new(
            3,
            Duration::from_millis(100),
            Duration::from_secs(5),
            2.0,
            false,
        );
        let d = p.delay_for_attempt(0);
        assert_eq!(d, Duration::from_millis(100));
    }

    #[test]
    fn delay_attempt_one_no_jitter() {
        let p = RetryPolicy::new(
            3,
            Duration::from_millis(100),
            Duration::from_secs(5),
            2.0,
            false,
        );
        let d = p.delay_for_attempt(1);
        assert_eq!(d, Duration::from_millis(200));
    }

    #[test]
    fn delay_attempt_two_no_jitter() {
        let p = RetryPolicy::new(
            3,
            Duration::from_millis(100),
            Duration::from_secs(5),
            2.0,
            false,
        );
        let d = p.delay_for_attempt(2);
        assert_eq!(d, Duration::from_millis(400));
    }

    #[test]
    fn delay_capped_at_max() {
        let p = RetryPolicy::new(
            10,
            Duration::from_secs(1),
            Duration::from_secs(5),
            10.0,
            false,
        );
        let d = p.delay_for_attempt(5);
        assert_eq!(d, Duration::from_secs(5));
    }

    #[test]
    fn delay_with_multiplier_three() {
        let p = RetryPolicy::new(
            3,
            Duration::from_millis(100),
            Duration::from_secs(60),
            3.0,
            false,
        );
        // attempt 2: 100ms * 3^2 = 900ms
        let d = p.delay_for_attempt(2);
        assert_eq!(d, Duration::from_millis(900));
    }

    #[test]
    fn delay_exponential_growth() {
        let p = RetryPolicy::new(
            5,
            Duration::from_millis(50),
            Duration::from_secs(60),
            2.0,
            false,
        );
        let d0 = p.delay_for_attempt(0);
        let d1 = p.delay_for_attempt(1);
        let d2 = p.delay_for_attempt(2);
        assert!(d1 > d0);
        assert!(d2 > d1);
    }

    // -----------------------------------------------------------------------
    // Jitter
    // -----------------------------------------------------------------------

    #[test]
    fn jitter_produces_varying_delays() {
        let p = RetryPolicy::new(
            3,
            Duration::from_millis(100),
            Duration::from_secs(5),
            2.0,
            true,
        );
        let delays: Vec<Duration> = (0..20).map(|_| p.delay_for_attempt(1)).collect();
        // With jitter, not all 20 samples should be identical.
        let all_same = delays.windows(2).all(|w| w[0] == w[1]);
        assert!(!all_same, "jitter should produce varying delays");
    }

    #[test]
    fn jitter_delay_bounded() {
        let p = RetryPolicy::new(
            3,
            Duration::from_millis(100),
            Duration::from_secs(5),
            2.0,
            true,
        );
        for _ in 0..100 {
            let d = p.delay_for_attempt(1);
            // base * 2^1 = 200ms; jitter scales [0, 200ms]
            assert!(d <= Duration::from_millis(200));
        }
    }

    #[test]
    fn no_jitter_deterministic() {
        let p = RetryPolicy::new(
            3,
            Duration::from_millis(100),
            Duration::from_secs(5),
            2.0,
            false,
        );
        let d1 = p.delay_for_attempt(1);
        let d2 = p.delay_for_attempt(1);
        assert_eq!(d1, d2);
    }

    // -----------------------------------------------------------------------
    // retry_with_policy
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn retry_succeeds_first_attempt() {
        let p = RetryPolicy::no_retry();
        let result = retry_with_policy(&p, || async { Ok::<_, String>(42) }).await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn retry_succeeds_after_failures() {
        let counter = Arc::new(AtomicU32::new(0));
        let p = RetryPolicy::new(
            3,
            Duration::from_millis(1),
            Duration::from_secs(1),
            1.0,
            false,
        );
        let c = counter.clone();
        let result = retry_with_policy(&p, || {
            let c = c.clone();
            async move {
                let n = c.fetch_add(1, Ordering::SeqCst);
                if n < 2 { Err("not yet") } else { Ok("done") }
            }
        })
        .await;
        assert_eq!(result.unwrap(), "done");
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn retry_exhausts_all_attempts() {
        let counter = Arc::new(AtomicU32::new(0));
        let p = RetryPolicy::new(
            2,
            Duration::from_millis(1),
            Duration::from_secs(1),
            1.0,
            false,
        );
        let c = counter.clone();
        let result: Result<(), &str> = retry_with_policy(&p, || {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err("fail")
            }
        })
        .await;
        assert!(result.is_err());
        // initial + 2 retries = 3 total
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn retry_returns_last_error() {
        let counter = Arc::new(AtomicU32::new(0));
        let p = RetryPolicy::new(
            2,
            Duration::from_millis(1),
            Duration::from_secs(1),
            1.0,
            false,
        );
        let c = counter.clone();
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

    #[tokio::test]
    async fn retry_no_retry_fails_immediately() {
        let counter = Arc::new(AtomicU32::new(0));
        let p = RetryPolicy::no_retry();
        let c = counter.clone();
        let result: Result<(), &str> = retry_with_policy(&p, || {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err("fail")
            }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn retry_succeeds_on_last_attempt() {
        let counter = Arc::new(AtomicU32::new(0));
        let p = RetryPolicy::new(
            3,
            Duration::from_millis(1),
            Duration::from_secs(1),
            1.0,
            false,
        );
        let c = counter.clone();
        let result = retry_with_policy(&p, || {
            let c = c.clone();
            async move {
                let n = c.fetch_add(1, Ordering::SeqCst);
                if n < 3 { Err("not yet") } else { Ok("last") }
            }
        })
        .await;
        assert_eq!(result.unwrap(), "last");
        assert_eq!(counter.load(Ordering::SeqCst), 4);
    }

    // -----------------------------------------------------------------------
    // CircuitBreaker
    // -----------------------------------------------------------------------

    #[test]
    fn circuit_breaker_starts_closed() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn circuit_breaker_initial_failures_zero() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));
        assert_eq!(cb.consecutive_failures(), 0);
    }

    #[test]
    fn circuit_breaker_threshold_getter() {
        let cb = CircuitBreaker::new(5, Duration::from_secs(10));
        assert_eq!(cb.failure_threshold(), 5);
    }

    #[test]
    fn circuit_breaker_recovery_timeout_getter() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(42));
        assert_eq!(cb.recovery_timeout(), Duration::from_secs(42));
    }

    #[tokio::test]
    async fn circuit_breaker_passes_success() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));
        let res: Result<String, CircuitBreakerError<String>> = cb
            .call(|| async { Ok::<_, String>("ok".to_string()) })
            .await;
        assert_eq!(res.unwrap(), "ok");
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn circuit_breaker_records_failure() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("boom") }).await;
        assert_eq!(cb.consecutive_failures(), 1);
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn circuit_breaker_opens_after_threshold() {
        let cb = CircuitBreaker::new(2, Duration::from_secs(30));
        for _ in 0..2 {
            let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
        }
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[tokio::test]
    async fn circuit_breaker_rejects_when_open() {
        let cb = CircuitBreaker::new(1, Duration::from_secs(300));
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
        assert_eq!(cb.state(), CircuitState::Open);

        let res: Result<String, CircuitBreakerError<&str>> =
            cb.call(|| async { Ok("should not run".into()) }).await;
        assert!(matches!(res, Err(CircuitBreakerError::Open)));
    }

    #[tokio::test]
    async fn circuit_breaker_half_open_after_timeout() {
        let cb = CircuitBreaker::new(1, Duration::from_millis(10));
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
        assert_eq!(cb.state(), CircuitState::Open);

        tokio::time::sleep(Duration::from_millis(20)).await;

        // The next call transitions to half-open internally and executes the probe.
        let res: Result<String, CircuitBreakerError<String>> =
            cb.call(|| async { Ok::<_, String>("probe".into()) }).await;
        assert_eq!(res.unwrap(), "probe");
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn circuit_breaker_closes_after_successful_probe() {
        let cb = CircuitBreaker::new(1, Duration::from_millis(10));
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
        tokio::time::sleep(Duration::from_millis(20)).await;

        let res: Result<String, CircuitBreakerError<String>> =
            cb.call(|| async { Ok::<_, String>("ok".into()) }).await;
        assert!(res.is_ok());
        assert_eq!(cb.state(), CircuitState::Closed);
        assert_eq!(cb.consecutive_failures(), 0);
    }

    #[tokio::test]
    async fn circuit_breaker_reopens_after_failed_probe() {
        let cb = CircuitBreaker::new(1, Duration::from_millis(10));
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
        tokio::time::sleep(Duration::from_millis(20)).await;

        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("still broken") }).await;
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[tokio::test]
    async fn circuit_breaker_success_resets_failure_count() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));
        let _: Result<String, _> = cb.call(|| async { Err::<String, _>("fail") }).await;
        assert_eq!(cb.consecutive_failures(), 1);
        let _: Result<String, _> = cb
            .call(|| async { Ok::<_, String>("ok".to_string()) })
            .await;
        assert_eq!(cb.consecutive_failures(), 0);
    }

    #[tokio::test]
    async fn circuit_breaker_inner_error_returned() {
        let cb = CircuitBreaker::new(5, Duration::from_secs(30));
        let res: Result<String, CircuitBreakerError<String>> =
            cb.call(|| async { Err("details".to_string()) }).await;
        match res {
            Err(CircuitBreakerError::Inner(e)) => assert_eq!(e, "details"),
            other => panic!("expected Inner error, got {other:?}"),
        }
    }

    #[test]
    fn circuit_breaker_debug_impl() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));
        let dbg = format!("{cb:?}");
        assert!(dbg.contains("CircuitBreaker"));
    }

    // -----------------------------------------------------------------------
    // Thread safety
    // -----------------------------------------------------------------------

    #[test]
    fn retry_policy_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RetryPolicy>();
    }

    #[test]
    fn circuit_breaker_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CircuitBreaker>();
    }

    #[test]
    fn circuit_state_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CircuitState>();
    }

    // -----------------------------------------------------------------------
    // Serialization roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn retry_policy_serde_roundtrip() {
        let p = RetryPolicy::default();
        let json = serde_json::to_string(&p).unwrap();
        let p2: RetryPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(p, p2);
    }

    #[test]
    fn retry_policy_no_retry_serde_roundtrip() {
        let p = RetryPolicy::no_retry();
        let json = serde_json::to_string(&p).unwrap();
        let p2: RetryPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(p, p2);
    }

    #[test]
    fn circuit_state_serde_roundtrip() {
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
    fn circuit_state_serde_snake_case() {
        let json = serde_json::to_string(&CircuitState::HalfOpen).unwrap();
        assert_eq!(json, r#""half_open""#);
    }

    #[test]
    fn retry_policy_custom_serde_roundtrip() {
        let p = RetryPolicy::new(
            7,
            Duration::from_millis(250),
            Duration::from_secs(30),
            1.5,
            false,
        );
        let json = serde_json::to_string_pretty(&p).unwrap();
        let p2: RetryPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(p, p2);
    }

    // -----------------------------------------------------------------------
    // CircuitBreakerError
    // -----------------------------------------------------------------------

    #[test]
    fn circuit_breaker_error_open_display() {
        let e: CircuitBreakerError<String> = CircuitBreakerError::Open;
        assert_eq!(e.to_string(), "circuit breaker is open");
    }

    #[test]
    fn circuit_breaker_error_inner_display() {
        let e: CircuitBreakerError<String> =
            CircuitBreakerError::Inner("something broke".to_string());
        assert_eq!(e.to_string(), "something broke");
    }
}
