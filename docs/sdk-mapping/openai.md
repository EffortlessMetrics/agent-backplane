# OpenAI SDK Surface Area

> Mapping reference for the OpenAI Chat Completions API and Responses API as implemented by `abp-shim-openai`.

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/chat/completions` | POST | Chat Completions (primary) |
| `/v1/responses` | POST | Responses API (newer, item-based) |

## Request Format

### Chat Completions Request

```jsonc
{
  "model": "gpt-4o",                    // required
  "messages": [                          // required
    {
      "role": "system",                  // "system" | "user" | "assistant" | "tool"
      "content": "You are helpful."      // string (or null for assistant w/ tool_calls)
    },
    {
      "role": "user",
      "content": "Hello"
    },
    {
      "role": "assistant",
      "content": null,
      "tool_calls": [                    // present when assistant invokes tools
        {
          "id": "call_abc123",
          "type": "function",
          "function": {
            "name": "get_weather",
            "arguments": "{\"city\":\"NYC\"}"   // JSON-encoded string
          }
        }
      ]
    },
    {
      "role": "tool",
      "tool_call_id": "call_abc123",     // correlates with tool_calls[].id
      "content": "{\"temp\": 72}"
    }
  ],
  "tools": [                             // optional
    {
      "type": "function",
      "function": {
        "name": "get_weather",
        "description": "Get current weather",
        "parameters": {                  // JSON Schema object
          "type": "object",
          "properties": {
            "city": { "type": "string" }
          },
          "required": ["city"]
        }
      }
    }
  ],
  "tool_choice": "auto",                // "auto" | "none" | "required" | {"type":"function","function":{"name":"..."}}
  "temperature": 0.7,                   // 0.0–2.0, optional
  "max_tokens": 4096,                   // optional
  "stop": ["\n\n"],                     // optional, up to 4 sequences
  "stream": false,                      // optional
  "response_format": {                  // optional
    "type": "json_schema",              // "text" | "json_object" | "json_schema"
    "json_schema": {
      "name": "my_schema",
      "schema": { /* JSON Schema */ }
    }
  }
}
```

### ABP Shim Struct: `ChatCompletionRequest`

| Field | Type | ABP Mapping |
|-------|------|-------------|
| `model` | `String` | → `WorkOrder.config.model` |
| `messages` | `Vec<Message>` | → `IrConversation` via `lowering::to_ir()` |
| `tools` | `Option<Vec<Tool>>` | → `IrToolDefinition` list |
| `tool_choice` | `Option<ToolChoice>` | → `vendor.tool_choice` |
| `temperature` | `Option<f64>` | → `vendor.temperature` |
| `max_tokens` | `Option<u32>` | → `vendor.max_tokens` |
| `stop` | `Option<Vec<String>>` | → `vendor.stop` |
| `stream` | `Option<bool>` | Controls streaming vs batch |
| `response_format` | `Option<ResponseFormat>` | → `vendor.response_format` |

## Response Format

### Chat Completions Response

```jsonc
{
  "id": "chatcmpl-abc123",
  "object": "chat.completion",
  "created": 1719000000,
  "model": "gpt-4o-2024-08-06",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": "Hello! How can I help?"
        // OR content: null + tool_calls: [...]
      },
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": 25,
    "completion_tokens": 10,
    "total_tokens": 35
  }
}
```

### ABP Shim Struct: `ChatCompletionResponse`

| Field | Type | ABP Source |
|-------|------|-----------|
| `id` | `String` | Generated from `Receipt.meta.run_id` |
| `object` | `String` | Always `"chat.completion"` |
| `created` | `u64` | From `Receipt.meta.started_at` |
| `model` | `String` | Echoed from request |
| `choices` | `Vec<Choice>` | Built from `Receipt.trace` events |
| `usage` | `Option<Usage>` | From `Receipt.usage` |

## Streaming Format

**Protocol:** Server-Sent Events (SSE) over HTTP.

**Content-Type:** `text/event-stream`

Each event is prefixed with `data: ` and terminated by `\n\n`. The stream ends with `data: [DONE]\n\n`.

### Stream Chunk

```jsonc
// data: {"id":"chatcmpl-abc","object":"chat.completion.chunk",...}
{
  "id": "chatcmpl-abc123",
  "object": "chat.completion.chunk",
  "created": 1719000000,
  "model": "gpt-4o",
  "choices": [
    {
      "index": 0,
      "delta": {
        "role": "assistant",       // first chunk only
        "content": "Hello",        // incremental text
        "tool_calls": [            // incremental tool call fragments
          {
            "index": 0,
            "id": "call_abc",      // first fragment only
            "type": "function",    // first fragment only
            "function": {
              "name": "get_",      // first fragment only (can be partial)
              "arguments": "{\"c"  // incremental JSON string
            }
          }
        ]
      },
      "finish_reason": null        // "stop" | "tool_calls" | "length" on final chunk
    }
  ],
  "usage": null                    // populated on final chunk if stream_options.include_usage = true
}
```

### ABP Shim Structs for Streaming

| Struct | Purpose |
|--------|---------|
| `StreamEvent` | Top-level chunk envelope |
| `StreamChoice` | Per-choice delta container |
| `Delta` | Incremental content/role/tool_calls |
| `StreamToolCall` | Incremental tool call fragment (indexed) |
| `StreamFunctionCall` | Incremental function name + arguments |

### ABP Event Mapping

| OpenAI Stream Event | ABP `AgentEventKind` |
|---------------------|----------------------|
| `delta.content` present | `AssistantDelta { text }` |
| `delta.tool_calls` present | Accumulated → `ToolCall` on completion |
| `finish_reason: "stop"` | `RunCompleted` |
| `finish_reason: "tool_calls"` | Tool calls emitted, awaiting results |
| `[DONE]` sentinel | Stream termination |

## Tool Calling Conventions

- **Invocation:** Model returns `tool_calls` array in the assistant message (or delta).
- **Correlation:** Each tool call has a unique `id` (e.g., `call_abc123`).
- **Result:** User sends a `role: "tool"` message with `tool_call_id` matching the call.
- **Arguments:** Always a JSON-encoded **string**, not a parsed object.
- **Parallel calls:** Multiple `tool_calls` in a single assistant message are supported.
- **Tool choice modes:**
  - `"auto"` — model decides
  - `"none"` — no tool calls
  - `"required"` — must call at least one tool
  - `{"type":"function","function":{"name":"X"}}` — force specific function

### ABP IR Mapping

```
OpenAI ToolCall.function.name       → IrToolCall.name
OpenAI ToolCall.function.arguments  → IrToolCall.input (parsed from JSON string)
OpenAI ToolCall.id                  → IrToolCall.id
OpenAI role:"tool" message          → IrToolResult { tool_use_id, content }
```

## System Message Handling

- System messages use `role: "system"` and appear as the **first message** in the array.
- Multiple system messages are allowed (concatenated by the model).
- System messages are **not** part of the conversation turn structure.

### ABP Mapping

```
OpenAI messages[role=system].content → IrMessage { role: IrRole::System, ... }
```

## Token Counting

| OpenAI Field | ABP `UsageNormalized` Field |
|-------------|----------------------------|
| `usage.prompt_tokens` | `input_tokens` |
| `usage.completion_tokens` | `output_tokens` |
| `usage.total_tokens` | `input_tokens + output_tokens` |

**Note:** OpenAI does not expose cache token counts in the standard Chat Completions API. The Responses API includes `input_tokens_details.cached_tokens`.

## Finish Reasons

| OpenAI `finish_reason` | Meaning | ABP `Outcome` |
|------------------------|---------|----------------|
| `"stop"` | Natural stop or stop sequence hit | `Complete` |
| `"tool_calls"` | Model wants to invoke tools | `Complete` (mid-turn) |
| `"length"` | `max_tokens` reached | `Partial` |
| `"content_filter"` | Content policy violation | `Failed` |
| `null` | Still generating (streaming) | N/A |

## Content Types

| Type | Support | Notes |
|------|---------|-------|
| Text | ✅ | Primary content type |
| Images (input) | ✅ | Via `content` array with `image_url` parts |
| Images (output) | ❌ | Not in Chat Completions |
| Audio (input) | ✅ | `input_audio` content part |
| Audio (output) | ✅ | Via `modalities: ["text", "audio"]` |
| Tool calls | ✅ | `tool_calls` in assistant message |
| Tool results | ✅ | `role: "tool"` messages |
| Structured output | ✅ | `response_format.type: "json_schema"` |

## Model Names

| Model | Context Window | Notes |
|-------|---------------|-------|
| `gpt-4o` | 128K | Default in ABP shim |
| `gpt-4o-mini` | 128K | Cost-optimized |
| `gpt-4-turbo` | 128K | Previous generation |
| `gpt-4` | 8K / 32K | Legacy |
| `gpt-3.5-turbo` | 16K | Legacy, cost-effective |
| `o1` | 200K | Reasoning model |
| `o1-mini` | 128K | Fast reasoning |
| `o3` | 200K | Advanced reasoning |
| `o3-mini` | 200K | Fast advanced reasoning |
| `o4-mini` | 200K | Latest reasoning |

## Error Codes

| HTTP Status | Error Type | Description |
|-------------|-----------|-------------|
| 400 | `invalid_request_error` | Malformed request |
| 401 | `authentication_error` | Invalid API key |
| 403 | `permission_error` | Insufficient permissions |
| 404 | `not_found_error` | Unknown model/endpoint |
| 429 | `rate_limit_error` | Rate limit exceeded |
| 500 | `server_error` | Internal server error |
| 503 | `overloaded_error` | Service overloaded |

### ABP Error Mapping

| OpenAI Error | ABP `ErrorCode` |
|-------------|-----------------|
| `invalid_request_error` | `IR_LOWERING_FAILED` or `CONFIG_INVALID` |
| `authentication_error` | `BACKEND_CRASHED` (auth failure) |
| `rate_limit_error` | `BACKEND_TIMEOUT` |
| `server_error` | `BACKEND_CRASHED` |

## Responses API Differences

The newer Responses API (`/v1/responses`) uses an **item-based** format instead of the messages array. See [`codex.md`](codex.md) for details, as Codex builds on the Responses API surface.

Key differences from Chat Completions:
- Uses `input` array of items instead of `messages`
- Response is a list of `output` items (messages, function calls)
- Supports `previous_response_id` for multi-turn chaining
- Built-in tools (`web_search`, `file_search`, `code_interpreter`)
- Usage includes `input_tokens_details` and `output_tokens_details`
