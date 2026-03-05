# abp-host

Sidecar process supervision and JSONL handshake for Agent Backplane.

Spawns external sidecar processes, negotiates the JSONL hello handshake,
and streams `AgentEvent`s back to the control plane.

## Key Types

| Type | Description |
|------|-------------|
| `SidecarSpec` | Configuration for spawning a sidecar process (command, args, env) |
| `SidecarClient` | A connected sidecar that has completed the hello handshake |
| `SidecarRun` | In-progress run with event stream, receipt future, and wait handle |
| `SidecarHello` | Data extracted from the sidecar's initial hello envelope |
| `HostError` | Errors from process management and protocol handling |

## Usage

```rust
use abp_host::SidecarSpec;

let mut spec = SidecarSpec::new("node");
spec.args.push("hosts/node/index.js".into());
spec.env.insert("NODE_ENV".into(), "production".into());
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
