// SPDX-License-Identifier: MIT OR Apache-2.0
//! Expanded IR types for dialect→IR→dialect transformation.
//!
//! These types extend the core IR (`IrConversation`, `IrMessage`, etc.) with
//! request/response-level wrappers that carry system prompts, tool
//! definitions, generation config, and usage statistics — everything needed
//! for a complete round-trip through the translation layer.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

// ── Re-export core IR types ─────────────────────────────────────────────

// We define our own higher-level types here but the building blocks
// (IrRole, IrContentBlock, IrMessage, IrToolDefinition, IrUsage) live
// in abp-core::ir and are re-exported from abp-ir.  This module is
// self-contained so that abp-dialect can stay independent of abp-core.

// ── Roles ───────────────────────────────────────────────────────────────

/// Normalized message role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IrRole {
    /// System prompt / instructions.
    System,
    /// User / human turn.
    User,
    /// Assistant / model turn.
    Assistant,
    /// Tool result turn.
    Tool,
}

// ── Content blocks ──────────────────────────────────────────────────────

/// A single content block inside an [`IrMessage`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IrContentBlock {
    /// Plain text content.
    Text {
        /// The text payload.
        text: String,
    },
    /// Base64-encoded image data.
    Image {
        /// MIME type (e.g. `"image/png"`).
        media_type: String,
        /// Base64-encoded image bytes.
        data: String,
    },
    /// A tool invocation requested by the model.
    ToolCall {
        /// Unique identifier for this tool invocation.
        id: String,
        /// Tool name.
        name: String,
        /// JSON-encoded input arguments.
        input: Value,
    },
    /// The result of a prior tool invocation.
    ToolResult {
        /// Identifier of the corresponding [`IrContentBlock::ToolCall`].
        tool_call_id: String,
        /// Nested content blocks for the result payload.
        content: Vec<IrContentBlock>,
        /// Whether the tool reported an error.
        is_error: bool,
    },
    /// Extended thinking / chain-of-thought block.
    Thinking {
        /// The thinking text.
        text: String,
    },
    /// Audio content block.
    Audio {
        /// MIME type (e.g. `"audio/wav"`).
        media_type: String,
        /// Base64-encoded audio bytes.
        data: String,
    },
    /// Vendor-specific content block preserved opaquely.
    Custom {
        /// Vendor-specific type tag.
        custom_type: String,
        /// Opaque payload.
        data: Value,
    },
}

impl IrContentBlock {
    /// Returns the text payload if this is a [`IrContentBlock::Text`] block.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }

    /// Returns `true` if this block represents a tool call.
    #[must_use]
    pub fn is_tool_call(&self) -> bool {
        matches!(self, Self::ToolCall { .. })
    }

    /// Returns `true` if this block represents a tool result.
    #[must_use]
    pub fn is_tool_result(&self) -> bool {
        matches!(self, Self::ToolResult { .. })
    }
}

// ── Messages ────────────────────────────────────────────────────────────

/// A single normalized message in a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IrMessage {
    /// The role of the message author.
    pub role: IrRole,
    /// Ordered content blocks that make up the message body.
    pub content: Vec<IrContentBlock>,
    /// Optional vendor-opaque metadata carried through the pipeline.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}

impl IrMessage {
    /// Create a new message with the given role and content blocks.
    #[must_use]
    pub fn new(role: IrRole, content: Vec<IrContentBlock>) -> Self {
        Self {
            role,
            content,
            metadata: BTreeMap::new(),
        }
    }

    /// Create a simple text message.
    #[must_use]
    pub fn text(role: IrRole, text: impl Into<String>) -> Self {
        Self::new(role, vec![IrContentBlock::Text { text: text.into() }])
    }

    /// Concatenate all text blocks into a single string.
    #[must_use]
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| b.as_text())
            .collect::<Vec<_>>()
            .join("")
    }

    /// Return all tool-call blocks.
    #[must_use]
    pub fn tool_calls(&self) -> Vec<&IrContentBlock> {
        self.content.iter().filter(|b| b.is_tool_call()).collect()
    }
}

// ── Tool definitions ────────────────────────────────────────────────────

/// A canonical tool definition for cross-dialect normalization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IrToolDefinition {
    /// Unique tool name.
    pub name: String,
    /// Human-readable description of the tool.
    pub description: String,
    /// JSON Schema describing the input parameters.
    pub parameters: Value,
}

// ── Usage stats ─────────────────────────────────────────────────────────

/// Normalized token-usage counters across all dialects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IrUsage {
    /// Number of input (prompt) tokens.
    pub input_tokens: u64,
    /// Number of output (completion) tokens.
    pub output_tokens: u64,
    /// Sum of input + output tokens.
    pub total_tokens: u64,
    /// Tokens served from a KV-cache read.
    pub cache_read_tokens: u64,
    /// Tokens written into a KV-cache.
    pub cache_write_tokens: u64,
}

impl IrUsage {
    /// Construct usage from input/output counts, computing `total_tokens`.
    #[must_use]
    pub fn from_io(input: u64, output: u64) -> Self {
        Self {
            input_tokens: input,
            output_tokens: output,
            total_tokens: input + output,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        }
    }

    /// Merge two usage records by summing all fields.
    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        Self {
            input_tokens: self.input_tokens + other.input_tokens,
            output_tokens: self.output_tokens + other.output_tokens,
            total_tokens: self.total_tokens + other.total_tokens,
            cache_read_tokens: self.cache_read_tokens + other.cache_read_tokens,
            cache_write_tokens: self.cache_write_tokens + other.cache_write_tokens,
        }
    }
}

// ── Generation config ───────────────────────────────────────────────────

