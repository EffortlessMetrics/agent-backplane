# SDK Surface Area Documentation

> Comprehensive mapping reference for how each vendor SDK's API maps to ABP's Intermediate Representation (IR).

## Overview

The Agent Backplane normalizes six vendor dialects into a single IR (`abp_core::ir`).
Each SDK crate (`abp-{vendor}-sdk`) provides:

- **`dialect.rs`** — Native request/response types, config, capability manifest, tool definitions, and `map_work_order` / `map_response` functions.
- **`lowering.rs`** — `to_ir()` and `from_ir()` functions that convert between vendor message types and `IrConversation`.

The IR types live in `abp_core::ir`:

| IR Type              | Purpose                                      |
|----------------------|----------------------------------------------|
| `IrRole`             | Normalized role: `System`, `User`, `Assistant`, `Tool` |
| `IrContentBlock`     | Discriminated union: `Text`, `Image`, `ToolUse`, `ToolResult`, `Thinking` |
| `IrMessage`          | Role + ordered content blocks + metadata     |
| `IrConversation`     | Ordered `Vec<IrMessage>` with accessors      |
| `IrToolDefinition`   | Canonical tool: name, description, JSON Schema parameters |
| `IrUsage`            | Normalized token counters (input, output, cache read/write) |

---

## 1. OpenAI Chat Completions (`abp-openai-sdk`)

### Key API Types

| Vendor Type          | ABP Type              | Notes                                    |
|----------------------|-----------------------|------------------------------------------|
| `OpenAIRequest`      | `WorkOrder`           | `map_work_order()` converts              |
| `OpenAIResponse`     | `Vec<AgentEvent>`     | `map_response()` converts                |
| `OpenAIMessage`      | `IrMessage`           | `lowering::to_ir()` / `from_ir()`        |
| `OpenAIToolDef`      | `CanonicalToolDef`    | `tool_def_to_openai()` / `tool_def_from_openai()` |
| `OpenAIToolCall`     | `IrContentBlock::ToolUse` | ID, function name, JSON arguments    |
| `OpenAIUsage`        | `UsageNormalized`     | `prompt_tokens` → `input_tokens`         |

### Message Format

- **Roles**: `system`, `user`, `assistant`, `tool`
- **Content**: Flat string (`Option<String>`)
- **Tool calls**: Array of `{id, type:"function", function:{name, arguments}}` on assistant messages
- **Tool results**: Separate message with `role:"tool"`, `tool_call_id`, and string `content`

### Streaming Protocol

- **Protocol**: SSE (Server-Sent Events)
- **Chunk type**: `chat.completion.chunk` (`ChatCompletionChunk`)
- **Delta**: `ChunkDelta` with optional `role`, `content`, `tool_calls`
- **Tool call accumulation**: `ToolCallAccumulator` reassembles streamed fragments
- **Mapping**: `streaming::map_chunk()` → `AgentEventKind::AssistantDelta`

### Capabilities

| Capability                  | Support Level |
|-----------------------------|---------------|
| Streaming                   | Native        |
| Tool Read/Write/Edit/Bash   | Emulated      |
| Tool Glob/Grep              | Emulated      |
| Structured Output (JSON Schema) | Native   |
| Hooks (Pre/Post Tool Use)   | Emulated      |
| MCP Client/Server           | Unsupported   |

### Configuration Parameters

| Parameter      | Type           | Default          |
|----------------|----------------|------------------|
| `model`        | `String`       | `"gpt-4o"`       |
| `max_tokens`   | `Option<u32>`  | `Some(4096)`     |
| `temperature`  | `Option<f64>`  | `None`           |
| `tool_choice`  | `Option<ToolChoice>` | `None`     |
| `response_format` | `Option<ResponseFormat>` | `None` |

### Known Mapping Challenges

- **Unknown roles** (e.g. `developer`) silently map to `IrRole::User`
- **Malformed tool arguments** are preserved as `Value::String` (not rejected)
- **Image content** not supported in `OpenAIMessage` (flat string only)
- **`top_p`**, **`stop`**, **`logprobs`**, **`seed`** not modeled in `OpenAIConfig`
- **Thinking/reasoning** blocks not natively supported; mapped to text on `from_ir`

---

## 2. Anthropic Claude (`abp-claude-sdk`)

### Key API Types

