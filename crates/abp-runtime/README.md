# abp-runtime

Orchestration engine for Agent Backplane — workspace preparation, backend
selection, event multiplexing, and canonical receipt production.

## Key Types

| Type | Description |
|------|-------------|
| `Runtime` | Central orchestrator: holds registered backends and executes work orders |
| `RunHandle` | Handle to a running work order with event stream and receipt future |
| `RuntimeError` | Errors originating from the orchestration layer |

## Responsibilities

1. **Workspace preparation** — stage or pass-through the working directory
2. **Policy enforcement** — compile and apply tool/path restrictions
3. **Backend selection** — route work orders to registered backends
4. **Event multiplexing** — fan out agent events to consumers
5. **Receipt production** — build, hash, and return canonical receipts

## Usage

```rust,no_run
use abp_runtime::Runtime;

let mut runtime = Runtime::new();
// Register backends, then execute work orders via runtime.run_streaming()
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
