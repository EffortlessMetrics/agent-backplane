# abp-shim-codex

Codex SDK shim for Agent Backplane — a drop-in compatible client that mirrors the OpenAI Codex Responses API surface but routes requests through ABP's intermediate representation.

## Overview

`abp-shim-codex` provides a `CodexClient` with `create()` and `create_stream()` methods that accept standard Codex-style request types. Internally, requests are converted to ABP IR, processed through the runtime pipeline, and responses are projected back into Codex-compatible types.

## Usage

```rust,no_run
use abp_shim_codex::{CodexRequestBuilder, codex_message};

let client = abp_shim_codex::CodexClient::new("codex-mini-latest");

let request = CodexRequestBuilder::new()
    .model("codex-mini-latest")
    .input(vec![
        codex_message("user", "What is 2 + 2?"),
    ])
    .build();

// Non-streaming (requires async runtime and a processor)
// let response = client.create(request).await?;

// Streaming
// let stream = client.create_stream(request).await?;
```

## Architecture

```text
Codex Request Types → IR (IrConversation) → WorkOrder → [Runtime] → Receipt → IR → Codex Response Types
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
