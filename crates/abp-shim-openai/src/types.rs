// SPDX-License-Identifier: MIT OR Apache-2.0
//! Strongly-typed OpenAI Chat Completions API types.
//!
//! These types mirror the real OpenAI REST API wire format as closely as
//! possible, using a role-tagged `ChatMessage` enum instead of a flat struct.
//! See <https://platform.openai.com/docs/api-reference/chat>.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Chat completion request ─────────────────────────────────────────────

/// OpenAI Chat Completions request with a role-tagged message enum.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ChatCompletionRequest {
    /// Model identifier (e.g. `gpt-4o`).
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<ChatMessage>,
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
    pub tools: Option<Vec<Tool>>,
    /// Controls which tool the model should call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
}

// ── Chat message (role-tagged) ──────────────────────────────────────────

/// A chat message discriminated by `role`.
///
/// Serializes with a `"role"` tag that matches the OpenAI wire format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum ChatMessage {
    /// System prompt.
    System {
        /// System prompt text.
        content: String,
    },
    /// User message with text or multimodal content.
    User {
        /// Message content (string or content-part array).
        content: MessageContent,
    },
    /// Assistant response, optionally with tool calls.
    Assistant {
        /// Text content (may be absent when tool calls are present).
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        /// Tool calls requested by the model.
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<ToolCall>>,
    },
    /// Tool result message.
    Tool {
        /// The tool output.
        content: String,
        /// ID of the tool call this result corresponds to.
        tool_call_id: String,
    },
}

// ── Message content ─────────────────────────────────────────────────────

/// Message content: either a plain string or an array of content parts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(untagged)]
pub enum MessageContent {
    /// Plain text content.
    Text(String),
    /// Array of typed content parts (text, images, etc.).
    Parts(Vec<ContentPart>),
}

/// A single content part inside a multimodal message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    /// Text content part.
    Text {
        /// The text.
        text: String,
    },
    /// Image URL content part.
    ImageUrl {
        /// The image URL payload.
        image_url: ImageUrl,
    },
}

/// An image URL reference inside a content part.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ImageUrl {
    /// The image URL.
    pub url: String,
    /// Optional detail level (`"low"`, `"high"`, or `"auto"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

// ── Chat completion response ────────────────────────────────────────────

/// OpenAI Chat Completions response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
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
    pub choices: Vec<Choice>,
    /// Token usage statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// A single choice in the completion response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Choice {
    /// Zero-based index.
    pub index: u32,
    /// The assistant's response message.
    pub message: ChoiceMessage,
    /// Reason the model stopped (`"stop"`, `"tool_calls"`, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// The assistant message inside a response [`Choice`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ChoiceMessage {
    /// Always `"assistant"`.
    pub role: String,
    /// Text content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Tool calls emitted by the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

// ── Streaming types ─────────────────────────────────────────────────────

/// A streaming chunk from the OpenAI Chat Completions API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct StreamChunk {
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct StreamChoice {
    /// Zero-based choice index.
    pub index: u32,
    /// The incremental delta.
    pub delta: StreamDelta,
    /// Finish reason — `None` until the stream ends.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// An incremental delta inside a streaming choice.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct StreamDelta {
    /// Role (usually only in the first chunk).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Text content fragment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Incremental tool call fragments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<StreamToolCall>>,
}

/// A tool call fragment inside a streaming delta.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct StreamToolCall {
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
    pub function: Option<StreamFunctionCall>,
}

/// Incremental function call data inside a streaming tool call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct StreamFunctionCall {
    /// Function name (first fragment only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Incremental arguments fragment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

// ── Token usage ─────────────────────────────────────────────────────────

/// Token usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Usage {
    /// Tokens consumed by the prompt.
    pub prompt_tokens: u64,
    /// Tokens generated in the completion.
    pub completion_tokens: u64,
    /// Total tokens (prompt + completion).
    pub total_tokens: u64,
}

