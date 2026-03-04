#![allow(dead_code, unused_imports)]

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

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
        assert_eq!(
            config.policy_for("anything"),
            &RateLimitPolicy::Unlimited
        );
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
        assert_eq!(
            config.policy_for("other"),
            &RateLimitPolicy::Unlimited
        );
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
            RateLimitPolicy::SlidingWindow { max_requests: 100, .. }
        ));
        assert_eq!(
            config.policy_for("local"),
            &RateLimitPolicy::Unlimited
        );
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
}
