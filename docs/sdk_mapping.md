# SDK Mapping Matrix

> Complete mapping reference between the 6 supported SDK dialects and the ABP
> intermediate representation (IR). Based on actual crate source code in
> `crates/abp-shim-*/`, `crates/abp-*-sdk/`, `crates/*-bridge/`,
> `crates/abp-ir/`, `crates/abp-capability/`, and `crates/abp-dialect/`.

---

## Table of Contents

1. [SDK Surface Area Overview](#1-sdk-surface-area-overview)
2. [Feature Matrix (Native / Emulated / Unsupported)](#2-feature-matrix-native--emulated--unsupported)
3. [Type Mapping Tables](#3-type-mapping-tables)
4. [Streaming Semantics](#4-streaming-semantics)
5. [Tool Calling Normalization](#5-tool-calling-normalization)
6. [Error Mapping](#6-error-mapping)
7. [Passthrough vs Mapped Mode](#7-passthrough-vs-mapped-mode)
8. [Capability Negotiation](#8-capability-negotiation)
9. [Lossy Mappings](#9-lossy-mappings)
10. [Known Limitations](#10-known-limitations)
11. [Architecture](#11-architecture)
12. [Related Documentation](#12-related-documentation)

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

## 2. Feature Matrix (Native / Emulated / Unsupported)

Support levels from `abp-capability`'s `CapabilityRegistry` (`crates/abp-capability/src/registry.rs`).

- **N** = Native (first-class, no translation)
- **E** = Emulated (via ABP adapter layer)
- **—** = Unsupported (not available)

### 2.1 Core Capability Matrix

| Capability | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|------------|:------:|:------:|:------:|:-----:|:-------:|:----:|
| Streaming | N | N | N | N | N | N |
| ToolUse | N | N | N | N | N | N |
| FunctionCalling | N | E | N | N | N | N |
| Vision | N | N | N | E | E | N |
| Audio | N | — | N | — | — | — |
| ImageInput | — | — | — | E | E | N |
| PdfInput | — | N | N | — | — | — |
| ExtendedThinking | — | N | — | — | — | — |
| CodeExecution | E | E | N | N | E | — |
| Embeddings | E | E | E | E | — | E |
| ImageGeneration | — | — | E | — | — | — |
| StructuredOutputJsonSchema | N | E | N | N | E | E |
| JsonMode | N | E | N | N | E | N |
| SystemMessage | N | N | N | N | N | N |
| Temperature | N | N | N | N | N | N |
| TopP | N | N | N | — | N | N |
| TopK | — | N | N | — | — | — |
| MaxTokens | N | N | N | N | N | N |
| StopSequences | N | N | N | — | N | N |
| Logprobs | N | — | — | N | — | — |
| SeedDeterminism | N | — | — | N | — | — |
| FrequencyPenalty | N | — | — | N | — | N |
| PresencePenalty | N | — | — | N | — | N |
| CacheControl | — | N | N | — | — | — |
| BatchMode | N | N | — | N | — | — |

### 2.2 Agent-Specific Capabilities

| Capability | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|------------|:------:|:------:|:------:|:-----:|:-------:|:----:|
| ToolRead | — | — | — | N | N | — |
| ToolWrite | — | — | — | N | N | — |
| ToolEdit | — | — | — | N | N | — |
| ToolBash | — | — | — | N | N | — |
| ToolGlob | — | — | — | N | N | — |
| ToolGrep | — | — | — | N | N | — |
| ToolWebSearch | — | — | — | — | N | — |
| ToolWebFetch | — | — | — | — | N | — |
| ToolAskUser | — | — | — | — | N | — |

### 2.3 Vendor-Exclusive Features

Features unique to a single SDK with no IR equivalent:

| Feature | SDK | Type | IR Handling |
|---------|-----|------|-------------|
| Safety settings | Gemini | `SafetySetting { category, threshold }` | Stored in `WorkOrder` vendor config; no IR mapping |
| Web search | Kimi | `SearchOptions { mode, result_count }` | Stored in `WorkOrder` vendor config; no IR mapping |
| References | Copilot | `Reference { type, id, uri, content }` | Preserved in `IrMessage.metadata` as opaque JSON |
| Confirmations | Copilot | `CopilotConfirmation` | Dropped during cross-SDK translation |
| Cache tokens | Claude | `cache_creation`/`cache_read` on usage | `IrUsage.cache_read_tokens` / `cache_write_tokens` |
| Search refs | Kimi | `KimiChunk.refs` | Dropped during cross-SDK translation |

### 2.4 System Message Handling

| SDK | Location | Extraction |
|-----|----------|------------|
| **OpenAI** | `ChatMessage::System` inside `messages[]` | `lowering::to_ir()` maps to `IrRole::System` |
| **Claude** | `MessagesRequest.system` (separate field) | `lowering::extract_system_prompt()` pulls from field |
| **Gemini** | `GenerateContentRequest.system_instruction` (separate `Content`) | `lowering::extract_system_instruction()` returns `Content` |
| **Codex** | `CodexRequest.instructions` (separate string) | `lowering::input_to_ir()` treats as system |
| **Copilot** | `CopilotChatMessage { role: "system" }` inside `messages[]` | `lowering::to_ir()` maps to `IrRole::System` |
| **Kimi** | `ChatMessage::System` inside `messages[]` | `lowering::to_ir()` maps to `IrRole::System` |

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

## 3. Type Mapping Tables

The IR layer is the hub through which all SDK types pass. Every shim converts
vendor types → IR → `WorkOrder` on the way in, and `Receipt` → IR → vendor
types on the way out.

### 3.1 IR Type Definitions

Source: `crates/abp-core/src/ir.rs` (core IR) and `crates/abp-dialect/src/ir.rs` (extended IR).

```rust
// --- Core IR (abp-core) ---

pub enum IrRole { System, User, Assistant, Tool }

pub enum IrContentBlock {
    Text { text: String },
    Image { media_type: String, data: String },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: Vec<IrContentBlock>, is_error: bool },
    Thinking { text: String },
}

pub struct IrMessage {
    pub role: IrRole,
    pub content: Vec<IrContentBlock>,
    pub metadata: BTreeMap<String, serde_json::Value>,
}

pub struct IrConversation { pub messages: Vec<IrMessage> }
pub struct IrToolDefinition { pub name: String, pub description: String, pub parameters: Value }
pub struct IrUsage {
    pub input_tokens: u64, pub output_tokens: u64, pub total_tokens: u64,
    pub cache_read_tokens: u64, pub cache_write_tokens: u64,
}

// --- Extended IR (abp-dialect) ---

pub enum IrContentBlock {
    Text { text }, Image { media_type, data },
    ToolCall { id, name, input },           // note: "ToolCall" vs core "ToolUse"
    ToolResult { tool_call_id, content, is_error },
    Thinking { text },
    Audio { media_type, data },             // extended: audio support
    Custom { custom_type, data: Value },    // extended: vendor-specific
}

// --- SDK-Types IR (abp-sdk-types) ---

pub struct IrChatRequest {
    pub model: String, pub messages: Vec<IrMessage>, pub max_tokens: Option<u64>,
    pub tools: Vec<IrToolDefinition>, pub tool_choice: Option<Value>,
    pub sampling: IrSamplingParams, pub stop_sequences: Vec<String>,
    pub stream: IrStreamConfig, pub response_format: Option<Value>,
    pub extra: BTreeMap<String, Value>,
}

pub struct IrChatResponse {
    pub id: String, pub model: String, pub choices: Vec<IrChoice>,
    pub usage: IrUsage, pub metadata: BTreeMap<String, Value>,
}

pub struct IrSamplingParams {
    pub temperature: Option<f64>, pub top_p: Option<f64>, pub top_k: Option<u32>,
    pub frequency_penalty: Option<f64>, pub presence_penalty: Option<f64>,
}

pub enum IrFinishReason { Stop, Length, ToolUse, ContentFilter, Error }
```

### 3.2 IrRole Mapping

| IR Role | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|---------|--------|--------|--------|-------|---------|------|
| `System` | `ChatMessage::System` | Extracted to `system` field | Extracted to `system_instruction` | `instructions` field | `role: "system"` | `ChatMessage::System` |
| `User` | `ChatMessage::User` | `role: "user"` | `role: "user"` | User input items | `role: "user"` | `ChatMessage::User` |
| `Assistant` | `ChatMessage::Assistant` | `role: "assistant"` | `role: "model"` | `Message { role: "assistant" }` | `role: "assistant"` | `ChatMessage::Assistant` |
| `Tool` | `ChatMessage::Tool` | `ContentBlock::ToolResult` | `Part::FunctionResponse` | `FunctionCallOutput` | `role: "tool"` | `ChatMessage::Tool` |

### 3.3 IrContentBlock Mapping

| IR Block | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|----------|--------|--------|--------|-------|---------|------|
| `Text` | `MessageContent::Text` / `ContentPart::Text` | `ContentBlock::Text` | `Part::Text` | `CodexContentPart::OutputText` | `content` string | Message `content` |
| `Image` | `ContentPart::ImageUrl` | `ContentBlock::Image(Base64)` | `Part::InlineData` | ❌ dropped | ❌ dropped | ❌ dropped |
| `ToolUse` | `ToolCall { id, function }` | `ContentBlock::ToolUse` | `Part::FunctionCall` | `CodexResponseItem::FunctionCall` | `CopilotFunctionCall` | `tool_calls[]` |
| `ToolResult` | `ChatMessage::Tool` | `ContentBlock::ToolResult` | `Part::FunctionResponse` | `FunctionCallOutput` | Tool message | `ChatMessage::Tool` |
| `Thinking` | ❌ dropped | `ContentBlock::Thinking` | ❌ dropped | `Reasoning` item | ❌ dropped | ❌ dropped |

### 3.4 IrToolDefinition Mapping

All SDKs normalize tool definitions to the same three fields:

| IR Field | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|----------|--------|--------|--------|-------|---------|------|
| `name` | `FunctionDef.name` | `ClaudeTool.name` | `FunctionDeclaration.name` | `function.name` | `CopilotFunctionDef.name` | `function.name` |
| `description` | `FunctionDef.description` | `ClaudeTool.description` | `FunctionDeclaration.description` | `function.description` | `CopilotFunctionDef.description` | `function.description` |
| `parameters` | `FunctionDef.parameters` | `ClaudeTool.input_schema` | `FunctionDeclaration.parameters` | `function.parameters` | `CopilotFunctionDef.parameters` | `function.parameters` |

### 3.5 IrUsage Field Mapping

| IR Field | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|----------|--------|--------|--------|-------|---------|------|
| `input_tokens` | `prompt_tokens` | `input_tokens` | `prompt_token_count` | `input_tokens` | `input_tokens` | `prompt_tokens` |
| `output_tokens` | `completion_tokens` | `output_tokens` | `candidates_token_count` | `output_tokens` | `output_tokens` | `completion_tokens` |
| `total_tokens` | `total_tokens` | (computed) | `total_token_count` | `total_tokens` | `total_tokens` | `total_tokens` |
| `cache_read_tokens` | — (0) | `cache_read_input_tokens` | — (0) | — (0) | — (0) | — (0) |
| `cache_write_tokens` | — (0) | `cache_creation_input_tokens` | — (0) | — (0) | — (0) | — (0) |

### 3.6 Conversion Function Reference

Each shim crate exposes these conversion functions in `convert.rs`:

| Function | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|----------|--------|--------|--------|-------|---------|------|
| Request → IR | `request_to_ir()` | `request_to_ir()` | `request_to_ir()` | `request_to_ir()` | `request_to_ir()` | `request_to_ir()` |
| Request → WorkOrder | `to_work_order()` | `to_work_order()` | `ir_to_work_order()` | `request_to_work_order()` | `request_to_work_order()` | `request_to_work_order()` |
| Receipt → Response | `from_receipt()` | `from_receipt()` | `ir_to_response()` | `receipt_to_response()` | `receipt_to_response()` | `receipt_to_response()` |
| Event → Stream | `from_agent_event()` | `from_agent_event()` | `receipt_to_stream_events()` | `events_to_stream_events()` | `events_to_stream_events()` | `events_to_stream_chunks()` |
| Usage mapping | `ir_usage_to_usage()` | `usage_from_raw()` | `usage_to_ir()` | `ir_usage_to_usage()` | `ir_usage_to_tuple()` | `ir_usage_to_usage()` |

Each SDK crate also has a `lowering` module:

| Function | Description | Present In |
|----------|-------------|------------|
| `to_ir()` | SDK messages → `IrConversation` | All 6 SDK crates |
| `from_ir()` | `IrConversation` → SDK messages | All 6 SDK crates |
| `extract_system_prompt()` | Pull system from conversation | Claude |
| `extract_system_instruction()` | Pull system as `Content` | Gemini |
| `input_to_ir()` | Codex input items → IR | Codex |
| `extract_references()` | Collect `CopilotReference` from metadata | Copilot |
| `usage_to_ir()` | SDK usage → `IrUsage` | Kimi, Gemini |

---

## 4. Streaming Semantics

### 4.1 Streaming Architecture by SDK

| SDK | Type Shape | Protocol | Key Types |
|-----|-----------|----------|-----------|
| **OpenAI** | Struct-based | SSE `data: {json}\n\n` | `StreamChunk { id, choices: [StreamChoice { delta: StreamDelta }] }` |
| **Claude** | Enum-based | SSE `event: {type}\ndata: {json}\n\n` | `StreamEvent` enum: `MessageStart`, `ContentBlockStart`, `ContentBlockDelta`, `ContentBlockStop`, `MessageDelta`, `MessageStop`, `Ping` |
| **Gemini** | Struct-based | SSE | `StreamEvent { candidates, usage_metadata }` (same shape as non-stream response) |
| **Codex** | Enum-based | SSE | `CodexStreamEvent` enum: `ResponseCreated`, `ResponseInProgress`, `OutputItemAdded`, `OutputItemDelta`, `OutputItemDone`, `ResponseCompleted`, `ResponseFailed`, `Error` |
| **Copilot** | Enum-based | SSE | `CopilotStreamEvent` enum: `CopilotReferences`, `TextDelta`, `FunctionCall`, `CopilotErrors`, `CopilotConfirmation`, `Done` |
| **Kimi** | Struct-based | SSE `data: {json}\n\n` | `KimiChunk { id, choices, usage, refs }` (OpenAI-compatible shape) |

### 4.2 IrStreamEvent Definition

Source: `crates/abp-dialect/src/ir.rs`.

```rust
pub enum IrStreamEvent {
    StreamStart { id: Option<String>, model: Option<String> },
    ContentBlockStart { index: usize, block: IrContentBlock },
    TextDelta { index: usize, text: String },
    ToolCallDelta { index: usize, arguments_delta: String },
    ThinkingDelta { index: usize, text: String },
    ContentBlockStop { index: usize },
    Usage { usage: IrUsage },
    StreamEnd { stop_reason: Option<IrStopReason> },
}
```

### 4.3 SDK → IrStreamEvent Mapping

| IrStreamEvent | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|---------------|--------|--------|--------|-------|---------|------|
| `StreamStart` | First `StreamChunk` (extract `id`, `model`) | `MessageStart { message }` | First `StreamEvent` | `ResponseCreated` | First chunk | First `KimiChunk` (extract `id`, `model`) |
| `ContentBlockStart` | First delta with content | `ContentBlockStart { index, content_block }` | First `Part` in candidate | `OutputItemAdded { item }` | — (implicit) | First delta with content |
| `TextDelta` | `StreamDelta { content }` | `ContentBlockDelta { delta: TextDelta }` | `Part::Text` in candidate | `OutputItemDelta { delta }` (text) | `TextDelta { text }` | `StreamDelta { content }` |
| `ToolCallDelta` | `StreamDelta { tool_calls[].function.arguments }` | `ContentBlockDelta { delta: InputJsonDelta }` | `Part::FunctionCall` (complete) | `OutputItemDelta { delta }` (function_call) | `FunctionCall { function_call }` | `StreamDelta { tool_calls[].function.arguments }` |
| `ThinkingDelta` | ❌ N/A | `ContentBlockDelta { delta: ThinkingDelta }` | ❌ N/A | `OutputItemDelta` (reasoning) | ❌ N/A | ❌ N/A |
| `ContentBlockStop` | — (implicit on next choice) | `ContentBlockStop { index }` | — (implicit) | `OutputItemDone { item }` | — (implicit) | — (implicit) |
| `Usage` | Final chunk with `usage` field | `MessageDelta { usage }` | `usage_metadata` on any event | `ResponseCompleted { response.usage }` | Final chunk `usage` | Final chunk `usage` |
| `StreamEnd` | `choices[].finish_reason` set | `MessageStop` | Last candidate `finish_reason` | `ResponseCompleted` | `Done` | `choices[].finish_reason` set |

### 4.4 Key Streaming Differences

| Aspect | Struct-based (OpenAI/Gemini/Kimi) | Enum-based (Claude/Codex/Copilot) |
|--------|----------------------------------|-----------------------------------|
| **Event identity** | Inferred from delta field presence | Explicit via enum variant / SSE `event:` header |
| **Content block lifecycle** | Implicit (no start/stop signals) | Explicit `ContentBlockStart`/`Stop` (Claude) or `OutputItemAdded`/`Done` (Codex) |
| **Tool call assembly** | Progressive deltas on `tool_calls[index].function.arguments`; requires `ParallelToolCallAssembler` | Dedicated delta variants; assembly via block index |
| **Usage delivery** | On final chunk as `usage` field | Separate event (`MessageDelta` in Claude, `ResponseCompleted` in Codex) |
| **Stream termination** | `finish_reason` on choice or `data: [DONE]` sentinel | `MessageStop` (Claude), `ResponseCompleted`/`ResponseFailed` (Codex), `Done` (Copilot) |

### 4.5 Canonical ABP Streaming Events

| ABP (canonical) | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|----------------|--------|--------|--------|-------|---------|------|
| `run_started` | `response.created` | `message_start` | `generate_content_start` | `ResponseCreated` | First chunk | First chunk |
| `run_completed` | `response.completed` | `message_stop` | `generate_content_end` | `ResponseCompleted` | `Done` | Final chunk |
| `assistant_delta` | `response.output_text.delta` | `content_block_delta` | `text_delta` | `OutputItemDelta` | `TextDelta` | Delta in choice |
| `tool_call` | `function_call` | `tool_use` | `function_call` | `FunctionCall` item | `FunctionCall` | `function_call` |
| `tool_result` | `function_call_output` | `tool_result` | `function_response` | `FunctionCallOutput` | Tool message | `tool_result` |

### 4.6 Stream Processing Infrastructure

Source: `crates/abp-stream/`.

| Component | Purpose |
|-----------|---------|
| `StreamAggregator` | Collects `AgentEvent`s → assembles final text, tool_calls, thinking, errors |
| `EventFilter` | Predicate-based filtering: `by_kind()`, `errors_only()`, `exclude_errors()` |
| `EventTransform` | In-flight event mutation: `map_text()`, `add_metadata()`, `chain()` |
| `StreamDemux` | Split one stream by criteria into multiple consumers |
| `FanOut` | Broadcast events to multiple receivers |
| `MergedStream` | Combine multiple event streams |
| `StreamMultiplexer` | Tagged stream multiplexing |
| `ReplayBuffer` | Record & replay event sequences |
| `BackpressuredSender` | Flow-controlled sending with backpressure policy |

---

## 5. Tool Calling Normalization

### 5.1 Tool Definition Formats

| SDK | Wrapper Type | Schema Field | Example Wire Format |
|-----|-------------|-------------|---------------------|
| **OpenAI** | `Tool { tool_type, function: FunctionDef }` | `parameters` | `{ "type": "function", "function": { "name": "...", "description": "...", "parameters": {...} } }` |
| **Claude** | `ClaudeTool { name, description, input_schema }` | `input_schema` | `{ "name": "...", "description": "...", "input_schema": {...} }` |
| **Gemini** | `ToolDeclaration { function_declarations[] }` | `parameters` | `{ "function_declarations": [{ "name": "...", "description": "...", "parameters": {...} }] }` |
| **Codex** | `CodexTool` enum: `Function`, `CodeInterpreter`, `FileSearch` | `function.parameters` | Same as OpenAI for `Function`; built-in types for others |
| **Copilot** | `CopilotTool { type, function: CopilotFunctionDef }` | `parameters` | Same as OpenAI |
| **Kimi** | `ToolDefinition` enum: `Function`, `BuiltinFunction` | `function.parameters` | Same as OpenAI for `Function`; `$web_search`, `$file_tool`, `$code_tool`, `$browser` for built-ins |

### 5.2 Tool Invocation (Request → Result)

| Phase | OpenAI | Claude | Gemini | Codex | Copilot | Kimi |
|-------|--------|--------|--------|-------|---------|------|
| **Tool call in response** | `ToolCall { id, type, function: { name, arguments } }` on assistant message | `ContentBlock::ToolUse { id, name, input }` | `Part::FunctionCall { name, args }` | `CodexResponseItem::FunctionCall { name, call_id, arguments }` | `CopilotToolCall { id, type, function }` | `tool_calls[].function { name, arguments }` on assistant message |
| **Tool result submission** | `ChatMessage::Tool { tool_call_id, content }` | `ContentBlock::ToolResult { tool_use_id, content, is_error }` | `Part::FunctionResponse { name, response }` | `CodexResponseItem::FunctionCallOutput { call_id, output }` | Message with `role: "tool"` + `tool_call_id` | `ChatMessage::Tool { tool_call_id, content }` |
| **Error reporting** | `content` field text | `is_error: true` on `ToolResult` | Error in `response` object | `output` field text | `content` field text | `content` field text |

### 5.3 Tool Choice Control

| SDK | Mode | Wire Format |
|-----|------|-------------|
| **OpenAI** | none / auto / required / named | `ToolChoice::Mode(None\|Auto\|Required)` or `ToolChoice::Function { name }` |
| **Claude** | auto / any / named | `ClaudeToolChoice::Auto\|Any\|Tool { name }` |
| **Gemini** | auto / any / none | `FunctionCallingConfig { mode: Auto\|Any\|None }` in `ToolConfig` |
| **Codex** | Same as OpenAI | OpenAI-compatible `tool_choice` |
| **Copilot** | Same as OpenAI | OpenAI-compatible `tool_choice` |
| **Kimi** | Same as OpenAI | OpenAI-compatible `tool_choice` |

### 5.4 Tool Choice IR Normalization

| IR `tool_choice` | OpenAI → | Claude → | Gemini → |
|------------------|----------|----------|----------|
| `null` | Default (auto) | Default (auto) | Default (auto) |
| `"none"` | `Mode::None` | ❌ no equivalent (omit tools) | `FunctionCallingMode::None` |
| `"auto"` | `Mode::Auto` | `Auto` | `FunctionCallingMode::Auto` |
| `"required"` | `Mode::Required` | `Any` | `FunctionCallingMode::Any` |
| `{"name": "X"}` | `Function { name: "X" }` | `Tool { name: "X" }` | ❌ no per-function forcing |

### 5.5 Built-in / Special Tool Handling

| SDK | Built-in Tools | IR Mapping |
|-----|---------------|------------|
| **Codex** | `CodeInterpreter`, `FileSearch` | Mapped to `IrToolDefinition` with synthesized schema |
| **Copilot** | File/selection/terminal references (not tools per se) | References stored in `IrMessage.metadata` |
| **Kimi** | `$web_search`, `$file_tool`, `$code_tool`, `$browser` | Mapped via `IrContentBlock::Custom { custom_type: "kimi_builtin_tool" }` |

### 5.6 Tool Name Translation (Canonical)

| ABP (canonical) | OpenAI (Codex) | Anthropic (Claude) | Gemini | Description |
|-----------------|----------------|-------------------|--------|-------------|
| `read_file` | `file_read` | `Read` | `readFile` | Read file contents |
| `write_file` | `file_write` | `Write` | `writeFile` | Write new file |
| `edit_file` | `apply_diff` | `Edit` | `editFile` | Edit existing file |
| `bash` | `shell` | `Bash` | `executeCommand` | Execute shell command |
| `glob` | `file_search` | `Glob` | `searchFiles` | Search files by pattern |

---

## 6. Error Mapping

### 6.1 ABP Error Taxonomy

Source: `crates/abp-error/src/lib.rs` — 35 `ErrorCode` variants across 13 categories.

| Category | Error Codes | Retryable |
|----------|-------------|-----------|
| **Protocol** | `protocol_invalid_envelope`, `protocol_handshake_failed`, `protocol_missing_ref_id`, `protocol_unexpected_message`, `protocol_version_mismatch` | No |
| **Backend** | `backend_not_found`, `backend_unavailable`, `backend_timeout`, `backend_rate_limited`, `backend_auth_failed`, `backend_model_not_found`, `backend_crashed` | `unavailable`/`timeout`/`rate_limited`/`crashed` |
| **Mapping** | `mapping_unsupported_capability`, `mapping_dialect_mismatch`, `mapping_lossy_conversion`, `mapping_unmappable_tool` | No |
| **Execution** | `execution_tool_failed`, `execution_workspace_error`, `execution_permission_denied` | No |
| **Contract** | `contract_version_mismatch`, `contract_schema_violation`, `contract_invalid_receipt` | No |
| **Capability** | `capability_unsupported`, `capability_emulation_failed` | No |
| **Policy** | `policy_denied`, `policy_invalid` | No |
| **Workspace** | `workspace_init_failed`, `workspace_staging_failed` | No |
| **IR** | `ir_lowering_failed`, `ir_invalid` | No |
| **Receipt** | `receipt_hash_mismatch`, `receipt_chain_broken` | No |
| **Dialect** | `dialect_unknown`, `dialect_mapping_failed` | No |
| **Config** | `config_invalid` | No |
| **Internal** | `internal` | No |

### 6.2 Vendor Error → ABP Error Code

Source: `crates/abp-error/src/vendor_map.rs`.

**OpenAI:**

| Vendor Signal | ABP ErrorCode |
|---------------|---------------|
| HTTP 401 | `backend_auth_failed` |
| HTTP 429 | `backend_rate_limited` |
| HTTP 404 | `backend_model_not_found` |
| `"insufficient_quota"` | `backend_rate_limited` |
| `"invalid_request_error"` | `contract_schema_violation` |

**Anthropic (Claude):**

| Vendor Signal | ABP ErrorCode |
|---------------|---------------|
| HTTP 401 / `"authentication_error"` | `backend_auth_failed` |
| HTTP 429 / `"rate_limit_error"` | `backend_rate_limited` |
| `"not_found_error"` | `backend_model_not_found` |
| `"overloaded_error"` | `backend_unavailable` |
| `"invalid_request_error"` | `contract_schema_violation` |

**Gemini:**

| Vendor Signal | ABP ErrorCode |
|---------------|---------------|
| HTTP 401 / `"UNAUTHENTICATED"` | `backend_auth_failed` |
| HTTP 429 / `"RESOURCE_EXHAUSTED"` | `backend_rate_limited` |
| HTTP 404 / `"NOT_FOUND"` | `backend_model_not_found` |
| `"PERMISSION_DENIED"` | `policy_denied` |
| `"INVALID_ARGUMENT"` | `contract_schema_violation` |

**HTTP status fallback** (all vendors):

| HTTP Status | ABP ErrorCode |
|-------------|---------------|
| 401 | `backend_auth_failed` |
| 403 | `policy_denied` |
| 404 | `backend_model_not_found` |
| 429 | `backend_rate_limited` |
| 408 / 504 | `backend_timeout` |
| 500–599 | `backend_unavailable` |

### 6.3 Error Recovery Classification

Source: `crates/abp-error/src/category.rs`.

| RecoveryCategory | Error Codes | Retryable | Delay |
|------------------|-------------|:---------:|-------|
| **Authentication** | `backend_auth_failed` | No | — |
| **RateLimit** | `backend_rate_limited` | Yes | 30s |
| **ModelCapability** | `backend_model_not_found`, `capability_unsupported`, `capability_emulation_failed` | No | — |
| **InputValidation** | `contract_schema_violation`, `contract_version_mismatch`, `config_invalid`, `ir_invalid` | No | — |
| **NetworkTransient** | `backend_timeout`, `backend_unavailable`, `backend_crashed` | Yes | 2s |
| **ServerInternal** | `backend_not_found`, `internal` | Yes | 5s |
| **ProtocolViolation** | All `protocol_*` codes | No | — |
| **MappingFailure** | All `mapping_*`, `dialect_*`, `ir_lowering_failed`, `receipt_*`, `contract_invalid_receipt` | No | — |
| **PolicyViolation** | `policy_denied`, `policy_invalid`, `execution_permission_denied` | No | — |
| **ResourceExhausted** | `workspace_*`, `execution_tool_failed`, `execution_workspace_error` | Yes | 10s |

### 6.4 Error Surfacing in Shim Conversions

When ABP produces `AgentEventKind::Error { message, error_code }`:

| Shim | Non-streaming | Streaming | ErrorCode preserved? |
|------|--------------|-----------|:--------------------:|
| **OpenAI** | `content = "Error: {message}"`, `finish_reason = "stop"` | `StreamDelta { content: "Error: {message}" }` | ❌ |
| **Claude** | Skipped | Skipped (`None`) | ❌ |
| **Gemini** | Skipped | Skipped | ❌ |
| **Codex** | `Message { content: "Error: {message}" }` | Skipped | ❌ |
| **Copilot** | `CopilotError { error_type, message, code }` | `CopilotErrors` (code: `None`) | ⚠️ Partial |
| **Kimi** | `content = "Error: {message}"`, `finish_reason = "stop"` | Skipped | ❌ |

### 6.5 Error Severity Classification

Source: `crates/abp-error-taxonomy`.

| Severity | Meaning | Examples |
|----------|---------|---------|
| **Fatal** | Unrecoverable; stop immediately | `backend_auth_failed`, `policy_denied`, `contract_version_mismatch` |
| **Retriable** | Transient; retry with backoff | `backend_timeout`, `backend_rate_limited`, `backend_unavailable` |
| **Degraded** | Proceed with reduced fidelity | `mapping_lossy_conversion`, `capability_emulation_failed` |
| **Informational** | Log only; no action required | `mapping_lossy_conversion` (warning-level) |

---

## 7. Passthrough vs Mapped Mode

Source: `crates/abp-projection/` and `docs/dialect_engine_matrix.md`.

### 7.1 The Routing Decision

Every request is classified by the intersection of **source dialect** and **target backend**:

| Source → Target | Same dialect backend | Different dialect backend |
|-----------------|:--------------------:|:------------------------:|
| **Mode** | **Passthrough** | **Mapped** |
| **Request rewriting** | None | Full IR translation |
| **Fidelity** | Lossless | Explicitly lossy |
| **Validation** | Schema-only | Early capability + mapping checks |

### 7.2 Passthrough Mode

When source dialect matches target engine (e.g., Claude → Claude backend):

- **No request rewriting** — vendor JSON is forwarded as-is
- **Bitwise-equivalent** stream after removing ABP framing
- **ABP role**: Observer/recorder only (receipts, telemetry, policy checks)
- **What's preserved**: Everything — all vendor-specific features, extensions, formatting
- **Set via**: `work_order.config.vendor.abp.mode = "passthrough"`

```
Claude Request → [ABP: observe + record] → Claude Backend
                                          ↓
Claude Response ← [ABP: receipt + hash] ← Claude Response
```

### 7.3 Mapped Mode (Default)

When source dialect differs from target engine (e.g., Claude → Gemini backend):

- **Full dialect translation** through IR pipeline
- **Early validation**: Capability checks before execution
- **Explicit lossy**: Known information loss is documented and labeled
- **What's preserved**: Core semantics (messages, tools, usage)
- **What's lost**: Vendor-specific extensions (see §9 Lossy Mappings)
- **Set via**: `work_order.config.vendor.abp.mode = "mapped"` (or omit — this is default)

```
Claude Request → [lower to IR] → [capability check] → [emulate missing]
                                                      ↓
                              [raise from IR] → Gemini Request → Backend
                                                               ↓
Claude Response ← [raise to Claude IR] ← [lower from IR] ← Gemini Response
```

### 7.4 Projection Matrix Routing

Source: `crates/abp-projection/src/lib.rs`.

The `ProjectionMatrix` selects the optimal backend using a composite score:

```rust
pub struct ProjectionScore {
    pub capability_coverage: f64,  // fraction of required capabilities satisfied
    pub mapping_fidelity: f64,     // fraction of features that map losslessly
    pub priority: f64,             // normalized backend priority
    pub total: f64,                // weighted final score
}

pub struct ProjectionConfig {
    pub capability_weight: f64,    // default: 0.4
    pub fidelity_weight: f64,     // default: 0.4
    pub priority_weight: f64,     // default: 0.2
    pub passthrough_bonus: f64,   // bonus for native dialect match
}
```

**Selection strategies** (`SelectionStrategy` enum):
- `HighestFidelity` — maximize mapping quality
- `LowestLatency` — minimize response time
- `LowestCost` — minimize token cost
- `RoundRobin` — distribute across backends
- `WeightedRandom` — probabilistic selection
- `FallbackChain` — ordered backend list with failover

### 7.5 Mapping Fidelity Labels

Source: `crates/abp-mapping/src/lib.rs`.

| Fidelity Level | Meaning | Example |
|----------------|---------|---------|
| `Lossless` | Perfect round-trip; no information lost | Text messages, tool definitions |
| `LossyLabeled { warning }` | Mapped but with documented loss | Extended thinking → dropped; system message relocation |
| `Unsupported { reason }` | Cannot be represented in target | Safety settings in non-Gemini targets |

### 7.6 What's Preserved and Lost by Mode

| Aspect | Passthrough | Mapped |
|--------|:-----------:|:------:|
| Text content | ✅ | ✅ |
| Message ordering | ✅ | ✅ |
| Tool definitions (name/desc/schema) | ✅ | ✅ |
| Tool call/result pairs | ✅ | ✅ |
| Usage/token counts | ✅ | ✅ (field-name translated) |
| System message position | ✅ (native format) | ⚠️ Relocated (lossless semantically) |
| Extended thinking | ✅ | ⚠️ Lost if target lacks support |
| Safety settings (Gemini) | ✅ | ❌ Lost in non-Gemini targets |
| References (Copilot) | ✅ | ⚠️ Preserved in metadata, ignored by other SDKs |
| Web search options (Kimi) | ✅ | ❌ Lost in non-Kimi targets |
| Cache tokens (Claude) | ✅ | ⚠️ Preserved in IR; ignored by other SDKs |
| `top_k` | ✅ | ❌ Lost in OpenAI/Codex/Copilot/Kimi |
| Vendor-specific streaming shape | ✅ | ❌ Normalized to target format |

---

## 8. Capability Negotiation

Source: `crates/abp-capability/`, `crates/abp-emulation/`, `crates/abp-core/`.

### 8.1 Overview

Every backend advertises a **capability manifest** — `BTreeMap<Capability, SupportLevel>`.
Work orders carry **capability requirements** specifying the minimum level needed.
The negotiation system compares these and produces a structured result.

### 8.2 Support Levels

| Level | Wire Value | Meaning |
|-------|------------|---------|
| **Native** | `"native"` | First-class support; no translation needed |
| **Emulated** | `"emulated"` | Provided via ABP adapter with acceptable fidelity |
| **Restricted** | `{"restricted": {"reason": "..."}}` | Supported but limited by policy/environment |
| **Unsupported** | `"unsupported"` | Cannot be provided at all |

### 8.3 Negotiation Flow

```
Work Order Requirements    Backend Manifest
         │                        │
         └────────┬───────────────┘
                  ▼
         negotiate(requirements, manifest)
                  │
                  ▼
         NegotiationResult {
           native: Vec<Capability>,
           emulated: Vec<(Capability, EmulationStrategy)>,
           unsupported: Vec<(Capability, String)>,
         }
                  │
                  ▼
         CompatibilityReport {
           compatible: bool,
           native_count, emulated_count, unsupported_count,
           summary: String,
           details: Vec<(String, SupportLevel)>,
         }
```

### 8.4 Emulation Strategies

When a capability is not natively supported, ABP can emulate it:

| Strategy | Latency | Quality | Fidelity | Applied To |
|----------|:-------:|:-------:|:--------:|------------|
| **ClientSide** | ~50ms | 90% | 0.9 | `StructuredOutputJsonSchema`, `JsonMode`, `PdfInput`, `CodeExecution`, all `Tool*` capabilities, hooks, checkpointing |
| **ServerFallback** | ~200ms | 70% | 0.7 | `FunctionCalling`, `ToolUse`, `ExtendedThinking`, `BatchMode`, `SessionResume`, `SessionFork`, MCP |
| **Approximate** | ~500ms | 40% | 0.4 | `Vision`, `ImageInput`, `Audio`, `ImageGeneration`, `Embeddings`, `CacheControl`, sampling params |

### 8.5 Emulation Engine

Source: `crates/abp-emulation/src/lib.rs`.

```rust
pub enum EmulationStrategy {
    SystemPromptInjection { prompt: String },  // Inject instructions via system prompt
    PostProcessing { detail: String },         // Transform output after generation
    Disabled { reason: String },               // Cannot be emulated
}

pub enum FidelityLabel {
    Native,
    Emulated { strategy: EmulationStrategy },
}
```

Pre-configured emulation strategies:
- `emulate_structured_output()` — JSON schema enforcement via system prompt
- `emulate_code_execution()` — Reason through code step-by-step
- `emulate_extended_thinking()` — Chain-of-thought via system prompt
- `emulate_image_input()` — Text description of images
- `emulate_stop_sequences()` — Post-processing truncation

### 8.6 Capability Reporting Per SDK

| Capability Area | OpenAI (GPT-4o) | Claude (3.5 Sonnet) | Gemini (1.5 Pro) | Codex | Copilot | Kimi (moonshot) |
|-----------------|:---:|:---:|:---:|:---:|:---:|:---:|
| **Total capabilities** | 24 | 24 | 23 | 22 | 24 | 19 |
| **Native** | 17 | 13 | 16 | 22 | 17 | 13 |
| **Emulated** | 3 | 5 | 2 | 3 | 5 | 2 |
| **Unsupported** | 4 | 6 | 5 | 6 | 10+ | 10+ |

---

## 9. Lossy Mappings

Conversions between SDKs that **lose information** because the target format has
no equivalent concept.

### 9.1 Thinking / Extended Reasoning

| Source | Lost When Targeting | What Happens |
|--------|-------------------|--------------|
| Claude `ContentBlock::Thinking { text }` | OpenAI, Gemini, Copilot, Kimi | Preserved as `IrContentBlock::Thinking` in IR; dropped during `from_ir()` for targets without thinking support |
| Codex `CodexResponseItem::Reasoning { text }` | OpenAI, Claude, Gemini, Copilot, Kimi | Mapped to `IrContentBlock::Thinking`; lost on targets without thinking support |

### 9.2 Gemini Safety Ratings

| Source | Lost When Targeting | What Happens |
|--------|-------------------|--------------|
| `SafetySetting` | All other SDKs | Stored in vendor config; not translated to IR |
| `Candidate.safety_ratings` | All other SDKs | Silently dropped |

### 9.3 Copilot-Exclusive Features

| Source | Lost When Targeting | What Happens |
|--------|-------------------|--------------|
| `Reference` (File, Selection, Terminal, WebPage, GitDiff) | All others | Preserved in `IrMessage.metadata`; ignored by other SDKs' `from_ir()` |
| `CopilotConfirmation` | All others | Dropped; no equivalent concept |
| `CopilotError` structured errors | All others | Flattened to `"Error: {message}"` string |

### 9.4 Kimi-Exclusive Features

| Source | Lost When Targeting | What Happens |
|--------|-------------------|--------------|
| `SearchOptions` | All others | Stored in vendor config; not translatable |
| `KimiChunk.refs` (search references) | All others | Dropped |
| Built-in tools (`$web_search`, etc.) | All others | Stored as `IrContentBlock::Custom { custom_type: "kimi_builtin_tool" }` |

### 9.5 System Message Relocation

| Source | Target | Effect |
|--------|--------|--------|
| In-line `messages[]` (OpenAI/Kimi/Copilot) | Claude / Gemini / Codex | Extracted to separate field. **Lossless** semantically but structural change. |
| Separate field (Claude/Gemini/Codex) | OpenAI / Kimi / Copilot | Injected as first message. **Lossless** semantically. |

### 9.6 Image Format Differences

| Source | Target | What Happens |
|--------|--------|--------------|
| OpenAI `ImageUrl { url, detail }` | Claude, Gemini | URL refs → base64 preferred; `detail` hint dropped |
| Claude `ImageSource::Base64` | OpenAI | Base64 preserved in IR; OpenAI prefers URL references |
| Any image | Codex, Copilot, Kimi | Image content blocks dropped entirely |

### 9.7 Configuration Parameters

| Parameter | Available In | Lost When Targeting |
|-----------|-------------|-------------------|
| `top_k` | Claude, Gemini | OpenAI, Codex, Copilot, Kimi |
| `response_format` / `response_schema` | OpenAI, Gemini, Codex | Claude, Copilot, Kimi |
| `response_mime_type` | Gemini | All others |
| `intent` | Copilot | All others |
| `text_format` | Codex | All others |
| `cache_*` usage fields | Claude | All others (IR preserves; SDKs ignore) |

---

## 10. Known Limitations

### 10.1 Unmappable Features

Features that **cannot** be translated between SDKs, even with emulation:

| Feature | Reason |
|---------|--------|
| Gemini safety settings → other SDKs | No equivalent content safety API; vendor-specific harm taxonomy |
| Copilot confirmations → other SDKs | User-approval flow requires Copilot-specific UI integration |
| Kimi built-in tools → other SDKs | `$web_search`/`$file_tool`/`$code_tool`/`$browser` are server-side Kimi features |
| Audio content → non-audio SDKs | Only OpenAI and Gemini support audio I/O |
| Copilot references → other SDKs | File/selection/terminal context references are IDE-specific |

### 10.2 Fidelity Gaps

| Mapping Path | Issue | Impact |
|-------------|-------|--------|
| Claude → OpenAI | `Thinking` blocks dropped | Chain-of-thought reasoning lost |
| Any → Codex/Copilot/Kimi | Image content dropped | Multimodal context lost |
| OpenAI → Claude | `detail` hint on images dropped | Image processing quality hint lost |
| Gemini → Any | Safety ratings dropped | Content safety metadata lost |
| Claude → Any | Cache token metrics ignored | Cache efficiency data unavailable |
| Codex → Claude | Item-based response structure → content blocks | Envelope structure changed |
| Gemini → OpenAI/Claude | `FunctionCallingConfig` mode differences | No per-function forcing in Gemini |

### 10.3 Streaming Normalization Gaps

| Issue | Affected SDKs | Consequence |
|-------|--------------|-------------|
| No explicit content block lifecycle | OpenAI, Gemini, Kimi (struct-based) | Block start/stop must be inferred from delta field presence |
| Parallel tool call assembly | OpenAI, Kimi | Requires `ParallelToolCallAssembler` to correlate streaming deltas by tool index |
| Error events in streaming | Claude, Gemini, Codex, Kimi | Error events silently dropped; only OpenAI and Copilot surface them |
| Usage timing | Varies | Some SDKs send usage mid-stream; others only at end |

### 10.4 Error Propagation Gaps

| Issue | Impact |
|-------|--------|
| `error_code` not propagated in 5 of 6 shims | Consumers cannot distinguish error types in SDK responses |
| Claude/Gemini drop error events entirely | Errors during generation are silently lost |
| Streaming error codes lost in Copilot | Non-streaming preserves code; streaming drops it |

### 10.5 Bidirectional Mapping Asymmetry

Some mappings are not symmetric — A→B→A ≠ identity:

| Forward | Reverse | Asymmetry |
|---------|---------|-----------|
| OpenAI system in `messages[]` → Claude `system` field | Claude `system` → OpenAI first message | System message position may shift |
| Claude `ToolUse.id` → Gemini (no ID) | Gemini → Claude (ID generated) | Tool correlation IDs are synthesized, not preserved |
| Codex `Reasoning` → IR `Thinking` → Claude `Thinking` | Claude `Thinking` → IR → Codex `Reasoning` | ✅ Symmetric (both have thinking) |
| OpenAI `ImageUrl` → IR `Image` (base64) | IR `Image` → OpenAI `ImageUrl` | Format changes (URL ↔ base64) |

### 10.6 Bridge Crate Limitations

All 6 bridge crates (`openai-bridge`, `claude-bridge`, `gemini-bridge`, `codex-bridge`,
`copilot-bridge`, `kimi-bridge`) share these constraints:

| Limitation | Detail |
|-----------|--------|
| Feature-gated IR translation | IR mapping requires `ir` or `normalized` feature flags; not available by default |
| Sidecar-based execution only | All bridges spawn Node.js sidecar processes; no direct HTTP client implementation |
| Node.js dependency | `BridgeError::NodeNotFound` — requires Node.js runtime for sidecar scripts |
| Single run at a time | V0.1 protocol assumes one concurrent run per sidecar process |

---

## 11. Architecture

### 11.1 End-to-End Data Flow

```
Vendor SDK Request
       │
       ▼
  shim types.rs              (vendor-specific Rust types)
       │
       ▼
  lowering::to_ir()          (SDK types → IrConversation)
       │
       ▼
  convert::to_work_order()   (IR → ABP WorkOrder)
       │
       ▼
  ┌──────────────────────────────────────────┐
  │  ABP Pipeline                            │
  │  ┌─────────────────┐                     │
  │  │ Capability       │ negotiate()         │
  │  │ Negotiation      │ → NegotiationResult │
  │  ├─────────────────┤                     │
  │  │ Emulation        │ apply_emulation()   │
  │  │ Engine           │ → fill gaps         │
  │  ├─────────────────┤                     │
  │  │ Projection       │ route_to_backend()  │
  │  │ Matrix           │ → select backend    │
  │  ├─────────────────┤                     │
  │  │ Mapper           │ translate()         │
  │  │ (if Mapped mode) │ → target dialect    │
  │  └─────────────────┘                     │
  │           ↓                               │
  │     Backend Execution → Receipt           │
  └──────────────────────────────────────────┘
       │
       ▼
  convert::from_receipt()     (Receipt → vendor response)
       │                      OR
  convert::from_agent_event() (event → stream chunk)
       │
       ▼
  Vendor SDK Response / Stream
```

### 11.2 Crate Dependency Hierarchy

```
abp-ir (intermediate representation)
  ↑
abp-openai-sdk / abp-claude-sdk / abp-gemini-sdk / abp-codex-sdk / abp-copilot-sdk / abp-kimi-sdk
  ↑                          (SDK types + lowering)
abp-shim-openai / abp-shim-claude / abp-shim-gemini / abp-shim-codex / abp-shim-copilot / abp-shim-kimi
  ↑                          (drop-in client replacements)
openai-bridge / claude-bridge / gemini-bridge / codex-bridge / copilot-bridge / kimi-bridge
  ↑                          (sidecar-based bridge implementations)
abp-capability + abp-emulation + abp-mapping + abp-projection + abp-mapper
  ↑                          (negotiation, mapping, routing)
abp-core (WorkOrder, Receipt, AgentEvent, CONTRACT_VERSION)
  ↑
abp-protocol → abp-host → abp-integrations → abp-runtime → abp-cli
```

### 11.3 Mapper Implementation Coverage

Source: `crates/abp-mapper/`. Bidirectional IR mappers exist for these dialect pairs:

| Pair | Forward | Reverse | Fidelity |
|------|:-------:|:-------:|----------|
| OpenAI ↔ Claude | ✅ | ✅ | Lossy (thinking, system location) |
| OpenAI ↔ Gemini | ✅ | ✅ | Lossy (safety settings, tool config) |
| Claude ↔ Gemini | ✅ | ✅ | Lossy (thinking, safety) |
| OpenAI ↔ Codex | ✅ | ✅ | Lossy (item structure) |
| OpenAI ↔ Kimi | ✅ | ✅ | Lossy (web search) |
| OpenAI ↔ Copilot | ✅ | ✅ | Lossy (references) |
| Claude ↔ Kimi | ✅ | ✅ | Lossy (thinking, web search) |
| Gemini ↔ Kimi | ✅ | ✅ | Lossy (safety, web search) |
| Codex ↔ Claude | ✅ | ✅ | Lossy (item structure, thinking) |

Identity mapper (`IrIdentityMapper`) handles same-dialect passthrough.

---

## 12. Related Documentation

- [SDK Surface Area](sdk_surface_area.md) — per-vendor implementation details
- [Sidecar Protocol](sidecar_protocol.md) — JSONL wire format specification
- [Dialect×Engine Matrix](dialect_engine_matrix.md) — passthrough vs mapped routing
- [Capabilities](capabilities.md) — capability model reference
- [Capability Negotiation](capability_negotiation.md) — manifest + requirements
- [Error Codes](error_codes.md) — stable error code taxonomy
- Per-SDK mapping details: [sdk-mapping/openai.md](sdk-mapping/openai.md), [sdk-mapping/claude.md](sdk-mapping/claude.md), [sdk-mapping/gemini.md](sdk-mapping/gemini.md), [sdk-mapping/codex.md](sdk-mapping/codex.md), [sdk-mapping/copilot.md](sdk-mapping/copilot.md), [sdk-mapping/kimi.md](sdk-mapping/kimi.md)