// ── Tool types ──────────────────────────────────────────────────────────

/// A function tool definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Tool {
    /// Tool type (always `"function"`).
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The function definition.
    pub function: FunctionDef,
}

/// The function definition inside a [`Tool`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct FunctionDef {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

/// A tool call emitted by the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct FunctionCall {
    /// Name of the function to invoke.
    pub name: String,
    /// JSON-encoded arguments for the function.
    pub arguments: String,
}

/// Controls which (if any) tool the model should call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(untagged)]
pub enum ToolChoice {
    /// A string shorthand: `"none"`, `"auto"`, or `"required"`.
    Mode(ToolChoiceMode),
    /// Force a specific function call.
    Function {
        /// Must be `"function"`.
        #[serde(rename = "type")]
        tool_type: String,
        /// The function to force.
        function: ToolChoiceFunctionRef,
    },
}

/// String-form tool choice modes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoiceMode {
    /// Model will not call any tool.
    None,
    /// Model decides whether to call a tool.
    Auto,
    /// Model must call at least one tool.
    Required,
}

/// A reference to a specific function in a forced tool choice.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ToolChoiceFunctionRef {
    /// Name of the function to force.
    pub name: String,
}

// ── Error response ──────────────────────────────────────────────────────

/// Structured error response from the OpenAI API.
///
/// Returned when the API responds with a non-2xx status code.
/// See <https://platform.openai.com/docs/guides/error-codes>.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ErrorResponse {
    /// The error payload.
    pub error: ErrorDetail,
}

/// Detailed error information inside an [`ErrorResponse`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ErrorDetail {
    /// Human-readable error message.
    pub message: String,
    /// Machine-readable error type (e.g. `"invalid_request_error"`, `"rate_limit_error"`).
    #[serde(rename = "type")]
    pub error_type: String,
    /// Parameter that caused the error, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    /// Machine-readable error code (e.g. `"model_not_found"`, `"context_length_exceeded"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl std::fmt::Display for ErrorResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.error.message, self.error.error_type)
    }
}

impl std::error::Error for ErrorResponse {}

impl ErrorResponse {
    /// Create an `invalid_request_error` response.
    #[must_use]
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            error: ErrorDetail {
                message: message.into(),
                error_type: "invalid_request_error".into(),
                param: None,
                code: None,
            },
        }
    }

    /// Create a `rate_limit_error` response.
    #[must_use]
    pub fn rate_limit(message: impl Into<String>) -> Self {
        Self {
            error: ErrorDetail {
                message: message.into(),
                error_type: "rate_limit_error".into(),
                param: None,
                code: Some("rate_limit_exceeded".into()),
            },
        }
    }

    /// Create a `model_not_found` error response.
    #[must_use]
    pub fn model_not_found(model: &str) -> Self {
        Self {
            error: ErrorDetail {
                message: format!(
                    "The model `{model}` does not exist or you do not have access to it."
                ),
                error_type: "invalid_request_error".into(),
                param: Some("model".into()),
                code: Some("model_not_found".into()),
            },
        }
    }

    /// Create an `authentication_error` response.
    #[must_use]
    pub fn auth_error(message: impl Into<String>) -> Self {
        Self {
            error: ErrorDetail {
                message: message.into(),
                error_type: "authentication_error".into(),
                param: None,
                code: None,
            },
        }
    }

    /// Create a `server_error` response.
    #[must_use]
    pub fn server_error(message: impl Into<String>) -> Self {
        Self {
            error: ErrorDetail {
                message: message.into(),
                error_type: "server_error".into(),
                param: None,
                code: None,
            },
        }
    }

    /// Try to parse an error from a JSON string.
    ///
    /// Falls back to a server error if the JSON does not match the expected format.
    #[must_use]
    pub fn parse_or_server_error(body: &str) -> Self {
        serde_json::from_str(body).unwrap_or_else(|_| Self::server_error(body))
    }
}
