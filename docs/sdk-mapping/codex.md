# OpenAI Codex / Responses API Surface Area

> Mapping reference for the OpenAI Responses API as used by Codex, implemented by `abp-shim-codex`.

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/responses` | POST | Responses API (item-based) |

## Relationship to Chat Completions

The Responses API is OpenAI's newer API that replaces Chat Completions for agentic use cases. Key architectural differences:

| Aspect | Chat Completions | Responses API (Codex) |
|--------|------------------|----------------------|
| Message format | `messages[]` array | `input[]` item array |
| Response format | `choices[].message` | `output[]` item array |
| Multi-turn | Re-send full history | `previous_response_id` chaining |
| Built-in tools | None | `web_search`, `file_search`, `code_interpreter` |
| Tool definition | `tools[]` with `function` | `tools[]` with `function` + built-in types |
| Output structure | Single message per choice | Multiple items (messages, function calls) |
| Streaming | SSE with deltas | SSE with typed events |

## Request Format

### Responses API Request

```jsonc
{
  "model": "codex-mini-latest",             // required
  "input": [                                 // required, array of input items
    {
      "type": "message",                     // input item type
      "role": "system",                      // "system" | "user" | "assistant"
      "content": "You are a coding assistant."
    },
    {
      "type": "message",
      "role": "user",
      "content": "Fix the bug in main.rs"
    },
    {
      "type": "function_call",               // previous function call
      "id": "fc_abc123",
      "call_id": "call_abc123",
      "name": "read_file",
      "arguments": "{\"path\":\"main.rs\"}"
    },
    {
      "type": "function_call_output",        // function result
      "call_id": "call_abc123",
      "output": "fn main() { panic!() }"
    }
  ],
  "max_output_tokens": 4096,                // optional
  "temperature": 0.0,                       // optional, 0.0–2.0
  "tools": [                                // optional
    {
      "type": "function",
      "name": "read_file",
      "description": "Read a file",
      "parameters": {
        "type": "object",
        "properties": {
          "path": { "type": "string" }
        },
        "required": ["path"]
      }
    }
  ],
  "text": {                                  // optional, structured output
    "format": {
      "type": "json_schema",
      "name": "output",
      "schema": { /* JSON Schema */ }
    }
  }
}
```

### ABP Shim Types

#### Input Items (`CodexInputItem` enum)

| Variant | Fields | Description |
|---------|--------|-------------|
| `Message` | `role, content` | Text message (system/user/assistant) |
| `FunctionCall` | `id, call_id, name, arguments` | Previous tool invocation |
| `FunctionCallOutput` | `call_id, output` | Tool result |

#### `CodexRequestBuilder`

| Field | Type | ABP Mapping |
|-------|------|-------------|
| `model` | `Option<String>` | → `WorkOrder.config.model` (default: `"codex-mini-latest"`) |
| `input` | `Vec<CodexInputItem>` | → `IrConversation` via `lowering::input_to_ir()` |
| `max_output_tokens` | `Option<u32>` | → `vendor.max_output_tokens` |
| `temperature` | `Option<f64>` | → `vendor.temperature` |
| `tools` | `Vec<CodexTool>` | → `IrToolDefinition` list |
| `text` | `Option<CodexTextFormat>` | → `vendor.text` |

## Response Format

### Responses API Response

```jsonc
{
  "id": "resp_abc123",
  "model": "codex-mini-latest",
  "output": [
    {
      "type": "message",
      "role": "assistant",
      "content": [
        {
          "type": "output_text",
          "text": "I found the bug..."
        }
      ]
    },
    {
      "type": "function_call",
      "id": "fc_xyz789",
      "call_id": "call_xyz789",
      "name": "write_file",
      "arguments": "{\"path\":\"main.rs\",\"content\":\"fn main() {}\"}"
    }
  ],
  "usage": {
    "input_tokens": 100,
    "output_tokens": 50,
    "total_tokens": 150
  },
  "status": "completed"
}
```

### ABP Shim Struct: `CodexResponse` (from `abp-codex-sdk`)

| Field | Type | ABP Source |
|-------|------|-----------|
| `id` | `String` | `"resp_" + Receipt.meta.run_id` |
| `model` | `String` | Echoed from request |
| `output` | `Vec<CodexResponseItem>` | Built from `Receipt.trace` events |
| `usage` | `Option<CodexUsage>` | From `Receipt.usage` |
| `status` | `Option<String>` | `"completed"` on success |

### Output Items (`CodexResponseItem` enum)

| Variant | Fields | Source |
|---------|--------|--------|
| `Message` | `role, content: Vec<CodexContentPart>` | From `AssistantMessage` / `AssistantDelta` events |
| `FunctionCall` | `id, call_id, name, arguments` | From `ToolCall` events |

### Content Parts (`CodexContentPart` enum)

| Variant | Fields | Notes |
|---------|--------|-------|
| `OutputText` | `text: String` | Text output |

## Streaming Format

**Protocol:** Server-Sent Events (SSE) over HTTP.

**Content-Type:** `text/event-stream`

Codex/Responses API uses typed SSE events:

```
event: response.created
data: {"type":"response.created","response":{"id":"resp_abc","status":"in_progress",...}}

event: response.output_item.delta
data: {"type":"response.output_item.delta","output_index":0,"delta":{"type":"output_text_delta","text":"Hello"}}

