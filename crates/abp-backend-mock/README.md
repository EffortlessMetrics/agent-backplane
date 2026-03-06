# abp-backend-mock

Mock backend for local testing without external API keys.

Implements the `Backend` trait with a deterministic, zero-cost backend that
emits synthetic events and receipts. Includes a scenario system for simulating
streaming, transient errors, timeouts, rate limiting, and custom event sequences.

## Key Types

| Type | Description |
|------|-------------|
| `MockBackend` | Simple backend that echoes a fixed response with zero token usage |
| `ScenarioMockBackend` | Configurable backend driven by a `MockScenario` |
| `MockScenario` | Enum describing behavior: success, streaming, errors, timeouts, custom |
| `EventSequenceBuilder` | Fluent builder for custom event sequences with per-step delays |
| `MockBackendRecorder` | Recording wrapper that captures every call to any `Backend` |
| `RecordedCall` | Snapshot of a single recorded backend invocation |

## Usage

```rust
use abp_backend_mock::MockBackend;
use abp_backend_core::Backend;

let backend = MockBackend;
assert_eq!(backend.identity().id, "mock");
```

```rust
use abp_backend_mock::scenarios::{EventSequenceBuilder, ScenarioMockBackend, MockScenario};

let scenario = MockScenario::Success {
    delay_ms: 0,
    text: "hello".into(),
};
let backend = ScenarioMockBackend::new(scenario);
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
