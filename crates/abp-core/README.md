# abp-core

Stable contract types for the Agent Backplane — WorkOrder, Receipt, AgentEvent,
and capabilities.

If you only take one dependency from the workspace, take this one.

## Key Types

| Type | Description |
|------|-------------|
| [`WorkOrder`] | A single unit of work with task, workspace, policy, and config |
| [`Receipt`] | Outcome of a completed run — metadata, usage, trace, verification |
| [`AgentEvent`] | Timestamped event emitted by an agent during a run |
| [`Capability`] | Discrete feature a backend may support (tools, hooks, MCP, etc.) |
| [`CapabilityManifest`] | Maps each capability to its support level for a backend |
| [`PolicyProfile`] | Security policy: tool allow/deny lists, path restrictions |

## Usage

```rust
use abp_core::{WorkOrderBuilder, ReceiptBuilder, Outcome};

// Build a work order
let wo = WorkOrderBuilder::new("Refactor auth module").build();
assert_eq!(wo.task, "Refactor auth module");

// Build and hash a receipt
let receipt = ReceiptBuilder::new("mock")
    .outcome(Outcome::Complete)
    .build()
    .with_hash()
    .unwrap();

assert!(receipt.receipt_sha256.is_some());
```

## Contract Version

All wire messages and receipts embed `CONTRACT_VERSION`:

```rust
assert_eq!(abp_core::CONTRACT_VERSION, "abp/v0.1");
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
