# GitHub Copilot SDK Surface Area

> Mapping reference for the GitHub Copilot Extensions API as implemented by `abp-shim-copilot`.

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| Copilot Extensions agent endpoint | POST | Agent request/response |

**Base URL:** `https://api.githubcopilot.com`

## Request Format

### Copilot Agent Request

```jsonc
{
  "model": "gpt-4o",                         // required
  "messages": [                               // required
    {
      "role": "system",                       // "system" | "user" | "assistant"
      "content": "You are a helpful agent.",
      "name": "copilot-agent",               // optional display name
      "copilot_references": []                // optional per-message references
    },
    {
      "role": "user",
      "content": "Fix the bug in main.rs",
      "copilot_references": [                 // optional references
        {
          "type": "file",
          "id": "file-1",
          "data": { "path": "src/main.rs" },
          "metadata": { "display_name": "main.rs" }
        },
        {
          "type": "snippet",
          "id": "snip-1",
          "data": {
            "content": "fn main() { panic!() }",
            "language": "rust"
          }
        },
        {
          "type": "repository",
          "id": "repo-1",
          "data": { "owner": "octocat", "name": "hello-world" }
        }
      ]
    }
  ],
  "tools": [                                  // optional
    {
      "type": "function",
      "function": {
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
    },
    {
      "type": "confirmation"                  // Copilot-specific tool type
    }
  ],
  "turn_history": [                           // optional, multi-turn context
    {
      "role": "user",
      "content": "Previous message"
    },
    {
      "role": "assistant",
      "content": "Previous response"
    }
  ],
  "references": [                             // optional, top-level request refs
    {
      "type": "web_search_result",
      "id": "search-1",
      "data": { "url": "https://...", "snippet": "..." }
    }
  ]
}
```

### ABP Shim Struct: `CopilotRequest`

| Field | Type | ABP Mapping |
|-------|------|-------------|
| `model` | `String` | → `WorkOrder.config.model` |
| `messages` | `Vec<CopilotMessage>` | → `IrConversation` via `lowering::to_ir()` |
| `tools` | `Option<Vec<CopilotTool>>` | → `IrToolDefinition` list |
| `turn_history` | `Vec<CopilotTurnEntry>` | → Prepended to `IrConversation` |
| `references` | `Vec<CopilotReference>` | → Preserved in vendor extensions |

## Response Format

### Copilot Agent Response

```jsonc
{
  "message": "I found and fixed the bug in main.rs.",
  "copilot_references": [
    {
      "type": "file",
      "id": "file-1",
      "data": { "path": "src/main.rs" }
    }
  ],
  "copilot_errors": [],
  "copilot_confirmation": null,
  "function_call": null
}
```

### Response with Function Call

```jsonc
{
  "message": "",
  "copilot_references": [],
  "copilot_errors": [],
  "copilot_confirmation": null,
  "function_call": {
    "name": "read_file",
    "arguments": "{\"path\":\"src/main.rs\"}",
    "id": "call_abc123"
  }
}
```

### Response with Confirmation

```jsonc
{
  "message": "I'd like to delete this file.",
  "copilot_references": [],
  "copilot_errors": [],
  "copilot_confirmation": {
    "id": "confirm-1",
    "title": "Delete File",
    "message": "Are you sure you want to delete src/temp.rs?",
    "accepted": null
  },
  "function_call": null
}
```

### ABP Shim Struct: `CopilotResponse`

| Field | Type | ABP Source |
|-------|------|-----------|
| `message` | `String` | From `Receipt.trace` text events |
| `copilot_references` | `Vec<CopilotReference>` | From vendor extensions |
| `copilot_errors` | `Vec<CopilotError>` | From `Receipt.trace` error events |
| `copilot_confirmation` | `Option<CopilotConfirmation>` | From confirmation events |
| `function_call` | `Option<CopilotFunctionCall>` | From `ToolCall` events |

## Streaming Format

**Protocol:** Server-Sent Events (SSE) over HTTP.

**Content-Type:** `text/event-stream`

Copilot uses typed SSE events with `data: ` prefix and `[DONE]` sentinel:

```
data: {"type":"copilot_references","references":[...]}

data: {"type":"text_delta","text":"Hello"}

data: {"type":"text_delta","text":" world"}

data: {"type":"function_call","function_call":{"name":"read_file","arguments":"{...}","id":"call_1"}}

data: {"type":"copilot_confirmation","confirmation":{"id":"c1","title":"Confirm","message":"OK?","accepted":null}}

data: {"type":"done"}

data: [DONE]
```

### Stream Event Types (`CopilotStreamEvent`)

| Event | Payload | Description |
|-------|---------|-------------|
| `copilot_references` | `references: Vec<CopilotReference>` | References emitted at start of stream |
| `copilot_errors` | `errors: Vec<CopilotError>` | Errors during processing |
| `text_delta` | `text: String` | Incremental text fragment |
| `function_call` | `function_call: CopilotFunctionCall` | Tool invocation during streaming |
| `copilot_confirmation` | `confirmation: CopilotConfirmation` | User approval request |
| `done` | `{}` | Stream completed |

