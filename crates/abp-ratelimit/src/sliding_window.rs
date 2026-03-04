#![allow(dead_code, unused_imports)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A sliding window counter rate limiter.
///
/// Tracks requests within a configurable time window and enforces
/// a maximum request count within that window.
#[derive(Debug, Clone)]
pub struct SlidingWindowCounter {
    inner: Arc<Mutex<SlidingWindowInner>>,
}

#[derive(Debug)]
struct SlidingWindowInner {
    window: Duration,
    max_requests: usize,
    timestamps: VecDeque<Instant>,
}

impl SlidingWindowCounter {
    /// Create a new sliding window counter.
    ///
    /// - `window`: The duration of the sliding window.
    /// - `max_requests`: Maximum number of requests allowed within the window.
    pub fn new(window: Duration, max_requests: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(SlidingWindowInner {
                window,
                max_requests,
                timestamps: VecDeque::new(),
            })),
        }
    }

    /// Try to acquire a permit for one request.
    ///
    /// Returns `true` if the request is allowed, `false` if rate limited.
    pub fn try_acquire(&self) -> bool {
        let mut inner = self.inner.lock().unwrap();
        inner.evict_expired();
        if inner.timestamps.len() < inner.max_requests {
            inner.timestamps.push_back(Instant::now());
            true
        } else {
            false
        }
    }

    /// Return the number of remaining requests allowed in the current window.
    pub fn remaining(&self) -> usize {
        let mut inner = self.inner.lock().unwrap();
        inner.evict_expired();
        inner.max_requests.saturating_sub(inner.timestamps.len())
    }

    /// Return the `Instant` at which the oldest request in the window expires,
    /// effectively when the next slot opens up.
    ///
    /// Returns `Instant::now()` if the window is not full.
    pub fn reset_at(&self) -> Instant {
        let mut inner = self.inner.lock().unwrap();
        inner.evict_expired();
        if let Some(&oldest) = inner.timestamps.front() {
            oldest + inner.window
        } else {
            Instant::now()
        }
    }

    /// Return the configured window duration.
    pub fn window(&self) -> Duration {
        let inner = self.inner.lock().unwrap();
        inner.window
    }

    /// Return the configured maximum requests per window.
    pub fn max_requests(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        inner.max_requests
    }
}

impl SlidingWindowInner {
    fn evict_expired(&mut self) {
        let cutoff = Instant::now() - self.window;
        while let Some(&front) = self.timestamps.front() {
            if front < cutoff {
                self.timestamps.pop_front();
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_max() {
        let counter = SlidingWindowCounter::new(Duration::from_secs(1), 3);
        assert!(counter.try_acquire());
        assert!(counter.try_acquire());
        assert!(counter.try_acquire());
        assert!(!counter.try_acquire());
    }

    #[test]
    fn remaining_decreases() {
        let counter = SlidingWindowCounter::new(Duration::from_secs(1), 5);
        assert_eq!(counter.remaining(), 5);
        counter.try_acquire();
        assert_eq!(counter.remaining(), 4);
        counter.try_acquire();
        assert_eq!(counter.remaining(), 3);
    }

    #[test]
    fn expires_after_window() {
        let counter = SlidingWindowCounter::new(Duration::from_millis(50), 2);
        assert!(counter.try_acquire());
        assert!(counter.try_acquire());
        assert!(!counter.try_acquire());
        std::thread::sleep(Duration::from_millis(60));
        // Window expired, should allow again
        assert!(counter.try_acquire());
    }

    #[test]
    fn reset_at_returns_future() {
        let counter = SlidingWindowCounter::new(Duration::from_secs(10), 2);
        counter.try_acquire();
        let reset = counter.reset_at();
        assert!(reset > Instant::now());
    }

    #[test]
    fn reset_at_empty_window() {
        let counter = SlidingWindowCounter::new(Duration::from_secs(1), 5);
        // No requests yet — reset_at should be approximately now
        let reset = counter.reset_at();
        assert!(reset <= Instant::now() + Duration::from_millis(10));
    }

    #[test]
    fn clone_shares_state() {
        let counter = SlidingWindowCounter::new(Duration::from_secs(1), 3);
        let clone = counter.clone();
        counter.try_acquire();
        counter.try_acquire();
        assert_eq!(clone.remaining(), 1);
    }

    #[test]
    fn zero_max_always_denies() {
        let counter = SlidingWindowCounter::new(Duration::from_secs(1), 0);
        assert!(!counter.try_acquire());
        assert_eq!(counter.remaining(), 0);
    }

    #[test]
    fn accessors() {
        let counter = SlidingWindowCounter::new(Duration::from_secs(30), 100);
        assert_eq!(counter.window(), Duration::from_secs(30));
        assert_eq!(counter.max_requests(), 100);
    }
}
