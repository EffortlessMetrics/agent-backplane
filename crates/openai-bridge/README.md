# openai-bridge

Standalone OpenAI Chat Completions bridge built on `sidecar-kit` transport.

Spawns an OpenAI sidecar process and communicates over JSONL stdio, providing three
execution modes: raw passthrough, mapped task dispatch, and normalized ABP event
mapping. Includes Chat Completions API types, SSE stream parsing, embeddings types,
and full function/tool calling support.

## Key Types

| Type | Description |
|------|-------------|
| `OpenAiBridge` | Main bridge handle wrapping configuration and run methods |
| `OpenAiBridgeConfig` | Bridge configuration (host script path, node command, env) |
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
| `ir` | Enables translation between OpenAI API types and `abp-sdk-types` IR |

## Usage

```rust,no_run
use openai_bridge::{OpenAiBridge, OpenAiBridgeConfig, RunOptions};

let config = OpenAiBridgeConfig::default();
let bridge = OpenAiBridge::new(config);
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.