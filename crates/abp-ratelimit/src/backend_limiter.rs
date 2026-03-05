#![allow(dead_code, unused_imports)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::policy::RateLimitPolicy;
use crate::sliding_window::SlidingWindowCounter;
use crate::token_bucket::TokenBucket;

/// Error returned when a rate limit is exceeded.
#[derive(Debug, Clone, thiserror::Error)]
pub enum RateLimitError {
    /// The request was rate limited.
    #[error("rate limited for backend `{backend_id}`")]
    Limited {
        /// The backend ID that was rate limited.
        backend_id: String,
    },
    /// No policy is configured for the backend.
    #[error("no rate limit policy for backend `{backend_id}`")]
    NoPolicyConfigured {
        /// The backend ID.
        backend_id: String,
    },
}

/// A per-backend rate limiter that applies different policies to different backends.
#[derive(Debug, Clone)]
pub struct BackendRateLimiter {
    inner: Arc<Mutex<BackendLimiterInner>>,
}

#[derive(Debug)]
struct BackendLimiterInner {
    policies: HashMap<String, RateLimitPolicy>,
    token_buckets: HashMap<String, TokenBucket>,
    sliding_windows: HashMap<String, SlidingWindowCounter>,
    active_permits: HashMap<String, usize>,
}

/// A permit granted by the rate limiter.
///
/// When dropped, the permit signals completion to the limiter,
/// allowing tracking of in-flight requests.
#[derive(Debug)]
pub struct RatePermit {
    backend_id: String,
    limiter: Arc<Mutex<BackendLimiterInner>>,
}

impl Drop for RatePermit {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.limiter.lock() {
            if let Some(count) = inner.active_permits.get_mut(&self.backend_id) {
                *count = count.saturating_sub(1);
            }
        }
    }
}

impl RatePermit {
    /// Return the backend ID this permit is for.
    pub fn backend_id(&self) -> &str {
        &self.backend_id
    }
}

impl BackendRateLimiter {
    /// Create a new empty backend rate limiter.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(BackendLimiterInner {
                policies: HashMap::new(),
                token_buckets: HashMap::new(),
                sliding_windows: HashMap::new(),
                active_permits: HashMap::new(),
            })),
        }
    }

    /// Set the rate limit policy for a backend.
    pub fn set_policy(&self, backend_id: &str, policy: RateLimitPolicy) {
        let mut inner = self.inner.lock().unwrap();
        // Create the corresponding limiter
        match &policy {
            RateLimitPolicy::TokenBucket { rate, burst } => {
                inner
                    .token_buckets
                    .insert(backend_id.to_string(), TokenBucket::new(*rate, *burst));
            }
            RateLimitPolicy::SlidingWindow {
                window_secs,
                max_requests,
            } => {
                inner.sliding_windows.insert(
                    backend_id.to_string(),
                    SlidingWindowCounter::new(Duration::from_secs_f64(*window_secs), *max_requests),
                );
            }
            RateLimitPolicy::Fixed { max_concurrent } => {
                inner.active_permits.insert(backend_id.to_string(), 0);
                // Store max_concurrent in the policy — checked at acquire time
                let _ = max_concurrent;
            }
            RateLimitPolicy::Unlimited => {}
        }
        inner.policies.insert(backend_id.to_string(), policy);
    }

    /// Try to acquire a rate permit for the given backend.
    ///
    /// Returns a [`RatePermit`] on success, or a [`RateLimitError`] if limited.
    pub fn try_acquire(&self, backend_id: &str) -> Result<RatePermit, RateLimitError> {
        let mut inner = self.inner.lock().unwrap();
        let policy = inner.policies.get(backend_id).cloned().ok_or_else(|| {
            RateLimitError::NoPolicyConfigured {
                backend_id: backend_id.to_string(),
            }
        })?;

        let allowed = match &policy {
            RateLimitPolicy::TokenBucket { .. } => {
                if let Some(bucket) = inner.token_buckets.get(backend_id) {
                    bucket.try_acquire(1)
                } else {
                    false
                }
            }
            RateLimitPolicy::SlidingWindow { .. } => {
                if let Some(window) = inner.sliding_windows.get(backend_id) {
                    window.try_acquire()
                } else {
                    false
                }
            }
            RateLimitPolicy::Fixed { max_concurrent } => {
                let current = inner.active_permits.get(backend_id).copied().unwrap_or(0);
                current < *max_concurrent
            }
            RateLimitPolicy::Unlimited => true,
        };

        if allowed {
            *inner
                .active_permits
                .entry(backend_id.to_string())
                .or_insert(0) += 1;
            let limiter = Arc::clone(&self.inner);
            Ok(RatePermit {
                backend_id: backend_id.to_string(),
                limiter,
            })
        } else {
            Err(RateLimitError::Limited {
                backend_id: backend_id.to_string(),
            })
        }
    }

    /// Return the number of active (in-flight) permits for a backend.
    pub fn active_permits(&self, backend_id: &str) -> usize {
        let inner = self.inner.lock().unwrap();
        inner.active_permits.get(backend_id).copied().unwrap_or(0)
    }

    /// Check whether a policy is configured for the given backend.
    pub fn has_policy(&self, backend_id: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.policies.contains_key(backend_id)
    }
}

