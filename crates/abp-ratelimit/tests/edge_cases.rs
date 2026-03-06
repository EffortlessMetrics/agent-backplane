//! Edge case tests for rate limiting primitives.

use std::sync::Arc;
use std::time::Duration;

use abp_ratelimit::{
    BackendRateLimiter, FallbackStrategy, RateLimitChain, RateLimitError, RateLimitPolicy,
    SlidingWindowCounter, TokenBucket,
};

// ---------------------------------------------------------------------------
// TokenBucket edge cases
// ---------------------------------------------------------------------------

#[test]
fn token_bucket_zero_burst_always_denies() {
    let bucket = TokenBucket::new(100.0, 0);
    assert!(!bucket.try_acquire(1));
    assert_eq!(bucket.available(), 0);
}

#[test]
fn token_bucket_zero_rate_no_refill() {
    let bucket = TokenBucket::new(0.0, 5);
    assert!(bucket.try_acquire(5));
    assert!(!bucket.try_acquire(1));
    // Even after sleeping, no refill
    std::thread::sleep(Duration::from_millis(20));
    assert!(!bucket.try_acquire(1));
}

#[test]
fn token_bucket_zero_acquire_always_succeeds() {
    let bucket = TokenBucket::new(10.0, 0);
    assert!(bucket.try_acquire(0));
}

#[test]
fn token_bucket_large_burst() {
    let bucket = TokenBucket::new(1.0, usize::MAX);
    assert!(bucket.try_acquire(1000));
    assert!(bucket.try_acquire(1000));
}

#[test]
fn token_bucket_burst_cap_after_time() {
    let bucket = TokenBucket::new(10000.0, 5);
    std::thread::sleep(Duration::from_millis(50));
    assert!(bucket.available() <= 5);
}

#[test]
fn token_bucket_exact_depletion_and_refill() {
    let bucket = TokenBucket::new(1000.0, 10);
    // Drain completely
    assert!(bucket.try_acquire(10));
    assert_eq!(bucket.available(), 0);
    assert!(!bucket.try_acquire(1));
    // Wait for some refill
    std::thread::sleep(Duration::from_millis(20));
    assert!(bucket.available() > 0);
}

#[tokio::test]
async fn token_bucket_wait_for_zero_rate_returns_immediately() {
    let bucket = TokenBucket::new(0.0, 5);
    bucket.try_acquire(5);
    // Should not hang — zero rate returns immediately
    bucket.wait_for(1).await;
}

#[tokio::test]
async fn token_bucket_wait_for_refill_works() {
    let bucket = TokenBucket::new(1000.0, 10);
    bucket.try_acquire(10);
    bucket.wait_for(1).await;
    // wait_for consumed 1 token, so should have been able to proceed
}

// ---------------------------------------------------------------------------
// SlidingWindowCounter edge cases
// ---------------------------------------------------------------------------

#[test]
fn sliding_window_zero_max_always_denies() {
    let counter = SlidingWindowCounter::new(Duration::from_secs(1), 0);
    assert!(!counter.try_acquire());
    assert_eq!(counter.remaining(), 0);
}

#[test]
fn sliding_window_expiry_restores_capacity() {
    let counter = SlidingWindowCounter::new(Duration::from_millis(30), 2);
    assert!(counter.try_acquire());
    assert!(counter.try_acquire());
    assert!(!counter.try_acquire());
    std::thread::sleep(Duration::from_millis(50));
    assert!(counter.try_acquire());
}

#[test]
fn sliding_window_reset_at_future_when_full() {
    let counter = SlidingWindowCounter::new(Duration::from_secs(10), 1);
    counter.try_acquire();
    let reset = counter.reset_at();
    assert!(reset > std::time::Instant::now());
}

#[test]
fn sliding_window_reset_at_now_when_empty() {
    let counter = SlidingWindowCounter::new(Duration::from_secs(1), 5);
    let reset = counter.reset_at();
    assert!(reset <= std::time::Instant::now() + Duration::from_millis(10));
}

#[test]
fn sliding_window_accessors() {
    let counter = SlidingWindowCounter::new(Duration::from_secs(30), 100);
    assert_eq!(counter.window(), Duration::from_secs(30));
    assert_eq!(counter.max_requests(), 100);
}