| Vendor Type              | ABP Type              | Notes                                |
|--------------------------|-----------------------|--------------------------------------|
| `ClaudeRequest`          | `WorkOrder`           | `map_work_order()` converts          |
| `ClaudeResponse`         | `Vec<AgentEvent>`     | `map_response()` converts            |
| `ClaudeMessage`          | `IrMessage`           | `lowering::to_ir()` / `from_ir()`    |
| `ClaudeContentBlock`     | `IrContentBlock`      | Direct 1:1 mapping                   |
| `ClaudeToolDef`          | `CanonicalToolDef`    | `input_schema` ↔ `parameters_schema` |
| `ClaudeUsage`            | `UsageNormalized`     | Includes cache tokens                |

### Message Format

- **Roles**: `user`, `assistant` (system is request-level, not a message)
- **Content**: Array of typed blocks: `text`, `tool_use`, `tool_result`, `thinking`, `image`
- **Tool calls**: `tool_use` content block with `{id, name, input}`
- **Tool results**: `tool_result` content block on a `user` message with `{tool_use_id, content, is_error}`
- **System prompt**: Separate `system` field on request (not in messages array)

### Streaming Protocol

- **Protocol**: SSE (Server-Sent Events)
- **Event types**: `ClaudeStreamEvent` (tagged enum)
  - `message_start` → `RunStarted`
  - `content_block_start` → `ToolCall` (for tool_use blocks)
  - `content_block_delta` → `AssistantDelta` (text_delta, thinking_delta, input_json_delta, signature_delta)
  - `message_stop` → `RunCompleted`
  - `error` → `Error`
  - `ping`, `content_block_stop`, `message_delta` → no ABP event

### Capabilities

| Capability                  | Support Level |
|-----------------------------|---------------|
| Streaming                   | Native        |
| Tool Read/Write/Edit/Bash   | Native        |
| Tool Glob/Grep/WebSearch/WebFetch | Native  |
| Structured Output (JSON Schema) | Native   |
| Hooks (Pre/Post Tool Use)   | Native        |
| MCP Client                  | Native        |
| MCP Server                  | Unsupported   |
| Checkpointing               | Emulated      |

### Configuration Parameters

| Parameter        | Type                      | Default                          |
|------------------|---------------------------|----------------------------------|
| `model`          | `String`                  | `"claude-sonnet-4-20250514"`     |
| `max_tokens`     | `u32`                     | `4096`                           |
| `system_prompt`  | `Option<String>`          | `None`                           |
| `thinking`       | `Option<ThinkingConfig>`  | `None` (budget_tokens when enabled) |

### Known Mapping Challenges

- **System prompt** lives outside messages array; `lowering::from_ir()` skips `IrRole::System` messages
- **Image URL source** lossily maps to `IrContentBlock::Text` (`"[image: {url}]"`)
- **Thinking signature** lost in IR roundtrip (signature is `None` after `from_ir`)
- **`temperature`**, **`top_p`**, **`stop_sequences`** not in `ClaudeConfig` struct
- **Cache control** (`ClaudeCacheControl`) has no IR equivalent
- **Passthrough mode** supported via `to_passthrough_event()` / `from_passthrough_event()`

---

## 3. Google Gemini (`abp-gemini-sdk`)

### Key API Types

| Vendor Type                  | ABP Type              | Notes                            |
|------------------------------|-----------------------|----------------------------------|
| `GeminiRequest`              | `WorkOrder`           | `map_work_order()` converts      |
| `GeminiResponse`             | `Vec<AgentEvent>`     | `map_response()` converts        |
| `GeminiContent`              | `IrMessage`           | `lowering::to_ir()` / `from_ir()`|
| `GeminiPart`                 | `IrContentBlock`      | Text, InlineData, FunctionCall, FunctionResponse |
| `GeminiFunctionDeclaration`  | `CanonicalToolDef`    | `parameters` ↔ `parameters_schema` |
| `GeminiUsageMetadata`        | `UsageNormalized`     | `prompt_token_count` → `input_tokens` |

### Message Format

- **Roles**: `user`, `model` (no explicit system role; uses `system_instruction`)
- **Content**: Array of `parts`: `Text(String)`, `InlineData`, `FunctionCall`, `FunctionResponse`
- **Tool calls**: `FunctionCall { name, args }` part (no per-call ID)
- **Tool results**: `FunctionResponse { name, response }` part (correlated by name)
- **System instruction**: Separate `system_instruction` field on request

### Streaming Protocol

- **Protocol**: JSON array streaming (newline-delimited JSON chunks)
- **Chunk type**: `GeminiStreamChunk` (same shape as `GeminiResponse`)
- **Mapping**: `map_stream_chunk()` → text parts emit `AssistantDelta`, function calls emit `ToolCall`

### Capabilities

| Capability                  | Support Level |
|-----------------------------|---------------|
| Streaming                   | Native        |
| Tool Read                   | Native        |
| Tool Write/Edit/Bash        | Emulated      |
| Tool Glob/Grep              | Unsupported   |
| Structured Output (JSON Schema) | Native   |
| MCP Client/Server           | Unsupported   |

