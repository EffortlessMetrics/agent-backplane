# abp-retry

Retry and timeout configuration primitives shared across Agent Backplane components.

This crate provides:

- `RetryPolicy`: exponential backoff with deterministic jitter
- `RetryPolicyBuilder`: fluent policy construction
- `TimeoutConfig`: optional run/event timeout settings
