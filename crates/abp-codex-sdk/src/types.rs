// SPDX-License-Identifier: MIT OR Apache-2.0
//! Codex CLI API types modeled on the OpenAI Chat Completions wire format.
//!
//! These types mirror the OpenAI REST API but extend it with Codex-specific
//! fields such as [`CodexRequest::instructions`] for the system prompt,
//! [`CodexFileChange`] for file mutations, and [`CodexCommand`] for shell
//! command execution.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Request ─────────────────────────────────────────────────────────────

/// Codex chat completion request.
///
/// Extends the OpenAI format with an `instructions` field that carries
/// the Codex CLI system prompt separately from the message array.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CodexRequest {
    /// Model identifier (e.g. `codex-mini-latest`).
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<CodexMessage>,
    /// Codex CLI system prompt injected outside the message array.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Sampling temperature (0.0–2.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Nucleus sampling parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Maximum tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Whether to stream the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Tool definitions available to the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<CodexTool>>,
    /// Controls which tool the model should call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<CodexToolChoice>,
}

// ── Messages ────────────────────────────────────────────────────────────

/// A chat message discriminated by `role`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum CodexMessage {
    /// System prompt.
    System {
        /// System prompt text.
        content: String,
    },
    /// User message.
    User {
        /// Message content.
        content: String,
    },
    /// Assistant response, optionally with tool calls.
    Assistant {
        /// Text content (may be absent when tool calls are present).
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        /// Tool calls requested by the model.
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<CodexToolCall>>,
    },
    /// Tool result message.
    Tool {
        /// The tool output.
        content: String,
        /// ID of the tool call this result corresponds to.
        tool_call_id: String,
    },
}

// ── Response ────────────────────────────────────────────────────────────

/// Codex chat completion response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CodexResponse {
    /// Unique response identifier.
    pub id: String,
    /// Object type (always `"chat.completion"`).
    pub object: String,
    /// Unix timestamp of creation.
    pub created: u64,
    /// Model used for the completion.
    pub model: String,
    /// Completion choices.
    pub choices: Vec<CodexChoice>,
    /// Token usage statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<CodexUsage>,
}

/// A single choice in the completion response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CodexChoice {
    /// Zero-based index.
    pub index: u32,
    /// The assistant's response message.
    pub message: CodexChoiceMessage,
    /// Reason the model stopped (`"stop"`, `"tool_calls"`, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// The assistant message inside a response [`CodexChoice`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CodexChoiceMessage {
    /// Always `"assistant"`.
    pub role: String,
    /// Text content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Tool calls emitted by the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<CodexToolCall>>,
}

// ── Streaming ───────────────────────────────────────────────────────────

/// A streaming chunk from the Codex API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CodexStreamChunk {
    /// Chunk identifier.
    pub id: String,
    /// Object type (always `"chat.completion.chunk"`).
    pub object: String,
    /// Unix timestamp of creation.
    pub created: u64,
    /// Model that produced this chunk.
    pub model: String,
    /// Streaming choices with deltas.
    pub choices: Vec<CodexStreamChoice>,
}

/// A single choice inside a streaming chunk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CodexStreamChoice {
    /// Zero-based choice index.
    pub index: u32,
    /// The incremental delta.
    pub delta: CodexStreamDelta,
    /// Finish reason — `None` until the stream ends.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// An incremental delta inside a streaming choice.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CodexStreamDelta {
    /// Role (usually only in the first chunk).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Text content fragment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Incremental tool call fragments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<CodexStreamToolCall>>,
}

/// A tool call fragment inside a streaming delta.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CodexStreamToolCall {
    /// Index of the tool call in the array.
    pub index: u32,
    /// Tool call ID (first fragment only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Call type (first fragment only, always `"function"`).
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,
    /// Incremental function call data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<CodexStreamFunctionCall>,
}

/// Incremental function call data inside a streaming tool call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CodexStreamFunctionCall {
    /// Function name (first fragment only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Incremental arguments fragment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

// ── Usage ───────────────────────────────────────────────────────────────

/// Token usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CodexUsage {
    /// Tokens consumed by the prompt.
    pub prompt_tokens: u64,
    /// Tokens generated in the completion.
    pub completion_tokens: u64,
    /// Total tokens (prompt + completion).
    pub total_tokens: u64,
}

// ── Tool types ──────────────────────────────────────────────────────────

/// A function tool definition for the Codex API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CodexTool {
    /// Tool type (always `"function"`).
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The function definition.
    pub function: CodexFunctionDef,
}

/// The function definition inside a [`CodexTool`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CodexFunctionDef {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

/// A tool call emitted by the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CodexToolCall {
    /// Unique identifier for this tool call.
    pub id: String,
    /// Call type (always `"function"`).
    #[serde(rename = "type")]
    pub call_type: String,
    /// The function invocation details.
    pub function: CodexFunctionCall,
}

/// The function invocation inside a [`CodexToolCall`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CodexFunctionCall {
    /// Name of the function to invoke.
    pub name: String,
    /// JSON-encoded arguments for the function.
    pub arguments: String,
}

/// Controls which (if any) tool the model should call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(untagged)]
pub enum CodexToolChoice {
    /// A string shorthand: `"none"`, `"auto"`, or `"required"`.
    Mode(CodexToolChoiceMode),
    /// Force a specific function call.
    Function {
        /// Must be `"function"`.
        #[serde(rename = "type")]
        tool_type: String,
        /// The function to force.
        function: CodexToolChoiceFunctionRef,
    },
}

/// String-form tool choice modes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CodexToolChoiceMode {
    /// Model will not call any tool.
    None,
    /// Model decides whether to call a tool.
    Auto,
    /// Model must call at least one tool.
    Required,
}

/// A reference to a specific function in a forced tool choice.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CodexToolChoiceFunctionRef {
    /// Name of the function to force.
    pub name: String,
}

// ── Codex-specific action types ─────────────────────────────────────────

/// Represents a file change that Codex can make.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CodexFileChange {
    /// Relative path to the file being changed.
    pub path: String,
    /// The kind of file operation.
    pub operation: FileOperation,
    /// New file content (for `Create` and `Update` operations).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Unified diff (for `Patch` operations).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
}

/// The kind of file operation within a [`CodexFileChange`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileOperation {
    /// Create a new file.
    Create,
    /// Overwrite an existing file.
    Update,
    /// Delete a file.
    Delete,
    /// Apply a unified diff patch to an existing file.
    Patch,
}

/// Represents a shell command that Codex can execute.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct CodexCommand {
    /// The shell command string to execute.
    pub command: String,
    /// Working directory for the command (relative to workspace root).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Maximum time in seconds before the command is killed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
    /// Standard output captured after execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    /// Standard error captured after execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    /// Process exit code (`None` if the command has not yet completed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}
