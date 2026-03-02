# abp-retry

Retry policy and timeout configuration primitives for Agent Backplane components.

This crate provides:

- `RetryPolicy` for deterministic jittered exponential backoff
- `RetryPolicyBuilder` for fluent policy construction
- `TimeoutConfig` for run/event timeout knobs

It is intended to be dependency-light and reusable by orchestration layers and integrations.
