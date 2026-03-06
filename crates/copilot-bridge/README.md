# copilot-bridge

Standalone GitHub Copilot bridge built on `sidecar-kit` transport.

Provides Copilot-specific types (references, confirmations, function calling)
and translation to/from the ABP intermediate representation. The Copilot Chat
API wire format is OpenAI-compatible with extensions for code references,
user confirmations, and agent mode.

## Features

| Feature | Description |
|---------|-------------|
| `ir` | Enables translation between Copilot types and `abp-sdk-types` IR types |
| `normalized` | Enables mapping to `abp-core` `AgentEvent` and `Receipt` types |

## Key Types

| Type | Description |
|------|-------------|
| `CopilotMessageRole` | Message role enum (System, User, Assistant, Tool) |
| `CopilotReference` | Structured context reference (file, snippet, repository, web result) |
| `CopilotReferenceType` | Discriminator for reference variants |

## Usage

```rust,no_run
// Enable features in Cargo.toml:
//   copilot-bridge = { path = "../copilot-bridge", features = ["ir"] }

use copilot_bridge::copilot_types::{CopilotMessageRole, CopilotReference};
use copilot_bridge::ir_translate;
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
