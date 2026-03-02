# Kimi (Moonshot) SDK Surface Area

> Mapping reference for the Kimi Chat Completions API as implemented by `abp-shim-kimi`.

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/chat/completions` | POST | Chat Completions (primary) |

**Base URL:** `https://api.moonshot.cn/v1`

## Request Format

### Chat Completions Request

```jsonc
{
  "model": "moonshot-v1-8k",              // required
  "messages": [                            // required
    {
      "role": "system",                    // "system" | "user" | "assistant" | "tool"
      "content": "You are helpful."
    },
    {
      "role": "user",
      "content": "Hello"
    },
    {
      "role": "assistant",
      "content": null,
      "tool_calls": [                      // present when assistant invokes tools
        {
          "id": "call_abc123",
          "type": "function",
          "function": {
            "name": "get_weather",
            "arguments": "{\"city\":\"NYC\"}"  // JSON-encoded string
          }
        }
      ]
    },
    {
      "role": "tool",
      "tool_call_id": "call_abc123",       // correlates with tool_calls[].id
      "content": "{\"temp\": 72}"
    }
  ],
  "tools": [                               // optional — function or built-in tools
    {
      "type": "function",
      "function": {
        "name": "get_weather",
        "description": "Get current weather",
        "parameters": {                    // JSON Schema object
          "type": "object",
          "properties": {
            "city": { "type": "string" }
          },
          "required": ["city"]
        }
      }
    },
    {
      "type": "builtin_function",          // Kimi built-in tool
      "function": {
        "name": "$web_search"              // "$web_search" | "$browser"
      }
    }
  ],
  "max_tokens": 4096,                     // optional
  "temperature": 0.7,                     // optional, 0.0–1.0
  "stream": false,                        // optional
  "use_search": true                      // optional, enable built-in web search
}
```

### ABP Shim Struct: `KimiRequest`

| Field | Type | ABP Mapping |
|-------|------|-------------|
| `model` | `String` | → `WorkOrder.config.model` |
| `messages` | `Vec<KimiMessage>` | → `IrConversation` via `lowering::to_ir()` |
| `max_tokens` | `Option<u32>` | → `vendor.max_tokens` |
| `temperature` | `Option<f64>` | → `vendor.temperature` |
| `stream` | `Option<bool>` | Controls streaming vs batch |
| `tools` | `Option<Vec<KimiTool>>` | → `IrToolDefinition` list |
| `use_search` | `Option<bool>` | → `vendor.use_search` (Kimi-specific) |

## Response Format

### Chat Completions Response

```jsonc
{
  "id": "cmpl-abc123",
  "model": "moonshot-v1-8k",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": "Hello! How can I help?",
        "tool_calls": null
      },
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": 25,
    "completion_tokens": 10,
    "total_tokens": 35
  },
  "refs": [                                // Kimi-specific citation references
    {
      "index": 0,
      "url": "https://example.com/source",
      "title": "Source Article"
    }
  ]
}
```

### ABP Shim Struct: `KimiResponse`

| Field | Type | ABP Source |
|-------|------|-----------|
| `id` | `String` | From response ID |
| `model` | `String` | Echoed from response |
| `choices` | `Vec<KimiChoice>` | Built from `Receipt.trace` events |
| `usage` | `Option<KimiUsage>` | From `Receipt.usage` |
| `refs` | `Option<Vec<KimiRef>>` | Kimi citation references (preserved in vendor extensions) |

## Streaming Format

**Protocol:** Server-Sent Events (SSE) over HTTP.

**Content-Type:** `text/event-stream`

Each event is prefixed with `data: ` and terminated by `\n\n`. The stream ends with `data: [DONE]\n\n`.

### Stream Chunk

```jsonc
// data: {"id":"cmpl-abc","object":"chat.completion.chunk",...}
{
  "id": "cmpl-abc123",
  "object": "chat.completion.chunk",
  "created": 1719000000,
  "model": "moonshot-v1-8k",
  "choices": [
    {
      "index": 0,
      "delta": {
        "role": "assistant",          // first chunk only
        "content": "Hello",           // incremental text
        "tool_calls": [               // incremental tool call fragments
          {
            "index": 0,
            "id": "call_abc",         // first fragment only
            "type": "function",       // first fragment only
            "function": {
              "name": "get_",         // first fragment only (can be partial)
              "arguments": "{\"c"     // incremental JSON string
            }
          }
        ]
      },
      "finish_reason": null           // "stop" | "tool_calls" | "length" on final chunk
    }
  ],
  "usage": null,                      // populated on final chunk
  "refs": null                        // citation refs, may appear in later chunks
}
```

### ABP Shim Structs for Streaming

