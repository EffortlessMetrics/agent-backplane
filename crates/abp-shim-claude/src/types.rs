// SPDX-License-Identifier: MIT OR Apache-2.0
//! Claude Messages API request/response types.
//!
//! These types mirror the **real** Anthropic Messages API JSON format,
//! suitable for serializing requests to `POST /v1/messages` and
//! deserializing responses (both synchronous and streamed SSE).

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// Request body for `POST /v1/messages`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagesRequest {
    /// Model identifier (e.g. `"claude-sonnet-4-20250514"`).
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<ClaudeMessage>,
    /// Maximum number of tokens to generate (required by the API).
    pub max_tokens: u32,
    /// Optional system prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Sampling temperature (0.0–1.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Nucleus-sampling probability mass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Top-K sampling parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Whether to stream the response via SSE.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Tool definitions available to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ClaudeTool>>,
    /// How the model should choose which tool to use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ClaudeToolChoice>,
}

/// A single message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClaudeMessage {
    /// `"user"` or `"assistant"`.
    pub role: String,
    /// Message content — either a plain string or an array of content blocks.
    pub content: ClaudeContent,
}

/// Message content — the Claude API accepts either a bare string or an array
/// of typed content blocks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ClaudeContent {
    /// Simple text string.
    Text(String),
    /// Array of structured content blocks.
    Blocks(Vec<ContentBlock>),
}

/// A typed content block within a message or response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text.
    Text {
        /// The text payload.
        text: String,
    },
    /// Base64 or URL image.
    Image {
        /// Image source details.
        source: ImageSource,
    },
    /// A tool invocation produced by the model.
    ToolUse {
        /// Unique tool-use identifier.
        id: String,
        /// Tool name.
        name: String,
        /// Arbitrary JSON input for the tool.
        input: serde_json::Value,
    },
    /// Result of a prior tool invocation, sent by the user.
    ToolResult {
        /// The `id` of the corresponding `tool_use` block.
        tool_use_id: String,
        /// Textual result content.
        content: String,
    },
}

/// Image source for an `Image` content block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    /// Base64-encoded image data.
    Base64 {
        /// MIME type (e.g. `"image/png"`).
        media_type: String,
        /// Base64-encoded bytes.
        data: String,
    },
    /// URL-referenced image.
    Url {
        /// The image URL.
        url: String,
    },
}

/// A tool definition exposed to the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClaudeTool {
    /// Tool name.
    pub name: String,
    /// Human-readable description of the tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema describing the tool's input parameters.
    pub input_schema: serde_json::Value,
}

/// Controls how the model selects a tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaudeToolChoice {
    /// Model decides whether to use a tool.
    Auto {},
    /// Model must use *some* tool.
    Any {},
    /// Model must use the named tool.
    Tool {
        /// Required tool name.
        name: String,
    },
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Synchronous response from `POST /v1/messages`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessagesResponse {
    /// Unique message identifier (e.g. `"msg_01XFDUDYJgAACzvnptvVoYEL"`).
    pub id: String,
    /// Always `"message"`.
    #[serde(rename = "type")]
    pub type_field: String,
    /// Always `"assistant"`.
    pub role: String,
    /// Response content blocks.
    pub content: Vec<ContentBlock>,
    /// Model that produced the response.
    pub model: String,
    /// Reason the model stopped generating.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    /// Token usage.
    pub usage: ClaudeUsage,
}

/// Token usage counters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClaudeUsage {
    /// Tokens consumed by the input.
    pub input_tokens: u64,
    /// Tokens generated in the output.
    pub output_tokens: u64,
    /// Tokens written to prompt cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    /// Tokens read from prompt cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
}

// ---------------------------------------------------------------------------
// Streaming (SSE) types
// ---------------------------------------------------------------------------

/// Server-sent event from the streaming Messages API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// Initial message metadata.
    MessageStart {
        /// Partial response with `id`, `model`, empty `content`, etc.
        message: MessagesResponse,
    },
    /// A new content block has begun.
    ContentBlockStart {
        /// Zero-based block index.
        index: u32,
        /// Initial (possibly empty) block.
        content_block: ContentBlock,
    },
    /// Incremental update to a content block.
    ContentBlockDelta {
        /// Block index.
        index: u32,
        /// The delta payload.
        delta: StreamDelta,
    },
    /// A content block has finished.
    ContentBlockStop {
        /// Block index.
        index: u32,
    },
    /// Message-level metadata update (stop reason, usage).
    MessageDelta {
        /// Delta payload.
        delta: MessageDeltaBody,
        /// Updated usage.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<ClaudeUsage>,
    },
    /// Stream has ended.
    MessageStop {},
    /// Keep-alive ping.
    Ping {},
}

/// Delta payload for `content_block_delta` events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamDelta {
    /// Incremental text fragment.
    TextDelta {
        /// Text fragment.
        text: String,
    },
    /// Incremental tool-input JSON.
    InputJsonDelta {
        /// Partial JSON string.
        partial_json: String,
    },
}

/// Body of a `message_delta` event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageDeltaBody {
    /// The stop reason, if the model has finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    /// Stop sequence that triggered the stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
}