### Configuration Parameters

| Parameter              | Type           | Default                        |
|------------------------|----------------|--------------------------------|
| `model`                | `String`       | `"gemini-2.5-flash"`           |
| `max_output_tokens`    | `Option<u32>`  | `Some(4096)`                   |
| `temperature`          | `Option<f64>`  | `None`                         |
| `top_p`                | `Option<f64>`  | In `GeminiGenerationConfig`    |
| `top_k`                | `Option<u32>`  | In `GeminiGenerationConfig`    |
| `stop_sequences`       | `Option<Vec<String>>` | In `GeminiGenerationConfig` |
| `response_mime_type`   | `Option<String>` | In `GeminiGenerationConfig`  |
| `response_schema`      | `Option<Value>` | In `GeminiGenerationConfig`   |

### Known Mapping Challenges

- **No per-call tool ID**: Gemini uses name-based correlation; IR synthesizes `"gemini_{name}"` IDs
- **Thinking blocks** lossily map to `GeminiPart::Text` (no native thinking support)
- **InlineData** maps to `IrContentBlock::Image` (only base64 supported)
- **Safety settings/ratings** have no IR equivalent (dropped)
- **Citation metadata** has no IR equivalent (dropped)
- **Grounding config** (Google Search) has no IR equivalent

---

## 4. OpenAI Codex / Responses API (`abp-codex-sdk`)

### Key API Types

| Vendor Type              | ABP Type              | Notes                                |
|--------------------------|-----------------------|--------------------------------------|
| `CodexRequest`           | `WorkOrder`           | `map_work_order()` converts          |
| `CodexResponse`          | `Vec<AgentEvent>`     | `map_response()` converts            |
| `CodexInputItem`         | `IrMessage`           | `lowering::input_to_ir()`            |
| `CodexResponseItem`      | `IrMessage`           | `lowering::to_ir()` / `from_ir()`    |
| `CodexContentPart`       | `IrContentBlock`      | `OutputText`, `Refusal`              |
| `CodexUsage`             | `IrUsage`             | `usage_to_ir()`                      |

### Message Format

- **Input items**: `EasyInputMessage { role, content }`, `Message`, `ItemReference`
- **Response items**: `Message`, `FunctionCall`, `FunctionCallOutput`, `Reasoning`
- **Roles**: `system`/`developer`, `user`, `assistant`
- **Content**: `CodexContentPart` enum (`OutputText`, `Refusal`)
- **Tool calls**: `FunctionCall { call_id, name, arguments }` as separate response item
- **Tool results**: `FunctionCallOutput { call_id, output }` as separate item

### Streaming Protocol

- **Protocol**: SSE (Server-Sent Events)
- **Event types**: `CodexStreamEvent` (tagged enum)
  - `response.created` → `RunStarted`
  - `response.output_item.added` → item begin
  - `response.output_text.delta` → `AssistantDelta`
  - `response.function_call_arguments.delta` → accumulated
  - `response.completed` → `RunCompleted`

### Capabilities

| Capability                  | Support Level |
|-----------------------------|---------------|
| Streaming                   | Native        |
| Tool Read/Write/Edit/Bash   | Native        |
| Tool Glob/Grep              | Emulated      |
| Structured Output (JSON Schema) | Native   |
| Hooks (Pre/Post Tool Use)   | Emulated      |
| MCP Client/Server           | Unsupported   |

### Configuration Parameters

| Parameter        | Type                     | Default               |
|------------------|--------------------------|-----------------------|
| `model`          | `String`                 | `"codex-mini-latest"` |
| `max_tokens`     | `Option<u32>`            | `Some(4096)`          |
| `temperature`    | `Option<f64>`            | `None`                |
| `instructions`   | `Option<String>`         | `None`                |

### Known Mapping Challenges

- **Item-based format** differs from message-based (items are not conversations)
- **Reasoning/thinking** maps to `IrContentBlock::Thinking` (with `ReasoningSummary`)
- **Refusal content** has no direct IR equivalent (mapped to text)
- **`developer` role** maps to `IrRole::System`
- **Sandbox config** (`CodexSandboxConfig`) has no IR equivalent
- **`from_ir()` skips** system/user messages (only assistant/tool produce response items)

---

## 5. Moonshot Kimi (`abp-kimi-sdk`)

### Key API Types

