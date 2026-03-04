#![allow(dead_code, unused_imports)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::policy::RateLimitPolicy;
use crate::sliding_window::SlidingWindowCounter;
use crate::token_bucket::TokenBucket;

/// Per-model rate limiter.
///
/// Different models may have very different rate limit budgets (e.g., GPT-4
/// is typically more constrained than GPT-3.5). This struct holds a separate
/// limiter instance for each registered model.
#[derive(Debug, Clone)]
pub struct ModelRateLimiter {
    inner: Arc<Mutex<ModelLimiterInner>>,
}

#[derive(Debug)]
struct ModelLimiterInner {
    policies: HashMap<String, RateLimitPolicy>,
    buckets: HashMap<String, TokenBucket>,
    windows: HashMap<String, SlidingWindowCounter>,
    default_policy: Option<RateLimitPolicy>,
}

/// Result of a rate-limit check for a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelLimitResult {
    /// Request is allowed.
    Allowed,
    /// Request is rate limited.
    Limited,
    /// Model is not registered and no default policy is set.
    UnknownModel,
}

impl ModelRateLimiter {
    /// Create a new per-model rate limiter with no registered models.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ModelLimiterInner {
                policies: HashMap::new(),
                buckets: HashMap::new(),
                windows: HashMap::new(),
                default_policy: None,
            })),
        }
    }

    /// Set a default policy applied to models that have no explicit registration.
    pub fn set_default_policy(&self, policy: RateLimitPolicy) {
        let mut inner = self.inner.lock().unwrap();
        inner.default_policy = Some(policy);
    }

    /// Register rate limits for a specific model.
    pub fn register_model_limits(&self, model: &str, policy: RateLimitPolicy) {
        let mut inner = self.inner.lock().unwrap();
        match &policy {
            RateLimitPolicy::TokenBucket { rate, burst } => {
                inner
                    .buckets
                    .insert(model.to_string(), TokenBucket::new(*rate, *burst));
            }
            RateLimitPolicy::SlidingWindow {
                window_secs,
                max_requests,
            } => {
                inner.windows.insert(
                    model.to_string(),
                    SlidingWindowCounter::new(Duration::from_secs_f64(*window_secs), *max_requests),
                );
            }
            RateLimitPolicy::Fixed { .. } | RateLimitPolicy::Unlimited => {}
        }
        inner.policies.insert(model.to_string(), policy);
    }

    /// Try to acquire a permit for the given model.
    pub fn try_acquire(&self, model: &str) -> ModelLimitResult {
        let mut inner = self.inner.lock().unwrap();

        // Resolve policy: explicit registration > default > unknown
        let policy = if let Some(p) = inner.policies.get(model) {
            p.clone()
        } else if let Some(ref default) = inner.default_policy {
            // Lazily create limiter for this model using the default policy
            let p = default.clone();
            drop(inner);
            self.register_model_limits(model, p.clone());
            inner = self.inner.lock().unwrap();
            p
        } else {
            return ModelLimitResult::UnknownModel;
        };

        let allowed = match &policy {
            RateLimitPolicy::TokenBucket { .. } => inner
                .buckets
                .get(model)
                .map(|b| b.try_acquire(1))
                .unwrap_or(false),
            RateLimitPolicy::SlidingWindow { .. } => inner
                .windows
                .get(model)
                .map(|w| w.try_acquire())
                .unwrap_or(false),
            RateLimitPolicy::Fixed { .. } => {
                // Fixed concurrency not tracked per-model (use BackendRateLimiter)
                true
            }
            RateLimitPolicy::Unlimited => true,
        };

        if allowed {
            ModelLimitResult::Allowed
        } else {
            ModelLimitResult::Limited
        }
    }

    /// Get the limiter's policy for a model, if registered.
    pub fn get_policy(&self, model: &str) -> Option<RateLimitPolicy> {
        let inner = self.inner.lock().unwrap();
        inner.policies.get(model).cloned()
    }

    /// Return a list of all registered model names.
    pub fn registered_models(&self) -> Vec<String> {
        let inner = self.inner.lock().unwrap();
        inner.policies.keys().cloned().collect()
    }

    /// Check whether a model has been registered.
    pub fn is_registered(&self, model: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.policies.contains_key(model)
    }
}

