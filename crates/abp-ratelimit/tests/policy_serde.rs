//! Integration tests for serde roundtrips and configuration parsing.

use std::collections::HashMap;

use abp_ratelimit::{FallbackStrategy, RateLimitConfig, RateLimitPolicy};

// ---------------------------------------------------------------------------
// RateLimitPolicy JSON roundtrip
// ---------------------------------------------------------------------------

#[test]
fn policy_token_bucket_json_roundtrip() {
    let policy = RateLimitPolicy::TokenBucket {
        rate: 10.0,
        burst: 20,
    };
    let json = serde_json::to_string(&policy).unwrap();
    let parsed: RateLimitPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, policy);
}

#[test]
fn policy_sliding_window_json_roundtrip() {
    let policy = RateLimitPolicy::SlidingWindow {
        window_secs: 60.0,
        max_requests: 100,
    };
    let json = serde_json::to_string(&policy).unwrap();
    let parsed: RateLimitPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, policy);
}

#[test]
fn policy_fixed_json_roundtrip() {
    let policy = RateLimitPolicy::Fixed { max_concurrent: 4 };
    let json = serde_json::to_string(&policy).unwrap();
    let parsed: RateLimitPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, policy);
}

#[test]
fn policy_unlimited_json_roundtrip() {
    let policy = RateLimitPolicy::Unlimited;
    let json = serde_json::to_string(&policy).unwrap();
    let parsed: RateLimitPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, policy);
}

// ---------------------------------------------------------------------------
// FallbackStrategy serde
// ---------------------------------------------------------------------------

#[test]
fn fallback_strategy_roundtrip() {
    for strat in [
        FallbackStrategy::AllMustAllow,
        FallbackStrategy::AnyMustAllow,
    ] {
        let json = serde_json::to_string(&strat).unwrap();
        let parsed: FallbackStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, strat);
    }
}

#[test]
fn fallback_strategy_snake_case_format() {
    let json = serde_json::to_string(&FallbackStrategy::AllMustAllow).unwrap();
    assert!(json.contains("all_must_allow"));
    let json = serde_json::to_string(&FallbackStrategy::AnyMustAllow).unwrap();
    assert!(json.contains("any_must_allow"));
}

// ---------------------------------------------------------------------------
// RateLimitConfig TOML parsing
// ---------------------------------------------------------------------------

#[test]
fn config_full_toml() {
    let toml_str = r#"
[default_policy]
type = "token_bucket"
rate = 50.0
burst = 100

[backends.openai]
type = "sliding_window"
window_secs = 60.0
max_requests = 1000

[backends.anthropic]
type = "token_bucket"
rate = 20.0
burst = 40

[backends.local]
type = "fixed"
max_concurrent = 4

[backends.gemini]
type = "unlimited"
"#;
    let config: RateLimitConfig = toml::from_str(toml_str).unwrap();
    assert!(matches!(
        config.policy_for("openai"),
        RateLimitPolicy::SlidingWindow {
            max_requests: 1000,
            ..
        }
    ));
    assert!(matches!(
        config.policy_for("anthropic"),
        RateLimitPolicy::TokenBucket { burst: 40, .. }
    ));
    assert_eq!(
        config.policy_for("local"),
        &RateLimitPolicy::Fixed { max_concurrent: 4 }
    );
    assert_eq!(config.policy_for("gemini"), &RateLimitPolicy::Unlimited);
    // Unknown falls back to default
    assert!(matches!(
        config.policy_for("unknown"),
        RateLimitPolicy::TokenBucket { burst: 100, .. }
    ));
}

#[test]
fn config_empty_toml_uses_defaults() {
    let config: RateLimitConfig = toml::from_str("").unwrap();
    assert_eq!(config.default_policy, RateLimitPolicy::Unlimited);
    assert!(config.backends.is_empty());
}

// ---------------------------------------------------------------------------
// RateLimitConfig JSON parsing
// ---------------------------------------------------------------------------

#[test]
fn config_json_roundtrip() {
    let config = RateLimitConfig {
        default_policy: RateLimitPolicy::TokenBucket {
            rate: 10.0,
            burst: 20,
        },
        backends: {
            let mut m = HashMap::new();
            m.insert(
                "openai".to_string(),
                RateLimitPolicy::SlidingWindow {
                    window_secs: 60.0,
                    max_requests: 500,
                },
            );
            m.insert(
                "local".to_string(),
                RateLimitPolicy::Fixed { max_concurrent: 2 },
            );
            m
        },
    };
    let json = serde_json::to_string_pretty(&config).unwrap();
    let parsed: RateLimitConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, config);
}

#[test]
fn config_from_json_string() {
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

// ---------------------------------------------------------------------------
// RateLimitConfig default
// ---------------------------------------------------------------------------

#[test]
fn config_default_is_unlimited_no_backends() {
    let config = RateLimitConfig::default();
    assert_eq!(config.default_policy, RateLimitPolicy::Unlimited);
    assert!(config.backends.is_empty());
    assert_eq!(config.policy_for("any"), &RateLimitPolicy::Unlimited);
}

// ---------------------------------------------------------------------------
// policy_for fallback
// ---------------------------------------------------------------------------

#[test]
fn policy_for_returns_override_when_present() {
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
        RateLimitPolicy::TokenBucket { burst: 10, .. }
    ));
    // Unknown gets default
    assert_eq!(config.policy_for("other"), &RateLimitPolicy::Unlimited);
}
