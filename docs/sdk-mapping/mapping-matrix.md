# SDK Mapping Matrix

> Cross-SDK feature comparison and mapping strategies for the Agent Backplane translation layer.

## Overview

This matrix documents how each SDK's features map to ABP's Intermediate Representation (IR) and where lossy translations occur. The ABP shim crates (`abp-shim-openai`, `abp-shim-claude`, `abp-shim-gemini`, `abp-shim-codex`, `abp-shim-copilot`, `abp-shim-kimi`) implement these mappings.

## Feature × SDK Matrix

### Message Roles

| Role | OpenAI | Claude | Gemini | Codex | Copilot | Kimi | ABP IR |
|------|--------|--------|--------|-------|---------|------|--------|
| System | `"system"` | top-level `system` field | `systemInstruction` | `"system"` in input | `"system"` | `"system"` | `IrRole::System` |
| User | `"user"` | `"user"` | `"user"` | `"user"` in input | `"user"` | `"user"` | `IrRole::User` |
| Assistant | `"assistant"` | `"assistant"` | `"model"` | `"assistant"` in input | `"assistant"` | `"assistant"` | `IrRole::Assistant` |
| Tool result | `"tool"` | `"user"` (tool_result block) | `"user"` (functionResponse) | `function_call_output` | N/A | `"tool"` | `IrRole::Tool` |

### Content Model

| Feature | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|---------|--------|--------|--------|-------|---------|------|
| Content type | string or null | array of blocks | array of parts | typed items | string | string |
| Multimodal | content array | content blocks | parts array | input items | ❌ | ❌ |
| Text | ✅ string | `{type:"text"}` | `{text:"..."}` | `output_text` part | ✅ string | ✅ string |
| Images (in) | `image_url` part | `{type:"image"}` | `inlineData` part | `input_image` item | ❌ | ❌ |
| Images (out) | ❌ | ❌ | ✅ (Imagen) | ❌ | ❌ | ❌ |
| Thinking | ❌ | `{type:"thinking"}` | ❌ | ❌ | ❌ | ❌ |

### Tool Calling

| Feature | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|---------|--------|--------|--------|-------|---------|------|
| Tool invocation | `tool_calls[]` in message | `tool_use` block | `functionCall` part | `function_call` item | `function_call` field | `tool_calls[]` |
| Correlation | `tool_call.id` | `tool_use.id` | function name | `call_id` | `function_call.id` | `tool_call.id` |
| Arguments format | JSON string | JSON object | JSON object | JSON string | JSON string | JSON string |
| Result delivery | `role:"tool"` msg | `tool_result` block | `functionResponse` part | `function_call_output` item | N/A (callback) | `role:"tool"` msg |
| Parallel calls | ✅ | ✅ | ✅ | ✅ | ❌ (single) | ✅ |
| Tool choice | `tool_choice` | ❌ (via prompt) | `toolConfig.mode` | `tool_choice` | ❌ | `tool_choice` |

### Streaming Protocol

| Feature | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|---------|--------|--------|--------|-------|---------|------|
| Protocol | SSE | SSE | JSONL | SSE | SSE | SSE |
| Content-Type | `text/event-stream` | `text/event-stream` | `application/json` | `text/event-stream` | `text/event-stream` | `text/event-stream` |
| Event prefix | `data: ` | `event: `+`data: ` | (none, raw JSON) | `event: `+`data: ` | `data: ` | `data: ` |
| End sentinel | `data: [DONE]` | `event: message_stop` | last JSON object | `event: response.completed` | `data: [DONE]` | `data: [DONE]` |
| Text delta field | `delta.content` | `delta.text` | `parts[].text` | `delta.text` | `text` | `delta.content` |
| Usage in stream | final chunk (opt-in) | `message_delta` event | final chunk | `response.completed` | ❌ | final chunk |

### Token Counting

| Field | OpenAI | Claude | Gemini | Codex | Kimi | ABP IR |
|-------|--------|--------|--------|-------|------|--------|
| Input tokens | `prompt_tokens` | `input_tokens` | `promptTokenCount` | `input_tokens` | `prompt_tokens` | `input_tokens` |
| Output tokens | `completion_tokens` | `output_tokens` | `candidatesTokenCount` | `output_tokens` | `completion_tokens` | `output_tokens` |
| Total tokens | `total_tokens` | (computed) | `totalTokenCount` | `total_tokens` | `total_tokens` | `total_tokens` |
| Cache tokens | ❌ (standard) | `cache_*_input_tokens` | ❌ | `cached_tokens` | ❌ | `cache_read/write_tokens` |

### Finish / Stop Reasons

| Meaning | OpenAI | Claude | Gemini | Codex | ABP Outcome |
|---------|--------|--------|--------|-------|-------------|
| Natural stop | `"stop"` | `"end_turn"` | `"STOP"` | `"completed"` | `Complete` |
| Tool use | `"tool_calls"` | `"tool_use"` | `"STOP"` | `"completed"` | `Complete` |
| Token limit | `"length"` | `"max_tokens"` | `"MAX_TOKENS"` | `"incomplete"` | `Partial` |
| Content filter | `"content_filter"` | N/A | `"SAFETY"` | `"incomplete"` | `Failed` |
| Stop sequence | `"stop"` | `"stop_sequence"` | `"STOP"` | N/A | `Complete` |