// ---------------------------------------------------------------------------
// BackendRateLimiter edge cases
// ---------------------------------------------------------------------------

#[test]
fn backend_limiter_no_policy_error() {
    let limiter = BackendRateLimiter::new();
    let err = limiter.try_acquire("unknown").unwrap_err();
    assert!(matches!(err, RateLimitError::NoPolicyConfigured { .. }));
}

#[test]
fn backend_limiter_unlimited_no_limit() {
    let limiter = BackendRateLimiter::new();
    limiter.set_policy("test", RateLimitPolicy::Unlimited);
    for _ in 0..1000 {
        assert!(limiter.try_acquire("test").is_ok());
    }
}

#[test]
fn backend_limiter_fixed_permit_drop_releases() {
    let limiter = BackendRateLimiter::new();
    limiter.set_policy("test", RateLimitPolicy::Fixed { max_concurrent: 1 });
    let permit = limiter.try_acquire("test").unwrap();
    assert!(limiter.try_acquire("test").is_err());
    assert_eq!(limiter.active_permits("test"), 1);
    drop(permit);
    assert_eq!(limiter.active_permits("test"), 0);
    assert!(limiter.try_acquire("test").is_ok());
}

#[test]
fn backend_limiter_has_policy() {
    let limiter = BackendRateLimiter::new();
    assert!(!limiter.has_policy("x"));
    limiter.set_policy("x", RateLimitPolicy::Unlimited);
    assert!(limiter.has_policy("x"));
}

#[test]
fn backend_limiter_permit_backend_id() {
    let limiter = BackendRateLimiter::new();
    limiter.set_policy("mybackend", RateLimitPolicy::Unlimited);
    let permit = limiter.try_acquire("mybackend").unwrap();
    assert_eq!(permit.backend_id(), "mybackend");
}

#[test]
fn backend_limiter_error_display() {
    let err = RateLimitError::Limited {
        backend_id: "openai".to_string(),
    };
    assert!(format!("{err}").contains("openai"));
    let err2 = RateLimitError::NoPolicyConfigured {
        backend_id: "test".to_string(),
    };
    assert!(format!("{err2}").contains("test"));
}

// ---------------------------------------------------------------------------
// RateLimitChain edge cases
// ---------------------------------------------------------------------------

#[test]
fn chain_empty_all_must_allow_is_vacuously_true() {
    let chain = RateLimitChain::new(FallbackStrategy::AllMustAllow);
    assert!(chain.try_acquire());
}

#[test]
fn chain_empty_any_must_allow_is_vacuously_false() {
    let chain = RateLimitChain::new(FallbackStrategy::AnyMustAllow);
    assert!(!chain.try_acquire());
}

#[test]
fn chain_single_limiter_behaves_as_that_limiter() {
    let chain = RateLimitChain::new(FallbackStrategy::AllMustAllow);
    chain.add_token_bucket(100.0, 3);
    assert!(chain.try_acquire());
    assert!(chain.try_acquire());
    assert!(chain.try_acquire());
    assert!(!chain.try_acquire());
}

#[test]
fn chain_len_is_empty() {
    let chain = RateLimitChain::new(FallbackStrategy::AllMustAllow);
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
    chain.add_token_bucket(10.0, 5);
    chain.add_sliding_window(Duration::from_secs(60), 10);
    assert!(!chain.is_empty());
    assert_eq!(chain.len(), 2);
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

// ---------------------------------------------------------------------------
// Concurrent token bucket
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_token_bucket_total_respects_burst() {
    let bucket = TokenBucket::new(0.0, 100); // zero rate = no refill
    let bucket = Arc::new(bucket);
    let mut handles = Vec::new();
    for _ in 0..20 {
        let b = Arc::clone(&bucket);
        handles.push(tokio::spawn(async move {
            let mut count = 0u32;
            for _ in 0..10 {
                if b.try_acquire(1) {
                    count += 1;
                }
            }
            count
        }));
    }
    let mut total = 0u32;
    for h in handles {
        total += h.await.unwrap();
    }
    // With zero rate and burst=100, exactly 100 should succeed
    assert_eq!(total, 100);
}