### ABP Event Mapping

| Copilot Stream Event | ABP `AgentEventKind` |
|---------------------|----------------------|
| `text_delta` | `AssistantDelta { text }` |
| `function_call` | `ToolCall { tool_name, input }` |
| `copilot_errors` | `Error { message }` |
| `copilot_references` | Preserved in vendor extensions |
| `copilot_confirmation` | Preserved in vendor extensions |
| `done` | `RunCompleted` |

## Tool Calling Conventions

- **Invocation:** Model returns a `function_call` field in the response or a `function_call` stream event.
- **Correlation:** By optional `id` field on `CopilotFunctionCall`.
- **Result:** N/A — Copilot uses a callback model rather than multi-turn tool result messages.
- **Arguments:** JSON-encoded **string** (same as OpenAI).
- **Parallel calls:** ❌ Single function call per response (no parallel tool calls).
- **Tool types:**
  - `"function"` — Standard function tool with name, description, JSON Schema parameters
  - `"confirmation"` — Copilot-specific confirmation tool (prompts user for approval)

### Key Differences from OpenAI

```
OpenAI:   tool_calls: [{ id, type, function: { name, arguments } }]  // array in message
Copilot:  function_call: { name, arguments, id }                     // single field in response
```

```
OpenAI:   Tool results sent as role:"tool" messages in next request
Copilot:  Tool results delivered via callback (not in request/response cycle)
```

### Confirmation Tool (Copilot-Specific)

Copilot supports a unique confirmation tool that prompts the user for approval before executing an action:

```jsonc
{
  "id": "confirm-1",
  "title": "Delete File",
  "message": "Are you sure you want to delete src/temp.rs?",
  "accepted": null    // null = pending, true = accepted, false = rejected
}
```

### ABP IR Mapping

```
Copilot function_call.name       → IrToolCall.name
Copilot function_call.arguments  → IrToolCall.input (parsed from JSON string)
Copilot function_call.id         → IrToolCall.id
```

### ABP Tool Definition Conversion

```
CanonicalToolDef.name              → CopilotTool(Function).function.name
CanonicalToolDef.description       → CopilotTool(Function).function.description
CanonicalToolDef.parameters_schema → CopilotTool(Function).function.parameters
```

**Note:** `Confirmation` tools have no canonical equivalent and are Copilot-specific.

## Reference System (Copilot-Specific)

Copilot has a rich reference system for attaching contextual information to messages and requests.

### Reference Types (`CopilotReferenceType`)

| Type | Description | Data Fields |
|------|-------------|-------------|
| `file` | File path reference | `path`, optional content |
| `snippet` | Code snippet with location | `content`, `language` |
| `repository` | Repository reference | `owner`, `name` |
| `web_search_result` | Web search result | `url`, `snippet` |

### `CopilotReference` Struct

| Field | Type | Description |
|-------|------|-------------|
| `ref_type` | `CopilotReferenceType` | Type discriminator (serialized as `type`) |
| `id` | `String` | Unique reference identifier |
| `data` | `serde_json::Value` | Structured data payload |
| `metadata` | `Option<BTreeMap<String, Value>>` | Optional display metadata (label, URI) |

### Where References Appear

| Location | Description |
|----------|-------------|
| `request.references` | Top-level request references |
| `message.copilot_references` | Per-message references |
| `response.copilot_references` | Response references |
| `stream: copilot_references` | Stream-initial references |

### ABP Mapping

References are preserved in ABP vendor extensions (`ext.copilot_references`). The `extract_references()` function collects references across all messages in an IR conversation.

## System Message Handling

- System messages use `role: "system"` in the `messages` array (same as OpenAI).
- Multiple system messages are allowed.
- An optional `system_prompt` override is available in `CopilotConfig`.

### ABP Mapping

```
Copilot messages[role=system].content → IrMessage { role: IrRole::System, ... }
```

## Token Counting

Copilot does not expose token usage in its standard response format. Usage is estimated or derived from the underlying model's response when available.

| ABP Field | Source |
|-----------|--------|
| `input_tokens` | Estimated from request |
| `output_tokens` | Estimated from response |
| `total_tokens` | Sum of input + output |

**Note:** No cache token breakdown is available.

## Error Handling

### `CopilotError` Struct

| Field | Type | Description |
|-------|------|-------------|
| `error_type` | `String` | Error classification |
| `message` | `String` | Human-readable error message |
| `code` | `Option<String>` | Optional error code |
| `identifier` | `Option<String>` | Optional error identifier |

### ABP Shim Error Types (`ShimError`)

| Variant | When |
|---------|------|
| `InvalidRequest(String)` | Request validation failed |
| `Internal(String)` | Internal conversion failure |
| `Serde(serde_json::Error)` | Serialization error |

### ABP Error Mapping

