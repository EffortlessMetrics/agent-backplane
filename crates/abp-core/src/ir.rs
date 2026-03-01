// SPDX-License-Identifier: MIT OR Apache-2.0
//! Intermediate Representation (IR) for cross-dialect message normalization.
//!
//! The IR captures the semantic meaning of messages, tool calls, and content
//! blocks in a vendor-neutral form. Dialect adapters lower vendor-specific
//! formats into the IR and raise the IR back into the target dialect.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ── Roles ───────────────────────────────────────────────────────────────

/// Normalized message role across all dialects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IrRole {
    /// System prompt / instructions.
    System,
    /// User / human turn.
    User,
    /// Assistant / model turn.
    Assistant,
    /// Tool result turn (correlates with a prior tool-use block).
    Tool,
}

// ── Content blocks ──────────────────────────────────────────────────────

/// A single content block inside an [`IrMessage`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
        /// Identifier of the corresponding [`IrContentBlock::ToolUse`].
        tool_use_id: String,
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
}

// ── Messages ────────────────────────────────────────────────────────────

/// A single normalized message in a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct IrMessage {
    /// The role of the message author.
    pub role: IrRole,

    /// Ordered content blocks that make up the message body.
    pub content: Vec<IrContentBlock>,

    /// Optional vendor-opaque metadata carried through the pipeline.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, serde_json::Value>,
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

    /// Returns `true` if every content block is [`IrContentBlock::Text`].
    #[must_use]
    pub fn is_text_only(&self) -> bool {
        self.content
            .iter()
            .all(|b| matches!(b, IrContentBlock::Text { .. }))
    }

    /// Concatenate all [`IrContentBlock::Text`] blocks into a single string.
    #[must_use]
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| match b {
                IrContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// Return all [`IrContentBlock::ToolUse`] blocks in this message.
    #[must_use]
    pub fn tool_use_blocks(&self) -> Vec<&IrContentBlock> {
        self.content
            .iter()
            .filter(|b| matches!(b, IrContentBlock::ToolUse { .. }))
            .collect()
    }
}

// ── Tool definitions ────────────────────────────────────────────────────

/// A canonical tool definition for cross-dialect normalization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct IrToolDefinition {
    /// Unique tool name.
    pub name: String,

    /// Human-readable description of the tool.
    pub description: String,

    /// JSON Schema describing the input parameters.
    pub parameters: serde_json::Value,
}

// ── Conversation ────────────────────────────────────────────────────────

/// An ordered sequence of [`IrMessage`]s with helper accessors.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct IrConversation {
    /// The messages in conversation order.
    pub messages: Vec<IrMessage>,
}

impl IrConversation {
    /// Create an empty conversation.
    #[must_use]
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }

    /// Create a conversation from an existing message list.
    #[must_use]
    pub fn from_messages(messages: Vec<IrMessage>) -> Self {
        Self { messages }
    }

    /// Append a message and return `self` for chaining.
    #[must_use]
    pub fn push(mut self, message: IrMessage) -> Self {
        self.messages.push(message);
        self
    }

    /// Return the first system message, if any.
    #[must_use]
    pub fn system_message(&self) -> Option<&IrMessage> {
        self.messages.iter().find(|m| m.role == IrRole::System)
    }

    /// Return the last assistant message, if any.
    #[must_use]
    pub fn last_assistant(&self) -> Option<&IrMessage> {
        self.messages
            .iter()
            .rev()
            .find(|m| m.role == IrRole::Assistant)
    }

    /// Collect every [`IrContentBlock::ToolUse`] across all messages.
    #[must_use]
    pub fn tool_calls(&self) -> Vec<&IrContentBlock> {
        self.messages
            .iter()
            .flat_map(|m| m.tool_use_blocks())
            .collect()
    }

    /// Return the number of messages.
    #[must_use]
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Return `true` if the conversation is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Return all messages with the given role.
    #[must_use]
    pub fn messages_by_role(&self, role: IrRole) -> Vec<&IrMessage> {
        self.messages.iter().filter(|m| m.role == role).collect()
    }

    /// Return the last message regardless of role, if any.
    #[must_use]
    pub fn last_message(&self) -> Option<&IrMessage> {
        self.messages.last()
    }
}

// ── Usage ───────────────────────────────────────────────────────────────

/// Normalized token-usage counters across all dialects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
    pub fn from_io(input_tokens: u64, output_tokens: u64) -> Self {
        Self {
            input_tokens,
            output_tokens,
            total_tokens: input_tokens + output_tokens,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        }
    }

    /// Construct usage with cache counters as well.
    #[must_use]
    pub fn with_cache(
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_write_tokens: u64,
    ) -> Self {
        Self {
            input_tokens,
            output_tokens,
            total_tokens: input_tokens + output_tokens,
            cache_read_tokens,
            cache_write_tokens,
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
