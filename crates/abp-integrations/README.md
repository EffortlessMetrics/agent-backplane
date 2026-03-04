# abp-integrations

Compatibility façade over the backend micro-crates for Agent Backplane.

Re-exports the `Backend` trait and concrete implementations so that
downstream crates only need a single dependency.

## Key Types

| Type | Description |
|------|-------------|
| `Backend` | Async trait for executing work orders and streaming events |
| `MockBackend` | In-process backend for testing (returns canned receipts) |
| `SidecarBackend` | Backend that delegates to an external sidecar process |

## Usage

```rust
use abp_integrations::MockBackend;

let backend = MockBackend;
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
