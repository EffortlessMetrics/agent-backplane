#![warn(missing_docs)]
#![allow(dead_code, unused_imports)]
//! Rate limiting primitives for Agent Backplane backends.
//!
//! This crate provides rate limiting strategies for controlling throughput
//! to backends:
//!
//! - [`TokenBucket`]: Classic token bucket algorithm with configurable rate and burst.
//! - [`SlidingWindowCounter`]: Sliding window counter for request rate tracking.
//! - [`BackendRateLimiter`]: Per-backend rate limiting keyed by backend ID.
//! - [`RateLimitPolicy`]: Policy configuration for selecting rate limit strategy.
//!
//! # Examples
//!
//! ## Token bucket
//!
//! ```rust
//! use abp_ratelimit::TokenBucket;
//!
//! let bucket = TokenBucket::new(10.0, 20);
//! assert!(bucket.try_acquire(1));
//! ```
//!
//! ## Per-backend limiter
//!
//! ```rust
//! use abp_ratelimit::{BackendRateLimiter, RateLimitPolicy};
//!
//! let limiter = BackendRateLimiter::new();
//! limiter.set_policy("openai", RateLimitPolicy::TokenBucket { rate: 10.0, burst: 20 });
//! let permit = limiter.try_acquire("openai");
//! assert!(permit.is_ok());
//! ```

mod adaptive;
mod backend_limiter;
mod circuit_breaker;
mod metrics;
mod per_model;
mod policy;
mod sliding_window;
mod token_bucket;

pub use adaptive::AdaptiveLimiter;
pub use backend_limiter::{BackendRateLimiter, RateLimitError, RatePermit};
pub use circuit_breaker::{CircuitBreaker, CircuitBreakerError, CircuitState};
pub use metrics::{MetricsSnapshot, RateLimitMetrics};
pub use per_model::{ModelLimitResult, ModelRateLimiter};
pub use policy::{RateLimitConfig, RateLimitPolicy};
pub use sliding_window::SlidingWindowCounter;
pub use token_bucket::TokenBucket;
