#![allow(dead_code, unused_imports)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::sliding_window::SlidingWindowCounter;
use crate::token_bucket::TokenBucket;

/// A rate limit policy specifying which algorithm to use.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RateLimitPolicy {
    /// Token bucket algorithm with a steady fill rate and burst capacity.
    TokenBucket {
        /// Tokens added per second.
        rate: f64,
        /// Maximum token capacity.
        burst: usize,
    },
    /// Sliding window counter with a time window and max requests.
    SlidingWindow {
        /// Window duration in seconds.
        window_secs: f64,
        /// Maximum requests within the window.
        max_requests: usize,
    },
    /// Fixed concurrency limit (max in-flight requests).
    Fixed {
        /// Maximum concurrent requests.
        max_concurrent: usize,
    },
    /// No rate limiting applied.
    Unlimited,
}

/// Configuration for rate limiting with per-backend overrides.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RateLimitConfig {
    /// Default policy applied to backends without a specific override.
    #[serde(default = "default_policy")]
    pub default_policy: RateLimitPolicy,
    /// Per-backend policy overrides keyed by backend ID.
    #[serde(default)]
    pub backends: HashMap<String, RateLimitPolicy>,
}

fn default_policy() -> RateLimitPolicy {
    RateLimitPolicy::Unlimited
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            default_policy: RateLimitPolicy::Unlimited,
            backends: HashMap::new(),
        }
    }
}

impl RateLimitConfig {
    /// Return the effective policy for a backend, falling back to the default.
    pub fn policy_for(&self, backend_id: &str) -> &RateLimitPolicy {
        self.backends
            .get(backend_id)
            .unwrap_or(&self.default_policy)
    }
}

/// Strategy for combining multiple rate limiters in a chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackStrategy {
    /// All limiters must allow the request (strictest).
    AllMustAllow,
    /// At least one limiter must allow the request (most lenient).
    AnyMustAllow,
}

/// Internal limiter variant used by [`RateLimitChain`].
#[derive(Debug)]
enum Limiter {
    Bucket(TokenBucket),
    Window(SlidingWindowCounter),
}

impl Limiter {
    fn try_acquire(&self) -> bool {
        match self {
            Limiter::Bucket(b) => b.try_acquire(1),
            Limiter::Window(w) => w.try_acquire(),
        }
    }
}

/// A chain of rate limiters evaluated with a configurable combination strategy.
///
/// Combines multiple [`TokenBucket`] and [`SlidingWindowCounter`] instances
/// and evaluates them according to a [`FallbackStrategy`].
#[derive(Debug, Clone)]
pub struct RateLimitChain {
    inner: Arc<Mutex<ChainInner>>,
}

#[derive(Debug)]
struct ChainInner {
    limiters: Vec<Limiter>,
    strategy: FallbackStrategy,
}

