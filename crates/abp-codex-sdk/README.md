# abp-codex-sdk

OpenAI Codex SDK adapter for Agent Backplane.

Registers the OpenAI Codex sidecar backend and provides bidirectional translation between
ABP contract types and the Codex/Responses API format. Includes dialect configuration,
model mapping, IR lowering, and streaming helpers.

## Features

- **Codex API types** -- Request/response types mirroring the OpenAI Codex/Responses API surface
- **Conversion module** -- Bidirectional conversion between Codex wire types and ABP contract types
- **Dialect module** -- Wire types, model name canonicalization, and capability manifest for the Codex backend
- **IR lowering** -- Bidirectional conversion between Codex messages and ABP intermediate representation
- **Streaming helpers** -- Mapping Codex SSE chunks to ABP `AgentEvent`s
- **Error types** -- Typed error responses matching the Codex/OpenAI error format
- **Serde + JSON Schema** -- All public types derive `Serialize`/`Deserialize` and `schemars::JsonSchema`

## Usage

```rust,no_run
use abp_codex_sdk::{register_default, BACKEND_NAME};
use abp_runtime::Runtime;
use std::path::Path;

let mut runtime = Runtime::new();
let registered = register_default(&mut runtime, Path::new("."), None)
    .expect("failed to register Codex backend");
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.