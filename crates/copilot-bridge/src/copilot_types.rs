// SPDX-License-Identifier: MIT OR Apache-2.0
//! GitHub Copilot Chat API types for direct integration.
//!
//! These mirror the Copilot agent API wire format (OpenAI-compatible with
//! extensions for references, confirmations, and function calls).

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ── Roles ───────────────────────────────────────────────────────────────

/// Message role in the Copilot Chat API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopilotMessageRole {
    /// System prompt / instructions.
    System,
    /// User / human turn.
    User,
    /// Assistant / model turn.
    Assistant,
    /// Tool result turn.
    Tool,
}

// ── References ──────────────────────────────────────────────────────────

/// The type discriminator for a Copilot reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopilotReferenceType {
    /// A file reference (path + optional content).
    File,
    /// A code snippet with location metadata.
    Snippet,
    /// A repository reference (owner/name).
    Repository,
    /// A web search result.
    WebSearchResult,
}

/// A reference attached to a Copilot message or response.
///
/// References provide structured context (files, snippets, repos, web results)
/// that the Copilot agent can use during processing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopilotReference {
    /// The reference type discriminator.
    #[serde(rename = "type")]
    pub ref_type: CopilotReferenceType,
    /// Unique identifier for this reference.
    pub id: String,
    /// Structured data payload for this reference.
    pub data: serde_json::Value,
    /// Optional metadata (e.g. display label, URI).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, serde_json::Value>>,
}

// ── Confirmations ───────────────────────────────────────────────────────

/// A confirmation prompt for user approval flows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopilotConfirmation {
    /// Unique identifier for this confirmation.
    pub id: String,
    /// Title displayed to the user.
    pub title: String,
    /// Detailed message explaining what the user is approving.
    pub message: String,
    /// Whether the confirmation has been accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted: Option<bool>,
}

// ── Tool types ──────────────────────────────────────────────────────────

/// The type of a Copilot tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopilotToolType {
    /// A standard function tool.
    Function,
    /// A confirmation prompt tool.
    Confirmation,
}

/// A Copilot tool definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopilotTool {
    /// The tool type.
    #[serde(rename = "type")]
    pub tool_type: CopilotToolType,
    /// The function definition (for function tools).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<CopilotFunctionDef>,
}

/// Function definition inside a Copilot tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopilotFunctionDef {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

// ── Messages ────────────────────────────────────────────────────────────

/// A single message in the Copilot conversation format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopilotMessage {
    /// Message role.
    pub role: CopilotMessageRole,
    /// Text content of the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Optional display name for the message author.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// References attached to this message.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copilot_references: Vec<CopilotReference>,
    /// Tool calls requested by the model (assistant messages only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<CopilotToolCall>>,
    /// ID of the tool call this message is a result for (tool messages only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// A tool call emitted by the Copilot model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopilotToolCall {
    /// Unique identifier for this tool call.
    pub id: String,
    /// Call type (always `"function"`).
    #[serde(rename = "type")]
    pub call_type: String,
    /// The function invocation details.
    pub function: CopilotFunctionCall,
}

/// Function call details inside a [`CopilotToolCall`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopilotFunctionCall {
    /// Name of the function to invoke.
    pub name: String,
    /// JSON-encoded arguments for the function.
    pub arguments: String,
}

// ── Turn history ────────────────────────────────────────────────────────

/// An entry in the turn history for multi-turn conversations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopilotTurnEntry {
    /// The user message for this turn.
    pub request: String,
    /// The assistant response for this turn.
    pub response: String,
}

// ── Request ─────────────────────────────────────────────────────────────

/// A request to the GitHub Copilot agent API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopilotChatRequest {
    /// Model identifier (e.g. `"gpt-4o"`).
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<CopilotMessage>,
    /// Tool definitions available to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<CopilotTool>>,
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
    /// Custom stop sequences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    /// Number of completions to generate (default 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    /// Controls which tool the model should call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    /// Top-level references for the request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copilot_references: Vec<CopilotReference>,
    /// Previous turns in the conversation (for multi-turn agents).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub turn_history: Vec<CopilotTurnEntry>,
}

// ── Response ────────────────────────────────────────────────────────────

/// A non-streaming response from the Copilot agent API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopilotChatResponse {
    /// Unique response identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Model used for the completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Completion choices.
    pub choices: Vec<CopilotChatChoice>,
    /// Token usage statistics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<CopilotUsage>,
    /// References emitted in the response.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copilot_references: Vec<CopilotReference>,
    /// Errors reported during processing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copilot_errors: Vec<CopilotError>,
    /// Confirmation prompt (if the agent requests user approval).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copilot_confirmation: Option<CopilotConfirmation>,
}

/// A single choice in the Copilot completion response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopilotChatChoice {
    /// Zero-based index.
    pub index: u32,
    /// The assistant's response message.
    pub message: CopilotMessage,
    /// Reason the model stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// Token usage statistics for the Copilot API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CopilotUsage {
    /// Tokens consumed by the prompt.
    pub prompt_tokens: u64,
    /// Tokens generated in the completion.
    pub completion_tokens: u64,
    /// Total tokens (prompt + completion).
    pub total_tokens: u64,
}

/// An error reported by the Copilot agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopilotError {
    /// Error type identifier.
    #[serde(rename = "type")]
    pub error_type: String,
    /// Human-readable error message.
    pub message: String,
    /// Machine-readable error code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Unique error identifier for correlation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
}

// ── Streaming types ─────────────────────────────────────────────────────

/// Server-sent events from the Copilot streaming API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CopilotStreamEvent {
    /// References emitted at the start of a response.
    CopilotReferences {
        /// The references payload.
        references: Vec<CopilotReference>,
    },
    /// Errors encountered during processing.
    CopilotErrors {
        /// The errors payload.
        errors: Vec<CopilotError>,
    },
    /// An incremental chat completion chunk (OpenAI-compatible delta).
    ChatCompletionChunk {
        /// The streaming chunk data.
        chunk: CopilotStreamChunk,
    },
    /// A confirmation prompt for user approval.
    CopilotConfirmation {
        /// The confirmation details.
        confirmation: CopilotConfirmation,
    },
    /// Stream completed.
    Done {},
}

/// A streaming chunk from the Copilot API (OpenAI-compatible with extensions).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopilotStreamChunk {
    /// Chunk identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Model that produced this chunk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Streaming choices with deltas.
    pub choices: Vec<CopilotStreamChoice>,
    /// Token usage (typically in final chunk only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<CopilotUsage>,
}

/// A single choice inside a streaming chunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopilotStreamFunctionCall {
    /// Function name (first fragment only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Incremental arguments fragment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}
