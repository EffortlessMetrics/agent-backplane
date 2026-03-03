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
}