event: response.output_item.done
data: {"type":"response.output_item.done","output_index":0,"item":{"type":"message",...}}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_abc","status":"completed",...}}
```

### Stream Event Types (`CodexStreamEvent`)

| Event | Payload | Description |
|-------|---------|-------------|
| `ResponseCreated` | `response: CodexResponse` | Initial response metadata |
| `OutputItemDelta` | `output_index, delta` | Incremental update |
| `OutputItemDone` | `output_index, item` | Output item completed |
| `ResponseCompleted` | `response: CodexResponse` | Final response with usage |

### Delta Types (`CodexStreamDelta`)

| Delta Type | Fields | Description |
|-----------|--------|-------------|
| `OutputTextDelta` | `text: String` | Incremental text |

### ABP Event Mapping

| Codex Stream Event | ABP `AgentEventKind` |
|-------------------|----------------------|
| `OutputItemDelta` (text) | `AssistantDelta { text }` |
| `OutputItemDone` (message) | `AssistantMessage { text }` |
| `OutputItemDone` (function_call) | `ToolCall { tool_name, input }` |
| `ResponseCompleted` | `RunCompleted` |

## Tool Calling Conventions

- **Invocation:** Model returns `function_call` output items.
- **Correlation:** By `call_id` (similar to OpenAI Chat Completions' `tool_call_id`).
- **Result:** Client sends `function_call_output` input item with matching `call_id`.
- **Arguments:** JSON-encoded **string** (same as Chat Completions).
- **Built-in tools:** Unlike Chat Completions, the Responses API has built-in tools:
  - `web_search` — internet search
  - `file_search` — vector store search
  - `code_interpreter` — sandboxed code execution
- **Sandbox config:** Codex supports `SandboxConfig` for execution environment.

### ABP IR Mapping

```
Codex function_call.name       → IrToolCall.name
Codex function_call.arguments  → IrToolCall.input (parsed from JSON string)
Codex function_call.call_id    → IrToolCall.id
Codex function_call_output     → IrToolResult { call_id, output }
```

## System Message Handling

- System messages are `input` items with `type: "message"` and `role: "system"`.
- Part of the `input` array (like OpenAI Chat Completions, unlike Claude/Gemini).

### ABP Mapping

```
Codex input[type=message, role=system] → IrMessage { role: IrRole::System, ... }
```

## Token Counting

| Codex Field | ABP `UsageNormalized` Field |
|------------|----------------------------|
| `usage.input_tokens` | `input_tokens` |
| `usage.output_tokens` | `output_tokens` |
| `usage.total_tokens` | `input_tokens + output_tokens` |

The Responses API also provides detailed breakdowns (not yet mapped):
- `input_tokens_details.cached_tokens`
- `output_tokens_details.reasoning_tokens`

## Finish Reasons / Status

Codex uses a `status` field instead of per-choice `finish_reason`:

| Codex `status` | Meaning | ABP `Outcome` |
|---------------|---------|----------------|
| `"completed"` | All output generated | `Complete` |
| `"in_progress"` | Still generating | N/A (streaming) |
| `"failed"` | Generation failed | `Failed` |
| `"incomplete"` | Stopped early (max tokens, content filter) | `Partial` |

## Content Types

| Type | Support | Notes |
|------|---------|-------|
| Text | ✅ | `output_text` content parts |
| Images (input) | ✅ | Via `input_image` items |
| Images (output) | ❌ | Not in Responses API |
| Tool calls | ✅ | `function_call` output items |
| Tool results | ✅ | `function_call_output` input items |
| Structured output | ✅ | `text.format` with JSON schema |
| Code execution | ✅ | Built-in `code_interpreter` tool |
| Web search | ✅ | Built-in `web_search` tool |

## Model Names

| Model | Context Window | Notes |
|-------|---------------|-------|
| `codex-mini-latest` | 200K | Default in ABP shim, fast |
| `o3` | 200K | Full reasoning |
| `o3-mini` | 200K | Fast reasoning |
| `o4-mini` | 200K | Latest fast reasoning |
| `gpt-4o` | 128K | General purpose |
| `gpt-4o-mini` | 128K | Cost-optimized |
| `gpt-4.1` | 1M | Latest GPT |

## Error Codes

Same as OpenAI Chat Completions (see [`openai.md`](openai.md#error-codes)).

| HTTP Status | Error Type | Description |
|-------------|-----------|-------------|
| 400 | `invalid_request_error` | Malformed request |
| 401 | `authentication_error` | Invalid API key |
| 429 | `rate_limit_error` | Rate limit exceeded |
| 500 | `server_error` | Internal server error |

## Key Differences from Chat Completions

### Input Format

```jsonc
// Chat Completions — flat message array
{ "messages": [{ "role": "user", "content": "Hello" }] }

// Responses API — typed input items
{ "input": [{ "type": "message", "role": "user", "content": "Hello" }] }
```

### Tool Call Format

```jsonc
// Chat Completions — tool_calls in assistant message
{
  "role": "assistant",
  "tool_calls": [{ "id": "call_1", "type": "function", "function": { "name": "f", "arguments": "{}" } }]
}

// Responses API — separate output item
{
  "type": "function_call",
  "id": "fc_1",
  "call_id": "call_1",
  "name": "f",
  "arguments": "{}"
}
```

### Tool Result Format

```jsonc
// Chat Completions — role: "tool" message
{ "role": "tool", "tool_call_id": "call_1", "content": "result" }

// Responses API — function_call_output input item
{ "type": "function_call_output", "call_id": "call_1", "output": "result" }
```

### Response Structure

```jsonc
// Chat Completions — choices with messages
{ "choices": [{ "message": { "role": "assistant", "content": "..." }, "finish_reason": "stop" }] }

// Responses API — output items with status
{ "output": [{ "type": "message", "role": "assistant", "content": [...] }], "status": "completed" }
```

### Multi-Turn Conversation

```jsonc
// Chat Completions — re-send entire history
{ "messages": [msg1, msg2, msg3, msg4, msg5] }

// Responses API — chain via previous_response_id
{ "input": [msg5], "previous_response_id": "resp_prev" }
```
