# sidecar-kit

Value-based JSONL transport layer for sidecar processes.

Provides the low-level building blocks for spawning and communicating with
sidecar processes that speak the ABP JSONL protocol. All payload fields use
`serde_json::Value`, making this crate independent of `abp-core` types where
possible. Includes frame codecs, event/receipt builders, middleware chains,
event pipelines, protocol state tracking, and test harnesses.

## Key Types

| Type | Description |
|------|-------------|
| `Frame` | A single JSONL protocol frame (hello, run, event, final, fatal) |
| `JsonlCodec` | Stateless codec for encoding/decoding frames |
| `SidecarClient` | Client that connects to a spawned sidecar and drives the protocol |
| `SidecarProcess` | Process handle for a spawned sidecar |
| `ProcessSpec` | Configuration for spawning a sidecar (command, args, env) |
| `EventBuilder` | Builder for constructing `AgentEvent` frames |
| `ReceiptBuilder` | Builder for constructing receipt frames |
| `MiddlewareChain` | Composable middleware pipeline for event processing |
| `EventPipeline` | Multi-stage event pipeline with validation, redaction, timestamps |
| `SidecarHarness` | Handler-based harness for implementing sidecars |

## Usage

```rust
use sidecar_kit::{Frame, hello_frame, event_text_message, final_frame};

let hello = hello_frame("my-sidecar", None);
let event = event_text_message("run-1", "Hello from sidecar");
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