/// Normalized generation parameters across all dialects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct IrGenerationConfig {
    /// Maximum number of tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    /// Sampling temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Nucleus sampling parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Top-k sampling parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Stop sequences.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop_sequences: Vec<String>,
    /// Extra vendor-specific parameters.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, Value>,
}

// ── IrRequest ───────────────────────────────────────────────────────────

/// A normalized request that can be lowered to any dialect.
///
/// Captures the full request surface area: system prompt, messages,
/// tool definitions, and generation config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrRequest {
    /// Optional model identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Optional system prompt (extracted from messages where applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Conversation messages.
    pub messages: Vec<IrMessage>,
    /// Tool definitions available for this request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<IrToolDefinition>,
    /// Generation configuration.
    #[serde(default)]
    pub config: IrGenerationConfig,
    /// Vendor-opaque metadata carried through the pipeline.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}

impl IrRequest {
    /// Create a minimal request with just messages.
    #[must_use]
    pub fn new(messages: Vec<IrMessage>) -> Self {
        Self {
            model: None,
            system_prompt: None,
            messages,
            tools: Vec::new(),
            config: IrGenerationConfig::default(),
            metadata: BTreeMap::new(),
        }
    }

    /// Builder: set the model.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Builder: set the system prompt.
    #[must_use]
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Builder: add a tool definition.
    #[must_use]
    pub fn with_tool(mut self, tool: IrToolDefinition) -> Self {
        self.tools.push(tool);
        self
    }

    /// Builder: set generation config.
    #[must_use]
    pub fn with_config(mut self, config: IrGenerationConfig) -> Self {
        self.config = config;
        self
    }

    /// Return the system message if one exists in `messages`.
    #[must_use]
    pub fn system_message(&self) -> Option<&IrMessage> {
        self.messages.iter().find(|m| m.role == IrRole::System)
    }

    /// Return all tool calls across all messages.
    #[must_use]
    pub fn all_tool_calls(&self) -> Vec<&IrContentBlock> {
        self.messages.iter().flat_map(|m| m.tool_calls()).collect()
    }
}

// ── IrResponse ──────────────────────────────────────────────────────────

/// Reason the model stopped generating.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IrStopReason {
    /// Normal end of turn.
    EndTurn,
    /// Hit a stop sequence.
    StopSequence,
    /// Reached the maximum token limit.
    MaxTokens,
    /// Model is requesting tool use.
    ToolUse,
    /// Content was filtered.
    ContentFilter,
    /// Unknown / vendor-specific reason.
    Other(String),
}

/// A normalized response from any dialect.
///
/// Captures content blocks, usage statistics, and stop reason in a
/// vendor-neutral form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrResponse {
    /// Optional response identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Optional model identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Content blocks produced by the model.
    pub content: Vec<IrContentBlock>,
    /// Reason the model stopped generating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<IrStopReason>,
    /// Token usage statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<IrUsage>,
    /// Vendor-opaque metadata.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}

impl IrResponse {
    /// Create a minimal response with content blocks.
    #[must_use]
    pub fn new(content: Vec<IrContentBlock>) -> Self {
        Self {
            id: None,
            model: None,
            content,
            stop_reason: None,
            usage: None,
            metadata: BTreeMap::new(),
        }
    }

    /// Create a simple text response.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::new(vec![IrContentBlock::Text { text: text.into() }])
    }

    /// Builder: set the response id.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Builder: set the model.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Builder: set the stop reason.
    #[must_use]
    pub fn with_stop_reason(mut self, reason: IrStopReason) -> Self {
        self.stop_reason = Some(reason);
        self
    }

    /// Builder: set usage stats.
    #[must_use]
    pub fn with_usage(mut self, usage: IrUsage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Concatenate all text blocks.
    #[must_use]
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| b.as_text())
            .collect::<Vec<_>>()
            .join("")
    }

    /// Return all tool-call blocks.
    #[must_use]
    pub fn tool_calls(&self) -> Vec<&IrContentBlock> {
        self.content.iter().filter(|b| b.is_tool_call()).collect()
    }

    /// Returns `true` if the model is requesting tool use.
    #[must_use]
    pub fn has_tool_calls(&self) -> bool {
        self.content.iter().any(|b| b.is_tool_call())
    }

    /// Builder: add a metadata entry.
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

// ── Standalone tool call ────────────────────────────────────────────────

/// A normalized tool invocation extracted from content blocks.
///
/// This is a convenience type for working with tool calls outside of the
/// content-block hierarchy — e.g. when routing calls through a policy
/// engine or logging them independently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IrToolCall {
    /// Unique identifier for this tool invocation.
    pub id: String,
    /// Tool name.
    pub name: String,
    /// JSON-encoded input arguments.
    pub arguments: Value,
}

impl IrToolCall {
    /// Create a new tool call.
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>, arguments: Value) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments,
        }
    }

    /// Convert this tool call into an [`IrContentBlock::ToolCall`].
    #[must_use]
    pub fn into_content_block(self) -> IrContentBlock {
        IrContentBlock::ToolCall {
            id: self.id,
            name: self.name,
            input: self.arguments,
        }
    }

    /// Try to extract a tool call from a content block.
    #[must_use]
    pub fn from_content_block(block: &IrContentBlock) -> Option<Self> {
        match block {
            IrContentBlock::ToolCall { id, name, input } => Some(Self {
                id: id.clone(),
                name: name.clone(),
                arguments: input.clone(),
            }),
            _ => None,
        }
    }
}

// ── Standalone tool result ──────────────────────────────────────────────

/// A normalized tool result extracted from content blocks.
///
/// Mirrors [`IrContentBlock::ToolResult`] as a standalone struct for
/// ergonomic construction and pattern matching.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IrToolResult {
    /// Identifier of the corresponding tool call.
    pub tool_call_id: String,
    /// Nested content blocks for the result payload.
    pub content: Vec<IrContentBlock>,
    /// Whether the tool reported an error.
    pub is_error: bool,
}

