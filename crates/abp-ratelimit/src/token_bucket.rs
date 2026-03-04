#![allow(dead_code, unused_imports)]

use std::sync::{Arc, Mutex};
use std::time::Instant;

/// A token bucket rate limiter.
///
/// Tokens are added at a fixed `rate` (tokens per second) up to a maximum
/// `burst` capacity. Callers acquire tokens before performing work.
#[derive(Debug, Clone)]
pub struct TokenBucket {
    inner: Arc<Mutex<TokenBucketInner>>,
}

#[derive(Debug)]
struct TokenBucketInner {
    rate: f64,
    burst: usize,
    available: f64,
    last_refill: Instant,
}

impl TokenBucket {
    /// Create a new token bucket with the given `rate` (tokens/sec) and `burst` capacity.
    ///
    /// The bucket starts full (available = burst).
    pub fn new(rate: f64, burst: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TokenBucketInner {
                rate,
                burst,
                available: burst as f64,
                last_refill: Instant::now(),
            })),
        }
    }

    /// Try to acquire `n` tokens without blocking.
    ///
    /// Returns `true` if tokens were acquired, `false` if insufficient tokens.
    pub fn try_acquire(&self, n: usize) -> bool {
        let mut inner = self.inner.lock().unwrap();
        inner.refill();
        let needed = n as f64;
        if inner.available >= needed {
            inner.available -= needed;
            true
        } else {
            false
        }
    }

    /// Wait asynchronously until `n` tokens are available, then acquire them.
    pub async fn wait_for(&self, n: usize) {
        loop {
            {
                let mut inner = self.inner.lock().unwrap();
                inner.refill();
                let needed = n as f64;
                if inner.available >= needed {
                    inner.available -= needed;
                    return;
                }
                // Calculate how long to wait for enough tokens
                let deficit = needed - inner.available;
                let wait_secs = if inner.rate > 0.0 {
                    deficit / inner.rate
                } else {
                    // Zero rate means tokens never refill; avoid infinite loop
                    return;
                };
                let wait = std::time::Duration::from_secs_f64(wait_secs);
                drop(inner);
                tokio::time::sleep(wait).await;
            }
        }
    }

    /// Return the current number of available tokens (after refill).
    pub fn available(&self) -> usize {
        let mut inner = self.inner.lock().unwrap();
        inner.refill();
        inner.available as usize
    }

    /// Return the configured rate (tokens per second).
    pub fn rate(&self) -> f64 {
        let inner = self.inner.lock().unwrap();
        inner.rate
    }

    /// Return the configured burst capacity.
    pub fn burst(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        inner.burst
    }
}

impl TokenBucketInner {
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        if elapsed > 0.0 {
            self.available = (self.available + elapsed * self.rate).min(self.burst as f64);
            self.last_refill = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_bucket_starts_full() {
        let bucket = TokenBucket::new(10.0, 20);
        assert_eq!(bucket.available(), 20);
    }

    #[test]
    fn acquire_reduces_tokens() {
        let bucket = TokenBucket::new(10.0, 20);
        assert!(bucket.try_acquire(5));
        assert_eq!(bucket.available(), 15);
    }

    #[test]
    fn acquire_more_than_available_fails() {
        let bucket = TokenBucket::new(10.0, 5);
        assert!(!bucket.try_acquire(6));
        assert_eq!(bucket.available(), 5);
    }

    #[test]
    fn zero_acquire_succeeds() {
        let bucket = TokenBucket::new(10.0, 5);
        assert!(bucket.try_acquire(0));
    }

    #[test]
    fn zero_rate_never_refills() {
        let bucket = TokenBucket::new(0.0, 5);
        assert!(bucket.try_acquire(5));
        assert!(!bucket.try_acquire(1));
    }

    #[test]
    fn max_burst_cap() {
        let bucket = TokenBucket::new(1000.0, 3);
        // Even after time passes, should not exceed burst
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(bucket.available() <= 3);
    }

    #[test]
    fn clone_shares_state() {
        let bucket = TokenBucket::new(10.0, 10);
        let clone = bucket.clone();
        assert!(bucket.try_acquire(5));
        assert_eq!(clone.available(), 5);
    }

    #[tokio::test]
    async fn wait_for_refill() {
        let bucket = TokenBucket::new(1000.0, 10);
        assert!(bucket.try_acquire(10));
        // Tokens depleted, wait_for should complete after refill
        bucket.wait_for(1).await;
    }

    #[tokio::test]
    async fn wait_for_zero_rate_returns() {
        let bucket = TokenBucket::new(0.0, 5);
        assert!(bucket.try_acquire(5));
        // Zero rate — wait_for should return immediately to avoid hang
        bucket.wait_for(1).await;
    }

    #[test]
    fn refill_over_time() {
        let bucket = TokenBucket::new(1000.0, 10);
        assert!(bucket.try_acquire(10));
        std::thread::sleep(std::time::Duration::from_millis(20));
        // After 20ms at 1000/s, should have ~20 tokens, but capped at burst=10
        assert!(bucket.available() > 0);
    }

    #[test]
    fn accessors() {
        let bucket = TokenBucket::new(42.0, 100);
        assert!((bucket.rate() - 42.0).abs() < f64::EPSILON);
        assert_eq!(bucket.burst(), 100);
    }
}
