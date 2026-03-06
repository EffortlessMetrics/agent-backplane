# abp-sidecar-sdk

Shared sidecar registration helpers for vendor SDK microcrates.

Provides the glue between vendor-specific SDK crates (e.g. `abp-claude-sdk`,
`abp-codex-sdk`) and the ABP runtime. Resolves host script paths, validates
command availability on `PATH`, and registers `SidecarBackend` instances with
the runtime backend registry.

## Key Types

| Type | Description |
|------|-------------|
| `SidecarBuilder` | High-level builder for constructing and registering sidecar backends |
| `EventEmitter` | Helper for emitting structured events from sidecar SDK code |
| `SidecarRuntime` | Runtime wrapper providing sidecar-specific lifecycle management |
| `register_sidecar_backend()` | Registers a sidecar backend from a host script path |

## Usage

```rust,no_run
use abp_sidecar_sdk::register_sidecar_backend;
use abp_runtime::Runtime;
use std::path::Path;

fn register(runtime: &mut Runtime) {
    let registered = register_sidecar_backend(
        runtime,
        "sidecar:claude",
        Path::new("hosts"),
        "claude/index.js",
        None,        // command override
        "node",      // default command
        "claude",    // provider label
    ).unwrap();
}
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