impl IrToolResult {
    /// Create a successful tool result with text content.
    #[must_use]
    pub fn text(tool_call_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            content: vec![IrContentBlock::Text { text: text.into() }],
            is_error: false,
        }
    }

    /// Create an error tool result with text content.
    #[must_use]
    pub fn error(tool_call_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            content: vec![IrContentBlock::Text { text: text.into() }],
            is_error: true,
        }
    }

    /// Create a tool result with arbitrary content blocks.
    #[must_use]
    pub fn new(
        tool_call_id: impl Into<String>,
        content: Vec<IrContentBlock>,
        is_error: bool,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            content,
            is_error,
        }
    }

    /// Convert this tool result into an [`IrContentBlock::ToolResult`].
    #[must_use]
    pub fn into_content_block(self) -> IrContentBlock {
        IrContentBlock::ToolResult {
            tool_call_id: self.tool_call_id,
            content: self.content,
            is_error: self.is_error,
        }
    }

    /// Try to extract a tool result from a content block.
    #[must_use]
    pub fn from_content_block(block: &IrContentBlock) -> Option<Self> {
        match block {
            IrContentBlock::ToolResult {
                tool_call_id,
                content,
                is_error,
            } => Some(Self {
                tool_call_id: tool_call_id.clone(),
                content: content.clone(),
                is_error: *is_error,
            }),
            _ => None,
        }
    }
}

// ── Config alias ────────────────────────────────────────────────────────

/// Alias for [`IrGenerationConfig`] for brevity in dialect adapters.
pub type IrConfig = IrGenerationConfig;

// ── IrGenerationConfig builder ──────────────────────────────────────────

impl IrGenerationConfig {
    /// Builder: set max_tokens.
    #[must_use]
    pub fn with_max_tokens(mut self, max_tokens: u64) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Builder: set temperature.
    #[must_use]
    pub fn with_temperature(mut self, temperature: f64) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Builder: set top_p.
    #[must_use]
    pub fn with_top_p(mut self, top_p: f64) -> Self {
        self.top_p = Some(top_p);
        self
    }

    /// Builder: set top_k.
    #[must_use]
    pub fn with_top_k(mut self, top_k: u32) -> Self {
        self.top_k = Some(top_k);
        self
    }

    /// Builder: add a stop sequence.
    #[must_use]
    pub fn with_stop_sequence(mut self, seq: impl Into<String>) -> Self {
        self.stop_sequences.push(seq.into());
        self
    }

    /// Builder: add a vendor-specific extra parameter.
    #[must_use]
    pub fn with_extra(mut self, key: impl Into<String>, value: Value) -> Self {
        self.extra.insert(key.into(), value);
        self
    }
}

// ── IrToolDefinition builder ────────────────────────────────────────────

impl IrToolDefinition {
    /// Create a new tool definition.
    #[must_use]
    pub fn new(name: impl Into<String>, description: impl Into<String>, parameters: Value) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }
}

// ── IrMessage builder ───────────────────────────────────────────────────

impl IrMessage {
    /// Builder: add a metadata entry.
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    /// Builder: append a content block.
    #[must_use]
    pub fn with_block(mut self, block: IrContentBlock) -> Self {
        self.content.push(block);
        self
    }

    /// Returns `true` if every content block is text.
    #[must_use]
    pub fn is_text_only(&self) -> bool {
        self.content
            .iter()
            .all(|b| matches!(b, IrContentBlock::Text { .. }))
    }

    /// Returns `true` if this message has any tool call blocks.
    #[must_use]
    pub fn has_tool_calls(&self) -> bool {
        self.content.iter().any(|b| b.is_tool_call())
    }

    /// Extract standalone [`IrToolCall`] values from this message.
    #[must_use]
    pub fn extract_tool_calls(&self) -> Vec<IrToolCall> {
        self.content
            .iter()
            .filter_map(IrToolCall::from_content_block)
            .collect()
    }
}

// ── IrRequest builder extras ────────────────────────────────────────────

impl IrRequest {
    /// Builder: add a message.
    #[must_use]
    pub fn with_message(mut self, message: IrMessage) -> Self {
        self.messages.push(message);
        self
    }

    /// Builder: add a metadata entry.
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    /// Builder: set tools from a vec.
    #[must_use]
    pub fn with_tools(mut self, tools: Vec<IrToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    /// Extract all standalone [`IrToolCall`] values across all messages.
    #[must_use]
    pub fn extract_all_tool_calls(&self) -> Vec<IrToolCall> {
        self.messages
            .iter()
            .flat_map(|m| m.extract_tool_calls())
            .collect()
    }
}

// ── Streaming events ────────────────────────────────────────────────────

/// Normalized streaming event across all dialects.
///
/// Maps the various vendor streaming chunk formats into a single enum
/// that the runtime can process uniformly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IrStreamEvent {
    /// Stream has started; may carry the response id and model.
    StreamStart {
        /// Optional response identifier.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Optional model identifier.
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },

    /// A new content block has started (text, tool_call, etc.).
    ContentBlockStart {
        /// Zero-based index of the content block within the response.
        index: usize,
        /// The initial content block (may be empty / partial).
        block: IrContentBlock,
    },

    /// An incremental text delta for an in-progress content block.
    TextDelta {
        /// Index of the content block this delta applies to.
        index: usize,
        /// The text fragment.
        text: String,
    },

    /// An incremental JSON delta for a tool-call's arguments.
    ToolCallDelta {
        /// Index of the content block this delta applies to.
        index: usize,
        /// Partial JSON string to append to the arguments buffer.
        arguments_delta: String,
    },

