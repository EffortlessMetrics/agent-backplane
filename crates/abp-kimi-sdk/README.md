# abp-kimi-sdk

Kimi (Moonshot) SDK adapter for Agent Backplane.

Registers the Kimi sidecar backend and provides bidirectional translation between
ABP contract types and the Moonshot/Kimi chat completions API format. Includes dialect
configuration, model mapping, IR lowering, file API support, and model listing types.

## Features

- **Chat Completions API types** -- Request/response types mirroring the Moonshot Kimi chat completions surface
- **Conversion module** -- Bidirectional conversion between Kimi wire types and ABP contract types
- **Dialect module** -- Wire types, model name canonicalization, and capability manifest for the Kimi backend
- **IR lowering** -- Bidirectional conversion between Kimi messages and ABP intermediate representation
- **File API types** -- Types for document parsing and file-based context via the Kimi File API
- **Models API types** -- Model listing types for the Moonshot Kimi Models endpoint
- **Serde + JSON Schema** -- All public types derive `Serialize`/`Deserialize` and `schemars::JsonSchema`

## Usage

```rust,no_run
use abp_kimi_sdk::{register_default, BACKEND_NAME};
use abp_runtime::Runtime;
use std::path::Path;

let mut runtime = Runtime::new();
let registered = register_default(&mut runtime, Path::new("."), None)
    .expect("failed to register Kimi backend");
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.