### Request Parameters

| Parameter | OpenAI | Claude | Gemini | Codex | Kimi |
|-----------|--------|--------|--------|-------|------|
| Model | `model` | `model` | URL path | `model` | `model` |
| Max tokens | `max_tokens` (opt) | `max_tokens` (req) | `maxOutputTokens` | `max_output_tokens` | `max_tokens` |
| Temperature | `temperature` | `temperature` | `temperature` | `temperature` | `temperature` |
| Top-p | `top_p` | `top_p` | `topP` | ❌ | `top_p` |
| Top-k | ❌ | `top_k` | `topK` | ❌ | ❌ |
| Stop seqs | `stop` | `stop_sequences` | `stopSequences` | ❌ | `stop` |
| Structured out | `response_format` | via tool schema | `responseMimeType`+`Schema` | `text.format` | ❌ |
| Stream | `stream` | `stream` | URL path suffix | `stream` | `stream` |

### Naming Conventions

| Aspect | OpenAI | Claude | Gemini | Codex |
|--------|--------|--------|--------|-------|
| Casing | snake_case | snake_case | camelCase | snake_case |
| Enum values | lowercase | snake_case | UPPER_SNAKE_CASE | lowercase |
| Object type tag | `object` field | `type` field | (none) | `type` field |
| ID prefix | `chatcmpl-` | `msg_` | (none) | `resp_` |
| Tool call ID prefix | `call_` | `toolu_` | (none) | `fc_` / `call_` |

## Mapping Strategies

### Lossless Mappings (Passthrough-safe)

These features map 1:1 across SDKs with no data loss:

| Feature | Strategy |
|---------|----------|
| Text messages | Direct content mapping |
| Role mapping | Lookup table (system/user/assistant/tool → IR) |
| Temperature | Direct numeric passthrough |
| Max tokens | Direct numeric passthrough (field name differs) |

### Lossy Mappings (Mapped mode)

These features require transformation and may lose information:

| Feature | Loss | Strategy |
|---------|------|----------|
| System message location | Position → field | Extract from messages array or top-level field |
| Tool call arguments | String ↔ object parse | Parse/serialize JSON at boundary |
| Tool correlation | ID scheme differs | Remap IDs, Gemini uses name-based correlation |
| Streaming format | SSE vs JSONL | Protocol adapter at transport layer |
| Content blocks | Structure varies | Flatten/unflatten content model |
| Extended thinking | Claude-only | Drop for non-Claude backends |
| Safety settings | Gemini-only | Drop for non-Gemini backends |
| Cache token counts | Claude-only | Report as 0 for other backends |

### Unsupported Translations

These features cannot be mapped across certain SDK pairs:

| Source Feature | Target | Handling |
|---------------|--------|----------|
| Claude thinking → OpenAI | No equivalent | Omit thinking blocks |
| Gemini safety → Claude | No equivalent | Omit safety settings |
| Codex built-in tools → Claude | No equivalent | Emit `CAPABILITY_UNSUPPORTED` |
| OpenAI audio → Gemini | Different API | Emit `CAPABILITY_UNSUPPORTED` |
| Copilot references → any | Copilot-specific | Preserved in vendor extensions |

## ABP IR Conversion Pipeline

```
SDK Request
    │
    ▼
┌─────────────────────┐
│  abp-{vendor}-sdk   │  dialect types + lowering functions
│  dialect::to_ir()   │
└────────┬────────────┘
         │
         ▼
┌─────────────────────┐
│  IrConversation     │  vendor-neutral messages + tools
│  IrMessage          │
│  IrToolDefinition   │
│  IrUsage            │
└────────┬────────────┘
         │
         ▼
┌─────────────────────┐
│  WorkOrder          │  ABP contract type
│  (abp-core)         │
└────────┬────────────┘
         │ execute
         ▼
┌─────────────────────┐
│  Receipt            │  ABP contract type
│  (abp-core)         │
└────────┬────────────┘
         │
         ▼
┌─────────────────────┐
│  IrConversation     │  vendor-neutral response
└────────┬────────────┘
         │
         ▼
┌─────────────────────┐
│  abp-{vendor}-sdk   │  dialect types + from_ir()
│  dialect::from_ir() │
└────────┬────────────┘
         │
         ▼
SDK Response
```

## Shim Crate Architecture

Each shim crate follows the same pattern:

```
abp-shim-{vendor}/
├── src/lib.rs          # Public API: Client, Builder, Request/Response types
│                       # Conversions: request_to_ir(), receipt_to_response()
│                       # Streaming: events_to_stream_events()
└── Cargo.toml          # Depends on abp-core + abp-{vendor}-sdk
```

### Common Types Across Shims

