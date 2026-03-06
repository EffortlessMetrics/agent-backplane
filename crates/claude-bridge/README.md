# claude-bridge

Standalone Claude SDK bridge built on `sidecar-kit` transport.

Spawns a Claude sidecar process and communicates over JSONL stdio, providing three
execution modes: raw passthrough, mapped task dispatch, and normalized ABP event
mapping. Includes Claude Messages API types, SSE parsing, extended thinking,
tool-use builders, and vision support.

## Key Types

| Type | Description |
|------|-------------|
| `ClaudeBridge` | Main bridge handle wrapping configuration and run methods |
| `ClaudeBridgeConfig` | Bridge configuration (host script path, node command, env) |
| `RunOptions` | Options for mapped-mode runs (model, max tokens, tools) |
| `BridgeError` | Typed error enum for bridge operations |
| `RawRun` | Raw JSONL run handle from `sidecar-kit` |

## Modes

- **Raw passthrough** (`run_raw`) -- sends a raw vendor JSON request, returns raw JSON events
- **Raw mapped** (`run_mapped_raw`) -- task string + options to raw JSON events
- **Normalized** (`run_normalized`, feature `normalized`) -- maps raw events to typed `AgentEvent` and `Receipt` from `abp-core`

## Features

| Feature | Description |
|---------|-------------|
| `normalized` | Enables `run_normalized` with `abp-core` types |
| `ir` | Enables translation between Claude API types and `abp-dialect` IR |

## Usage

```rust,no_run
use claude_bridge::{ClaudeBridge, ClaudeBridgeConfig, RunOptions};

let config = ClaudeBridgeConfig::default();
let bridge = ClaudeBridge::new(config);
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.