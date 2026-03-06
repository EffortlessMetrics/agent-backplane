# abp-gemini-sdk

Google Gemini SDK adapter for Agent Backplane.

Registers the Gemini CLI sidecar backend and provides bidirectional translation between
ABP contract types and the Google Gemini `generateContent` API format. Includes dialect
configuration, model mapping, capability manifests, IR lowering, and streaming support.

## Features

- **GenerateContent API types** -- `GeminiContent`, `GeminiPart`, request/response types mirroring the Gemini REST API with `From` conversions to/from ABP `WorkOrder` and `Receipt`
- **Streaming types** -- `StreamGenerateContentResponse`, `map_stream_chunk`, and `FunctionCallAccumulator` for reassembling streamed function-call fragments
- **Dialect module** -- Wire types, model name canonicalization, and capability manifest for the Gemini backend
- **Conversion module** -- Free-function conversions (`to_work_order`, `from_receipt`, `from_agent_event`) using camelCase wire types
- **IR lowering** -- `lowering::to_ir` lifts Gemini content into IR conversations; `lowering::from_ir` lowers back
- **Error types** -- `GeminiErrorResponse`, `GeminiErrorDetail`, `GeminiErrorStatus` for deserializing API error bodies
- **Serde + JSON Schema** -- All public types derive `Serialize`/`Deserialize` and `schemars::JsonSchema`

## Usage

```rust,no_run
use abp_gemini_sdk::{register_default, BACKEND_NAME};
use abp_runtime::Runtime;
use std::path::Path;

let mut runtime = Runtime::new();
let registered = register_default(&mut runtime, Path::new("."), None)
    .expect("failed to register Gemini backend");
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.