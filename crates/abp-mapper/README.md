# abp-mapper

Dialect mapping engine for the Agent Backplane.

Translates requests, responses, and streaming events between agent-SDK dialects
(OpenAI, Claude, Gemini, Codex, Kimi, Copilot). Operates at two levels:
JSON-level mapping via the `Mapper` trait and IR-level mapping via `IrMapper`,
which translates through the intermediate representation defined in `abp-ir`.

## Key Types

| Type | Description |
|------|-------------|
| `Mapper` | Trait for directional JSON-level dialect translation (request, response, event) |
| `IrMapper` | Trait for IR-level cross-dialect translation via the intermediate representation |
| `DialectRequest` | Source dialect tag paired with a raw JSON body for mapping input |
| `DialectResponse` | Target dialect tag paired with the mapped JSON body |
| `IdentityMapper` | Pass-through mapper that returns input unchanged |
| `MappingError` | Error type for mapping failures |

## Usage

```rust
use abp_mapper::{Mapper, IdentityMapper, DialectRequest};
use abp_dialect::Dialect;
use serde_json::json;

let mapper = IdentityMapper;
let req = DialectRequest {
    dialect: Dialect::OpenAi,
    body: json!({"model": "gpt-4", "messages": []}),
};
let mapped = mapper.map_request(&req).unwrap();
assert_eq!(mapped, req.body);
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