impl Default for BackendRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_always_allows() {
        let limiter = BackendRateLimiter::new();
        limiter.set_policy("test", RateLimitPolicy::Unlimited);
        for _ in 0..100 {
            assert!(limiter.try_acquire("test").is_ok());
        }
    }

    #[test]
    fn no_policy_returns_error() {
        let limiter = BackendRateLimiter::new();
        let err = limiter.try_acquire("unknown").unwrap_err();
        assert!(matches!(err, RateLimitError::NoPolicyConfigured { .. }));
    }

    #[test]
    fn token_bucket_policy() {
        let limiter = BackendRateLimiter::new();
        limiter.set_policy(
            "openai",
            RateLimitPolicy::TokenBucket {
                rate: 10.0,
                burst: 3,
            },
        );
        assert!(limiter.try_acquire("openai").is_ok());
        assert!(limiter.try_acquire("openai").is_ok());
        assert!(limiter.try_acquire("openai").is_ok());
        assert!(limiter.try_acquire("openai").is_err());
    }

    #[test]
    fn sliding_window_policy() {
        let limiter = BackendRateLimiter::new();
        limiter.set_policy(
            "anthropic",
            RateLimitPolicy::SlidingWindow {
                window_secs: 1.0,
                max_requests: 2,
            },
        );
        assert!(limiter.try_acquire("anthropic").is_ok());
        assert!(limiter.try_acquire("anthropic").is_ok());
        assert!(limiter.try_acquire("anthropic").is_err());
    }

    #[test]
    fn fixed_concurrency_policy() {
        let limiter = BackendRateLimiter::new();
        limiter.set_policy("local", RateLimitPolicy::Fixed { max_concurrent: 2 });
        let p1 = limiter.try_acquire("local").unwrap();
        let p2 = limiter.try_acquire("local").unwrap();
        assert!(limiter.try_acquire("local").is_err());
        drop(p1);
        // After dropping p1, should allow again
        assert!(limiter.try_acquire("local").is_ok());
        drop(p2);
    }

    #[test]
    fn permit_drop_decrements() {
        let limiter = BackendRateLimiter::new();
        limiter.set_policy("test", RateLimitPolicy::Unlimited);
        {
            let _p = limiter.try_acquire("test").unwrap();
            assert_eq!(limiter.active_permits("test"), 1);
        }
        assert_eq!(limiter.active_permits("test"), 0);
    }

    #[test]
    fn per_backend_isolation() {
        let limiter = BackendRateLimiter::new();
        limiter.set_policy(
            "a",
            RateLimitPolicy::TokenBucket {
                rate: 10.0,
                burst: 1,
            },
        );
        limiter.set_policy(
            "b",
            RateLimitPolicy::TokenBucket {
                rate: 10.0,
                burst: 1,
            },
        );
        // Exhaust a's bucket
        assert!(limiter.try_acquire("a").is_ok());
        assert!(limiter.try_acquire("a").is_err());
        // b should still be available
        assert!(limiter.try_acquire("b").is_ok());
    }

    #[test]
    fn has_policy_check() {
        let limiter = BackendRateLimiter::new();
        assert!(!limiter.has_policy("x"));
        limiter.set_policy("x", RateLimitPolicy::Unlimited);
        assert!(limiter.has_policy("x"));
    }

    #[test]
    fn default_impl() {
        let limiter = BackendRateLimiter::default();
        assert!(!limiter.has_policy("anything"));
    }

    #[test]
    fn permit_backend_id() {
        let limiter = BackendRateLimiter::new();
        limiter.set_policy("mybackend", RateLimitPolicy::Unlimited);
        let permit = limiter.try_acquire("mybackend").unwrap();
        assert_eq!(permit.backend_id(), "mybackend");
    }

    #[test]
    fn error_display() {
        let err = RateLimitError::Limited {
            backend_id: "openai".to_string(),
        };
        assert!(format!("{err}").contains("openai"));

        let err2 = RateLimitError::NoPolicyConfigured {
            backend_id: "test".to_string(),
        };
        assert!(format!("{err2}").contains("test"));
    }
}
