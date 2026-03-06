# abp-ratelimit

Rate limiting primitives for Agent Backplane backends.

Provides token bucket, sliding window, and per-backend rate limiters with
configurable policies. Backends are keyed by ID so each provider gets
independent throughput control, and an adaptive limiter adjusts rates based
on observed response headers.

## Key Types

| Type | Description |
|------|-------------|
| `TokenBucket` | Classic token bucket algorithm with configurable rate and burst |
| `SlidingWindowCounter` | Sliding window counter for request rate tracking |
| `BackendRateLimiter` | Per-backend rate limiting keyed by backend ID |
| `RateLimitPolicy` | Policy configuration for selecting rate limit strategy |
| `AdaptiveLimiter` | Adjusts limits dynamically based on backend feedback |
| `QuotaManager` | Tracks quota usage against configurable limits |

## Usage

```rust
use abp_ratelimit::TokenBucket;

let bucket = TokenBucket::new(10.0, 20);
assert!(bucket.try_acquire(1));
```

```rust
use abp_ratelimit::{BackendRateLimiter, RateLimitPolicy};

let limiter = BackendRateLimiter::new();
limiter.set_policy("openai", RateLimitPolicy::TokenBucket { rate: 10.0, burst: 20 });
let permit = limiter.try_acquire("openai");
assert!(permit.is_ok());
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.