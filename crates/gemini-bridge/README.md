# gemini-bridge

Standalone Gemini SDK bridge built on `sidecar-kit` transport.

Spawns a Gemini sidecar process and communicates over JSONL stdio, providing raw
passthrough and normalized execution modes. Includes GenerateContent API types,
function calling helpers, multimodal content support, and safety analysis.

## Key Types

| Type | Description |
|------|-------------|
| `GeminiBridge` | Main bridge handle wrapping configuration |
| `GeminiBridgeConfig` | Bridge configuration (host script path, node command, env) |
| `BridgeError` | Typed error enum for bridge operations |

## Modes

- **Raw passthrough** -- sends a raw vendor JSON request, returns raw JSON events
- **Normalized** (feature `normalized`) -- maps raw events to typed `WorkOrder`, `Receipt`, and streaming events from `abp-core`

## Features

| Feature | Description |
|---------|-------------|
| `normalized` | Enables ABP contract type mapping via `abp-core` and `abp-sdk-types` |

## Modules

- `gemini_types` -- GenerateContent API request, response, streaming, and content block types
- `function_calling` -- Tool/function calling builders, validation, and extraction
- `multimodal` -- Blob, FileData, and VideoMetadata content types
- `safety` -- Safety profiles, analysis, and typed block reasons
- `ir_translate` -- High-level Gemini to IR translation

## Usage

```rust,no_run
use gemini_bridge::{GeminiBridge, GeminiBridgeConfig};

let config = GeminiBridgeConfig::default();
let bridge = GeminiBridge::new(config);
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.