| Vendor Type          | ABP Type              | Notes                                    |
|----------------------|-----------------------|------------------------------------------|
| `KimiRequest`        | `WorkOrder`           | `map_work_order()` converts              |
| `KimiResponse`       | `Vec<AgentEvent>`     | `map_response()` converts                |
| `KimiMessage`        | `IrMessage`           | `lowering::to_ir()` / `from_ir()`        |
| `KimiToolCall`       | `IrContentBlock::ToolUse` | OpenAI-compatible format             |
| `KimiUsage`          | `IrUsage`             | `usage_to_ir()`                          |

### Message Format

- **Roles**: `system`, `user`, `assistant`, `tool` (OpenAI-compatible)
- **Content**: Flat string (`Option<String>`)
- **Tool calls**: OpenAI-compatible `tool_calls` array on assistant messages
- **Tool results**: `role:"tool"` with `tool_call_id`

### Streaming Protocol

- **Protocol**: SSE (OpenAI-compatible)
- **Chunk type**: `KimiStreamChunk` (mirrors OpenAI `chat.completion.chunk`)
- **Mapping**: Same pattern as OpenAI streaming

### Capabilities

| Capability                  | Support Level |
|-----------------------------|---------------|
| Streaming                   | Native        |
| Tool Read                   | Native        |
| Tool Write                  | Emulated      |
| Tool Edit/Bash              | Unsupported   |
| Web Search                  | Native        |
| Structured Output (JSON Schema) | Emulated |
| MCP Client/Server           | Unsupported   |

### Configuration Parameters

| Parameter      | Type           | Default             |
|----------------|----------------|---------------------|
| `model`        | `String`       | `"moonshot-v1-8k"`  |
| `max_tokens`   | `Option<u32>`  | `Some(4096)`        |
| `temperature`  | `Option<f64>`  | `None`              |

### Known Mapping Challenges

- **`refs` field** (citation references) has no IR equivalent
- **`search_plus`** (built-in web search config) has no IR equivalent
- **`k1` reasoning mode** not modeled in IR thinking blocks
- **`use_search`** parameter for enabling built-in search has no IR mapping
- **Context window variants** (8k/32k/128k) are model-level, not config-level

---

## 6. GitHub Copilot (`abp-copilot-sdk`)

### Key API Types

| Vendor Type              | ABP Type              | Notes                                |
|--------------------------|-----------------------|--------------------------------------|
| `CopilotRequest`         | `WorkOrder`           | `map_work_order()` converts          |
| `CopilotResponse`        | `Vec<AgentEvent>`     | `map_response()` converts            |
| `CopilotMessage`         | `IrMessage`           | `lowering::to_ir()` / `from_ir()`    |
| `CopilotReference`       | IR metadata           | Stored in `IrMessage.metadata["copilot_references"]` |
| `CopilotConfirmation`    | —                     | No IR equivalent                     |

### Message Format

- **Roles**: `system`, `user`, `assistant` (OpenAI-compatible base)
- **Content**: Flat string content
- **Tool calls**: OpenAI-compatible format
- **References**: Extra `copilot_references` field on messages (file refs, snippets)
- **Confirmations**: Client-side action confirmations (no IR mapping)
- **Agent mode**: `agent_mode` field for behavior control

### Streaming Protocol

- **Protocol**: SSE (OpenAI-compatible)
- **Mapping**: Same pattern as OpenAI streaming

### Capabilities

| Capability                  | Support Level |
|-----------------------------|---------------|
| Streaming                   | Native        |
| Tool Read/Write/Edit/Bash   | Emulated      |
| Tool Glob/Grep              | Unsupported   |
| Web Search                  | Native        |
| Structured Output (JSON Schema) | Emulated |
| Hooks (Pre/Post Tool Use)   | Emulated      |
| MCP Client/Server           | Unsupported   |

### Configuration Parameters

| Parameter      | Type           | Default       |
|----------------|----------------|---------------|
| `model`        | `String`       | `"gpt-4o"`   |
| `max_tokens`   | `Option<u32>`  | `Some(4096)`  |
| `temperature`  | `Option<f64>`  | `None`        |

### Known Mapping Challenges

- **`references`** preserved in IR metadata but lossy for non-Copilot targets
- **`confirmations`** have no IR equivalent (dropped on cross-dialect mapping)
- **`agent_mode`** has no IR equivalent
- **`copilot_references`** roundtrip via IR metadata JSON serialization
- **Model routing** through Copilot proxy (not direct API) — models may be aliased

---

## Cross-SDK Comparison Matrix

### Role Mapping