| Type | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|------|--------|--------|--------|-------|---------|------|
| Error | `ShimError` | `ShimError` | `GeminiError` | `ShimError` | `ShimError` | `ShimError` |
| Client | `OpenAiClient` | `AnthropicClient` | `GeminiClient` | `CodexClient` | `CopilotClient` | `KimiClient` |
| Builder | `ChatCompletionRequestBuilder` | (direct struct) | (builder methods) | `CodexRequestBuilder` | `CopilotRequestBuilder` | `KimiRequestBuilder` |
| ProcessFn | `Box<dyn Fn(&WorkOrder) → Receipt>` | (handler callbacks) | (internal) | `Box<dyn Fn(&WorkOrder) → Receipt>` | `Box<dyn Fn(&WorkOrder) → Receipt>` | `Box<dyn Fn(&WorkOrder) → Receipt>` |

### Conversion Functions

| Function | Direction | Present In |
|----------|-----------|------------|
| `request_to_ir()` | SDK Request → IR | All shims |
| `request_to_work_order()` | SDK Request → WorkOrder | All shims |
| `receipt_to_response()` | Receipt → SDK Response | All shims |
| `events_to_stream_events()` | AgentEvents → Stream Events | All shims |
| `ir_to_messages()` | IR → SDK Messages | OpenAI, Copilot, Kimi |
| `messages_to_ir()` | SDK Messages → IR | OpenAI, Copilot, Kimi |

## Capability Support Matrix

Based on `abp-core::Capability` enum and backend capability manifests:

| Capability | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|-----------|--------|--------|--------|-------|---------|------|
| `Streaming` | ✅ Native | ✅ Native | ✅ Native | ✅ Native | ✅ Native | ✅ Native |
| `ToolUse` | ✅ Native | ✅ Native | ✅ Native | ✅ Native | ✅ Native | ✅ Native |
| `ToolRead` | ✅ Native | ✅ Native | ✅ Native | ✅ Native | ✅ Native | ⚠️ Emulated |
| `ToolWrite` | ✅ Native | ✅ Native | ✅ Native | ✅ Native | ✅ Native | ⚠️ Emulated |
| `ToolEdit` | ✅ Native | ✅ Native | ⚠️ Emulated | ✅ Native | ✅ Native | ⚠️ Emulated |
| `ToolBash` | ✅ Native | ✅ Native | ⚠️ Emulated | ✅ Native | ✅ Native | ❌ |
| `ToolGlob` | ✅ Native | ✅ Native | ⚠️ Emulated | ✅ Native | ✅ Native | ⚠️ Emulated |
| `ToolGrep` | ✅ Native | ✅ Native | ⚠️ Emulated | ✅ Native | ✅ Native | ⚠️ Emulated |
| `ToolWebSearch` | ⚠️ Emulated | ⚠️ Emulated | ✅ Native | ✅ Native | ✅ Native | ✅ Native |
| `ToolWebFetch` | ⚠️ Emulated | ⚠️ Emulated | ✅ Native | ✅ Native | ✅ Native | ⚠️ Emulated |
| `ExtendedThinking` | ❌ | ✅ Native | ❌ | ❌ | ❌ | ✅ Native |
| `ImageInput` | ✅ Native | ✅ Native | ✅ Native | ✅ Native | ❌ | ❌ |
| `PdfInput` | ❌ | ✅ Native | ✅ Native | ❌ | ❌ | ❌ |
| `StructuredOutputJsonSchema` | ✅ Native | ⚠️ Emulated | ✅ Native | ✅ Native | ❌ | ❌ |
| `SessionResume` | ❌ | ❌ | ❌ | ✅ Native | ❌ | ❌ |
| `Checkpointing` | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| `CodeExecution` | ❌ | ❌ | ✅ Native | ✅ Native | ❌ | ❌ |
| `McpClient` | ❌ | ✅ Native | ✅ Native | ❌ | ✅ Native | ❌ |
| `StopSequences` | ✅ Native | ✅ Native | ✅ Native | ❌ | ❌ | ✅ Native |
| `SeedDeterminism` | ✅ Native | ❌ | ❌ | ❌ | ❌ | ❌ |

**Legend:** ✅ Native — ⚠️ Emulated — ❌ Unsupported

## Error Code Mapping

| ABP `ErrorCode` | OpenAI Source | Claude Source | Gemini Source | Codex Source |
|----------------|---------------|---------------|---------------|--------------|
| `CONFIG_INVALID` | `invalid_request_error` | `invalid_request_error` | `INVALID_ARGUMENT` | `invalid_request_error` |
| `BACKEND_CRASHED` | `server_error` | `api_error` | `INTERNAL` | `server_error` |
| `BACKEND_TIMEOUT` | `rate_limit_error` | `rate_limit_error` / `overloaded_error` | `RESOURCE_EXHAUSTED` | `rate_limit_error` |
| `BACKEND_NOT_FOUND` | 404 | 404 | `NOT_FOUND` | 404 |
| `IR_LOWERING_FAILED` | Parse/validation | Parse/validation | Parse/validation | Parse/validation |
| `DIALECT_MAPPING_FAILED` | Type mismatch | Type mismatch | Type mismatch | Type mismatch |
| `CAPABILITY_UNSUPPORTED` | Missing feature | Missing feature | Missing feature | Missing feature |
