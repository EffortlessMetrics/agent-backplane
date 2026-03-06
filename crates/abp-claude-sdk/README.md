# abp-claude-sdk

Anthropic Claude SDK adapter for Agent Backplane.

Registers the Claude sidecar backend and provides bidirectional translation between
ABP contract types and the Anthropic Messages API format. Includes dialect configuration,
model name canonicalization, capability manifests, IR lowering, and streaming support.

## Features

- **Messages API types** -- `ClaudeMessage`, `ClaudeContentBlock`, request/response types mirroring the Anthropic Messages API with `From` conversions to/from ABP `WorkOrder` and `Receipt`
- **Streaming types** -- `StreamAccumulator` for building complete responses from SSE stream events, with bidirectional conversion between stream events and ABP `AgentEvent`s
- **Dialect module** -- Wire types, model name canonicalization, and capability manifest for the Claude backend
- **IR lowering** -- `lowering::to_ir` lifts Claude messages into IR conversations; `lowering::from_ir` lowers back
- **Error types** -- Typed `ErrorResponse`, `ErrorType`, and `ErrorDetail` matching the Anthropic JSON error envelope
- **Models API** -- Types for listing and retrieving model information via the Anthropic Models API
- **Serde + JSON Schema** -- All public types derive `Serialize`/`Deserialize` and `schemars::JsonSchema`

## Usage

```rust,no_run
use abp_claude_sdk::{register_default, BACKEND_NAME};
use abp_runtime::Runtime;
use std::path::Path;

let mut runtime = Runtime::new();
let registered = register_default(&mut runtime, Path::new("."), None)
    .expect("failed to register Claude backend");
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.