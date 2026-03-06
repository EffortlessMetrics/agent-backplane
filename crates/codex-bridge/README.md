# codex-bridge

Codex Responses API bridge for Agent Backplane -- IR translation layer.

Translates between OpenAI Codex Responses API types (from `abp-codex-sdk`)
and the vendor-agnostic Intermediate Representation defined in `abp-dialect`.
Handles role mapping, tool definitions, content parts, streaming deltas,
and usage statistics in both directions.

## Features

| Feature | Description |
|---------|-------------|
| `ir` | Enables the `ir_translate` module for bidirectional Codex/IR conversion (depends on `abp-dialect`) |

## Usage

```rust,no_run
// Enable the `ir` feature in Cargo.toml:
//   codex-bridge = { path = "../codex-bridge", features = ["ir"] }

use codex_bridge::ir_translate::{codex_request_to_ir, ir_to_codex_request};
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
