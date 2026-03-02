# abp-shim-kimi

Kimi SDK shim for Agent Backplane — a drop-in compatible client that mirrors the Moonshot Kimi Chat Completions API surface but routes requests through ABP's intermediate representation.

## Overview

`abp-shim-kimi` provides a `KimiClient` with `create()` and `create_stream()` methods that accept standard Kimi-style request types. Internally, requests are converted to ABP IR, processed through the runtime pipeline, and responses are projected back into Kimi-compatible types.

## Usage

```rust,no_run
use abp_shim_kimi::{KimiClient, KimiRequestBuilder, Message};

let client = KimiClient::new("moonshot-v1-8k");

let request = KimiRequestBuilder::new()
    .model("moonshot-v1-8k")
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
Kimi Request Types → IR (IrConversation) → WorkOrder → [Runtime] → Receipt → IR → Kimi Response Types
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
