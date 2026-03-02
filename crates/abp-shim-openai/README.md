# abp-shim-openai

OpenAI SDK shim for Agent Backplane — a drop-in compatible client that mirrors the OpenAI Chat Completions API surface but routes requests through ABP's intermediate representation.

## Overview

`abp-shim-openai` provides an `OpenAiClient` with `chat.completions.create()` and `chat.completions.create_stream()` methods that accept standard OpenAI-style request types. Internally, requests are converted to ABP IR, processed through the runtime pipeline, and responses are projected back into OpenAI-compatible types.

## Usage

```rust,no_run
use abp_shim_openai::{OpenAiClient, ChatCompletionRequest, Message};

let client = OpenAiClient::new("gpt-4o");

let request = ChatCompletionRequest::builder()
    .model("gpt-4o")
    .messages(vec![
        Message::user("What is 2 + 2?"),
    ])
    .build();

// Non-streaming (requires async runtime and a processor)
// let response = client.chat().completions().create(request).await?;

// Streaming
// let stream = client.chat().completions().create_stream(request).await?;
```

## Architecture

```text
OpenAI Request Types → IR (IrConversation) → WorkOrder → [Runtime] → Receipt → IR → OpenAI Response Types
```

Part of the [Agent Backplane](https://github.com/EffortlessMetrics/agent-backplane) workspace.

## License

Licensed under MIT OR Apache-2.0.
