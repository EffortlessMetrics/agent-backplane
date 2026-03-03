// SPDX-License-Identifier: MIT OR Apache-2.0
//! Anthropic Claude Messages API type definitions.
//!
//! Mirrors the Anthropic Messages API request/response surface.
//! See <https://docs.anthropic.com/en/api/messages>.

use serde::{Deserialize, Serialize};

// ── Message types ───────────────────────────────────────────────────────

/// A single message in the Claude conversation format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ClaudeMessage {
    /// Message role (`user` or `assistant`).
    pub role: String,
    /// Text content of the message.
    pub content: String,
}

/// A content block in a Claude response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaudeContentBlock {
    /// A text content block.
    Text {
        /// The text content.
        text: String,
    },
    /// A tool use request from the assistant.
    ToolUse {
        /// Unique tool use identifier.
        id: String,
        /// Name of the tool to invoke.
        name: String,
        /// JSON input for the tool.
        input: serde_json::Value,
    },
    /// A tool result returned to the model.
    ToolResult {
        /// ID of the tool use this result corresponds to.
        tool_use_id: String,
        /// Text content of the tool result.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        /// Whether the tool execution produced an error.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
    /// An extended thinking block.
    Thinking {
        /// The model's internal reasoning text.
        thinking: String,
        /// Cryptographic signature for thinking verification.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
}

// ── Tool types ──────────────────────────────────────────────────────────

/// Anthropic-style tool definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ClaudeToolDef {
    /// Tool name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the tool's input.
    pub input_schema: serde_json::Value,
}

/// Configuration for Claude's extended thinking feature.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ThinkingConfig {
    /// Discriminator (always `"enabled"`).
    #[serde(rename = "type")]
    pub thinking_type: String,
    /// Maximum tokens for internal reasoning.
    pub budget_tokens: u32,
}

// ── Request ─────────────────────────────────────────────────────────────

/// Anthropic Claude Messages API request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ClaudeRequest {
    /// Model identifier (e.g. `claude-sonnet-4-20250514`).
    pub model: String,
    /// Maximum tokens to generate.
    pub max_tokens: u32,
    /// Optional system prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Conversation messages.
    pub messages: Vec<ClaudeMessage>,
    /// Tool definitions available to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ClaudeToolDef>>,
    /// Extended thinking configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    /// Whether to stream the response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

// ── Response ────────────────────────────────────────────────────────────

/// Anthropic Claude Messages API response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ClaudeResponse {
    /// Unique message identifier.
    pub id: String,
    /// Model that generated the response.
    pub model: String,
    /// Role of the response (always `assistant`).
    pub role: String,
    /// Content blocks in the response.
    pub content: Vec<ClaudeContentBlock>,
    /// Reason the model stopped generating.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    /// Token usage statistics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ClaudeUsage>,
}

/// Token usage reported by the Anthropic API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ClaudeUsage {
    /// Number of input tokens consumed.
    pub input_tokens: u64,
    /// Number of output tokens generated.
    pub output_tokens: u64,
    /// Tokens written to the prompt cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    /// Tokens read from the prompt cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
}

// ── Streaming ───────────────────────────────────────────────────────────

/// Server-sent event types from the Anthropic streaming API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaudeStreamEvent {
    /// Initial message metadata at stream start.
    MessageStart {
        /// The initial (incomplete) response object.
        message: ClaudeResponse,
    },
    /// A new content block begins.
    ContentBlockStart {
        /// Zero-based index of the content block.
        index: u32,
        /// The initial content block.
        content_block: ClaudeContentBlock,
    },
    /// Incremental update to a content block.
    ContentBlockDelta {
        /// Index of the content block being updated.
        index: u32,
        /// The incremental delta payload.
        delta: ClaudeStreamDelta,
    },
    /// A content block has finished.
    ContentBlockStop {
        /// Index of the completed content block.
        index: u32,
    },
    /// Top-level message metadata update.
    MessageDelta {
        /// The message-level delta.
        delta: ClaudeMessageDelta,
        /// Updated usage statistics.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<ClaudeUsage>,
    },
    /// The message stream has ended.
    MessageStop {},
    /// Keep-alive ping event.
    Ping {},
    /// An error occurred during streaming.
    Error {
        /// The error details.
        error: ClaudeApiError,
    },
}

/// Delta payload within a `content_block_delta` streaming event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaudeStreamDelta {
    /// Incremental text output.
    TextDelta {
        /// The text fragment.
        text: String,
    },
    /// Incremental JSON for tool input.
    InputJsonDelta {
        /// Partial JSON string.
        partial_json: String,
    },
    /// Incremental thinking text.
    ThinkingDelta {
        /// The thinking fragment.
        thinking: String,
    },
    /// Incremental signature data.
    SignatureDelta {
        /// The signature fragment.
        signature: String,
    },
}

/// Delta payload within a `message_delta` streaming event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ClaudeMessageDelta {
    /// Reason the model stopped generating.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    /// Stop sequence that triggered the stop, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
}

/// Error object returned by the Anthropic API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ClaudeApiError {
    /// Error type identifier.
    #[serde(rename = "type")]
    pub error_type: String,
    /// Human-readable error message.
    pub message: String,
}

// ── Model config ────────────────────────────────────────────────────────

/// Vendor-specific configuration for the Anthropic Claude API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ClaudeConfig {
    /// Base URL for the Messages API.
    pub base_url: String,
    /// Model identifier (e.g. `claude-sonnet-4-20250514`).
    pub model: String,
    /// Maximum tokens to generate.
    pub max_tokens: u32,
    /// System prompt override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Extended thinking configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.anthropic.com/v1".into(),
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 4096,
            system_prompt: None,
            thinking: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serde_roundtrip() {
        let req = ClaudeRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 4096,
            system: Some("You are helpful.".into()),
            messages: vec![ClaudeMessage {
                role: "user".into(),
                content: "Hello".into(),
            }],
            tools: Some(vec![ClaudeToolDef {
                name: "read_file".into(),
                description: "Read a file".into(),
                input_schema: serde_json::json!({"type": "object"}),
            }]),
            thinking: None,
            stream: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: ClaudeRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn response_serde_roundtrip() {
        let resp = ClaudeResponse {
            id: "msg_123".into(),
            model: "claude-sonnet-4-20250514".into(),
            role: "assistant".into(),
            content: vec![
                ClaudeContentBlock::Text {
                    text: "Hello!".into(),
                },
                ClaudeContentBlock::ToolUse {
                    id: "tu_1".into(),
                    name: "read_file".into(),
                    input: serde_json::json!({"path": "src/main.rs"}),
                },
            ],
            stop_reason: Some("end_turn".into()),
            usage: Some(ClaudeUsage {
                input_tokens: 100,
                output_tokens: 50,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: ClaudeResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn stream_event_text_delta_roundtrip() {
        let event = ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta {
                text: "Hello".into(),
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }

    #[test]
    fn content_block_thinking_roundtrip() {
        let block = ClaudeContentBlock::Thinking {
            thinking: "Let me think...".into(),
            signature: Some("sig123".into()),
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn config_default_values() {
        let cfg = ClaudeConfig::default();
        assert!(cfg.base_url.contains("anthropic.com"));
        assert!(cfg.model.contains("claude"));
        assert!(cfg.max_tokens > 0);
    }
}
