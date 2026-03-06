# abp-shim-claude

Claude SDK shim for Agent Backplane -- a drop-in compatible client that mirrors the Anthropic Messages API surface but routes requests through ABP's intermediate representation.

## Overview

`abp-shim-claude` provides an `AnthropicClient` with `messages().create()` and `messages().create_stream()` methods that accept standard Anthropic-style request types. Internally, requests are converted to ABP IR, processed through the runtime pipeline, and responses are projected back into Anthropic-compatible types.

## Key Types

| Type | Description |
|------|-------------|
| `AnthropicClient` | Drop-in client with Messages API surface |
| `MessageRequest` | Anthropic-compatible message creation request |
| `MessageResponse` | Anthropic-compatible message response |
| `ContentBlock` | Text, tool use, tool result, image, and thinking content blocks |
| `StreamEvent` | SSE stream event types mirroring the Anthropic streaming protocol |
| `ShimError` | Error type covering validation, API, and internal failures |

## Usage

```rust,no_run
use abp_shim_claude::{AnthropicClient, MessageRequest, Message};

let client = AnthropicClient::new("claude-sonnet-4-20250514");

let request = MessageRequest::builder()
    .model("claude-sonnet-4-20250514")
    .messages(vec![
        Message::user("What is 2 + 2?"),
    ])
    .build();

// Non-streaming (requires async runtime and a processor)
// let response = client.messages().create(request).await?;

// Streaming
// let stream = client.messages().create_stream(request).await?;
```

## Architecture

```text
Anthropic Request Types -> IR (IrConversation) -> WorkOrder -> [Runtime] -> Receipt -> IR -> Anthropic Response Types
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
