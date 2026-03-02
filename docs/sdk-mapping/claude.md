# Anthropic Claude SDK Surface Area

> Mapping reference for the Anthropic Messages API as implemented by `abp-shim-claude`.

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/messages` | POST | Messages API (primary) |

## Request Format

### Messages Request

```jsonc
{
  "model": "claude-sonnet-4-20250514",   // required
  "max_tokens": 4096,                     // required (unlike OpenAI)
  "messages": [                            // required
    {
      "role": "user",                      // "user" | "assistant" only
      "content": "Hello"                   // string OR array of content blocks
    },
    {
      "role": "assistant",
      "content": [
        { "type": "text", "text": "Let me check..." },
        {
          "type": "tool_use",
          "id": "toolu_abc123",
          "name": "get_weather",
          "input": { "city": "NYC" }       // parsed JSON object (not string)
        }
      ]
    },
    {
      "role": "user",
      "content": [
        {
          "type": "tool_result",
          "tool_use_id": "toolu_abc123",
          "content": "72°F, sunny",
          "is_error": false
        }
      ]
    }
  ],
  "system": "You are helpful.",            // optional, separate from messages
  "temperature": 0.7,                      // 0.0–1.0, optional
  "stop_sequences": ["Human:"],            // optional
  "thinking": {                            // optional, extended thinking
    "type": "enabled",
    "budget_tokens": 10000
  },
  "stream": false                          // optional
}
```

### ABP Shim Struct: `MessageRequest`

| Field | Type | ABP Mapping |
|-------|------|-------------|
| `model` | `String` | → `WorkOrder.config.model` |
| `max_tokens` | `u32` | → `vendor.max_tokens` |
| `messages` | `Vec<Message>` | → `IrConversation` via Claude SDK `lowering::to_ir()` |
| `system` | `Option<String>` | → `ClaudeRequest.system` (separate field) |
| `temperature` | `Option<f64>` | → `vendor.temperature` |
| `stop_sequences` | `Option<Vec<String>>` | → `vendor.stop_sequences` |
| `thinking` | `Option<ThinkingConfig>` | → `ClaudeRequest.thinking` |
| `stream` | `Option<bool>` | Controls streaming vs batch |

## Response Format

### Messages Response

```jsonc
{
  "id": "msg_abc123",
  "type": "message",
  "role": "assistant",
  "content": [
    {
      "type": "text",
      "text": "Hello! How can I help?"
    }
  ],
  "model": "claude-sonnet-4-20250514",
  "stop_reason": "end_turn",
  "stop_sequence": null,
  "usage": {
    "input_tokens": 25,
    "output_tokens": 10,
    "cache_creation_input_tokens": 0,
    "cache_read_input_tokens": 0
  }
}
```

### ABP Shim Struct: `MessageResponse`

| Field | Type | ABP Source |
|-------|------|-----------|
| `id` | `String` | From `ClaudeResponse.id` |
| `response_type` | `String` | Always `"message"` |
| `role` | `String` | Always `"assistant"` |
| `content` | `Vec<ContentBlock>` | Mapped from `ClaudeContentBlock` |
| `model` | `String` | Echoed from response |
| `stop_reason` | `Option<String>` | From `ClaudeResponse.stop_reason` |
| `stop_sequence` | `Option<String>` | From response |
| `usage` | `Usage` | Mapped from `ClaudeUsage` |

## Streaming Format

**Protocol:** Server-Sent Events (SSE) over HTTP.

**Content-Type:** `text/event-stream`

Events use the `event:` + `data:` format:

```
event: message_start
data: {"type":"message_start","message":{...}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":10}}

event: message_stop
data: {"type":"message_stop"}
```

### Stream Event Types

| Event | Payload | Description |
|-------|---------|-------------|
| `message_start` | `{ message: MessageResponse }` | Initial message metadata (incomplete) |
| `content_block_start` | `{ index, content_block }` | New content block begins |
| `content_block_delta` | `{ index, delta }` | Incremental update to a block |
| `content_block_stop` | `{ index }` | Block is complete |
| `message_delta` | `{ delta, usage }` | Message-level metadata (stop_reason) |
| `message_stop` | `{}` | Stream terminated |
| `ping` | `{}` | Keep-alive |
| `error` | `{ error: { type, message } }` | Error during streaming |

### Delta Types (`StreamDelta`)

| Delta Type | Fields | Description |
|-----------|--------|-------------|
| `text_delta` | `text: String` | Incremental text content |
| `input_json_delta` | `partial_json: String` | Incremental tool input JSON |
| `thinking_delta` | `thinking: String` | Incremental thinking content |
| `signature_delta` | `signature: String` | Incremental signature |

### ABP Event Mapping

| Claude Stream Event | ABP `AgentEventKind` |
|--------------------|----------------------|
| `content_block_delta` (text_delta) | `AssistantDelta { text }` |
| `content_block_delta` (input_json_delta) | Accumulated → `ToolCall` |
| `content_block_delta` (thinking_delta) | Extended thinking (logged) |
| `content_block_start` (tool_use) | Tool call begins |
| `message_delta` (stop_reason) | `RunCompleted` |
| `message_stop` | Stream termination |
| `error` | `Error { message }` |

## Tool Calling Conventions

- **Invocation:** Model returns `tool_use` content blocks within the assistant message.
- **Correlation:** Each tool use has a unique `id` (e.g., `toolu_abc123`).
- **Result:** User sends a `tool_result` content block with `tool_use_id`.
- **Arguments:** A **parsed JSON object** (not a string like OpenAI).
- **Parallel calls:** Multiple `tool_use` blocks in a single response are supported.
- **No `tool_choice` equivalent in v1:** Use system prompt guidance instead.

### Key Difference from OpenAI

```
OpenAI:  tool_calls[].function.arguments = "{\"city\":\"NYC\"}"  (JSON string)
Claude:  content[].input                 = {"city": "NYC"}       (JSON object)
```

### ABP IR Mapping

```
Claude tool_use.name      → IrToolCall.name
Claude tool_use.input     → IrToolCall.input (already parsed)
Claude tool_use.id        → IrToolCall.id
Claude tool_result        → IrToolResult { tool_use_id, content, is_error }
```

## System Message Handling

- System prompt is a **separate top-level field**, not part of the `messages` array.
- Only one system prompt per request (string).
- System prompt is **not** a message with a role.

### Key Difference from OpenAI

```
OpenAI:  messages: [{ role: "system", content: "..." }, { role: "user", ... }]
Claude:  system: "...",  messages: [{ role: "user", ... }]
```

### ABP Mapping

```
Claude system field → IrMessage { role: IrRole::System, ... } (prepended to conversation)
                    → ClaudeRequest.system (preserved as-is for passthrough)
```

## Token Counting

| Claude Field | ABP `UsageNormalized` Field |
|-------------|----------------------------|
| `usage.input_tokens` | `input_tokens` |
| `usage.output_tokens` | `output_tokens` |
| `usage.cache_creation_input_tokens` | `cache_write_tokens` |
| `usage.cache_read_input_tokens` | `cache_read_tokens` |

**Note:** Claude provides explicit cache token breakdowns, which ABP preserves in `UsageNormalized`.

## Finish Reasons

| Claude `stop_reason` | Meaning | ABP `Outcome` |
|---------------------|---------|----------------|
| `"end_turn"` | Natural completion | `Complete` |
| `"tool_use"` | Model wants to invoke tools | `Complete` (mid-turn) |
| `"max_tokens"` | Token limit reached | `Partial` |
| `"stop_sequence"` | Custom stop sequence hit | `Complete` |
| `null` | Still generating | N/A |

## Content Types

### Content Block Types (`ContentBlock` enum)

| Type | Tag | Fields | Notes |
|------|-----|--------|-------|
| Text | `"text"` | `text: String` | Primary content |
| Tool Use | `"tool_use"` | `id, name, input` | Assistant tool invocation |
| Tool Result | `"tool_result"` | `tool_use_id, content, is_error` | User tool response |
| Thinking | `"thinking"` | `thinking, signature` | Extended thinking block |
| Image | `"image"` | `source: ImageSource` | Image input |

### Image Sources

| Source Type | Fields | Description |
|------------|--------|-------------|
| `base64` | `media_type, data` | Inline base64-encoded image |
| `url` | `url` | URL-referenced image |

### Supported Media Types

| Type | Support | Notes |
|------|---------|-------|
| Text | ✅ | Primary |
| Images (input) | ✅ | JPEG, PNG, GIF, WebP |
| Images (output) | ❌ | Not supported |
| PDFs (input) | ✅ | Via `document` content block |
| Tool calls | ✅ | `tool_use` content blocks |
| Tool results | ✅ | `tool_result` content blocks |
| Extended thinking | ✅ | `thinking` content blocks |
| Structured output | ✅ | Via tool use with schema |

## Model Names

| Model | Context Window | Notes |
|-------|---------------|-------|
| `claude-sonnet-4-20250514` | 200K | Default in shim |
| `claude-opus-4-20250514` | 200K | Most capable |
| `claude-haiku-3-5-20241022` | 200K | Fastest, cheapest |
| `claude-3-5-sonnet-20241022` | 200K | Previous gen Sonnet |
| `claude-3-opus-20240229` | 200K | Previous gen Opus |

## Error Codes

| HTTP Status | Error Type | Description |
|-------------|-----------|-------------|
| 400 | `invalid_request_error` | Malformed request |
| 401 | `authentication_error` | Invalid API key |
| 403 | `permission_error` | Insufficient permissions |
| 404 | `not_found_error` | Unknown model/endpoint |
| 429 | `rate_limit_error` | Rate limit exceeded |
| 500 | `api_error` | Internal server error |
| 529 | `overloaded_error` | API overloaded |

### ABP Shim Error Types (`ShimError`)

| Variant | When |
|---------|------|
| `InvalidRequest(String)` | Request validation failed |
| `ApiError { error_type, message }` | Backend returned an error |
| `Internal(String)` | Internal conversion failure |

### ABP Error Mapping

| Claude Error | ABP `ErrorCode` |
|-------------|-----------------|
| `invalid_request_error` | `IR_LOWERING_FAILED` or `CONFIG_INVALID` |
| `authentication_error` | `BACKEND_CRASHED` |
| `rate_limit_error` | `BACKEND_TIMEOUT` |
| `overloaded_error` | `BACKEND_TIMEOUT` |
| `api_error` | `BACKEND_CRASHED` |

## Extended Thinking

Claude supports extended thinking via the `thinking` request parameter:

```jsonc
{
  "thinking": {
    "type": "enabled",
    "budget_tokens": 10000   // tokens allocated for reasoning
  }
}
```

When enabled, the response includes `thinking` content blocks **before** the text response. Thinking blocks include a `signature` field for verification.

### Streaming Thinking

During streaming, thinking appears as `thinking_delta` events followed by `signature_delta` events before the text content begins.

### ABP Mapping

Thinking blocks are preserved in the `AgentEvent` trace and mapped to the `ExtendedThinking` capability in the capability manifest.
