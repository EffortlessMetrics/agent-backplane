# abp-copilot-sdk

GitHub Copilot SDK adapter for Agent Backplane.

Registers the Copilot sidecar backend and provides bidirectional translation between
ABP contract types and the GitHub Copilot agent protocol format. Includes dialect
configuration, model mapping, IR lowering, and conversion helpers.

## Features

- **Copilot agent protocol types** -- Request/response types mirroring the GitHub Copilot agent protocol surface
- **Conversion module** -- Bidirectional conversion between Copilot wire types and ABP contract types
- **Dialect module** -- Wire types, model name canonicalization, and capability manifest for the Copilot backend
- **IR lowering** -- Bidirectional conversion between Copilot messages and ABP intermediate representation
- **Serde + JSON Schema** -- All public types derive `Serialize`/`Deserialize` and `schemars::JsonSchema`

## Usage

```rust,no_run
use abp_copilot_sdk::{register_default, BACKEND_NAME};
use abp_runtime::Runtime;
use std::path::Path;

let mut runtime = Runtime::new();
let registered = register_default(&mut runtime, Path::new("."), None)
    .expect("failed to register Copilot backend");
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.