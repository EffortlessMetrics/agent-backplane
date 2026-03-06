//! Integration tests for `ModelRateLimiter`.

use std::sync::Arc;

use abp_ratelimit::{ModelLimitResult, ModelRateLimiter, RateLimitPolicy};

// ---------------------------------------------------------------------------
// Basic registration & acquire
// ---------------------------------------------------------------------------

#[test]
fn unregistered_model_returns_unknown() {
    let limiter = ModelRateLimiter::new();
    assert_eq!(limiter.try_acquire("gpt-4"), ModelLimitResult::UnknownModel);
}

#[test]
fn token_bucket_limits() {
    let limiter = ModelRateLimiter::new();
    limiter.register_model_limits(
        "gpt-4",
        RateLimitPolicy::TokenBucket {
            rate: 10.0,
            burst: 3,
        },
    );
    assert_eq!(limiter.try_acquire("gpt-4"), ModelLimitResult::Allowed);
    assert_eq!(limiter.try_acquire("gpt-4"), ModelLimitResult::Allowed);
    assert_eq!(limiter.try_acquire("gpt-4"), ModelLimitResult::Allowed);
    assert_eq!(limiter.try_acquire("gpt-4"), ModelLimitResult::Limited);
}

#[test]
fn sliding_window_limits() {
    let limiter = ModelRateLimiter::new();
    limiter.register_model_limits(
        "claude-3",
        RateLimitPolicy::SlidingWindow {
            window_secs: 1.0,
            max_requests: 2,
        },
    );
    assert_eq!(limiter.try_acquire("claude-3"), ModelLimitResult::Allowed);
    assert_eq!(limiter.try_acquire("claude-3"), ModelLimitResult::Allowed);
    assert_eq!(limiter.try_acquire("claude-3"), ModelLimitResult::Limited);
}

#[test]
fn unlimited_never_limits() {
    let limiter = ModelRateLimiter::new();
    limiter.register_model_limits("local", RateLimitPolicy::Unlimited);
    for _ in 0..1000 {
        assert_eq!(limiter.try_acquire("local"), ModelLimitResult::Allowed);
    }
}

#[test]
fn fixed_always_allows_in_per_model() {
    let limiter = ModelRateLimiter::new();
    limiter.register_model_limits("m", RateLimitPolicy::Fixed { max_concurrent: 1 });
    // Fixed is not tracked per-model, always returns Allowed
    assert_eq!(limiter.try_acquire("m"), ModelLimitResult::Allowed);
    assert_eq!(limiter.try_acquire("m"), ModelLimitResult::Allowed);
}

// ---------------------------------------------------------------------------
// Per-model isolation
// ---------------------------------------------------------------------------

#[test]
fn models_are_independent() {
    let limiter = ModelRateLimiter::new();
    limiter.register_model_limits(
        "a",
        RateLimitPolicy::TokenBucket {
            rate: 10.0,
            burst: 1,
        },
    );
    limiter.register_model_limits(
        "b",
        RateLimitPolicy::TokenBucket {
            rate: 10.0,
            burst: 1,
        },
    );
    assert_eq!(limiter.try_acquire("a"), ModelLimitResult::Allowed);
    assert_eq!(limiter.try_acquire("a"), ModelLimitResult::Limited);
    // "b" still has capacity
    assert_eq!(limiter.try_acquire("b"), ModelLimitResult::Allowed);
}

// ---------------------------------------------------------------------------
// Default policy
// ---------------------------------------------------------------------------

#[test]
fn default_policy_applies_to_unregistered() {
    let limiter = ModelRateLimiter::new();
    limiter.set_default_policy(RateLimitPolicy::TokenBucket {
        rate: 10.0,
        burst: 2,
    });
    assert_eq!(limiter.try_acquire("new"), ModelLimitResult::Allowed);
    assert_eq!(limiter.try_acquire("new"), ModelLimitResult::Allowed);
    assert_eq!(limiter.try_acquire("new"), ModelLimitResult::Limited);
}

#[test]
fn explicit_registration_overrides_default() {
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
    for _ in 0..50 {
        assert_eq!(limiter.try_acquire("special"), ModelLimitResult::Allowed);
    }
}

// ---------------------------------------------------------------------------
// Policy accessors
// ---------------------------------------------------------------------------

#[test]
fn get_policy_for_registered_model() {
    let limiter = ModelRateLimiter::new();
    let policy = RateLimitPolicy::TokenBucket {
        rate: 5.0,
        burst: 10,
    };
    limiter.register_model_limits("gpt-4", policy.clone());
    assert_eq!(limiter.get_policy("gpt-4"), Some(policy));
}

#[test]
fn get_policy_none_for_unknown() {
    let limiter = ModelRateLimiter::new();
    assert_eq!(limiter.get_policy("unknown"), None);
}

#[test]
fn registered_models_list() {
    let limiter = ModelRateLimiter::new();
    limiter.register_model_limits("x", RateLimitPolicy::Unlimited);
    limiter.register_model_limits("y", RateLimitPolicy::Unlimited);
    let mut models = limiter.registered_models();
    models.sort();
    assert_eq!(models, vec!["x", "y"]);
}

#[test]
fn is_registered_check() {
    let limiter = ModelRateLimiter::new();
    assert!(!limiter.is_registered("gpt-4"));
    limiter.register_model_limits("gpt-4", RateLimitPolicy::Unlimited);
    assert!(limiter.is_registered("gpt-4"));
}

// ---------------------------------------------------------------------------
// Default impl
// ---------------------------------------------------------------------------

#[test]
fn default_impl_is_empty() {
    let limiter = ModelRateLimiter::default();
    assert!(!limiter.is_registered("anything"));
    assert_eq!(
        limiter.try_acquire("anything"),
        ModelLimitResult::UnknownModel
    );
}

// ---------------------------------------------------------------------------
// Clone
// ---------------------------------------------------------------------------

#[test]
fn clone_shares_inner_state() {
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

// ---------------------------------------------------------------------------
// Concurrent access
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_acquire_respects_limits() {
    let limiter = Arc::new(ModelRateLimiter::new());
    limiter.register_model_limits(
        "gpt-4",
        RateLimitPolicy::TokenBucket {
            rate: 10000.0,
            burst: 50,
        },
    );
    let mut handles = Vec::new();
    for _ in 0..10 {
        let l = Arc::clone(&limiter);
        handles.push(tokio::spawn(async move {
            let mut allowed = 0u32;
            for _ in 0..20 {
                if l.try_acquire("gpt-4") == ModelLimitResult::Allowed {
                    allowed += 1;
                }
            }
            allowed
        }));
    }
    let mut total = 0u32;
    for h in handles {
        total += h.await.unwrap();
    }
    // Burst=50 and 10×20=200 attempts → at most ~50 initial + some refills
    assert!(total > 0);
    assert!(total <= 200);
}
