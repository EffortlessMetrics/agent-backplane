#![allow(dead_code, unused_imports)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::token_bucket::TokenBucket;

/// Adaptive rate limiter that adjusts limits based on backend response headers.
///
/// Parses standard rate limit headers (`retry-after`, `x-ratelimit-remaining`,
/// `x-ratelimit-reset`) and dynamically tunes the underlying token bucket.
#[derive(Debug, Clone)]
pub struct AdaptiveLimiter {
    inner: Arc<Mutex<AdaptiveInner>>,
}

#[derive(Debug)]
struct AdaptiveInner {
    /// Current effective rate (requests per second).
    current_rate: f64,
    /// Base rate configured at construction time.
    base_rate: f64,
    /// Burst capacity.
    burst: usize,
    /// Underlying token bucket.
    bucket: TokenBucket,
    /// If set, requests should be blocked until this instant.
    retry_after: Option<Instant>,
    /// Last known remaining quota from headers.
    remaining: Option<u64>,
    /// Last known reset time from headers.
    reset_at: Option<Instant>,
    /// Minimum rate floor (never adapt below this).
    min_rate: f64,
}

impl AdaptiveLimiter {
    /// Create a new adaptive limiter with the given base `rate` and `burst`.
    pub fn new(rate: f64, burst: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(AdaptiveInner {
                current_rate: rate,
                base_rate: rate,
                burst,
                bucket: TokenBucket::new(rate, burst),
                retry_after: None,
                remaining: None,
                reset_at: None,
                min_rate: 0.1,
            })),
        }
    }

    /// Set the minimum rate floor. The adaptive limiter will never drop below this.
    pub fn with_min_rate(self, min_rate: f64) -> Self {
        {
            let mut inner = self.inner.lock().unwrap();
            inner.min_rate = min_rate;
        }
        self
    }

    /// Update rate limits from response headers.
    ///
    /// Recognised headers (case-insensitive keys):
    /// - `retry-after`: seconds to wait before retrying.
    /// - `x-ratelimit-remaining`: remaining requests in the current window.
    /// - `x-ratelimit-reset`: unix timestamp (seconds) when the window resets.
    pub fn update_from_headers(&self, headers: &HashMap<String, String>) {
        let mut inner = self.inner.lock().unwrap();

        // retry-after
        if let Some(val) = headers.get("retry-after") {
            if let Ok(secs) = val.parse::<f64>() {
                inner.retry_after = Some(Instant::now() + Duration::from_secs_f64(secs));
            }
        }

        // x-ratelimit-remaining
        if let Some(val) = headers.get("x-ratelimit-remaining") {
            if let Ok(rem) = val.parse::<u64>() {
                inner.remaining = Some(rem);
            }
        }

        // x-ratelimit-reset (unix timestamp)
        if let Some(val) = headers.get("x-ratelimit-reset") {
            if let Ok(ts) = val.parse::<u64>() {
                let now_unix = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                if ts > now_unix {
                    let delta = ts - now_unix;
                    inner.reset_at = Some(Instant::now() + Duration::from_secs(delta));
                }
            }
        }

        // Adapt rate based on remaining quota and reset window
        if let (Some(remaining), Some(reset_at)) = (inner.remaining, inner.reset_at) {
            let until_reset = reset_at
                .checked_duration_since(Instant::now())
                .unwrap_or(Duration::from_secs(1));
            let secs = until_reset.as_secs_f64().max(1.0);
            let adapted = (remaining as f64 / secs).max(inner.min_rate);
            inner.current_rate = adapted;
            inner.bucket = TokenBucket::new(adapted, inner.burst);
        } else if inner.remaining == Some(0) {
            // Quota exhausted but no reset info — drop to minimum
            inner.current_rate = inner.min_rate;
            inner.bucket = TokenBucket::new(inner.min_rate, inner.burst);
        }
    }

    /// Return the current effective rate (requests per second).
    pub fn current_rate(&self) -> f64 {
        let inner = self.inner.lock().unwrap();
        inner.current_rate
    }

    /// Return the base rate configured at construction.
    pub fn base_rate(&self) -> f64 {
        let inner = self.inner.lock().unwrap();
        inner.base_rate
    }

    /// Check whether requests should currently be throttled.
    ///
    /// Returns `true` if the caller should wait (i.e., a `retry-after` is active
    /// or the underlying bucket is exhausted).
    pub fn should_throttle(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        if let Some(retry_after) = inner.retry_after {
            if Instant::now() < retry_after {
                return true;
            }
        }
        !inner.bucket.try_acquire(0)
    }

    /// Try to acquire a single request permit.
    ///
    /// Returns `true` if allowed, `false` if throttled.
    pub fn try_acquire(&self) -> bool {
        let mut inner = self.inner.lock().unwrap();

        // Honour retry-after
        if let Some(retry_after) = inner.retry_after {
            if Instant::now() < retry_after {
                return false;
            }
            // Expired — clear it
            inner.retry_after = None;
        }

        inner.bucket.try_acquire(1)
    }

    /// Reset the limiter back to its base rate, clearing any adaptive state.
    pub fn reset(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.current_rate = inner.base_rate;
        inner.bucket = TokenBucket::new(inner.base_rate, inner.burst);
        inner.retry_after = None;
        inner.remaining = None;
        inner.reset_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_limiter_uses_base_rate() {
        let limiter = AdaptiveLimiter::new(10.0, 20);
        assert!((limiter.current_rate() - 10.0).abs() < f64::EPSILON);
        assert!((limiter.base_rate() - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn try_acquire_up_to_burst() {
        let limiter = AdaptiveLimiter::new(10.0, 3);
        assert!(limiter.try_acquire());
        assert!(limiter.try_acquire());
        assert!(limiter.try_acquire());
        assert!(!limiter.try_acquire());
    }

    #[test]
    fn retry_after_blocks_acquire() {
        let limiter = AdaptiveLimiter::new(100.0, 100);
        let mut headers = HashMap::new();
        headers.insert("retry-after".to_string(), "60".to_string());
        limiter.update_from_headers(&headers);
        assert!(!limiter.try_acquire());
    }

    #[test]
    fn retry_after_sets_throttle() {
        let limiter = AdaptiveLimiter::new(100.0, 100);
        let mut headers = HashMap::new();
        headers.insert("retry-after".to_string(), "60".to_string());
        limiter.update_from_headers(&headers);
        assert!(limiter.should_throttle());
    }

    #[test]
    fn expired_retry_after_allows() {
        let limiter = AdaptiveLimiter::new(100.0, 100);
        let mut headers = HashMap::new();
        // 0 seconds — already expired
        headers.insert("retry-after".to_string(), "0".to_string());
        limiter.update_from_headers(&headers);
        // retry_after is in the past, should allow
        assert!(limiter.try_acquire());
    }

    #[test]
    fn remaining_zero_drops_rate() {
        let limiter = AdaptiveLimiter::new(100.0, 50).with_min_rate(0.5);
        let mut headers = HashMap::new();
        headers.insert("x-ratelimit-remaining".to_string(), "0".to_string());
        limiter.update_from_headers(&headers);
        assert!((limiter.current_rate() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn remaining_with_reset_adapts_rate() {
        let limiter = AdaptiveLimiter::new(100.0, 50).with_min_rate(0.1);
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut headers = HashMap::new();
        headers.insert("x-ratelimit-remaining".to_string(), "50".to_string());
        headers.insert("x-ratelimit-reset".to_string(), (now_unix + 10).to_string());
        limiter.update_from_headers(&headers);
        // ~50 remaining / ~10 seconds ≈ 5.0 req/s
        let rate = limiter.current_rate();
        assert!(rate > 3.0 && rate < 7.0, "rate was {rate}");
    }

    #[test]
    fn reset_restores_base_rate() {
        let limiter = AdaptiveLimiter::new(100.0, 50);
        let mut headers = HashMap::new();
        headers.insert("x-ratelimit-remaining".to_string(), "0".to_string());
        limiter.update_from_headers(&headers);
        assert!(limiter.current_rate() < 100.0);
        limiter.reset();
        assert!((limiter.current_rate() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn with_min_rate_sets_floor() {
        let limiter = AdaptiveLimiter::new(100.0, 50).with_min_rate(5.0);
        let mut headers = HashMap::new();
        headers.insert("x-ratelimit-remaining".to_string(), "0".to_string());
        limiter.update_from_headers(&headers);
        assert!((limiter.current_rate() - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn unknown_headers_ignored() {
        let limiter = AdaptiveLimiter::new(10.0, 5);
        let mut headers = HashMap::new();
        headers.insert("x-custom-header".to_string(), "foobar".to_string());
        limiter.update_from_headers(&headers);
        assert!((limiter.current_rate() - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn invalid_header_values_ignored() {
        let limiter = AdaptiveLimiter::new(10.0, 5);
        let mut headers = HashMap::new();
        headers.insert("retry-after".to_string(), "not-a-number".to_string());
        headers.insert("x-ratelimit-remaining".to_string(), "invalid".to_string());
        limiter.update_from_headers(&headers);
        assert!((limiter.current_rate() - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_headers_no_change() {
        let limiter = AdaptiveLimiter::new(42.0, 10);
        limiter.update_from_headers(&HashMap::new());
        assert!((limiter.current_rate() - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn clone_shares_state() {
        let limiter = AdaptiveLimiter::new(10.0, 3);
        let clone = limiter.clone();
        limiter.try_acquire();
        limiter.try_acquire();
        limiter.try_acquire();
        assert!(!clone.try_acquire());
    }
}
