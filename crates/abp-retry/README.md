# abp-retry

Retry and circuit breaker middleware for Agent Backplane backend calls.

Provides configurable retry policies with exponential backoff and optional
jitter, plus circuit breakers that short-circuit calls to unhealthy backends
to prevent cascading failures. An error classifier drives per-error retry
decisions, and a token-bucket retry budget caps total retries across callers.

## Key Types

| Type | Description |
|------|-------------|
| `RetryPolicy` | Configurable retry logic with exponential backoff and jitter |
| `CircuitBreaker` | Prevents cascading failures by short-circuiting unhealthy backends |
| `ErrorClassifier` | Per-error retry decisions (retry, retry-after, or do-not-retry) |
| `RetryBudget` | Token-bucket limiter that caps total retries across callers |
| `RetryMetrics` | Atomic counters for observability of retry behavior |

## Usage

```rust,no_run
use abp_retry::{RetryPolicy, retry_with_policy};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let policy = RetryPolicy::default();
let result = retry_with_policy(&policy, || async {
    Ok::<_, String>("success".to_string())
}).await;
assert!(result.is_ok());
# Ok(())
# }
```

```rust,no_run
use abp_retry::CircuitBreaker;
use std::time::Duration;

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let cb = CircuitBreaker::new(3, Duration::from_secs(30));
let result = cb.call(|| async {
    Ok::<_, String>("healthy".to_string())
}).await;
assert!(result.is_ok());
# Ok(())
# }
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.