| IR Role     | OpenAI     | Claude      | Gemini      | Codex          | Kimi       | Copilot    |
|-------------|------------|-------------|-------------|----------------|------------|------------|
| `System`    | `system`   | request-level `system` | `system_instruction` | `developer`/`system` | `system` | `system` |
| `User`      | `user`     | `user`      | `user`      | `user`         | `user`     | `user`     |
| `Assistant` | `assistant`| `assistant` | `model`     | `assistant`    | `assistant`| `assistant`|
| `Tool`      | `tool`     | `user`+blocks | `user`+FunctionResponse | separate item | `tool` | (OpenAI-compat) |

### Content Model

| Feature            | OpenAI         | Claude              | Gemini            | Codex              | Kimi           | Copilot        |
|--------------------|----------------|---------------------|-------------------|--------------------|----------------|----------------|
| Text               | flat string    | `text` block        | `Text` part       | `OutputText` part  | flat string    | flat string    |
| Image input        | ✗              | `image` block       | `InlineData` part | ✗                  | ✗              | ✗              |
| Tool use           | `tool_calls[]` | `tool_use` block    | `FunctionCall`    | `FunctionCall` item| `tool_calls[]` | `tool_calls[]` |
| Tool result        | `role:tool`    | `tool_result` block | `FunctionResponse`| `FunctionCallOutput` | `role:tool` | (OpenAI-compat)|
| Thinking           | ✗              | `thinking` block    | ✗                 | `Reasoning` item   | ✗              | ✗              |
| Tool use ID        | ✓ per-call     | ✓ per-block         | ✗ (name-based)    | ✓ (`call_id`)      | ✓ per-call     | ✓ per-call     |

### Streaming Protocol

| SDK     | Protocol | Delta type          | Tool accumulation |
|---------|----------|---------------------|-------------------|
| OpenAI  | SSE      | `chat.completion.chunk` | `ToolCallAccumulator` |
| Claude  | SSE      | `ClaudeStreamEvent` (typed) | Structured events |
| Gemini  | JSON     | `GeminiStreamChunk` | Per-chunk complete |
| Codex   | SSE      | `CodexStreamEvent` (typed) | Argument accumulation |
| Kimi    | SSE      | OpenAI-compatible   | OpenAI-compatible |
| Copilot | SSE      | OpenAI-compatible   | OpenAI-compatible |

### Parameter Support

| Parameter      | OpenAI | Claude | Gemini | Codex | Kimi | Copilot |
|----------------|--------|--------|--------|-------|------|---------|
| `temperature`  | ✓      | ✗*     | ✓      | ✓     | ✓    | ✓       |
| `max_tokens`   | ✓      | ✓      | ✓      | ✓     | ✓    | ✓       |
| `top_p`        | ✗*     | ✗      | ✓      | ✗     | ✗    | ✗       |
| `top_k`        | ✗      | ✗      | ✓      | ✗     | ✗    | ✗       |
| `stop`         | ✗*     | ✗      | ✓      | ✗     | ✗    | ✗       |
| `seed`         | ✗      | ✗      | ✗      | ✗     | ✗    | ✗       |

*✗ = Not in SDK Config struct (may be supported by API but not modeled)*

---

## Lowering Function Summary

Each SDK crate provides these core translation functions:

| SDK Crate          | `to_ir()`           | `from_ir()`         | `map_work_order()`  | `map_response()` | `map_stream_event()` |
|--------------------|---------------------|---------------------|---------------------|-------------------|-----------------------|
| `abp-openai-sdk`   | ✓ `lowering::to_ir` | ✓ `lowering::from_ir` | ✓ `dialect::map_work_order` | ✓ `dialect::map_response` | ✓ `streaming::map_chunk` |
| `abp-claude-sdk`   | ✓ `lowering::to_ir` | ✓ `lowering::from_ir` | ✓ `dialect::map_work_order` | ✓ `dialect::map_response` | ✓ `dialect::map_stream_event` |
| `abp-gemini-sdk`   | ✓ `lowering::to_ir` | ✓ `lowering::from_ir` | ✓ `dialect::map_work_order` | ✓ `dialect::map_response` | ✓ `dialect::map_stream_event` |
| `abp-codex-sdk`    | ✓ `lowering::to_ir` | ✓ `lowering::from_ir` | ✓ `dialect::map_work_order` | ✓ `dialect::map_response` | ✓ `dialect::map_stream_event` |
| `abp-kimi-sdk`     | ✓ `lowering::to_ir` | ✓ `lowering::from_ir` | ✓ `dialect::map_work_order` | ✓ `dialect::map_response` | ✓ `dialect::map_stream_event` |
| `abp-copilot-sdk`  | ✓ `lowering::to_ir` | ✓ `lowering::from_ir` | ✓ `dialect::map_work_order` | ✓ `dialect::map_response` | ✓ `dialect::map_stream_event` |
