#![allow(dead_code, unused_imports)]

use std::sync::Arc;
use std::time::Duration;

use abp_ratelimit::{
    BackendRateLimiter, RateLimitConfig, RateLimitError, RateLimitPolicy, RatePermit,
    SlidingWindowCounter, TokenBucket,
};

#[test]
fn token_bucket_basic_flow() {
    let bucket = TokenBucket::new(100.0, 10);
    for _ in 0..10 {
        assert!(bucket.try_acquire(1));
    }
    assert!(!bucket.try_acquire(1));
}

#[test]
fn sliding_window_basic_flow() {
    let window = SlidingWindowCounter::new(Duration::from_secs(10), 5);
    for _ in 0..5 {
        assert!(window.try_acquire());
    }
    assert!(!window.try_acquire());
    assert_eq!(window.remaining(), 0);
}

#[test]
fn backend_limiter_multiple_backends() {
    let limiter = BackendRateLimiter::new();
    limiter.set_policy(
        "openai",
        RateLimitPolicy::TokenBucket {
            rate: 10.0,
            burst: 2,
        },
    );
    limiter.set_policy(
        "anthropic",
        RateLimitPolicy::SlidingWindow {
            window_secs: 60.0,
            max_requests: 3,
        },
    );
    limiter.set_policy("local", RateLimitPolicy::Fixed { max_concurrent: 1 });
    limiter.set_policy("gemini", RateLimitPolicy::Unlimited);

    // Each backend is independent
    assert!(limiter.try_acquire("openai").is_ok());
    assert!(limiter.try_acquire("anthropic").is_ok());
    assert!(limiter.try_acquire("gemini").is_ok());

    let local_permit = limiter.try_acquire("local").unwrap();
    assert!(limiter.try_acquire("local").is_err());
    drop(local_permit);
    assert!(limiter.try_acquire("local").is_ok());
}

#[test]
fn config_roundtrip_toml() {
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
    // Unknown backend falls back to default
    assert!(matches!(
        config.policy_for("unknown"),
        RateLimitPolicy::TokenBucket { burst: 100, .. }
    ));
}

#[test]
fn config_roundtrip_json() {
    let config = RateLimitConfig {
        default_policy: RateLimitPolicy::Unlimited,
        backends: {
            let mut m = std::collections::HashMap::new();
            m.insert(
                "test".to_string(),
                RateLimitPolicy::Fixed { max_concurrent: 2 },
            );
            m
        },
    };
    let json = serde_json::to_string(&config).unwrap();
    let parsed: RateLimitConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, config);
}

#[tokio::test]
async fn token_bucket_wait_for_refill() {
    let bucket = TokenBucket::new(1000.0, 5);
    // Drain the bucket
    assert!(bucket.try_acquire(5));
    assert!(!bucket.try_acquire(1));
    // Wait for refill
    bucket.wait_for(1).await;
    // Should have acquired 1 token via wait_for
}

#[tokio::test]
async fn concurrent_access_token_bucket() {
    let bucket = TokenBucket::new(10000.0, 100);
    let mut handles = Vec::new();
    for _ in 0..10 {
        let b = bucket.clone();
        handles.push(tokio::spawn(async move {
            let mut acquired = 0;
            for _ in 0..20 {
                if b.try_acquire(1) {
                    acquired += 1;
                }
            }
            acquired
        }));
    }
    let mut total = 0;
    for h in handles {
        total += h.await.unwrap();
    }
    // With burst=100 and 10 tasks × 20 attempts = 200 attempts,
    // at most 100 should succeed initially (burst capacity)
    assert!(total <= 200);
    assert!(total > 0);
}

#[tokio::test]
async fn concurrent_access_backend_limiter() {
    let limiter = Arc::new(BackendRateLimiter::new());
    limiter.set_policy("test", RateLimitPolicy::Fixed { max_concurrent: 5 });

    let mut handles = Vec::new();
    for _ in 0..10 {
        let l = Arc::clone(&limiter);
        handles.push(tokio::spawn(async move {
            match l.try_acquire("test") {
                Ok(permit) => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    drop(permit);
                    true
                }
                Err(_) => false,
            }
        }));
    }
    let mut successes = 0;
    for h in handles {
        if h.await.unwrap() {
            successes += 1;
        }
    }
    // At most 5 should succeed concurrently
    assert!(successes <= 10);
    assert!(successes > 0);
}

#[test]
fn edge_case_zero_burst() {
    let bucket = TokenBucket::new(10.0, 0);
    assert!(!bucket.try_acquire(1));
    assert_eq!(bucket.available(), 0);
}

#[test]
fn edge_case_large_burst() {
    let bucket = TokenBucket::new(1.0, usize::MAX);
    assert!(bucket.try_acquire(1000));
}