    /// A thinking / chain-of-thought delta.
    ThinkingDelta {
        /// Index of the content block this delta applies to.
        index: usize,
        /// The thinking text fragment.
        text: String,
    },

    /// A content block has finished.
    ContentBlockStop {
        /// Index of the completed content block.
        index: usize,
    },

    /// Usage statistics update (may arrive mid-stream or at end).
    Usage {
        /// Token usage counters.
        usage: IrUsage,
    },

    /// The stream has ended.
    StreamEnd {
        /// Reason the model stopped generating.
        #[serde(skip_serializing_if = "Option::is_none")]
        stop_reason: Option<IrStopReason>,
    },

    /// An error occurred during streaming.
    Error {
        /// Machine-readable error code.
        code: String,
        /// Human-readable error message.
        message: String,
    },
}

impl IrStreamEvent {
    /// Create a stream-start event.
    #[must_use]
    pub fn stream_start() -> Self {
        Self::StreamStart {
            id: None,
            model: None,
        }
    }

    /// Create a text-delta event.
    #[must_use]
    pub fn text_delta(index: usize, text: impl Into<String>) -> Self {
        Self::TextDelta {
            index,
            text: text.into(),
        }
    }

    /// Create a tool-call argument delta event.
    #[must_use]
    pub fn tool_call_delta(index: usize, arguments_delta: impl Into<String>) -> Self {
        Self::ToolCallDelta {
            index,
            arguments_delta: arguments_delta.into(),
        }
    }

    /// Create a stream-end event.
    #[must_use]
    pub fn stream_end(stop_reason: Option<IrStopReason>) -> Self {
        Self::StreamEnd { stop_reason }
    }

