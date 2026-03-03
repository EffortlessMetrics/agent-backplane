// SPDX-License-Identifier: MIT OR Apache-2.0
//! OpenAI Chat Completions API type definitions.
//!
//! Mirrors the OpenAI Chat Completions request/response surface.
//! See <https://platform.openai.com/docs/api-reference/chat>.

use serde::{Deserialize, Serialize};

// ── Message types ───────────────────────────────────────────────────────

/// A single message in the OpenAI Chat Completions format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct OpenAiMessage {
    /// Message role (`system`, `user`, `assistant`, or `tool`).
    pub role: String,
    /// Text content of the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Tool calls requested by the assistant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAiToolCall>>,
    /// ID of the tool call this message is responding to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

// ── Tool types ──────────────────────────────────────────────────────────

/// OpenAI-style function tool definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct OpenAiToolDef {
    /// Tool type (always `"function"`).
    #[serde(rename = "type")]
    pub tool_type: String,
    /// The function definition payload.
    pub function: OpenAiFunctionDef,
}

/// The function payload inside an [`OpenAiToolDef`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct OpenAiFunctionDef {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

/// A tool call emitted by the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct OpenAiToolCall {
    /// Unique identifier for this tool call.
    pub id: String,
    /// Call type (always `"function"`).
    #[serde(rename = "type")]
    pub call_type: String,
    /// The function invocation details.
    pub function: OpenAiFunctionCall,
}

/// The function invocation inside an [`OpenAiToolCall`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct OpenAiFunctionCall {
    /// Name of the function to invoke.
    pub name: String,
    /// JSON-encoded arguments for the function.
    pub arguments: String,
}

/// Controls which (if any) tool the model should call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoiceMode {
    /// Model will not call any tool.
    None,
    /// Model decides whether to call a tool.
    Auto,
    /// Model must call at least one tool.
    Required,
}

/// A reference to a specific function in a [`ToolChoice::Function`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ToolChoiceFunctionRef {
    /// Name of the function to force.
    pub name: String,
}

// ── Response format ─────────────────────────────────────────────────────

/// Response format constraint for the OpenAI API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseFormat {
    /// Plain text (default).
    Text {},
    /// JSON object output.
    JsonObject {},
    /// JSON Schema-constrained output.
    JsonSchema {
        /// The JSON Schema definition.
        json_schema: serde_json::Value,
    },
}

// ── Request ─────────────────────────────────────────────────────────────

/// OpenAI Chat Completions API request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct OpenAiRequest {
    /// Model identifier (e.g. `gpt-4o`).
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<OpenAiMessage>,
    /// Tool definitions available to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<OpenAiToolDef>>,
    /// Controls which tool the model should call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// Sampling temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Maximum tokens to generate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Response format constraint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
    /// Whether to stream the response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

// ── Response ────────────────────────────────────────────────────────────

/// OpenAI Chat Completions API response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct OpenAiResponse {
    /// Unique response identifier.
    pub id: String,
    /// Object type (e.g. `chat.completion`).
    pub object: String,
    /// Model used for the completion.
    pub model: String,
    /// Completion choices.
    pub choices: Vec<OpenAiChoice>,
    /// Token usage statistics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<OpenAiUsage>,
}

/// A single choice in the Chat Completions response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct OpenAiChoice {
    /// Zero-based index of this choice.
    pub index: u32,
    /// The assistant's response message.
    pub message: OpenAiMessage,
    /// Reason the model stopped generating.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// Token usage reported by the OpenAI API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct OpenAiUsage {
    /// Tokens consumed by the prompt.
    pub prompt_tokens: u64,
    /// Tokens generated in the completion.
    pub completion_tokens: u64,
    /// Total tokens (prompt + completion).
    pub total_tokens: u64,
}

// ── Streaming ───────────────────────────────────────────────────────────

/// A streaming chunk from the OpenAI Chat Completions API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct OpenAiStreamChunk {
    /// Chunk identifier.
    pub id: String,
    /// Object type (`chat.completion.chunk`).
    pub object: String,
    /// Model that produced this chunk.
    pub model: String,
    /// Choices with streaming deltas.
    pub choices: Vec<OpenAiStreamChoice>,
    /// Usage info (only in the final chunk when requested).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<OpenAiUsage>,
}

/// A single choice within a streaming chunk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct OpenAiStreamChoice {
    /// Zero-based choice index.
    pub index: u32,
    /// The incremental delta for this choice.
    pub delta: OpenAiStreamDelta,
    /// Finish reason — `None` until the stream ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// An incremental delta within a streaming choice.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct OpenAiStreamDelta {
    /// Role (usually only in the first chunk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Text content fragment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Incremental tool call fragments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAiToolCall>>,
}

// ── Model config ────────────────────────────────────────────────────────

/// Vendor-specific configuration for the OpenAI Chat Completions API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct OpenAiConfig {
    /// Base URL for the Chat Completions API.
    pub base_url: String,
    /// Model identifier (e.g. `gpt-4o`).
    pub model: String,
    /// Maximum tokens to generate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Temperature for sampling (0.0–2.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-4o".into(),
            max_tokens: Some(4096),
            temperature: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serde_roundtrip() {
        let req = OpenAiRequest {
            model: "gpt-4o".into(),
            messages: vec![
                OpenAiMessage {
                    role: "system".into(),
                    content: Some("You are helpful.".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                OpenAiMessage {
                    role: "user".into(),
                    content: Some("Hello".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            tools: Some(vec![OpenAiToolDef {
                tool_type: "function".into(),
                function: OpenAiFunctionDef {
                    name: "read_file".into(),
                    description: "Read a file".into(),
                    parameters: serde_json::json!({"type": "object"}),
                },
            }]),
            tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Auto)),
            temperature: Some(0.7),
            max_tokens: Some(4096),
            response_format: None,
            stream: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: OpenAiRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn response_serde_roundtrip() {
        let resp = OpenAiResponse {
            id: "chatcmpl-123".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![OpenAiChoice {
                index: 0,
                message: OpenAiMessage {
                    role: "assistant".into(),
                    content: Some("Hi!".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(OpenAiUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: OpenAiResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn stream_chunk_serde_roundtrip() {
        let chunk = OpenAiStreamChunk {
            id: "chatcmpl-123".into(),
            object: "chat.completion.chunk".into(),
            model: "gpt-4o".into(),
            choices: vec![OpenAiStreamChoice {
                index: 0,
                delta: OpenAiStreamDelta {
                    role: Some("assistant".into()),
                    content: Some("Hi".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let back: OpenAiStreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(chunk, back);
    }

    #[test]
    fn tool_call_serde_roundtrip() {
        let tc = OpenAiToolCall {
            id: "call_abc".into(),
            call_type: "function".into(),
            function: OpenAiFunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path": "src/main.rs"}"#.into(),
            },
        };
        let json = serde_json::to_string(&tc).unwrap();
        let back: OpenAiToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(tc, back);
    }

    #[test]
    fn config_default_values() {
        let cfg = OpenAiConfig::default();
        assert!(cfg.base_url.contains("openai.com"));
        assert_eq!(cfg.model, "gpt-4o");
        assert!(cfg.max_tokens.unwrap_or(0) > 0);
    }
}