impl RateLimitChain {
    /// Create a new chain with the given combination strategy.
    pub fn new(strategy: FallbackStrategy) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ChainInner {
                limiters: Vec::new(),
                strategy,
            })),
        }
    }

    /// Add a token bucket limiter to the chain.
    pub fn add_token_bucket(&self, rate: f64, burst: usize) {
        let mut inner = self.inner.lock().unwrap();
        inner
            .limiters
            .push(Limiter::Bucket(TokenBucket::new(rate, burst)));
    }

    /// Add a sliding window limiter to the chain.
    pub fn add_sliding_window(&self, window: Duration, max_requests: usize) {
        let mut inner = self.inner.lock().unwrap();
        inner
            .limiters
            .push(Limiter::Window(SlidingWindowCounter::new(
                window,
                max_requests,
            )));
    }

    /// Try to acquire a single permit through the chain.
    ///
    /// With [`FallbackStrategy::AllMustAllow`], every limiter must allow.
    /// With [`FallbackStrategy::AnyMustAllow`], at least one must allow.
    pub fn try_acquire(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        match inner.strategy {
            FallbackStrategy::AllMustAllow => inner.limiters.iter().all(|l| l.try_acquire()),
            FallbackStrategy::AnyMustAllow => inner.limiters.iter().any(|l| l.try_acquire()),
        }
    }

    /// Return the number of limiters in the chain.
    pub fn len(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        inner.limiters.len()
    }

    /// Return whether the chain has no limiters.
    pub fn is_empty(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.limiters.is_empty()
    }

    /// Return the configured strategy.
    pub fn strategy(&self) -> FallbackStrategy {
        let inner = self.inner.lock().unwrap();
        inner.strategy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_token_bucket() {
        let policy = RateLimitPolicy::TokenBucket {
            rate: 10.0,
            burst: 20,
        };
        let json = serde_json::to_string(&policy).unwrap();
        assert!(json.contains("token_bucket"));
        let parsed: RateLimitPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, policy);
    }

    #[test]
    fn serialize_sliding_window() {
        let policy = RateLimitPolicy::SlidingWindow {
            window_secs: 60.0,
            max_requests: 100,
        };
        let json = serde_json::to_string(&policy).unwrap();
        assert!(json.contains("sliding_window"));
        let parsed: RateLimitPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, policy);
    }

    #[test]
    fn serialize_fixed() {
        let policy = RateLimitPolicy::Fixed { max_concurrent: 5 };
        let json = serde_json::to_string(&policy).unwrap();
        let parsed: RateLimitPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, policy);
    }

    #[test]
    fn serialize_unlimited() {
        let policy = RateLimitPolicy::Unlimited;
        let json = serde_json::to_string(&policy).unwrap();
        let parsed: RateLimitPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, policy);
    }

    #[test]
    fn config_default() {
        let config = RateLimitConfig::default();
        assert_eq!(config.default_policy, RateLimitPolicy::Unlimited);
        assert!(config.backends.is_empty());
    }

    #[test]
    fn config_policy_for_default_fallback() {
        let config = RateLimitConfig::default();
        assert_eq!(config.policy_for("anything"), &RateLimitPolicy::Unlimited);
    }

    #[test]
    fn config_policy_for_override() {
        let mut config = RateLimitConfig::default();
        config.backends.insert(
            "openai".to_string(),
            RateLimitPolicy::TokenBucket {
                rate: 5.0,
                burst: 10,
            },
        );
        assert!(matches!(
            config.policy_for("openai"),
            RateLimitPolicy::TokenBucket { rate, burst } if (*rate - 5.0).abs() < f64::EPSILON && *burst == 10
        ));
        assert_eq!(config.policy_for("other"), &RateLimitPolicy::Unlimited);
    }

    #[test]
    fn config_from_toml() {
        let toml_str = r#"
[default_policy]
type = "token_bucket"
rate = 10.0
burst = 20

[backends.openai]
type = "sliding_window"
window_secs = 60.0
max_requests = 100

[backends.local]
type = "unlimited"
"#;
        let config: RateLimitConfig = toml::from_str(toml_str).unwrap();
        assert!(matches!(
            config.default_policy,
            RateLimitPolicy::TokenBucket { rate, burst } if (rate - 10.0).abs() < f64::EPSILON && burst == 20
        ));
        assert!(matches!(
            config.policy_for("openai"),
            RateLimitPolicy::SlidingWindow {
                max_requests: 100,
                ..
            }
        ));
        assert_eq!(config.policy_for("local"), &RateLimitPolicy::Unlimited);
    }

    #[test]
    fn config_from_json() {
        let json_str = r#"{
            "default_policy": { "type": "unlimited" },
            "backends": {
                "anthropic": { "type": "fixed", "max_concurrent": 3 }
            }
        }"#;
        let config: RateLimitConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(config.default_policy, RateLimitPolicy::Unlimited);
        assert_eq!(
            config.policy_for("anthropic"),
            &RateLimitPolicy::Fixed { max_concurrent: 3 }
        );
    }

    #[test]
    fn empty_toml_uses_defaults() {
        let config: RateLimitConfig = toml::from_str("").unwrap();
        assert_eq!(config.default_policy, RateLimitPolicy::Unlimited);
        assert!(config.backends.is_empty());
    }

    // --- RateLimitChain tests ---

    #[test]
    fn chain_all_must_allow_succeeds_when_all_pass() {
        let chain = RateLimitChain::new(FallbackStrategy::AllMustAllow);
        chain.add_token_bucket(100.0, 10);
        chain.add_sliding_window(Duration::from_secs(60), 10);
        assert!(chain.try_acquire());
    }

    #[test]
    fn chain_all_must_allow_fails_when_one_exhausted() {
        let chain = RateLimitChain::new(FallbackStrategy::AllMustAllow);
        chain.add_token_bucket(100.0, 1);
        chain.add_sliding_window(Duration::from_secs(60), 10);
        assert!(chain.try_acquire()); // consumes from both
        assert!(!chain.try_acquire()); // bucket exhausted
    }

    #[test]
    fn chain_any_must_allow_succeeds_when_one_passes() {
        let chain = RateLimitChain::new(FallbackStrategy::AnyMustAllow);
        chain.add_token_bucket(100.0, 0); // always denied
        chain.add_sliding_window(Duration::from_secs(60), 10); // allows
        assert!(chain.try_acquire());
    }

    #[test]
    fn chain_any_must_allow_fails_when_all_exhausted() {
        let chain = RateLimitChain::new(FallbackStrategy::AnyMustAllow);
        chain.add_token_bucket(100.0, 0);
        chain.add_sliding_window(Duration::from_secs(60), 0);
        assert!(!chain.try_acquire());
    }

    #[test]
    fn chain_empty_all_must_allow_succeeds() {
        let chain = RateLimitChain::new(FallbackStrategy::AllMustAllow);
        // No limiters — vacuously true
        assert!(chain.try_acquire());
    }

    #[test]
    fn chain_empty_any_must_allow_fails() {
        let chain = RateLimitChain::new(FallbackStrategy::AnyMustAllow);
        // No limiters — vacuously false
        assert!(!chain.try_acquire());
    }

    #[test]
    fn chain_len_and_is_empty() {
        let chain = RateLimitChain::new(FallbackStrategy::AllMustAllow);
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
        chain.add_token_bucket(10.0, 5);
        assert!(!chain.is_empty());
        assert_eq!(chain.len(), 1);
    }

    #[test]
    fn chain_strategy_accessor() {
        let chain = RateLimitChain::new(FallbackStrategy::AnyMustAllow);
        assert_eq!(chain.strategy(), FallbackStrategy::AnyMustAllow);
    }

    #[test]
    fn chain_clone_shares_state() {
        let chain = RateLimitChain::new(FallbackStrategy::AllMustAllow);
        chain.add_token_bucket(100.0, 2);
        let clone = chain.clone();
        chain.try_acquire();
        chain.try_acquire();
        assert!(!clone.try_acquire());
    }

    #[test]
    fn fallback_strategy_serde_roundtrip() {
        let strat = FallbackStrategy::AllMustAllow;
        let json = serde_json::to_string(&strat).unwrap();
        let parsed: FallbackStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, strat);

        let strat2 = FallbackStrategy::AnyMustAllow;
        let json2 = serde_json::to_string(&strat2).unwrap();
        let parsed2: FallbackStrategy = serde_json::from_str(&json2).unwrap();
        assert_eq!(parsed2, strat2);
    }
}
