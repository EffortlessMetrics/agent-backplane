// SPDX-License-Identifier: MIT OR Apache-2.0
//! Claude Messages API types for direct integration.
//!
//! These mirror the Anthropic Messages API surface so the bridge can construct
//! requests, parse responses, and process streaming events without depending
//! on external Claude SDK crates.

use serde::{Deserialize, Serialize};

// ── Roles ───────────────────────────────────────────────────────────────

/// Message role in the Claude Messages API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// User / human turn.
    User,
    /// Assistant / model turn.
    Assistant,
}

// ── Content blocks ──────────────────────────────────────────────────────

/// A content block in a Claude message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text content.
    Text {
        /// The text payload.
        text: String,
    },
    /// A tool invocation requested by the model.
    ToolUse {
        /// Unique identifier for this tool invocation.
        id: String,
        /// Tool name.
        name: String,
        /// JSON-encoded input arguments.
        input: serde_json::Value,
    },
    /// The result of a prior tool invocation.
    ToolResult {
        /// Identifier of the corresponding [`ContentBlock::ToolUse`].
        tool_use_id: String,
        /// Text content of the result.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        /// Whether the tool reported an error.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
    /// Extended thinking / chain-of-thought block.
    Thinking {
        /// The thinking text.
        thinking: String,
        /// Cryptographic signature for thinking verification.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    /// Image content.
    Image {
        /// Image source.
        source: ImageSource,
    },
}

/// Image source for [`ContentBlock::Image`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    /// Base64-encoded image data.
    Base64 {
        /// MIME type (e.g. `"image/png"`).
        media_type: String,
        /// Base64-encoded image bytes.
        data: String,
    },
    /// URL-referenced image.
    Url {
        /// The image URL.
        url: String,
    },
}

/// System block in a system message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SystemBlock {
    /// Text system block.
    Text {
        /// The text payload.
        text: String,
        /// Cache control configuration.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControl>,
    },
}

/// Cache control directive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheControl {
    /// Cache type (e.g. `"ephemeral"`).
    #[serde(rename = "type")]
    pub cache_type: String,
}

// ── Messages ────────────────────────────────────────────────────────────

/// Message content: either a list of blocks or a plain string.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// Structured content blocks.
    Blocks(Vec<ContentBlock>),
    /// Plain text shorthand.
    Text(String),
}

/// A single message in a conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// Message role.
    pub role: Role,
    /// Message content.
    pub content: MessageContent,
}

/// System message: either structured blocks or plain text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SystemMessage {
    /// Structured system blocks.
    Blocks(Vec<SystemBlock>),
    /// Plain text shorthand.
    Text(String),
}

// ── Tool definitions ────────────────────────────────────────────────────

/// A tool definition for the Claude Messages API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for input parameters.
    pub input_schema: serde_json::Value,
}

/// Tool choice configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolChoice {
    /// Let the model decide.
    Auto {},
    /// Force tool use.
    Any {},
    /// Force a specific tool.
    Tool {
        /// Name of the tool to force.
        name: String,
    },
}

/// Extended thinking configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThinkingConfig {
    /// Thinking type (e.g. `"enabled"`).
    #[serde(rename = "type")]
    pub thinking_type: String,
    /// Maximum budget for thinking tokens.
    pub budget_tokens: u32,
}

/// Request metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestMetadata {
    /// Optional user identifier for abuse detection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

// ── Request ─────────────────────────────────────────────────────────────

/// Claude Messages API request body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessagesRequest {
    /// Model identifier (e.g. `"claude-sonnet-4-20250514"`).
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<Message>,
    /// Maximum number of tokens to generate.
    pub max_tokens: u32,
    /// System message / instructions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<SystemMessage>,
    /// Available tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    /// Request metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<RequestMetadata>,
    /// Enable streaming.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Custom stop sequences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// Sampling temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Top-p (nucleus) sampling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Top-k sampling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Tool choice configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// Extended thinking configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
}

// ── Response ────────────────────────────────────────────────────────────

/// Claude Messages API response body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessagesResponse {
    /// Unique message identifier.
    pub id: String,
    /// Object type (always `"message"`).
    #[serde(rename = "type")]
    pub response_type: String,
    /// Response role (always `"assistant"`).
    pub role: String,
    /// Content blocks in the response.
    pub content: Vec<ContentBlock>,
    /// Model used for generation.
    pub model: String,
    /// Reason the model stopped generating.
    pub stop_reason: Option<String>,
    /// Stop sequence that triggered stopping, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    /// Token usage counters.
    pub usage: Usage,
}

/// Stop reason enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Natural end of turn.
    EndTurn,
    /// Hit the max_tokens limit.
    MaxTokens,
    /// Hit a custom stop sequence.
    StopSequence,
    /// Model wants to call a tool.
    ToolUse,
}

// ── Usage ───────────────────────────────────────────────────────────────

/// Token usage counters from the Claude API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Number of input (prompt) tokens.
    pub input_tokens: u64,
    /// Number of output (completion) tokens.
    pub output_tokens: u64,
    /// Tokens used for cache creation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    /// Tokens served from cache reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
}

// ── Streaming events ────────────────────────────────────────────────────

/// A Server-Sent Event from the Claude streaming API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// Start of a new message. Contains the initial (incomplete) response.
    MessageStart {
        /// The initial message object.
        message: MessagesResponse,
    },
    /// Start of a new content block.
    ContentBlockStart {
        /// Zero-based index of this content block.
        index: u32,
        /// The initial content block (may be partial).
        content_block: ContentBlock,
    },
    /// Incremental delta within a content block.
    ContentBlockDelta {
        /// Index of the content block being updated.
        index: u32,
        /// The delta payload.
        delta: StreamDelta,
    },
    /// End of a content block.
    ContentBlockStop {
        /// Index of the completed content block.
        index: u32,
    },
    /// Message-level delta (stop reason, final usage).
    MessageDelta {
        /// Message-level changes.
        delta: MessageDelta,
        /// Updated usage counters.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
    },
    /// End of the message stream.
    MessageStop {},
    /// Keep-alive ping.
    Ping {},
    /// An error during streaming.
    Error {
        /// The API error.
        error: ApiError,
    },
}

/// Delta within a content block during streaming.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamDelta {
    /// Incremental text.
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

/// Message-level delta during streaming.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageDelta {
    /// Stop reason, set at the end of generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    /// Stop sequence, if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
}

// ── API errors ──────────────────────────────────────────────────────────

/// Claude API error object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiError {
    /// Error type (e.g. `"invalid_request_error"`, `"overloaded_error"`).
    #[serde(rename = "type")]
    pub error_type: String,
    /// Human-readable error message.
    pub message: String,
}
