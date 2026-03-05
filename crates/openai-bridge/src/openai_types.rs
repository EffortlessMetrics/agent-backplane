// SPDX-License-Identifier: MIT OR Apache-2.0
//! OpenAI Chat Completions API types for direct integration.
//!
//! These mirror the OpenAI REST API wire format so the bridge can construct
//! requests, parse responses, and process streaming events without depending
//! on external OpenAI SDK crates.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

// ── Roles ───────────────────────────────────────────────────────────────

/// Message role in the OpenAI Chat Completions API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatMessageRole {
    /// System prompt / instructions.
    System,
    /// User / human turn.
    User,
    /// Assistant / model turn.
    Assistant,
    /// Tool result turn.
    Tool,
}

// ── Messages ────────────────────────────────────────────────────────────

/// A single message in a conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Message role.
    pub role: ChatMessageRole,
    /// Text content (may be absent for assistant messages with tool calls).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Tool calls requested by the model (assistant messages only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// ID of the tool call this message is a result for (tool messages only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

// ── Tool definitions ────────────────────────────────────────────────────

/// A tool definition for the OpenAI Chat Completions API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool type (always `"function"`).
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The function definition.
    pub function: FunctionDefinition,
}

/// The function definition inside a [`ToolDefinition`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionDefinition {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

/// A tool call emitted by the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique identifier for this tool call.
    pub id: String,
    /// Call type (always `"function"`).
    #[serde(rename = "type")]
    pub call_type: String,
    /// The function invocation details.
    pub function: FunctionCall,
}

/// The function invocation inside a [`ToolCall`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionCall {
    /// Name of the function to invoke.
    pub name: String,
    /// JSON-encoded arguments for the function.
    pub arguments: String,
}

// ── Request ─────────────────────────────────────────────────────────────

/// OpenAI Chat Completions request body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    /// Model identifier (e.g. `"gpt-4o"`).
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<ChatMessage>,
    /// Available tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    /// Sampling temperature (0.0–2.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Maximum tokens to generate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Whether to stream the response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Nucleus sampling parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Frequency penalty (-2.0 to 2.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    /// Presence penalty (-2.0 to 2.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    /// Custom stop sequences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    /// Number of completions to generate (default 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    /// Controls which tool the model should call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
}

// ── Response ────────────────────────────────────────────────────────────

/// OpenAI Chat Completions response body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    /// Unique response identifier.
    pub id: String,
    /// Object type (always `"chat.completion"`).
    pub object: String,
    /// Unix timestamp of creation.
    pub created: u64,
    /// Model used for the completion.
    pub model: String,
    /// Completion choices.
    pub choices: Vec<ChatCompletionChoice>,
    /// Token usage statistics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// A single choice in the completion response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionChoice {
    /// Zero-based index.
    pub index: u32,
    /// The assistant's response message.
    pub message: ChatMessage,
    /// Reason the model stopped (`"stop"`, `"tool_calls"`, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

// ── Token usage ─────────────────────────────────────────────────────────

/// Token usage statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Tokens consumed by the prompt.
    pub prompt_tokens: u64,
    /// Tokens generated in the completion.
    pub completion_tokens: u64,
    /// Total tokens (prompt + completion).
    pub total_tokens: u64,
}

// ── Streaming types ─────────────────────────────────────────────────────

/// A streaming chunk from the OpenAI Chat Completions API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionChunk {
    /// Chunk identifier.
    pub id: String,
    /// Object type (always `"chat.completion.chunk"`).
    pub object: String,
    /// Unix timestamp of creation.
    pub created: u64,
    /// Model that produced this chunk.
    pub model: String,
    /// Streaming choices with deltas.
    pub choices: Vec<StreamChoice>,
}

/// A single choice inside a streaming chunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamChoice {
    /// Zero-based choice index.
    pub index: u32,
    /// The incremental delta.
    pub delta: StreamDelta,
    /// Finish reason — `None` until the stream ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// An incremental delta inside a streaming choice.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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

/// A tool call fragment inside a streaming delta.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamToolCall {
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
    pub function: Option<StreamFunctionCall>,
}

/// Incremental function call data inside a streaming tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamFunctionCall {
    /// Function name (first fragment only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Incremental arguments fragment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

// ── API errors ──────────────────────────────────────────────────────────

/// OpenAI API error response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiError {
    /// The error payload.
    pub error: ApiErrorDetail,
}

/// Detailed error information inside an [`ApiError`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiErrorDetail {
    /// Human-readable error message.
    pub message: String,
    /// Machine-readable error type (e.g. `"invalid_request_error"`).
    #[serde(rename = "type")]
    pub error_type: String,
    /// Parameter that caused the error, if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    /// Machine-readable error code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}
