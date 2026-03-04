// SPDX-License-Identifier: MIT OR Apache-2.0
//! Kimi Chat Completions API types (request, response, streaming, tool calls).
//!
//! These types model the Moonshot Kimi wire format, which is OpenAI-compatible
//! with extensions for built-in tools (`$web_search`, `$browser`, `$file_tool`,
//! `$code_tool`), citation references, and the `k1` reasoning mode.

use serde::{Deserialize, Serialize};

// ── Roles ───────────────────────────────────────────────────────────────

/// Message roles in the Kimi API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// System prompt.
    System,
    /// User message.
    User,
    /// Assistant (model) message.
    Assistant,
    /// Tool result message.
    Tool,
}

// ── Messages ────────────────────────────────────────────────────────────

/// A message in a Kimi chat completions request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Message role.
    pub role: Role,
    /// Text content of the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Tool call ID this message responds to (role=tool only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Tool calls requested by the model (role=assistant only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

// ── Tool definitions ────────────────────────────────────────────────────

/// A tool definition in a Kimi request — either a user function or a built-in.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolDefinition {
    /// A user-defined function tool.
    Function {
        /// The function definition payload.
        function: FunctionDefinition,
    },
    /// A Kimi built-in function (e.g. `$web_search`, `$browser`).
    BuiltinFunction {
        /// The built-in function descriptor.
        function: BuiltinFunctionDef,
    },
}

/// A user-defined function tool definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionDefinition {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

/// A Kimi built-in function descriptor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BuiltinFunctionDef {
    /// Built-in name (e.g. `"$web_search"`, `"$browser"`, `"$file_tool"`, `"$code_tool"`).
    pub name: String,
}

// ── Tool calls ──────────────────────────────────────────────────────────

/// A tool call in a Kimi response or request message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    /// Unique tool call identifier.
    pub id: String,
    /// Call type (always `"function"`).
    #[serde(rename = "type")]
    pub call_type: String,
    /// The function invocation details.
    pub function: FunctionCall,
}

/// The function payload within a tool call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionCall {
    /// Name of the function to invoke.
    pub name: String,
    /// JSON-encoded arguments.
    pub arguments: String,
}

// ── Request ─────────────────────────────────────────────────────────────

/// Kimi chat completions request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiRequest {
    /// Model identifier (e.g. `moonshot-v1-8k`).
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<Message>,
    /// Maximum tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Sampling temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Whether to stream the response via SSE.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Tool definitions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    /// Enable Kimi built-in web search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_search: Option<bool>,
}

// ── Response ────────────────────────────────────────────────────────────

/// Kimi chat completions response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KimiResponse {
    /// Unique response identifier.
    pub id: String,
    /// Model that generated the response.
    pub model: String,
    /// Completion choices.
    pub choices: Vec<Choice>,
    /// Token usage statistics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Citation references when search was used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refs: Option<Vec<KimiRef>>,
}

/// A single choice in a Kimi completions response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    /// Zero-based index.
    pub index: u32,
    /// The assistant's response message.
    pub message: ResponseMessage,
    /// Reason the model stopped generating.
    pub finish_reason: Option<String>,
}

/// A message within a Kimi response choice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMessage {
    /// Message role.
    pub role: String,
    /// Text content.
    pub content: Option<String>,
    /// Tool calls requested by the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

/// Token usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Usage {
    /// Tokens consumed by the prompt.
    pub prompt_tokens: u64,
    /// Tokens generated in the completion.
    pub completion_tokens: u64,
    /// Total tokens.
    pub total_tokens: u64,
}

/// A citation reference from Kimi search.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiRef {
    /// One-based citation index.
    pub index: u32,
    /// URL of the cited source.
    pub url: String,
    /// Title of the cited source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

// ── Streaming types ─────────────────────────────────────────────────────

/// A streaming SSE chunk from the Kimi API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamChunk {
    /// Chunk identifier.
    pub id: String,
    /// Object type (always `"chat.completion.chunk"`).
    pub object: String,
    /// Unix timestamp.
    pub created: u64,
    /// Model that produced this chunk.
    pub model: String,
    /// Choices with streaming deltas.
    pub choices: Vec<StreamChoice>,
    /// Usage (only in final chunk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Citation references (may appear in later chunks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refs: Option<Vec<KimiRef>>,
}

/// A single choice within a streaming chunk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamChoice {
    /// Zero-based choice index.
    pub index: u32,
    /// The incremental delta.
    pub delta: StreamDelta,
    /// Finish reason — `None` until the stream ends.
    pub finish_reason: Option<String>,
}

/// An incremental delta within a streaming chunk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct StreamDelta {
    /// Role (usually only in the first chunk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Text content fragment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Incremental tool call fragments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<StreamToolCall>>,
}

/// An incremental tool call fragment within a streaming delta.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamToolCall {
    /// Index in the tool_calls array.
    pub index: u32,
    /// Tool call ID (only in first fragment).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Call type (only in first fragment).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,
    /// Incremental function data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<StreamFunctionCall>,
}

/// Incremental function call data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamFunctionCall {
    /// Function name (only in first fragment).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Partial JSON arguments string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

// ── Built-in tool constants ─────────────────────────────────────────────

/// Kimi built-in tool name constants.
pub mod builtin {
    /// Web search built-in function name.
    pub const WEB_SEARCH: &str = "$web_search";
    /// File analysis built-in function name.
    pub const FILE_TOOL: &str = "$file_tool";
    /// Code execution built-in function name.
    pub const CODE_TOOL: &str = "$code_tool";
    /// Web browser built-in function name.
    pub const BROWSER: &str = "$browser";

    /// Returns `true` if the given name is a known Kimi built-in tool.
    #[must_use]
    pub fn is_builtin(name: &str) -> bool {
        matches!(name, WEB_SEARCH | FILE_TOOL | CODE_TOOL | BROWSER)
    }
}
