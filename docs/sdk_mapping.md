# SDK Mapping Matrix

> Complete mapping reference between the 6 supported SDK dialects and the ABP
> intermediate representation (IR). Based on actual crate source code in
> `crates/abp-shim-*/`, `crates/abp-*-sdk/`, and `crates/abp-ir/`.

---

## Table of Contents

1. [SDK Surface Area Overview](#1-sdk-surface-area-overview)
2. [Feature Mapping Matrix](#2-feature-mapping-matrix)
3. [Lossy Mappings](#3-lossy-mappings)
4. [IR (Intermediate Representation)](#4-ir-intermediate-representation)
5. [Error Handling](#5-error-handling)
6. [Architecture](#architecture)
7. [Related Documentation](#related-documentation)

---

## 1. SDK Surface Area Overview

Each vendor has a **dialect SDK crate** (`abp-<vendor>-sdk`) that defines wire-format
types plus a `lowering` module for IR conversion, and a **shim crate**
(`abp-shim-<vendor>`) that provides a drop-in client replacement routing
requests through ABP `WorkOrder` → `Receipt` pipeline.

### 1.1 OpenAI Chat Completions

| Aspect | Details |
|--------|---------|
| **SDK crate** | `abp-openai-sdk` · Shim: `abp-shim-openai` |
| **API shape** | `POST /v1/chat/completions` |
| **Default model** | `gpt-4` |
| **Request type** | `ChatCompletionRequest` — `model`, `messages: Vec<ChatMessage>`, `temperature`, `top_p`, `max_tokens`, `stream`, `tools: Vec<Tool>`, `tool_choice: ToolChoice` |
| **Response type** | `ChatCompletionResponse` — `id`, `object`, `created`, `model`, `choices: Vec<Choice>`, `usage: Usage` |
| **Message enum** | `ChatMessage` — role-tagged: `System { content }`, `User { content: MessageContent }`, `Assistant { content, tool_calls }`, `Tool { tool_call_id, content }` |
| **Streaming** | SSE `StreamChunk` structs containing `choices: Vec<StreamChoice>` with `delta: StreamDelta` (incremental `role`, `content`, `tool_calls`). Struct-based (not enum). |
| **Tool/function calling** | `Tool { tool_type: "function", function: FunctionDef { name, description, parameters } }`. `ToolCall { id, call_type, function: FunctionCall { name, arguments } }`. Supports `ToolChoice` enum: `Mode(none/auto/required)` or `Function { name }`. |
| **System messages** | In-line as `ChatMessage::System { content }` within the `messages` array. |
| **Multimodal** | `MessageContent::Parts(Vec<ContentPart>)` where `ContentPart` is `Text { text }` or `ImageUrl { image_url: ImageUrl }`. Image via URL reference with optional `detail` level. |
| **Lowering** | `lowering::to_ir()` → `IrConversation`, `lowering::from_ir()` → `Vec<OpenAIMessage>` |

### 1.2 Anthropic Claude (Messages API)

| Aspect | Details |
|--------|---------|
| **SDK crate** | `abp-claude-sdk` · Shim: `abp-shim-claude` |
| **API shape** | `POST /v1/messages` |
| **Default model** | `claude-sonnet-4-20250514` |
| **Request type** | `MessagesRequest` — `model`, `messages: Vec<ClaudeMessage>`, `max_tokens`, `system` (separate field), `temperature`, `top_p`, `top_k`, `stream`, `tools: Vec<ClaudeTool>`, `tool_choice: ClaudeToolChoice` |
| **Response type** | `MessagesResponse` — `id`, `type`, `role`, `content: Vec<ContentBlock>`, `model`, `stop_reason`, `usage: ClaudeUsage` |
| **Message struct** | `ClaudeMessage { role, content: ClaudeContent }` where `ClaudeContent` is `Text(String)` or `Blocks(Vec<ContentBlock>)`. |
| **Content blocks** | `ContentBlock` enum: `Text`, `Image` (with `ImageSource`: Base64 or URL), `ToolUse { id, name, input }`, `ToolResult { tool_use_id, content, is_error }`, `Thinking { text }` |
| **Streaming** | SSE `StreamEvent` enum: `MessageStart`, `ContentBlockStart`, `ContentBlockDelta`, `ContentBlockStop`, `MessageDelta`, `MessageStop`, `Ping`. Deltas: `TextDelta`, `InputJsonDelta`, `ThinkingDelta`, `SignatureDelta`. |
| **Tool calling** | `ClaudeTool { name, description, input_schema }`. Tool use via `ContentBlock::ToolUse`. `ClaudeToolChoice` enum: `Auto`, `Any`, `Tool { name }`. |
| **System messages** | Separate `system` field on request — **not** in the `messages` array. |
| **Multimodal** | `ContentBlock::Image` with `ImageSource::Base64 { media_type, data }` or `ImageSource::Url { url }`. |
| **Extended thinking** | Native `ContentBlock::Thinking { text }` / `StreamDelta::ThinkingDelta { thinking }`. |
| **Lowering** | `lowering::to_ir()` / `lowering::from_ir()`, `extract_system_prompt()` |

### 1.3 Google Gemini

| Aspect | Details |
|--------|---------|
| **SDK crate** | `abp-gemini-sdk` · Shim: `abp-shim-gemini` |
| **API shape** | `POST /v1beta/models/{model}:generateContent` |
| **Default model** | `gemini-2.5-flash` |
| **Request type** | `GenerateContentRequest` — `model`, `contents: Vec<Content>`, `system_instruction: Option<Content>`, `generation_config: GenerationConfig`, `safety_settings: Vec<SafetySetting>`, `tools: Vec<ToolDeclaration>`, `tool_config: ToolConfig` |
| **Response type** | `GenerateContentResponse` — `candidates: Vec<Candidate>`, `usage_metadata: UsageMetadata`. Helpers: `.text()`, `.function_calls()`. |
| **Content struct** | `Content { role, parts: Vec<Part> }` with builder methods `Content::user()`, `Content::model()`. |
| **Part enum** | `Part`: `Text(String)`, `InlineData { mime_type, data }` (base64), `FunctionCall { name, args }`, `FunctionResponse { name, response }`. |
| **Streaming** | `StreamEvent` struct (not enum) with `candidates` and `usage_metadata`. Struct-based like OpenAI. |
| **Tool calling** | `ToolDeclaration { function_declarations: Vec<FunctionDeclaration> }`. `FunctionDeclaration { name, description, parameters }`. `ToolConfig` with `FunctionCallingConfig { mode }`. |
| **System messages** | Separate `system_instruction: Option<Content>` field on request. |
| **Multimodal** | `Part::InlineData { mime_type, data }` for base64 images. |
| **Safety settings** | `SafetySetting { category: HarmCategory, threshold: BlockThreshold }` — unique to Gemini. |
| **Generation config** | `GenerationConfig` with `max_output_tokens`, `temperature`, `top_p`, `top_k`, `stop_sequences`, `response_mime_type`, `response_schema`. |
| **Lowering** | `lowering::to_ir()` / `lowering::from_ir()`, `extract_system_instruction()` |

### 1.4 OpenAI Codex (Responses API)

| Aspect | Details |
|--------|---------|
| **SDK crate** | `abp-codex-sdk` · Shim: `abp-shim-codex` |
| **API shape** | `POST /v1/responses` |
| **Default model** | `codex-mini-latest` |
| **Request type** | `CodexRequest` — `model`, `messages` (via builder), `instructions` (separate system field), `temperature`, `max_output_tokens`, `tools`, `tool_choice`, `text_format` |
| **Response type** | `CodexResponse` — `id`, `object`, `created`, `model`, `choices: Vec<CodexChoice>`, `usage: Usage`. Items are `CodexResponseItem` enum. |
| **Response items** | `CodexResponseItem` enum: `Message { role, content: Vec<CodexContentPart> }`, `FunctionCall { name, call_id, arguments }`, `FunctionCallOutput { call_id, output }`, `Reasoning { text }` |
| **Streaming** | `CodexStreamEvent` enum: `ResponseCreated`, `ResponseInProgress`, `OutputItemAdded`, `OutputItemDelta`, `OutputItemDone`, `ResponseCompleted`, `ResponseFailed`, `Error { message, code }`. Enum-based. |
| **Tool calling** | Same OpenAI `function` type wrapper. `CodexResponseItem::FunctionCall { name, call_id, arguments }` / `FunctionCallOutput { call_id, output }`. |
| **System messages** | Separate `instructions` field — **not** in messages. |
| **Multimodal** | Not supported in current implementation. |
| **Lowering** | `lowering::input_to_ir()` (request), `lowering::to_ir()` / `lowering::from_ir()` (response items) |

### 1.5 GitHub Copilot (Extensions API)

| Aspect | Details |
|--------|---------|
| **SDK crate** | `abp-copilot-sdk` · Shim: `abp-shim-copilot` |
| **API shape** | Copilot Extensions API |
| **Default model** | `gpt-4o` |
| **Request type** | `CopilotChatRequest` — `model`, `messages: Vec<CopilotChatMessage>`, `temperature`, `top_p`, `max_tokens`, `tools`, `tool_choice`, `intent`, `references: Vec<Reference>` |
| **Response type** | `CopilotChatResponse` — `id`, `type`, `role`, `content`, `model`, `usage`, `metadata` |
| **Message struct** | `CopilotChatMessage { role, content, name, tool_calls, tool_call_id }`. Shim `Message` has `copilot_references: Vec<CopilotReference>`. |
| **References** | `Reference { type, id, uri, content, metadata }` with types: `File`, `Selection`, `Terminal`, `WebPage`, `GitDiff`. Unique to Copilot. |
| **Streaming** | `CopilotStreamEvent` enum: `CopilotReferences { references }`, `CopilotErrors { errors }`, `TextDelta { text }`, `FunctionCall { function_call }`, `CopilotConfirmation { confirmation }`, `Done`. Enum-based. |
| **Tool calling** | `CopilotTool` with `CopilotFunctionDef`. Supports `CopilotConfirmation` for user-approval flows. |
| **System messages** | In-line in `messages` array as `role: "system"`. |
| **Multimodal** | Not supported in current implementation. |
| **Lowering** | `lowering::to_ir()` / `lowering::from_ir()` / `lowering::extract_references()` |

### 1.6 Moonshot Kimi

| Aspect | Details |
|--------|---------|
| **SDK crate** | `abp-kimi-sdk` · Shim: `abp-shim-kimi` |
| **API shape** | `POST /v1/chat/completions` (OpenAI-compatible) |
| **Default model** | `moonshot-v1-8k` |
| **Request type** | `KimiChatRequest` — OpenAI-compatible fields plus `search_options: SearchOptions { mode: SearchMode, result_count }` |
| **Response type** | `KimiChatResponse` — `id`, `object`, `created`, `model`, `choices`, `usage` (with Kimi extensions) |
| **Message enum** | `ChatMessage` — role-tagged: `System`, `User`, `Assistant` (with optional `tool_calls`), `Tool` |
| **Streaming** | `KimiChunk` struct with `id`, `object`, `created`, `model`, `choices`, `usage`, `refs`. Struct-based like OpenAI. |
| **Tool calling** | OpenAI-compatible `function` type with `function.parameters`. |
| **System messages** | In-line in `messages` array as `ChatMessage::System`. |
| **Multimodal** | Not supported in current implementation. |
| **Web search** | `SearchOptions { mode: SearchMode(Auto/Always/Never), result_count }` — unique to Kimi. |
| **Lowering** | `lowering::to_ir()` / `lowering::from_ir()`, `usage_to_ir()` |

---

## 2. Feature Mapping Matrix

### 2.1 Core Features

| Feature | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|---------|--------|--------|--------|-------|---------|------|
| Text messages | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| System messages | ✅ (in messages) | ✅ (separate `system` field) | ✅ (separate `system_instruction`) | ✅ (separate `instructions`) | ✅ (in messages) | ✅ (in messages) |
| Multi-turn conversation | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Tool/function calling | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Streaming | ✅ (struct) | ✅ (enum/SSE) | ✅ (struct) | ✅ (enum) | ✅ (enum) | ✅ (struct) |
| Image input | ✅ (URL) | ✅ (base64/URL) | ✅ (inline base64) | ❌ | ❌ | ❌ |
| Extended thinking | ❌ | ✅ (`Thinking` block) | ❌ | ✅ (`Reasoning` item) | ❌ | ❌ |
| Temperature | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Top-p | ✅ | ✅ | ✅ | ❌ | ✅ | ❌ |
| Top-k | ❌ | ✅ | ✅ | ❌ | ❌ | ❌ |
| Max tokens | ✅ `max_tokens` | ✅ `max_tokens` | ✅ `max_output_tokens` | ✅ `max_output_tokens` | ✅ `max_tokens` | ✅ `max_tokens` |
| Stop sequences | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ |
| Tool choice control | ✅ (none/auto/required/named) | ✅ (auto/any/named) | ✅ (`FunctionCallingConfig`) | ✅ | ✅ | ✅ |
| Structured output | ✅ `ResponseFormat` | ❌ | ✅ `response_schema` | ✅ `text_format` | ❌ | ❌ |
| Safety settings | ❌ | ❌ | ✅ `SafetySetting` | ❌ | ❌ | ❌ |
| Web search | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ `SearchOptions` |
| References/context | ❌ | ❌ | ❌ | ❌ | ✅ `Reference` | ❌ |
| Confirmations | ❌ | ❌ | ❌ | ❌ | ✅ `CopilotConfirmation` | ❌ |
| Cache tokens | ❌ | ✅ `cache_creation`/`cache_read` | ❌ | ❌ | ❌ | ❌ |

### 2.2 System Message Handling

| SDK | Location | Extraction |
|-----|----------|------------|
| **OpenAI** | `ChatMessage::System` inside `messages[]` | `lowering::to_ir()` maps to `IrRole::System` |
| **Claude** | `MessagesRequest.system` (separate field) | `lowering::extract_system_prompt()` pulls from field; shim `convert::to_work_order()` stores in vendor map |
| **Gemini** | `GenerateContentRequest.system_instruction` (separate `Content`) | `lowering::extract_system_instruction()` returns `GeminiContent` |
| **Codex** | `CodexRequest.instructions` (separate string) | `lowering::input_to_ir()` treats instructions as system |
| **Copilot** | `CopilotChatMessage { role: "system" }` inside `messages[]` | `lowering::to_ir()` maps to `IrRole::System` |
| **Kimi** | `ChatMessage::System` inside `messages[]` | `lowering::to_ir()` maps to `IrRole::System` |

### 2.3 Tool Definition Formats

| SDK | Wrapper Type | Schema Field | Tool ID Field | Example Shape |
|-----|-------------|-------------|---------------|---------------|
| **OpenAI** | `Tool { tool_type, function: FunctionDef }` | `parameters` | `ToolCall.id` | `{ type: "function", function: { name, description, parameters } }` |
| **Claude** | `ClaudeTool` | `input_schema` | `ContentBlock::ToolUse.id` | `{ name, description, input_schema }` |
| **Gemini** | `ToolDeclaration { function_declarations }` | `parameters` | Generated per-call | `{ function_declarations: [{ name, description, parameters }] }` |
| **Codex** | OpenAI-compatible | `function.parameters` | `FunctionCall.call_id` | `{ type: "function", function: { name, description, parameters } }` |
| **Copilot** | `CopilotTool { CopilotFunctionDef }` | `parameters` | `tool_call_id` | `{ type: "function", function: { name, description, parameters } }` |
| **Kimi** | OpenAI-compatible | `function.parameters` | `tool_call.id` | `{ type: "function", function: { name, description, parameters } }` |

### 2.4 Streaming Architecture

| SDK | Type Shape | Protocol | Key Types |
|-----|-----------|----------|-----------|
| **OpenAI** | Struct-based | SSE `data: {json}\n\n` | `StreamChunk { id, choices: [StreamChoice { delta: StreamDelta }] }` |
| **Claude** | Enum-based | SSE `event: {type}\ndata: {json}\n\n` | `StreamEvent::MessageStart\|ContentBlockStart\|ContentBlockDelta\|ContentBlockStop\|MessageDelta\|MessageStop\|Ping\|Error` |
| **Gemini** | Struct-based | SSE | `StreamEvent { candidates, usage_metadata }` (same shape as response) |
| **Codex** | Enum-based | SSE | `CodexStreamEvent::ResponseCreated\|ResponseInProgress\|OutputItemAdded\|OutputItemDelta\|OutputItemDone\|ResponseCompleted\|ResponseFailed\|Error` |
| **Copilot** | Enum-based | SSE | `CopilotStreamEvent::CopilotReferences\|TextDelta\|FunctionCall\|CopilotErrors\|CopilotConfirmation\|Done` |
| **Kimi** | Struct-based | SSE `data: {json}\n\n` | `KimiChunk { id, choices, usage, refs }` (OpenAI-compatible shape) |

### 2.5 Usage / Token Reporting

| SDK | Type | Input Tokens | Output Tokens | Total | Cache Fields |
|-----|------|-------------|---------------|-------|-------------|
| **OpenAI** | `Usage` | `prompt_tokens` | `completion_tokens` | `total_tokens` | — |
| **Claude** | `ClaudeUsage` | `input_tokens` | `output_tokens` | (computed) | `cache_creation_input_tokens`, `cache_read_input_tokens` |
| **Gemini** | `UsageMetadata` | `prompt_token_count` | `candidates_token_count` | `total_token_count` | — |
| **Codex** | `Usage` | `input_tokens` | `output_tokens` | `total_tokens` | — |
| **Copilot** | Tuple | `input_tokens` | `output_tokens` | `total_tokens` | — |
| **Kimi** | `Usage` | `prompt_tokens` | `completion_tokens` | `total_tokens` | — |

---

## 3. Lossy Mappings

Conversions between SDKs that **lose information** because the target format has
no equivalent concept. Each entry identifies the source feature, what is lost,
and which shim conversion function is involved.

### 3.1 Thinking / Extended Reasoning

| Source | Lost When Targeting | What Happens | Code Path |
|--------|-------------------|--------------|-----------|
| Claude `ContentBlock::Thinking { text }` | OpenAI, Gemini, Copilot, Kimi | `IrContentBlock::Thinking` preserved in IR but dropped during `from_ir()` for targets that have no thinking block type | `abp-shim-claude/src/lib.rs` `content_block_to_ir()` |
| Codex `CodexResponseItem::Reasoning { text }` | OpenAI, Claude, Gemini, Copilot, Kimi | Reasoning items mapped to `IrContentBlock::Thinking` in IR; lost on targets without thinking support | `abp-codex-sdk/src/lowering.rs` `to_ir()` |

### 3.2 Gemini Safety Ratings

| Source | Lost When Targeting | What Happens |
|--------|-------------------|--------------|
| `SafetySetting { category: HarmCategory, threshold: BlockThreshold }` | All other SDKs | Safety settings are Gemini-exclusive; stored in vendor config during `to_work_order()` but not translated to IR. Other SDKs have no equivalent. |
| `Candidate.safety_ratings` in response | All other SDKs | Safety rating metadata on response candidates has no IR or other-SDK equivalent; silently dropped. |

### 3.3 Copilot References

| Source | Lost When Targeting | What Happens |
|--------|-------------------|--------------|
| `CopilotReference` / `Reference` (File, Selection, Terminal, WebPage, GitDiff) | All other SDKs | References are preserved in `IrMessage.metadata` as opaque JSON via `lowering::extract_references()`. Other SDKs' `from_ir()` implementations ignore this metadata. |
| `CopilotConfirmation` (user-approval flow) | All other SDKs | No equivalent approval/confirmation concept exists; dropped during cross-SDK translation. |

### 3.4 Copilot Errors (Structured)

| Source | Lost When Targeting | What Happens |
|--------|-------------------|--------------|
| `CopilotError { error_type, message, code, identifier }` | OpenAI, Claude, Gemini, Codex, Kimi | Structured error objects are flattened to `format!("Error: {message}")` string or dropped entirely by other shims. |

### 3.5 Kimi Web Search

| Source | Lost When Targeting | What Happens |
|--------|-------------------|--------------|
| `SearchOptions { mode: SearchMode, result_count }` | All other SDKs | Web search control is Kimi-exclusive; stored in vendor config within `WorkOrder` but not representable in other SDKs. |
| `KimiChunk.refs` (search result references) | All other SDKs | Streaming search references have no equivalent; dropped during cross-SDK translation. |

### 3.6 System Message Location

| Source | Lost When Targeting | What Happens |
|--------|-------------------|--------------|
| OpenAI/Kimi system message in `messages[]` | Claude, Gemini, Codex | Extracted from array and placed in separate field (`system`, `system_instruction`, `instructions`). **Lossless** round-trip but structural change. |
| Claude `system` field | OpenAI, Kimi, Copilot | Injected as first message in `messages[]`. **Lossless** round-trip but structural change. |

### 3.7 Image Format Differences

| Source | Lost When Targeting | What Happens |
|--------|-------------------|--------------|
| OpenAI `ImageUrl { url, detail }` | Claude, Gemini | URL-referenced images; Claude/Gemini prefer base64 `InlineData`. The `detail` hint (low/high/auto) is OpenAI-specific and dropped. |
| Claude `ImageSource::Base64 { media_type, data }` | OpenAI | OpenAI uses URL references; base64→URL requires hosting. Currently mapped to `IrContentBlock::Image` with base64 data preserved in IR. |
| Codex, Copilot, Kimi | N/A | No image support; image content blocks are dropped during `from_ir()`. |

### 3.8 Claude Cache Token Metrics

| Source | Lost When Targeting | What Happens |
|--------|-------------------|--------------|
| `ClaudeUsage.cache_creation_input_tokens` / `cache_read_input_tokens` | OpenAI, Gemini, Codex, Copilot, Kimi | IR `IrUsage` preserves `cache_read_tokens` and `cache_write_tokens`; other SDKs' usage types only have input/output/total and ignore cache fields. |

### 3.9 Codex Response Item Structure

| Source | Lost When Targeting | What Happens |
|--------|-------------------|--------------|
| `CodexResponseItem` enum (Message/FunctionCall/FunctionCallOutput/Reasoning) | OpenAI, Claude, Gemini, Copilot, Kimi | Item-based structure differs from message/choice structure. `FunctionCall` items are mapped to `IrContentBlock::ToolUse`; `FunctionCallOutput` to `IrContentBlock::ToolResult`. The item envelope structure is lost. |

### 3.10 Configuration Parameters

| Parameter | Available In | Lost When Targeting |
|-----------|-------------|-------------------|
| `top_k` | Claude, Gemini | OpenAI, Codex, Copilot, Kimi (no `top_k` field) |
| `response_format` / `response_schema` | OpenAI, Gemini, Codex | Claude, Copilot, Kimi |
| `response_mime_type` | Gemini | All others |
| `intent` | Copilot | All others |
| `text_format` | Codex | All others |

---

## 4. IR (Intermediate Representation)

The IR layer (`crates/abp-ir/src/lib.rs`) is the hub through which all SDK
types pass. Every shim converts vendor types → IR → `WorkOrder` on the way in,
and `Receipt` → IR → vendor types on the way out.

### 4.1 IR Type Definitions

```rust
// crates/abp-ir/src/lib.rs

pub enum IrRole {
    System,       // System prompt / instructions
    User,         // Human / user turn
    Assistant,    // Model / assistant turn
    Tool,         // Tool result turn
}

pub enum IrContentBlock {
    Text { text: String },
    Image { media_type: String, data: String },           // base64
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: Vec<IrContentBlock>, is_error: bool },
    Thinking { text: String },
}

pub struct IrMessage {
    pub role: IrRole,
    pub content: Vec<IrContentBlock>,
    pub metadata: BTreeMap<String, serde_json::Value>,    // vendor-opaque
}

pub struct IrConversation {
    pub messages: Vec<IrMessage>,
}

pub struct IrToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,                    // JSON Schema
}

pub struct IrUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}
```

### 4.2 IrRole Mapping

| IR Role | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|---------|--------|--------|--------|-------|---------|------|
| `System` | `ChatMessage::System` | Extracted to `system` field | Extracted to `system_instruction` | `instructions` field | `role: "system"` | `ChatMessage::System` |
| `User` | `ChatMessage::User` | `role: "user"` | `role: "user"` | User input items | `role: "user"` | `ChatMessage::User` |
| `Assistant` | `ChatMessage::Assistant` | `role: "assistant"` | `role: "model"` | `Message { role: "assistant" }` | `role: "assistant"` | `ChatMessage::Assistant` |
| `Tool` | `ChatMessage::Tool` | `ContentBlock::ToolResult` | `Part::FunctionResponse` | `FunctionCallOutput` | `role: "tool"` | `ChatMessage::Tool` |

### 4.3 IrContentBlock Mapping

| IR Block | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|----------|--------|--------|--------|-------|---------|------|
| `Text { text }` | `MessageContent::Text` or `ContentPart::Text` | `ContentBlock::Text` | `Part::Text` | `CodexContentPart::OutputText` | `content` string | Message `content` |
| `Image { media_type, data }` | `ContentPart::ImageUrl` | `ContentBlock::Image(ImageSource::Base64)` | `Part::InlineData` | ❌ dropped | ❌ dropped | ❌ dropped |
| `ToolUse { id, name, input }` | `ToolCall { id, function }` | `ContentBlock::ToolUse` | `Part::FunctionCall` | `CodexResponseItem::FunctionCall` | `CopilotFunctionCall` | Tool call in `tool_calls` |
| `ToolResult { tool_use_id, content, is_error }` | `ChatMessage::Tool { tool_call_id }` | `ContentBlock::ToolResult` | `Part::FunctionResponse` | `CodexResponseItem::FunctionCallOutput` | Tool message | `ChatMessage::Tool` |
| `Thinking { text }` | ❌ dropped | `ContentBlock::Thinking` | ❌ dropped | `CodexResponseItem::Reasoning` | ❌ dropped | ❌ dropped |

### 4.4 IrUsage Field Mapping

| IR Field | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|----------|--------|--------|--------|-------|---------|------|
| `input_tokens` | `prompt_tokens` | `input_tokens` | `prompt_token_count` | `input_tokens` | `input_tokens` | `prompt_tokens` |
| `output_tokens` | `completion_tokens` | `output_tokens` | `candidates_token_count` | `output_tokens` | `output_tokens` | `completion_tokens` |
| `total_tokens` | `total_tokens` | (computed: in+out) | `total_token_count` | `total_tokens` | `total_tokens` | `total_tokens` |
| `cache_read_tokens` | — (0) | `cache_read_input_tokens` | — (0) | — (0) | — (0) | — (0) |
| `cache_write_tokens` | — (0) | `cache_creation_input_tokens` | — (0) | — (0) | — (0) | — (0) |

### 4.5 IrToolDefinition Mapping

All SDKs map their tool definitions to/from `IrToolDefinition` with the same
three fields:

| IR Field | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|----------|--------|--------|--------|-------|---------|------|
| `name` | `FunctionDef.name` | `ClaudeTool.name` | `FunctionDeclaration.name` | `function.name` | `CopilotFunctionDef.name` | `function.name` |
| `description` | `FunctionDef.description` | `ClaudeTool.description` | `FunctionDeclaration.description` | `function.description` | `CopilotFunctionDef.description` | `function.description` |
| `parameters` | `FunctionDef.parameters` | `ClaudeTool.input_schema` | `FunctionDeclaration.parameters` | `function.parameters` | `CopilotFunctionDef.parameters` | `function.parameters` |

### 4.6 Conversion Function Reference

Each shim crate exposes these conversion functions in `convert.rs`:

| Function | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|----------|--------|--------|--------|-------|---------|------|
| Request → IR | `request_to_ir()` | `request_to_ir()` | `request_to_ir()` | `request_to_ir()` | `request_to_ir()` | `request_to_ir()` |
| Request → WorkOrder | `to_work_order()` | `to_work_order()` | `ir_to_work_order()` | `request_to_work_order()` | `request_to_work_order()` | `request_to_work_order()` |
| Receipt → Response | `from_receipt()` | `from_receipt()` | `ir_to_response()` | `receipt_to_response()` | `receipt_to_response()` | `receipt_to_response()` |
| Event → Stream | `from_agent_event()` | `from_agent_event()` | `receipt_to_stream_events()` | `events_to_stream_events()` | `events_to_stream_events()` | `events_to_stream_chunks()` |
| Response → IR | `ir_to_messages()` | — | — | `response_to_ir()` | `response_to_ir()` | `response_to_ir()` |
| IR → Messages | `ir_to_messages()` | — | — | `ir_to_response_items()` | `ir_to_messages()` | `ir_to_messages()` |
| Usage → IR | `ir_usage_to_usage()` | `usage_from_raw()` | `usage_to_ir()` | `ir_usage_to_usage()` | `ir_usage_to_tuple()` | `ir_usage_to_usage()` |

Each SDK crate also has a `lowering` module with:

| Function | Description | Present In |
|----------|-------------|------------|
| `to_ir()` | SDK messages → `IrConversation` | All 6 SDK crates |
| `from_ir()` | `IrConversation` → SDK messages | All 6 SDK crates |
| `extract_system_prompt()` | Pull system from conversation | Claude |
| `extract_system_instruction()` | Pull system as `Content` | Gemini |
| `input_to_ir()` | Codex input items → IR | Codex |
| `extract_references()` | Collect `CopilotReference` from metadata | Copilot |
| `usage_to_ir()` | SDK usage → `IrUsage` | Kimi, Gemini |

### 4.7 End-to-End Data Flow

```
Vendor SDK Request
       │
       ▼
  shim types.rs        (vendor-specific Rust types)
       │
       ▼
  lowering::to_ir()    (SDK types → IrConversation)
       │
       ▼
  convert::to_work_order()  (IR → ABP WorkOrder)
       │
       ▼
  ┌─────────────┐
  │  ABP Core   │     WorkOrder → Backend → Receipt
  └─────────────┘
       │
       ▼
  convert::from_receipt()   (Receipt trace → vendor response)
       │                    OR
  convert::from_agent_event()  (individual event → stream chunk)
       │
       ▼
  Vendor SDK Response / Stream
```

---

## 5. Error Handling

### 5.1 ABP Error Taxonomy

The `abp-error-taxonomy` crate defines 31 `ErrorCode` variants across 12
categories. The full taxonomy:

| Category | Codes | Retryable |
|----------|-------|-----------|
| **Protocol** | `ProtocolInvalidEnvelope`, `ProtocolHandshakeFailed`, `ProtocolMissingRefId`, `ProtocolUnexpectedMessage`, `ProtocolVersionMismatch` | No |
| **Backend** | `BackendNotFound`, `BackendUnavailable`, `BackendTimeout`, `BackendRateLimited`, `BackendAuthFailed`, `BackendModelNotFound`, `BackendCrashed` | `Unavailable`, `Timeout`, `RateLimited`, `Crashed` are retryable |
| **Execution** | `ExecutionToolFailed`, `ExecutionWorkspaceError`, `ExecutionPermissionDenied` | No |
| **Mapping** | `MappingUnsupportedCapability`, `MappingDialectMismatch`, `MappingLossyConversion`, `MappingUnmappableTool` | No |
| **Contract** | `ContractVersionMismatch`, `ContractSchemaViolation`, `ContractInvalidReceipt` | No |
| **Capability** | `CapabilityUnsupported`, `CapabilityEmulationFailed` | No |
| **Policy** | `PolicyDenied`, `PolicyInvalid` | No |
| **Workspace** | `WorkspaceInitFailed`, `WorkspaceStagingFailed` | No |
| **IR** | `IrLoweringFailed`, `IrInvalid` | No |
| **Receipt** | `ReceiptHashMismatch`, `ReceiptChainBroken` | No |
| **Dialect** | `DialectUnknown`, `DialectMappingFailed` | No |
| **Config** | `ConfigInvalid` | No |
| **Internal** | `Internal` | No |

All codes serialize to `snake_case` (e.g., `backend_timeout`). Error metadata
is carried in `ErrorInfo { code, message, details: BTreeMap, retryable: bool }`.

### 5.2 AgentEventKind::Error in Shim Conversions

When the ABP pipeline produces an `AgentEventKind::Error { message, error_code }`,
each shim maps it differently. This table shows the **actual implemented behavior**
in each shim's `convert.rs`:

| Shim | Non-streaming (`*_to_response`) | Streaming (`*_to_stream_*`) | ErrorCode Used? |
|------|--------------------------------|----------------------------|-----------------|
| **OpenAI** | `content = "Error: {message}"`, `finish_reason = "stop"` | `StreamDelta { content: "Error: {message}" }` with `finish_reason: "stop"` | ❌ No |
| **Claude** | Not handled (error events skipped) | Not handled (returns `None`) | ❌ No |
| **Gemini** | Not handled (error events skipped) | Not handled (events skipped) | ❌ No |
| **Codex** | `CodexResponseItem::Message { content: "Error: {message}" }` | Not handled (events skipped) | ❌ No |
| **Copilot** | `CopilotError { error_type: "backend_error", message, code }` | `CopilotStreamEvent::CopilotErrors` (but `code: None` — error_code not propagated) | ⚠️ Partial (non-streaming only) |
| **Kimi** | `content = "Error: {message}"`, `finish_reason = "stop"` | Not handled (events skipped) | ❌ No |

**Key observations:**

1. **Copilot is the only shim that captures `error_code`** in non-streaming responses, mapping it to `CopilotError.code`. However, the streaming path loses the code.
2. **OpenAI and Kimi** surface errors as assistant text content with an `"Error: "` prefix.
3. **Claude and Gemini** silently drop error events — they fall through to `None` / are skipped in the event-matching logic.
4. **Codex** surfaces errors as response items in non-streaming mode but drops them in streaming.

### 5.3 Shim-Level Error Types

Each shim crate defines its own `ShimError` enum for request/response conversion failures:

| Shim | Error Type | Variants |
|------|-----------|----------|
| **OpenAI** | (errors in `convert.rs` return `String`) | N/A — uses string errors |
| **Claude** | (errors in `convert.rs` return `String`) | N/A — uses string errors |
| **Gemini** | `GeminiError` | `RequestConversion`, `ResponseConversion`, `BackendError`, `Serde` |
| **Codex** | `ShimError` | `InvalidRequest`, `Internal`, `Serde` |
| **Copilot** | `ShimError` | `InvalidRequest`, `Internal`, `Serde` |
| **Kimi** | `ShimError` | `InvalidRequest`, `Internal`, `Serde` |

### 5.4 Error Mapping Guidance

When mapping errors across SDKs:

| ABP ErrorCode | Suggested SDK Response |
|---------------|----------------------|
| `BackendRateLimited` | HTTP 429 or vendor-specific rate-limit error |
| `BackendTimeout` | HTTP 504 or timeout error |
| `BackendAuthFailed` | HTTP 401 / authentication error |
| `MappingLossyConversion` | Warning-level; proceed with degraded output |
| `MappingUnmappableTool` | Tool excluded from request; log warning |
| `CapabilityUnsupported` | Feature not available; fall back or error |
| `PolicyDenied` | Operation blocked by policy; surface to user |
| `IrLoweringFailed` | SDK → IR conversion failure; invalid request |

---

## Architecture

### Crate Dependency Hierarchy

```
abp-ir (intermediate representation)
  ↑
abp-openai-sdk / abp-claude-sdk / abp-gemini-sdk / abp-codex-sdk / abp-copilot-sdk / abp-kimi-sdk
  ↑                          (SDK types + lowering)
abp-shim-openai / abp-shim-claude / abp-shim-gemini / abp-shim-codex / abp-shim-copilot / abp-shim-kimi
  ↑                          (drop-in client replacements)
abp-core (WorkOrder, Receipt, AgentEvent)
  ↑
abp-protocol → abp-host → abp-integrations → abp-runtime → abp-cli
```

### Projection Matrix

The projection matrix in `abp-integrations` provides:
- **Dialect enum**: `Abp`, `Claude`, `Codex`, `Gemini`, `Kimi`
- **WorkOrder translation**: ABP → vendor request JSON
- **Tool name mapping**: Bidirectional between all dialect pairs
- **Event kind mapping**: Canonical event types ↔ vendor event labels

### Tool Name Translation

| ABP (canonical) | OpenAI (Codex) | Anthropic (Claude) | Gemini | Description |
|-----------------|----------------|-------------------|--------|-------------|
| `read_file` | `file_read` | `Read` | `readFile` | Read file contents |
| `write_file` | `file_write` | `Write` | `writeFile` | Write new file |
| `edit_file` | `apply_diff` | `Edit` | `editFile` | Edit existing file |
| `bash` | `shell` | `Bash` | `executeCommand` | Execute shell command |
| `glob` | `file_search` | `Glob` | `searchFiles` | Search files by pattern |

### Streaming Event Translation

| ABP (canonical) | OpenAI | Anthropic | Gemini |
|----------------|--------|-----------|--------|
| `run_started` | `response.created` | `message_start` | `generate_content_start` |
| `run_completed` | `response.completed` | `message_stop` | `generate_content_end` |
| `assistant_message` | `response.output_text.done` | `content_block_stop` | `text` |
| `assistant_delta` | `response.output_text.delta` | `content_block_delta` | `text_delta` |
| `tool_call` | `function_call` | `tool_use` | `function_call` |
| `tool_result` | `function_call_output` | `tool_result` | `function_response` |

---

## Related Documentation

- [SDK Surface Area](sdk_surface_area.md) — per-vendor implementation details
- [Sidecar Protocol](sidecar_protocol.md) — JSONL wire format specification
- [Dialect×Engine Matrix](dialect_engine_matrix.md) — passthrough vs mapped routing
- [Capabilities](capabilities.md) — capability model reference
- [Error Codes](error_codes.md) — stable error code taxonomy
- [Capability Negotiation](capability_negotiation.md) — manifest + requirements
