// SPDX-License-Identifier: MIT OR Apache-2.0
//! Copilot Chat Completions API wire-format types.
//!
//! These mirror the OpenAI Chat Completions wire format with Copilot-specific
//! extensions (`intent`, `references`, `copilot_tokens`).  They are intended
//! for direct (de)serialization of JSON payloads exchanged with the GitHub
//! Copilot API surface.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ── Reference types ─────────────────────────────────────────────────────

/// Discriminator for the kind of context reference attached to a request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceType {
    /// A file from the workspace or repository.
    File,
    /// An editor selection / highlighted range.
    Selection,
    /// Terminal output.
    Terminal,
    /// A web page or URL.
    WebPage,
    /// A git diff (staged, unstaged, or between refs).
    GitDiff,
}

/// A structured context reference supplied alongside a Copilot chat request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Reference {
    /// The kind of reference.
    #[serde(rename = "type")]
    pub ref_type: ReferenceType,
    /// Unique identifier for this reference.
    pub id: String,
    /// Optional URI pointing to the referenced resource.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// Optional inline content of the referenced resource.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Optional extra metadata (language, line range, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, serde_json::Value>>,
}

// ── Chat message ────────────────────────────────────────────────────────

/// A single message in the Copilot chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CopilotChatMessage {
    /// Message role (`system`, `user`, `assistant`, or `tool`).
    pub role: String,
    /// Text content of the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Optional display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Tool calls emitted by the assistant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<CopilotToolCall>>,
    /// Tool call ID (for `tool` role messages).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// A tool call inside an assistant message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CopilotToolCall {
    /// Unique identifier for this tool call.
    pub id: String,
    /// Call type (always `"function"`).
    #[serde(rename = "type")]
    pub call_type: String,
    /// Function invocation details.
    pub function: CopilotFunctionCall,
}

/// Function name + arguments inside a [`CopilotToolCall`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CopilotFunctionCall {
    /// Name of the function to invoke.
    pub name: String,
    /// JSON-encoded arguments string.
    pub arguments: String,
}

// ── Chat request ────────────────────────────────────────────────────────

/// OpenAI-compatible chat completions request with Copilot extensions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CopilotChatRequest {
    /// Model identifier (e.g. `gpt-4o`).
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<CopilotChatMessage>,
    /// Sampling temperature (0.0–2.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Nucleus sampling parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Maximum tokens to generate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Whether to stream the response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Tool definitions available to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<CopilotTool>>,
    /// Controls which tool the model should call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    // -- Copilot extensions ------------------------------------------------
    /// Copilot-specific intent string (e.g. `"conversation"`, `"code-review"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    /// Copilot-specific context references (files, selections, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub references: Option<Vec<Reference>>,
}

/// A function tool definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CopilotTool {
    /// Tool type (always `"function"`).
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The function definition.
    pub function: CopilotToolFunction,
}

/// Function definition inside a [`CopilotTool`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CopilotToolFunction {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

// ── Chat response ───────────────────────────────────────────────────────

/// OpenAI-compatible chat completions response with Copilot metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CopilotChatResponse {
    /// Unique response identifier.
    pub id: String,
    /// Object type (always `"chat.completion"`).
    pub object: String,
    /// Unix timestamp of creation.
    pub created: u64,
    /// Model that generated the completion.
    pub model: String,
    /// Completion choices.
    pub choices: Vec<CopilotChatChoice>,
    /// Token usage statistics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<CopilotUsage>,
}

/// A single choice in the chat completion response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CopilotChatChoice {
    /// Zero-based index.
    pub index: u32,
    /// The assistant's response message.
    pub message: CopilotChatChoiceMessage,
    /// Reason the model stopped (`"stop"`, `"tool_calls"`, `"length"`, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// The assistant message inside a response [`CopilotChatChoice`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CopilotChatChoiceMessage {
    /// Always `"assistant"`.
    pub role: String,
    /// Text content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Tool calls emitted by the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<CopilotToolCall>>,
}

// ── Streaming types ─────────────────────────────────────────────────────