| Struct | Purpose |
|--------|---------|
| `KimiChunk` | Top-level SSE chunk envelope |
| `KimiChunkChoice` | Per-choice delta container |
| `KimiChunkDelta` | Incremental content/role/tool_calls |
| `KimiChunkToolCall` | Incremental tool call fragment (indexed) |
| `KimiChunkFunctionCall` | Incremental function name + arguments |
| `ToolCallAccumulator` | Collects incremental fragments, emits complete `ToolCall` events on finish |

### ABP Event Mapping

| Kimi Stream Event | ABP `AgentEventKind` |
|-------------------|----------------------|
| `delta.content` present | `AssistantDelta { text }` |
| `delta.tool_calls` present | Accumulated → `ToolCall` on completion |
| `finish_reason: "stop"` | `RunCompleted` |
| `finish_reason: "tool_calls"` | Tool calls emitted, awaiting results |
| `[DONE]` sentinel | Stream termination |

## Tool Calling Conventions

- **Invocation:** Model returns `tool_calls` array in the assistant message (or delta).
- **Correlation:** Each tool call has a unique `id` (e.g., `call_abc123`).
- **Result:** User sends a `role: "tool"` message with `tool_call_id` matching the call.
- **Arguments:** Always a JSON-encoded **string** (same as OpenAI, unlike Claude/Gemini).
- **Parallel calls:** Multiple `tool_calls` in a single assistant message are supported.
- **Built-in tools:** Kimi provides built-in tools not present in OpenAI:
  - `$web_search` — Internet search (Kimi's native web search capability)
  - `$browser` — Web page reading/browsing

### Key Difference from OpenAI

```
OpenAI:   tools: [{ type: "function", function: { ... } }]
Kimi:     tools: [{ type: "function", function: { ... } },
                  { type: "builtin_function", function: { name: "$web_search" } }]
```

Kimi adds `builtin_function` as a tool type alongside standard `function` tools.

### ABP IR Mapping

```
Kimi ToolCall.function.name       → IrToolCall.name
Kimi ToolCall.function.arguments  → IrToolCall.input (parsed from JSON string)
Kimi ToolCall.id                  → IrToolCall.id
Kimi role:"tool" message          → IrToolResult { tool_use_id, content }
```

### ABP Tool Definition Conversion

```
CanonicalToolDef.name              → KimiToolDef.function.name
CanonicalToolDef.description       → KimiToolDef.function.description
CanonicalToolDef.parameters_schema → KimiToolDef.function.parameters
```

## System Message Handling

- System messages use `role: "system"` and appear in the `messages` array (same as OpenAI).
- Multiple system messages are allowed.
- System messages are **not** a separate top-level field (unlike Claude/Gemini).

### ABP Mapping

```
Kimi messages[role=system].content → IrMessage { role: IrRole::System, ... }
```

## Token Counting

| Kimi Field | ABP `UsageNormalized` Field |
|------------|----------------------------|
| `usage.prompt_tokens` | `input_tokens` |
| `usage.completion_tokens` | `output_tokens` |
| `usage.total_tokens` | `input_tokens + output_tokens` |

**Note:** Kimi does not expose cache token counts.

## Finish Reasons

| Kimi `finish_reason` | Meaning | ABP `Outcome` |
|---------------------|---------|----------------|
| `"stop"` | Natural stop or stop sequence hit | `Complete` |
| `"tool_calls"` | Model wants to invoke tools | `Complete` (mid-turn) |
| `"length"` | `max_tokens` reached | `Partial` |
| `null` | Still generating (streaming) | N/A |

## Content Types

| Type | Support | Notes |
|------|---------|-------|
| Text | ✅ | Primary content type |
| Images (input) | ❌ | Not supported in current API |
| Images (output) | ❌ | Not supported |
| Tool calls | ✅ | `tool_calls` in assistant message |
| Tool results | ✅ | `role: "tool"` messages |
| Web search | ✅ | Built-in `$web_search` tool |
| Browser | ✅ | Built-in `$browser` tool |
| Citation refs | ✅ | `refs` array in response (Kimi-specific) |
| Structured output | ⚠️ | Emulated via tool schema |
| Extended thinking | ✅ | Via `k1` reasoning model |

## Citation References (Kimi-Specific)

Kimi responses can include citation references in a `refs` array:

```jsonc
{
  "refs": [
    {
      "index": 0,
      "url": "https://example.com/article",
      "title": "Relevant Article Title"
    }
  ]
}
```

### `KimiRef` Struct

| Field | Type | Description |
|-------|------|-------------|
| `index` | `u32` | Citation index (0-based) |
| `url` | `String` | Source URL |
| `title` | `String` | Source title |

References are preserved in ABP vendor extensions but have no direct IR equivalent.

## K1 Reasoning Mode

Kimi supports extended reasoning via the `k1` model, analogous to OpenAI's `o1`/`o3` reasoning models. When `use_k1_reasoning` is enabled in the config, the model performs chain-of-thought reasoning before responding.

### ABP Mapping

The `k1` reasoning mode maps to the `ExtendedThinking` capability. Reasoning content is preserved in the `AgentEvent` trace.

## Model Names

| Model | Context Window | Notes |
|-------|---------------|-------|
| `moonshot-v1-8k` | 8K | Default in ABP shim |
| `moonshot-v1-32k` | 32K | Medium context |
| `moonshot-v1-128k` | 128K | Long context |
| `kimi-latest` | Varies | Latest stable model |
| `k1` | Varies | Reasoning model |

### Model Canonicalization

```
Vendor model:    "moonshot-v1-8k"
Canonical form:  "moonshot/moonshot-v1-8k"
```

The `to_canonical_model()` function adds the `moonshot/` prefix; `from_canonical_model()` strips it.

## Configuration

### `KimiConfig` Struct

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `api_key` | `String` | `""` | Moonshot API key |
| `base_url` | `String` | `"https://api.moonshot.cn/v1"` | API base URL |
| `model` | `String` | `"moonshot-v1-8k"` | Default model |
| `max_tokens` | `Option<u32>` | `Some(4096)` | Default max output tokens |
| `temperature` | `Option<f64>` | `None` | Default temperature |
| `use_k1_reasoning` | `Option<bool>` | `None` | Enable k1 reasoning mode |

## Error Codes

| HTTP Status | Error Type | Description |
|-------------|-----------|-------------|
| 400 | `invalid_request_error` | Malformed request |
| 401 | `authentication_error` | Invalid API key |
| 403 | `permission_error` | Insufficient permissions |
| 429 | `rate_limit_error` | Rate limit exceeded |
| 500 | `server_error` | Internal server error |

### ABP Shim Error Types (`ShimError`)

| Variant | When |
|---------|------|
| `InvalidRequest(String)` | Request validation failed |
| `Internal(String)` | Internal conversion failure |
| `Serde(serde_json::Error)` | Serialization error |

### ABP Error Mapping

| Kimi Error | ABP `ErrorCode` |
|-----------|-----------------|
| `invalid_request_error` | `IR_LOWERING_FAILED` or `CONFIG_INVALID` |
| `authentication_error` | `BACKEND_CRASHED` |
| `rate_limit_error` | `BACKEND_TIMEOUT` |
| `server_error` | `BACKEND_CRASHED` |

## Key Differences from OpenAI

### Built-in Tools

```jsonc
// OpenAI — no built-in tools in Chat Completions
{ "tools": [{ "type": "function", "function": { ... } }] }

// Kimi — built-in tools alongside functions
{ "tools": [
    { "type": "function", "function": { ... } },
    { "type": "builtin_function", "function": { "name": "$web_search" } }
  ]
}
```

### Web Search Toggle

```jsonc
// Kimi — top-level search toggle
{ "use_search": true }

// OpenAI — no equivalent (Responses API has web_search tool)
```

### Citation References

```jsonc
// Kimi — citation references in response
{ "refs": [{ "index": 0, "url": "...", "title": "..." }] }

// OpenAI — no equivalent in Chat Completions
```

### API Compatibility

Kimi's Chat Completions API is largely OpenAI-compatible with these additions:
- `builtin_function` tool type for `$web_search` and `$browser`
- `use_search` request field to enable web search
- `refs` response field for citation references
- `k1` reasoning model

## Capability Support Matrix

| Capability | Support | Notes |
|-----------|---------|-------|
| `Streaming` | ✅ Native | SSE protocol, same as OpenAI |
| `ToolUse` | ✅ Native | Function tools + built-in tools |
| `ToolRead` | ✅ Native | Via function tools |
| `ToolWrite` | ⚠️ Emulated | Via function tools |
| `ToolEdit` | ❌ Unsupported | No native edit capability |
| `ToolBash` | ❌ Unsupported | No native shell execution |
| `ToolWebSearch` | ✅ Native | Built-in `$web_search` tool |
| `StructuredOutputJsonSchema` | ⚠️ Emulated | Via tool schema constraints |
| `ExtendedThinking` | ✅ Native | Via `k1` reasoning model |
| `ImageInput` | ❌ Unsupported | Not in current API |
| `McpClient` | ❌ Unsupported | Not available |
| `McpServer` | ❌ Unsupported | Not available |
| `StopSequences` | ✅ Native | Via `stop` parameter |

## Naming Convention Comparison

| Concept | OpenAI | Kimi |
|---------|--------|------|
| Max tokens | `max_tokens` | `max_tokens` |
| Stop sequences | `stop` | `stop` |
| Finish reason | `finish_reason` | `finish_reason` |
| Token usage | `prompt_tokens` | `prompt_tokens` |
| Tool args | `arguments` (string) | `arguments` (string) |
| Model role | `"assistant"` | `"assistant"` |
| System msg | in messages array | in messages array |
| API casing | snake_case | snake_case |

Kimi follows OpenAI's naming conventions almost exactly. The primary API surface differences are the built-in tool types and citation references.