| Copilot Error | ABP `ErrorCode` |
|--------------|-----------------|
| Request validation failure | `IR_LOWERING_FAILED` or `CONFIG_INVALID` |
| Authentication failure | `BACKEND_CRASHED` |
| Rate limit | `BACKEND_TIMEOUT` |
| Internal error | `BACKEND_CRASHED` |

## Passthrough Fidelity

Copilot supports passthrough mode for bitwise-equivalent stream forwarding:

| Function | Purpose |
|----------|---------|
| `to_passthrough_event()` | Wraps raw event in `AgentEvent` with `ext.raw_message` + dialect marker |
| `from_passthrough_event()` | Extracts original event from passthrough `AgentEvent` |
| `verify_passthrough_fidelity()` | Validates roundtrip integrity of events |

In passthrough mode, the stream is forwarded without transformation, preserving Copilot-specific features like references and confirmations.

## Model Names

| Model | Context Window | Notes |
|-------|---------------|-------|
| `gpt-4o` | 128K | Default in ABP shim |
| `gpt-4o-mini` | 128K | Cost-optimized |
| `gpt-4-turbo` | 128K | Previous generation |
| `gpt-4` | 8K / 32K | Legacy |
| `o1` | 200K | Reasoning model |
| `o1-mini` | 128K | Fast reasoning |
| `o3-mini` | 200K | Fast advanced reasoning |
| `claude-sonnet-4` | 200K | Claude via Copilot |
| `claude-3.5-sonnet` | 200K | Claude via Copilot |

### Model Canonicalization

```
Vendor model:    "gpt-4o"
Canonical form:  "copilot/gpt-4o"
```

The `to_canonical_model()` function adds the `copilot/` prefix; `from_canonical_model()` strips it.

## Configuration

### `CopilotConfig` Struct

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `token` | `String` | `""` | GitHub Copilot authentication token |
| `base_url` | `String` | `"https://api.githubcopilot.com"` | API base URL |
| `model` | `String` | `"gpt-4o"` | Default model |
| `system_prompt` | `Option<String>` | `None` | Optional system prompt override |

## Content Types

| Type | Support | Notes |
|------|---------|-------|
| Text | ✅ | Primary content type (string) |
| Images (input) | ❌ | Not supported |
| Images (output) | ❌ | Not supported |
| Tool calls | ✅ | Single `function_call` per response |
| Tool results | ❌ | Callback-based, not in request/response |
| References | ✅ | Copilot-specific reference system |
| Confirmations | ✅ | Copilot-specific approval prompts |
| Structured output | ⚠️ | Emulated via tool schema |
| Web search | ✅ | Via web search result references |

## Capability Support Matrix

| Capability | Support | Notes |
|-----------|---------|-------|
| `Streaming` | ✅ Native | SSE protocol with typed events |
| `ToolUse` | ✅ Native | Single function call per response |
| `ToolRead` | ⚠️ Emulated | Via function tools |
| `ToolWrite` | ⚠️ Emulated | Via function tools |
| `ToolEdit` | ⚠️ Emulated | Via function tools |
| `ToolBash` | ⚠️ Emulated | Via function tools |
| `ToolGlob` | ❌ Unsupported | Not available |
| `ToolGrep` | ❌ Unsupported | Not available |
| `ToolWebSearch` | ✅ Native | Via reference system |
| `StructuredOutputJsonSchema` | ⚠️ Emulated | Via tool schema constraints |
| `HooksPreToolUse` | ⚠️ Emulated | Via confirmation tool |
| `HooksPostToolUse` | ⚠️ Emulated | Via confirmation tool |
| `ExtendedThinking` | ❌ Unsupported | Not available |
| `ImageInput` | ❌ Unsupported | Not available |
| `McpClient` | ❌ Unsupported | Not available |
| `McpServer` | ❌ Unsupported | Not available |

## Key Differences from OpenAI

### Content Model

```
OpenAI:   content: string | null | array of content parts
Copilot:  content: string (always a string)
```

### Tool Invocation

```
OpenAI:   assistant message with tool_calls array (parallel calls)
Copilot:  response-level function_call field (single call)
```

### Tool Results

```
OpenAI:   role: "tool" message in next request
Copilot:  callback-based delivery (not in request/response)
```

### References

```
OpenAI:   No equivalent
Copilot:  Rich reference system (files, snippets, repos, web results)
```

### Confirmations

```
OpenAI:   No equivalent
Copilot:  copilot_confirmation for user approval prompts
```

### Multi-Turn History

```
OpenAI:   Re-send full message history
Copilot:  Separate turn_history field + current messages
```

## Naming Convention Comparison

| Concept | OpenAI | Copilot |
|---------|--------|---------|
| Max tokens | `max_tokens` | N/A (model default) |
| Finish reason | `finish_reason` | N/A (done event) |
| Token usage | `usage` object | Not exposed |
| Tool args | `arguments` (string) | `arguments` (string) |
| Model role | `"assistant"` | `"assistant"` |
| System msg | in messages array | in messages array |
| API casing | snake_case | snake_case |
| ID prefix | `chatcmpl-` | N/A |
