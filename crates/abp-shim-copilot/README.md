# abp-shim-copilot

Copilot SDK shim for Agent Backplane — a drop-in compatible client that mirrors the GitHub Copilot Agent API surface but routes requests through ABP's intermediate representation.

## Overview

`abp-shim-copilot` provides a `CopilotClient` with `create()` and `create_stream()` methods that accept standard Copilot-style request types. Internally, requests are converted to ABP IR, processed through the runtime pipeline, and responses are projected back into Copilot-compatible types.

## Usage

```rust,no_run
use abp_shim_copilot::{CopilotClient, CopilotRequestBuilder, Message};

let client = CopilotClient::new("gpt-4o");

let request = CopilotRequestBuilder::new()
    .model("gpt-4o")
    .messages(vec![
        Message::user("What is 2 + 2?"),
    ])
    .build();

// Non-streaming (requires async runtime and a processor)
// let response = client.create(request).await?;

// Streaming
// let stream = client.create_stream(request).await?;
```

## Architecture

```text
Copilot Request Types → IR (IrConversation) → WorkOrder → [Runtime] → Receipt → IR → Copilot Response Types
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
