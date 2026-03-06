# abp-backend-sidecar

Sidecar backend adapter bridging JSONL protocol agents into the Agent Backplane.

Implements the `Backend` trait by spawning an external sidecar process via
`abp-host`, performing the JSONL hello handshake, streaming `AgentEvent`s
from the sidecar's stdout, and collecting the final `Receipt`.

## Key Types

| Type | Description |
|------|-------------|
| `SidecarBackend` | `Backend` implementation that delegates to a sidecar process |

## Usage

```rust,no_run
use abp_backend_sidecar::SidecarBackend;
use abp_host::SidecarSpec;

let mut spec = SidecarSpec::new("node");
spec.args.push("hosts/claude/index.js".into());

let backend = SidecarBackend::new(spec);
// backend.run(run_id, work_order, events_tx).await
```

## Protocol Flow

```text
SidecarBackend ──spawn──▸ Sidecar process
Sidecar        ──hello──▸ SidecarBackend    (capability check)
SidecarBackend ──run────▸ Sidecar           (WorkOrder)
Sidecar        ──event──▸ SidecarBackend    (forwarded to events_tx)
Sidecar        ──final──▸ SidecarBackend    (Receipt returned)
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