/// A streaming chunk from the Copilot chat completions API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CopilotStreamChunk {
    /// Chunk identifier.
    pub id: String,
    /// Object type (always `"chat.completion.chunk"`).
    pub object: String,
    /// Unix timestamp of creation.
    pub created: u64,
    /// Model that produced this chunk.
    pub model: String,
    /// Streaming choices with deltas.
    pub choices: Vec<CopilotStreamChoice>,
}

/// A single choice inside a streaming chunk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CopilotStreamChoice {
    /// Zero-based choice index.
    pub index: u32,
    /// The incremental delta.
    pub delta: CopilotStreamDelta,
    /// Finish reason — `None` until the stream ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// An incremental delta inside a streaming choice.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CopilotStreamDelta {
    /// Role (usually only in the first chunk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Text content fragment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Incremental tool call fragments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<CopilotStreamToolCall>>,
}

/// A tool call fragment inside a streaming delta.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CopilotStreamToolCall {
    /// Index of the tool call in the array.
    pub index: u32,
    /// Tool call ID (first fragment only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Call type (first fragment only, always `"function"`).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,
    /// Incremental function call data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<CopilotStreamFunctionCall>,
}

/// Incremental function call data inside a streaming tool call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CopilotStreamFunctionCall {
    /// Function name (first fragment only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Incremental arguments fragment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

// ── Token usage ─────────────────────────────────────────────────────────

/// Token usage statistics with Copilot-specific extensions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CopilotUsage {
    /// Tokens consumed by the prompt.
    pub prompt_tokens: u64,
    /// Tokens generated in the completion.
    pub completion_tokens: u64,
    /// Total tokens (prompt + completion).
    pub total_tokens: u64,
    /// Copilot-specific token accounting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copilot_tokens: Option<u32>,
}

// ── Copilot-specific code suggestion types ──────────────────────────────

/// The type of code completion requested from Copilot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompletionType {
    /// Inline ghost-text completion (single line or multi-line).
    Inline,
    /// Full function/block completion.
    Block,
    /// Fill-in-the-middle (FIM) completion.
    FillInMiddle,
}

/// A request for Copilot code completions (ghost text / inline suggestions).
///
/// This represents the Copilot-specific code completion API surface,
/// distinct from the chat completions endpoint. It carries editor context
/// (file path, cursor position, surrounding code) to produce inline
/// code suggestions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CopilotCompletionRequest {
    /// Model identifier for completions (e.g. `copilot-codex`).
    pub model: String,
    /// The code prefix (text before the cursor).
    pub prompt: String,
    /// Optional code suffix (text after the cursor) for fill-in-the-middle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
    /// Maximum tokens to generate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Sampling temperature (0.0–1.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Nucleus sampling parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Number of completions to generate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    /// Stop sequences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    /// The type of completion being requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_type: Option<CompletionType>,
    /// Programming language of the source file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// File path for the source being edited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// Zero-based cursor line position in the editor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_line: Option<u32>,
    /// Zero-based cursor column position in the editor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_column: Option<u32>,
}

/// A single code suggestion returned by the Copilot completion API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CopilotCompletionChoice {
    /// Zero-based index of this suggestion.
    pub index: u32,
    /// The generated code text.
    pub text: String,
    /// Reason the model stopped generating (`"stop"`, `"length"`, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// A response from the Copilot code completions API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CopilotCompletionResponse {
    /// Unique response identifier.
    pub id: String,
    /// Object type (always `"text_completion"`).
    pub object: String,
    /// Unix timestamp of creation.
    pub created: u64,
    /// Model that generated the completions.
    pub model: String,
    /// One or more completion choices.
    pub choices: Vec<CopilotCompletionChoice>,
    /// Token usage statistics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<CopilotUsage>,
}

/// A panel-style code suggestion with metadata for ranking/filtering.
///
/// Used when Copilot presents multiple suggestions in a panel UI rather
/// than inline ghost text. Each suggestion carries a confidence score
/// and display range information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CopilotCodeSuggestion {
    /// Unique identifier for this suggestion.
    pub id: String,
    /// The suggested code text.
    pub text: String,
    /// Confidence score (0.0–1.0) for ranking suggestions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// The start line in the original file where the suggestion applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range_start_line: Option<u32>,
    /// The end line in the original file where the suggestion applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range_end_line: Option<u32>,
    /// Programming language of the suggestion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// The type of completion that produced this suggestion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_type: Option<CompletionType>,
}
