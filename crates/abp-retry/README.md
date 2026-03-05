# abp-retry

Generic retry primitives for Agent Backplane microcrates.

This crate provides:
- Retry configuration (`RetryConfig`)
- Exponential backoff with optional jitter (`compute_delay`)
- Retry metadata capture (`RetryMetadata`, `RetryAttempt`)
- Generic async retry loop (`retry_async`)

It is intentionally independent from sidecar/host-specific error types.