    /// Create an error event.
    #[must_use]
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Error {
            code: code.into(),
            message: message.into(),
        }
    }

    /// Returns `true` if this is a terminal event (stream end or error).
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::StreamEnd { .. } | Self::Error { .. })
    }

    /// Returns the text content if this is a text delta.
    #[must_use]
    pub fn as_text_delta(&self) -> Option<&str> {
        match self {
            Self::TextDelta { text, .. } => Some(text),
            _ => None,
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── IrRole ──────────────────────────────────────────────────────

    #[test]
    fn role_serde_roundtrip() {
        for role in [
            IrRole::System,
            IrRole::User,
            IrRole::Assistant,
            IrRole::Tool,
        ] {
            let json = serde_json::to_string(&role).unwrap();
            let back: IrRole = serde_json::from_str(&json).unwrap();
            assert_eq!(role, back);
        }
    }

    #[test]
    fn role_serde_rename() {
        assert_eq!(
            serde_json::to_string(&IrRole::System).unwrap(),
            "\"system\""
        );
        assert_eq!(serde_json::to_string(&IrRole::User).unwrap(), "\"user\"");
        assert_eq!(
            serde_json::to_string(&IrRole::Assistant).unwrap(),
            "\"assistant\""
        );
        assert_eq!(serde_json::to_string(&IrRole::Tool).unwrap(), "\"tool\"");
    }

    // ── IrContentBlock ──────────────────────────────────────────────

    #[test]
    fn content_block_text_roundtrip() {
        let block = IrContentBlock::Text {
            text: "hello".into(),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "text");
        assert_eq!(json["text"], "hello");
        let back: IrContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn content_block_image_roundtrip() {
        let block = IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "abc123==".into(),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "image");
        let back: IrContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn content_block_tool_call_roundtrip() {
        let block = IrContentBlock::ToolCall {
            id: "tc_1".into(),
            name: "read_file".into(),
            input: json!({"path": "/tmp/a.txt"}),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "tool_call");
        let back: IrContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn content_block_tool_result_roundtrip() {
        let block = IrContentBlock::ToolResult {
            tool_call_id: "tc_1".into(),
            content: vec![IrContentBlock::Text {
                text: "file contents".into(),
            }],
            is_error: false,
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "tool_result");
        let back: IrContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn content_block_thinking_roundtrip() {
        let block = IrContentBlock::Thinking {
            text: "let me think...".into(),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "thinking");
        let back: IrContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn content_block_audio_roundtrip() {
        let block = IrContentBlock::Audio {
            media_type: "audio/wav".into(),
            data: "AAAA".into(),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "audio");
        let back: IrContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn content_block_custom_roundtrip() {
        let block = IrContentBlock::Custom {
            custom_type: "vendor_x".into(),
            data: json!({"key": "value"}),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "custom");
        let back: IrContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn content_block_as_text() {
        let text_block = IrContentBlock::Text { text: "hi".into() };
        assert_eq!(text_block.as_text(), Some("hi"));

        let img = IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "x".into(),
        };
        assert_eq!(img.as_text(), None);
    }

    #[test]
    fn content_block_is_tool_call() {
        let tc = IrContentBlock::ToolCall {
            id: "x".into(),
            name: "y".into(),
            input: json!({}),
        };
        assert!(tc.is_tool_call());
        assert!(!tc.is_tool_result());
    }

    #[test]
    fn content_block_is_tool_result() {
        let tr = IrContentBlock::ToolResult {
            tool_call_id: "x".into(),
            content: vec![],
            is_error: false,
        };
        assert!(tr.is_tool_result());
        assert!(!tr.is_tool_call());
    }

    // ── IrMessage ───────────────────────────────────────────────────

    #[test]
    fn message_text_constructor() {
        let msg = IrMessage::text(IrRole::User, "hello");
        assert_eq!(msg.role, IrRole::User);
        assert_eq!(msg.text_content(), "hello");
        assert!(msg.is_text_only());
    }

    #[test]
    fn message_serde_roundtrip() {
        let msg = IrMessage::text(IrRole::Assistant, "response");
        let json = serde_json::to_value(&msg).unwrap();
        let back: IrMessage = serde_json::from_value(json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn message_metadata_skipped_when_empty() {
        let msg = IrMessage::text(IrRole::User, "hi");
        let json = serde_json::to_value(&msg).unwrap();
        assert!(json.get("metadata").is_none());
    }

    #[test]
    fn message_with_metadata() {
        let msg = IrMessage::text(IrRole::User, "hi").with_metadata("source", json!("test"));
        assert_eq!(msg.metadata.get("source"), Some(&json!("test")));
        let json = serde_json::to_value(&msg).unwrap();
        assert!(json.get("metadata").is_some());
    }

    #[test]
    fn message_with_block_builder() {
        let msg = IrMessage::new(IrRole::Assistant, vec![])
            .with_block(IrContentBlock::Text { text: "a".into() })
            .with_block(IrContentBlock::Text { text: "b".into() });
        assert_eq!(msg.content.len(), 2);
        assert_eq!(msg.text_content(), "ab");
    }

    #[test]
    fn message_tool_calls() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "I'll help".into(),
                },
                IrContentBlock::ToolCall {
                    id: "tc_1".into(),
                    name: "read_file".into(),
                    input: json!({}),
                },
            ],
        );
        assert_eq!(msg.tool_calls().len(), 1);
        assert!(msg.has_tool_calls());
        assert!(!msg.is_text_only());
    }

    #[test]
    fn message_extract_tool_calls() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text { text: "x".into() },
                IrContentBlock::ToolCall {
                    id: "tc_1".into(),
                    name: "search".into(),
                    input: json!({"q": "rust"}),
                },
                IrContentBlock::ToolCall {
                    id: "tc_2".into(),
                    name: "read".into(),
                    input: json!({"path": "a.rs"}),
                },
            ],
        );
        let calls = msg.extract_tool_calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "search");
        assert_eq!(calls[1].name, "read");
    }

    #[test]
    fn message_text_content_concatenation() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "hello ".into(),
                },
                IrContentBlock::Thinking { text: "hmm".into() },
                IrContentBlock::Text {
                    text: "world".into(),
                },
            ],
        );
        assert_eq!(msg.text_content(), "hello world");
    }

    // ── IrToolDefinition ────────────────────────────────────────────

    #[test]
    fn tool_definition_new() {
        let tool = IrToolDefinition::new(
            "read_file",
            "Read a file from disk",
            json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        );
        assert_eq!(tool.name, "read_file");
        assert_eq!(tool.description, "Read a file from disk");
    }

    #[test]
    fn tool_definition_serde_roundtrip() {
        let tool = IrToolDefinition::new("write", "Write file", json!({}));
        let json = serde_json::to_value(&tool).unwrap();
        let back: IrToolDefinition = serde_json::from_value(json).unwrap();
        assert_eq!(tool, back);
    }

    // ── IrToolCall ──────────────────────────────────────────────────

    #[test]
    fn tool_call_new() {
        let tc = IrToolCall::new("tc_1", "search", json!({"query": "rust"}));
        assert_eq!(tc.id, "tc_1");
        assert_eq!(tc.name, "search");
        assert_eq!(tc.arguments, json!({"query": "rust"}));
    }

    #[test]
    fn tool_call_serde_roundtrip() {
        let tc = IrToolCall::new("tc_1", "search", json!({"q": "x"}));
        let json = serde_json::to_value(&tc).unwrap();
        let back: IrToolCall = serde_json::from_value(json).unwrap();
        assert_eq!(tc, back);
    }

    #[test]
    fn tool_call_into_content_block() {
        let tc = IrToolCall::new("tc_1", "search", json!({"q": "x"}));
        let block = tc.into_content_block();
        assert!(block.is_tool_call());
        match &block {
            IrContentBlock::ToolCall { id, name, input } => {
                assert_eq!(id, "tc_1");
                assert_eq!(name, "search");
                assert_eq!(input, &json!({"q": "x"}));
            }
            _ => panic!("expected ToolCall"),
        }
    }

    #[test]
    fn tool_call_from_content_block() {
        let block = IrContentBlock::ToolCall {
            id: "tc_2".into(),
            name: "write".into(),
            input: json!({"data": "hello"}),
        };
        let tc = IrToolCall::from_content_block(&block).unwrap();
        assert_eq!(tc.id, "tc_2");
        assert_eq!(tc.name, "write");
    }

    #[test]
    fn tool_call_from_non_tool_block_is_none() {
        let block = IrContentBlock::Text { text: "hi".into() };
        assert!(IrToolCall::from_content_block(&block).is_none());
    }

    // ── IrToolResult ────────────────────────────────────────────────

    #[test]
    fn tool_result_text() {
        let tr = IrToolResult::text("tc_1", "file contents here");
        assert_eq!(tr.tool_call_id, "tc_1");
        assert!(!tr.is_error);
        assert_eq!(tr.content.len(), 1);
    }

    #[test]
    fn tool_result_error() {
        let tr = IrToolResult::error("tc_1", "file not found");
        assert!(tr.is_error);
    }

    #[test]
    fn tool_result_serde_roundtrip() {
        let tr = IrToolResult::text("tc_1", "ok");
        let json = serde_json::to_value(&tr).unwrap();
        let back: IrToolResult = serde_json::from_value(json).unwrap();
        assert_eq!(tr, back);
    }

    #[test]
    fn tool_result_into_content_block() {
        let tr = IrToolResult::text("tc_1", "data");
        let block = tr.into_content_block();
        assert!(block.is_tool_result());
    }

    #[test]
    fn tool_result_from_content_block() {
        let block = IrContentBlock::ToolResult {
            tool_call_id: "tc_1".into(),
            content: vec![IrContentBlock::Text { text: "ok".into() }],
            is_error: false,
        };
        let tr = IrToolResult::from_content_block(&block).unwrap();
        assert_eq!(tr.tool_call_id, "tc_1");
        assert!(!tr.is_error);
    }

    #[test]
    fn tool_result_from_non_result_block_is_none() {
        let block = IrContentBlock::Text { text: "hi".into() };
        assert!(IrToolResult::from_content_block(&block).is_none());
    }

    #[test]
    fn tool_result_new_with_blocks() {
        let tr = IrToolResult::new(
            "tc_1",
            vec![
                IrContentBlock::Text {
                    text: "line 1".into(),
                },
                IrContentBlock::Text {
                    text: "line 2".into(),
                },
            ],
            false,
        );
        assert_eq!(tr.content.len(), 2);
    }

    // ── IrUsage ─────────────────────────────────────────────────────

    #[test]
    fn usage_from_io() {
        let u = IrUsage::from_io(100, 50);
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 50);
        assert_eq!(u.total_tokens, 150);
        assert_eq!(u.cache_read_tokens, 0);
    }

    #[test]
    fn usage_merge() {
        let a = IrUsage::from_io(100, 50);
        let b = IrUsage::from_io(200, 80);
        let c = a.merge(b);
        assert_eq!(c.input_tokens, 300);
        assert_eq!(c.output_tokens, 130);
        assert_eq!(c.total_tokens, 430);
    }

    #[test]
    fn usage_serde_roundtrip() {
        let u = IrUsage::from_io(10, 20);
        let json = serde_json::to_value(&u).unwrap();
        let back: IrUsage = serde_json::from_value(json).unwrap();
        assert_eq!(u, back);
    }

    #[test]
    fn usage_default_is_zero() {
        let u = IrUsage::default();
        assert_eq!(u.input_tokens, 0);
        assert_eq!(u.output_tokens, 0);
        assert_eq!(u.total_tokens, 0);
    }

    // ── IrGenerationConfig / IrConfig ───────────────────────────────

    #[test]
    fn config_default_is_empty() {
        let cfg = IrGenerationConfig::default();
        assert!(cfg.max_tokens.is_none());
        assert!(cfg.temperature.is_none());
        assert!(cfg.top_p.is_none());
        assert!(cfg.top_k.is_none());
        assert!(cfg.stop_sequences.is_empty());
        assert!(cfg.extra.is_empty());
    }

    #[test]
    fn config_builder_chain() {
        let cfg = IrGenerationConfig::default()
            .with_max_tokens(4096)
            .with_temperature(0.7)
            .with_top_p(0.9)
            .with_top_k(40)
            .with_stop_sequence("END")
            .with_stop_sequence("STOP")
            .with_extra("seed", json!(42));

        assert_eq!(cfg.max_tokens, Some(4096));
        assert_eq!(cfg.temperature, Some(0.7));
        assert_eq!(cfg.top_p, Some(0.9));
        assert_eq!(cfg.top_k, Some(40));
        assert_eq!(cfg.stop_sequences, vec!["END", "STOP"]);
        assert_eq!(cfg.extra.get("seed"), Some(&json!(42)));
    }

    #[test]
    fn config_serde_roundtrip() {
        let cfg = IrGenerationConfig::default()
            .with_max_tokens(1024)
            .with_temperature(0.5);
        let json = serde_json::to_value(&cfg).unwrap();
        let back: IrGenerationConfig = serde_json::from_value(json).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn config_empty_fields_skipped() {
        let cfg = IrGenerationConfig::default();
        let json = serde_json::to_value(&cfg).unwrap();
        assert!(json.get("max_tokens").is_none());
        assert!(json.get("stop_sequences").is_none());
        assert!(json.get("extra").is_none());
    }

    #[test]
    fn config_alias_works() {
        let _cfg: IrConfig = IrGenerationConfig::default().with_max_tokens(100);
    }

    // ── IrRequest ───────────────────────────────────────────────────

    #[test]
    fn request_new_minimal() {
        let req = IrRequest::new(vec![IrMessage::text(IrRole::User, "hello")]);
        assert_eq!(req.messages.len(), 1);
        assert!(req.model.is_none());
        assert!(req.system_prompt.is_none());
        assert!(req.tools.is_empty());
    }

    #[test]
    fn request_builder_chain() {
        let req = IrRequest::new(vec![IrMessage::text(IrRole::User, "hi")])
            .with_model("gpt-4")
            .with_system_prompt("You are helpful.")
            .with_tool(IrToolDefinition::new("search", "Search", json!({})))
            .with_config(
                IrGenerationConfig::default()
                    .with_max_tokens(1024)
                    .with_temperature(0.5),
            )
            .with_metadata("trace_id", json!("abc-123"));

        assert_eq!(req.model, Some("gpt-4".into()));
        assert_eq!(req.system_prompt, Some("You are helpful.".into()));
        assert_eq!(req.tools.len(), 1);
        assert_eq!(req.config.max_tokens, Some(1024));
        assert_eq!(req.metadata.get("trace_id"), Some(&json!("abc-123")));
    }

    #[test]
    fn request_serde_roundtrip() {
        let req = IrRequest::new(vec![IrMessage::text(IrRole::User, "test")])
            .with_model("claude-3")
            .with_system_prompt("Be concise.");
        let json = serde_json::to_value(&req).unwrap();
        let back: IrRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn request_system_message_lookup() {
        let req = IrRequest::new(vec![
            IrMessage::text(IrRole::System, "instructions"),
            IrMessage::text(IrRole::User, "hello"),
        ]);
        let sys = req.system_message().unwrap();
        assert_eq!(sys.text_content(), "instructions");
    }

    #[test]
    fn request_all_tool_calls() {
        let req = IrRequest::new(vec![
            IrMessage::text(IrRole::User, "find files"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolCall {
                    id: "tc_1".into(),
                    name: "glob".into(),
                    input: json!({"pattern": "*.rs"}),
                }],
            ),
        ]);
        assert_eq!(req.all_tool_calls().len(), 1);
    }

    #[test]
    fn request_with_message_builder() {
        let req = IrRequest::new(vec![])
            .with_message(IrMessage::text(IrRole::User, "a"))
            .with_message(IrMessage::text(IrRole::Assistant, "b"));
        assert_eq!(req.messages.len(), 2);
    }

    #[test]
    fn request_with_tools_builder() {
        let tools = vec![
            IrToolDefinition::new("a", "tool a", json!({})),
            IrToolDefinition::new("b", "tool b", json!({})),
        ];
        let req = IrRequest::new(vec![]).with_tools(tools);
        assert_eq!(req.tools.len(), 2);
    }

    #[test]
    fn request_extract_all_tool_calls() {
        let req = IrRequest::new(vec![IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::ToolCall {
                    id: "t1".into(),
                    name: "a".into(),
                    input: json!({}),
                },
                IrContentBlock::ToolCall {
                    id: "t2".into(),
                    name: "b".into(),
                    input: json!({}),
                },
            ],
        )]);
        let calls = req.extract_all_tool_calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "t1");
        assert_eq!(calls[1].id, "t2");
    }

    // ── IrStopReason ────────────────────────────────────────────────

    #[test]
    fn stop_reason_serde_roundtrip() {
        for reason in [
            IrStopReason::EndTurn,
            IrStopReason::StopSequence,
            IrStopReason::MaxTokens,
            IrStopReason::ToolUse,
            IrStopReason::ContentFilter,
            IrStopReason::Other("custom".into()),
        ] {
            let json = serde_json::to_value(&reason).unwrap();
            let back: IrStopReason = serde_json::from_value(json).unwrap();
            assert_eq!(reason, back);
        }
    }

    // ── IrResponse ──────────────────────────────────────────────────

    #[test]
    fn response_text_constructor() {
        let resp = IrResponse::text("hello world");
        assert_eq!(resp.text_content(), "hello world");
    }

    #[test]
    fn response_builder_chain() {
        let resp = IrResponse::text("answer")
            .with_id("resp_1")
            .with_model("gpt-4")
            .with_stop_reason(IrStopReason::EndTurn)
            .with_usage(IrUsage::from_io(100, 50))
            .with_metadata("latency_ms", json!(250));

        assert_eq!(resp.id, Some("resp_1".into()));
        assert_eq!(resp.model, Some("gpt-4".into()));
        assert_eq!(resp.stop_reason, Some(IrStopReason::EndTurn));
        assert_eq!(resp.usage.unwrap().total_tokens, 150);
        assert_eq!(resp.metadata.get("latency_ms"), Some(&json!(250)));
    }

    #[test]
    fn response_serde_roundtrip() {
        let resp = IrResponse::text("hi")
            .with_id("r1")
            .with_stop_reason(IrStopReason::MaxTokens)
            .with_usage(IrUsage::from_io(10, 5));
        let json = serde_json::to_value(&resp).unwrap();
        let back: IrResponse = serde_json::from_value(json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn response_has_tool_calls() {
        let resp = IrResponse::new(vec![IrContentBlock::ToolCall {
            id: "tc_1".into(),
            name: "search".into(),
            input: json!({}),
        }]);
        assert!(resp.has_tool_calls());
        assert_eq!(resp.tool_calls().len(), 1);
    }

    #[test]
    fn response_text_content_skips_non_text() {
        let resp = IrResponse::new(vec![
            IrContentBlock::Text { text: "a".into() },
            IrContentBlock::Thinking { text: "hmm".into() },
            IrContentBlock::Text { text: "b".into() },
        ]);
        assert_eq!(resp.text_content(), "ab");
    }

    // ── IrStreamEvent ───────────────────────────────────────────────

    #[test]
    fn stream_event_stream_start() {
        let ev = IrStreamEvent::stream_start();
        match &ev {
            IrStreamEvent::StreamStart { id, model } => {
                assert!(id.is_none());
                assert!(model.is_none());
            }
            _ => panic!("expected StreamStart"),
        }
        assert!(!ev.is_terminal());
    }

    #[test]
    fn stream_event_text_delta() {
        let ev = IrStreamEvent::text_delta(0, "hello");
        assert_eq!(ev.as_text_delta(), Some("hello"));
        assert!(!ev.is_terminal());
    }

    #[test]
    fn stream_event_tool_call_delta() {
        let ev = IrStreamEvent::tool_call_delta(1, "{\"q\":");
        match &ev {
            IrStreamEvent::ToolCallDelta {
                index,
                arguments_delta,
            } => {
                assert_eq!(*index, 1);
                assert_eq!(arguments_delta, "{\"q\":");
            }
            _ => panic!("expected ToolCallDelta"),
        }
    }

    #[test]
    fn stream_event_stream_end() {
        let ev = IrStreamEvent::stream_end(Some(IrStopReason::EndTurn));
        assert!(ev.is_terminal());
    }

    #[test]
    fn stream_event_error() {
        let ev = IrStreamEvent::error("rate_limit", "Too many requests");
        assert!(ev.is_terminal());
        match &ev {
            IrStreamEvent::Error { code, message } => {
                assert_eq!(code, "rate_limit");
                assert_eq!(message, "Too many requests");
            }
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn stream_event_serde_roundtrip_text_delta() {
        let ev = IrStreamEvent::text_delta(2, "world");
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["type"], "text_delta");
        let back: IrStreamEvent = serde_json::from_value(json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn stream_event_serde_roundtrip_stream_start() {
        let ev = IrStreamEvent::StreamStart {
            id: Some("resp_1".into()),
            model: Some("gpt-4".into()),
        };
        let json = serde_json::to_value(&ev).unwrap();
        let back: IrStreamEvent = serde_json::from_value(json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn stream_event_serde_roundtrip_content_block_start() {
        let ev = IrStreamEvent::ContentBlockStart {
            index: 0,
            block: IrContentBlock::Text { text: "".into() },
        };
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["type"], "content_block_start");
        let back: IrStreamEvent = serde_json::from_value(json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn stream_event_serde_roundtrip_content_block_stop() {
        let ev = IrStreamEvent::ContentBlockStop { index: 3 };
        let json = serde_json::to_value(&ev).unwrap();
        let back: IrStreamEvent = serde_json::from_value(json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn stream_event_serde_roundtrip_usage() {
        let ev = IrStreamEvent::Usage {
            usage: IrUsage::from_io(50, 25),
        };
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["type"], "usage");
        let back: IrStreamEvent = serde_json::from_value(json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn stream_event_serde_roundtrip_stream_end() {
        let ev = IrStreamEvent::stream_end(Some(IrStopReason::ToolUse));
        let json = serde_json::to_value(&ev).unwrap();
        let back: IrStreamEvent = serde_json::from_value(json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn stream_event_serde_roundtrip_error() {
        let ev = IrStreamEvent::error("server_error", "Internal error");
        let json = serde_json::to_value(&ev).unwrap();
        let back: IrStreamEvent = serde_json::from_value(json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn stream_event_serde_roundtrip_thinking_delta() {
        let ev = IrStreamEvent::ThinkingDelta {
            index: 0,
            text: "analyzing...".into(),
        };
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["type"], "thinking_delta");
        let back: IrStreamEvent = serde_json::from_value(json).unwrap();
        assert_eq!(ev, back);
    }

    #[test]
    fn stream_event_non_text_delta_as_text_is_none() {
        let ev = IrStreamEvent::stream_start();
        assert!(ev.as_text_delta().is_none());
    }

    #[test]
    fn stream_event_stream_end_none_stop_reason() {
        let ev = IrStreamEvent::stream_end(None);
        assert!(ev.is_terminal());
    }

    // ── Integration / complex scenarios ─────────────────────────────

    #[test]
    fn full_request_response_roundtrip() {
        let req = IrRequest::new(vec![
            IrMessage::text(IrRole::System, "You are a coder."),
            IrMessage::text(IrRole::User, "Write hello world in Rust"),
        ])
        .with_model("claude-3-sonnet")
        .with_tool(IrToolDefinition::new(
            "write_file",
            "Write a file",
            json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}),
        ))
        .with_config(IrGenerationConfig::default().with_max_tokens(4096));

        let req_json = serde_json::to_value(&req).unwrap();
        let req_back: IrRequest = serde_json::from_value(req_json).unwrap();
        assert_eq!(req, req_back);

        let resp = IrResponse::new(vec![
            IrContentBlock::Text {
                text: "Here's the code:".into(),
            },
            IrContentBlock::ToolCall {
                id: "tc_1".into(),
                name: "write_file".into(),
                input: json!({"path": "main.rs", "content": "fn main() { println!(\"Hello!\"); }"}),
            },
        ])
        .with_id("resp_1")
        .with_model("claude-3-sonnet")
        .with_stop_reason(IrStopReason::ToolUse)
        .with_usage(IrUsage::from_io(150, 80));

        let resp_json = serde_json::to_value(&resp).unwrap();
        let resp_back: IrResponse = serde_json::from_value(resp_json).unwrap();
        assert_eq!(resp, resp_back);
    }

    #[test]
    fn tool_call_result_roundtrip_integration() {
        let call = IrToolCall::new("tc_1", "read_file", json!({"path": "main.rs"}));
        let block = call.clone().into_content_block();
        let extracted = IrToolCall::from_content_block(&block).unwrap();
        assert_eq!(call.id, extracted.id);
        assert_eq!(call.name, extracted.name);
        assert_eq!(call.arguments, extracted.arguments);

        let result = IrToolResult::text("tc_1", "fn main() {}");
        let result_block = result.clone().into_content_block();
        let extracted_result = IrToolResult::from_content_block(&result_block).unwrap();
        assert_eq!(result.tool_call_id, extracted_result.tool_call_id);
        assert_eq!(result.is_error, extracted_result.is_error);
    }

    #[test]
    fn streaming_sequence_scenario() {
        let events = vec![
            IrStreamEvent::stream_start(),
            IrStreamEvent::ContentBlockStart {
                index: 0,
                block: IrContentBlock::Text { text: "".into() },
            },
            IrStreamEvent::text_delta(0, "Hello"),
            IrStreamEvent::text_delta(0, " world"),
            IrStreamEvent::ContentBlockStop { index: 0 },
            IrStreamEvent::Usage {
                usage: IrUsage::from_io(10, 5),
            },
            IrStreamEvent::stream_end(Some(IrStopReason::EndTurn)),
        ];

        // Verify first/last terminal status
        assert!(!events[0].is_terminal());
        assert!(events.last().unwrap().is_terminal());

        // Verify all roundtrip through serde
        for ev in &events {
            let json = serde_json::to_value(ev).unwrap();
            let back: IrStreamEvent = serde_json::from_value(json).unwrap();
            assert_eq!(ev, &back);
        }

        // Collect text deltas
        let text: String = events
            .iter()
            .filter_map(|e| e.as_text_delta())
            .collect::<Vec<_>>()
            .join("");
        assert_eq!(text, "Hello world");
    }
}