impl Default for ModelRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_model_returns_unknown() {
        let limiter = ModelRateLimiter::new();
        assert_eq!(limiter.try_acquire("gpt-4"), ModelLimitResult::UnknownModel);
    }

    #[test]
    fn register_token_bucket() {
        let limiter = ModelRateLimiter::new();
        limiter.register_model_limits(
            "gpt-4",
            RateLimitPolicy::TokenBucket {
                rate: 10.0,
                burst: 2,
            },
        );
        assert_eq!(limiter.try_acquire("gpt-4"), ModelLimitResult::Allowed);
        assert_eq!(limiter.try_acquire("gpt-4"), ModelLimitResult::Allowed);
        assert_eq!(limiter.try_acquire("gpt-4"), ModelLimitResult::Limited);
    }

    #[test]
    fn register_sliding_window() {
        let limiter = ModelRateLimiter::new();
        limiter.register_model_limits(
            "gpt-3.5-turbo",
            RateLimitPolicy::SlidingWindow {
                window_secs: 1.0,
                max_requests: 3,
            },
        );
        assert_eq!(
            limiter.try_acquire("gpt-3.5-turbo"),
            ModelLimitResult::Allowed
        );
        assert_eq!(
            limiter.try_acquire("gpt-3.5-turbo"),
            ModelLimitResult::Allowed
        );
        assert_eq!(
            limiter.try_acquire("gpt-3.5-turbo"),
            ModelLimitResult::Allowed
        );
        assert_eq!(
            limiter.try_acquire("gpt-3.5-turbo"),
            ModelLimitResult::Limited
        );
    }

    #[test]
    fn unlimited_always_allows() {
        let limiter = ModelRateLimiter::new();
        limiter.register_model_limits("local", RateLimitPolicy::Unlimited);
        for _ in 0..100 {
            assert_eq!(limiter.try_acquire("local"), ModelLimitResult::Allowed);
        }
    }

    #[test]
    fn per_model_isolation() {
        let limiter = ModelRateLimiter::new();
        limiter.register_model_limits(
            "gpt-4",
            RateLimitPolicy::TokenBucket {
                rate: 10.0,
                burst: 1,
            },
        );
        limiter.register_model_limits(
            "gpt-3.5-turbo",
            RateLimitPolicy::TokenBucket {
                rate: 10.0,
                burst: 1,
            },
        );
        assert_eq!(limiter.try_acquire("gpt-4"), ModelLimitResult::Allowed);
        assert_eq!(limiter.try_acquire("gpt-4"), ModelLimitResult::Limited);
        // gpt-3.5-turbo is independent
        assert_eq!(
            limiter.try_acquire("gpt-3.5-turbo"),
            ModelLimitResult::Allowed
        );
    }

    #[test]
    fn get_policy_returns_registered() {
        let limiter = ModelRateLimiter::new();
        let policy = RateLimitPolicy::TokenBucket {
            rate: 5.0,
            burst: 10,
        };
        limiter.register_model_limits("gpt-4", policy.clone());
        assert_eq!(limiter.get_policy("gpt-4"), Some(policy));
    }

    #[test]
    fn get_policy_returns_none_for_unknown() {
        let limiter = ModelRateLimiter::new();
        assert_eq!(limiter.get_policy("unknown"), None);
    }

    #[test]
    fn registered_models_list() {
        let limiter = ModelRateLimiter::new();
        limiter.register_model_limits("a", RateLimitPolicy::Unlimited);
        limiter.register_model_limits("b", RateLimitPolicy::Unlimited);
        let mut models = limiter.registered_models();
        models.sort();
        assert_eq!(models, vec!["a", "b"]);
    }

    #[test]
    fn is_registered_check() {
        let limiter = ModelRateLimiter::new();
        assert!(!limiter.is_registered("gpt-4"));
        limiter.register_model_limits("gpt-4", RateLimitPolicy::Unlimited);
        assert!(limiter.is_registered("gpt-4"));
    }

    #[test]
    fn default_impl() {
        let limiter = ModelRateLimiter::default();
        assert!(!limiter.is_registered("anything"));
    }

    #[test]
    fn default_policy_applies_to_unknown() {
        let limiter = ModelRateLimiter::new();
        limiter.set_default_policy(RateLimitPolicy::TokenBucket {
            rate: 10.0,
            burst: 2,
        });
        assert_eq!(limiter.try_acquire("new-model"), ModelLimitResult::Allowed);
        assert_eq!(limiter.try_acquire("new-model"), ModelLimitResult::Allowed);
        assert_eq!(limiter.try_acquire("new-model"), ModelLimitResult::Limited);
    }

    #[test]
    fn explicit_overrides_default() {
        let limiter = ModelRateLimiter::new();
        limiter.set_default_policy(RateLimitPolicy::TokenBucket {
            rate: 10.0,
            burst: 1,
        });
        limiter.register_model_limits(
            "special",
            RateLimitPolicy::TokenBucket {
                rate: 10.0,
                burst: 100,
            },
        );
        // explicit registration wins
        for _ in 0..50 {
            assert_eq!(limiter.try_acquire("special"), ModelLimitResult::Allowed);
        }
    }

    #[test]
    fn fixed_policy_always_allows() {
        let limiter = ModelRateLimiter::new();
        limiter.register_model_limits("m", RateLimitPolicy::Fixed { max_concurrent: 2 });
        assert_eq!(limiter.try_acquire("m"), ModelLimitResult::Allowed);
    }

    #[test]
    fn clone_shares_state() {
        let limiter = ModelRateLimiter::new();
        limiter.register_model_limits(
            "m",
            RateLimitPolicy::TokenBucket {
                rate: 10.0,
                burst: 2,
            },
        );
        let clone = limiter.clone();
        limiter.try_acquire("m");
        limiter.try_acquire("m");
        assert_eq!(clone.try_acquire("m"), ModelLimitResult::Limited);
    }
}
