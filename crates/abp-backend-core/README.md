# abp-backend-core

Shared backend trait and capability helpers for the Agent Backplane.

Defines the `Backend` async trait that all backend implementations must satisfy,
along with lifecycle hooks, health checking, capability requirement validation,
and execution mode extraction from work orders.

## Key Types

| Type | Description |
|------|-------------|
| `Backend` | Async trait for executing work orders and streaming `AgentEvent`s |
| `LifecycleBackend` | Extended trait adding `init()` and `shutdown()` lifecycle hooks |
| `HealthCheckable` | Trait for backends that can report their own health status |
| `BackendRegistry` | Registry for looking up backends by name |
| `SelectionStrategy` | Strategy for choosing a backend from the registry |
| `BackendHealth` | Health snapshot returned by `HealthCheckable::check_health()` |
| `BackendMetrics` | Runtime metrics collected from backend operations |

## Usage

```rust,no_run
use abp_backend_core::Backend;
use abp_core::{AgentEvent, WorkOrder};
use tokio::sync::mpsc;
use uuid::Uuid;

async fn run_backend(backend: &dyn Backend, work_order: WorkOrder) {
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(64);
    let receipt = backend.run(Uuid::new_v4(), work_order, tx).await.unwrap();
}
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
