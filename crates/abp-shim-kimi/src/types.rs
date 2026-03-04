// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Kimi-specific shim types.
//!
//! Contains the convenience [`Message`] wrapper, [`Usage`] statistics,
//! [`KimiRequestBuilder`] for constructing [`KimiRequest`]s, and Kimi-specific
//! extension types for file references, plugins, web search, and streaming.

use abp_kimi_sdk::dialect::{KimiMessage, KimiRequest, KimiTool, KimiToolCall};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ── Message constructors ────────────────────────────────────────────────

/// A chat message in the Kimi format (convenience wrapper).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    /// Message role.
    pub role: String,
    /// Text content of the message.
    pub content: Option<String>,
    /// Tool calls (assistant messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<KimiToolCall>>,
    /// Tool call ID this message responds to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    /// Create a system message.
    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create a user message.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create an assistant message.
    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Create an assistant message with tool calls.
    #[must_use]
    pub fn assistant_with_tool_calls(tool_calls: Vec<KimiToolCall>) -> Self {
        Self {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    /// Create a tool result message.
    #[must_use]
    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

// ── Token usage ─────────────────────────────────────────────────────────

/// Token usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    /// Tokens consumed by the prompt.
    pub prompt_tokens: u64,
    /// Tokens generated in the completion.
    pub completion_tokens: u64,
    /// Total tokens (prompt + completion).
    pub total_tokens: u64,
}

// ── Request builder ─────────────────────────────────────────────────────

/// Builder for [`KimiRequest`].
#[derive(Debug, Default)]
pub struct KimiRequestBuilder {
    model: Option<String>,
    messages: Vec<Message>,
    max_tokens: Option<u32>,
    temperature: Option<f64>,
    stream: Option<bool>,
    tools: Option<Vec<KimiTool>>,
    use_search: Option<bool>,
}

impl KimiRequestBuilder {
    /// Create a new builder for a Kimi request.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the model.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the messages.
    #[must_use]
    pub fn messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }

    /// Set the maximum tokens.
    #[must_use]
    pub fn max_tokens(mut self, max: u32) -> Self {
        self.max_tokens = Some(max);
        self
    }

    /// Set the temperature.
    #[must_use]
    pub fn temperature(mut self, temp: f64) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Set the stream flag.
    #[must_use]
    pub fn stream(mut self, stream: bool) -> Self {
        self.stream = Some(stream);
        self
    }

    /// Set the tools.
    #[must_use]
    pub fn tools(mut self, tools: Vec<KimiTool>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set the use_search flag.
    #[must_use]
    pub fn use_search(mut self, use_search: bool) -> Self {
        self.use_search = Some(use_search);
        self
    }

    /// Build the request, defaulting model to `"moonshot-v1-8k"` if unset.
    #[must_use]
    pub fn build(self) -> KimiRequest {
        KimiRequest {
            model: self.model.unwrap_or_else(|| "moonshot-v1-8k".into()),
            messages: self.messages.into_iter().map(to_kimi_message).collect(),
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            stream: self.stream,
            tools: self.tools,
            use_search: self.use_search,
        }
    }
}

/// Convert a shim [`Message`] to a [`KimiMessage`].
pub(crate) fn to_kimi_message(msg: Message) -> KimiMessage {
    KimiMessage {
        role: msg.role,
        content: msg.content,
        tool_call_id: msg.tool_call_id,
        tool_calls: msg.tool_calls,
    }
}

// ── Kimi-specific extension types ───────────────────────────────────────

/// Kimi Chat Completions request — OpenAI-compatible with Kimi extensions.
///
/// Extends the standard chat completions request with `use_search` for web
/// search, `ref_file_ids` for file context, and `plugin_ids` / `plugins` for
/// Kimi plugin invocation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiChatRequest {
    /// Model identifier (e.g. `moonshot-v1-8k`).
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<Message>,
    /// Sampling temperature (0.0–1.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Nucleus sampling parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Maximum tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Whether to stream the response via SSE.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,

    // ── Kimi extensions ─────────────────────────────────────────────
    /// Enable Kimi's built-in web search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_search: Option<bool>,
    /// File IDs to include as context (from the Kimi Files API).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_file_ids: Option<Vec<String>>,
    /// Plugin IDs to activate for this request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_ids: Option<Vec<String>>,
    /// Detailed plugin configurations for this request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugins: Option<Vec<KimiPluginConfig>>,
}

/// Kimi Chat Completions response with Kimi-specific metadata.
///
/// Extends the standard OpenAI response shape with optional web search
/// results embedded by Kimi when `use_search` is enabled.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiChatResponse {
    /// Unique response identifier.
    pub id: String,
    /// Object type (always `"chat.completion"`).
    pub object: String,
    /// Unix timestamp of creation.
    pub created: u64,
    /// Model used for the completion.
    pub model: String,
    /// Completion choices.
    pub choices: Vec<KimiChatChoice>,
    /// Token usage statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Web search results when search was active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_results: Option<Vec<KimiSearchResult>>,
}

/// A single choice in a [`KimiChatResponse`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiChatChoice {
    /// Zero-based index.
    pub index: u32,
    /// The assistant's response message.
    pub message: KimiChatChoiceMessage,
    /// Reason the model stopped (`"stop"`, `"tool_calls"`, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// The assistant message inside a [`KimiChatChoice`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiChatChoiceMessage {
    /// Always `"assistant"`.
    pub role: String,
    /// Text content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Tool calls emitted by the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<KimiToolCall>>,
}

/// A streaming SSE event from the Kimi Chat Completions API.
///
/// Each event represents an incremental delta or the final stop signal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiStreamEvent {
    /// Chunk identifier (same across all chunks in one stream).
    pub id: String,
    /// Object type (always `"chat.completion.chunk"`).
    pub object: String,
    /// Unix timestamp of creation.
    pub created: u64,
    /// Model that produced this chunk.
    pub model: String,
    /// Streaming choices with deltas.
    pub choices: Vec<KimiStreamChoice>,
    /// Usage info (only present in the final chunk when requested).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Web search results (may appear in later chunks).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_results: Option<Vec<KimiSearchResult>>,
}

/// A single choice inside a [`KimiStreamEvent`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiStreamChoice {
    /// Zero-based choice index.
    pub index: u32,
    /// The incremental delta.
    pub delta: KimiStreamDelta,
    /// Finish reason — `None` until the stream ends.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// An incremental delta inside a streaming choice.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct KimiStreamDelta {
    /// Role (usually only in the first chunk).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Text content fragment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Incremental tool call fragments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<KimiToolCall>>,
}

/// File reference for Kimi's file context feature.
///
/// Represents an uploaded file that can be included in a request via
/// `ref_file_ids` to provide document context to the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiFileReference {
    /// The file ID from Kimi's Files API (e.g. `"file-abc123"`).
    pub file_id: String,
    /// Original filename, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// Purpose of the file (e.g. `"file-extract"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
}

/// Plugin configuration for Kimi's plugin invocation.
///
/// When `plugin_ids` or `plugins` are set on a [`KimiChatRequest`], Kimi
/// activates the specified plugins during completion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiPluginConfig {
    /// Unique plugin identifier.
    pub plugin_id: String,
    /// Human-readable plugin name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Whether this plugin is enabled (default: true).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Plugin-specific settings.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, serde_json::Value>,
}

/// Web search result returned by Kimi when search is enabled.
///
/// Kimi embeds these in responses when `use_search` is active, providing
/// citation sources the model used for its answer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiSearchResult {
    /// One-based index of this search result.
    pub index: u32,
    /// URL of the cited source.
    pub url: String,
    /// Title of the cited source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Snippet / summary of the cited content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}
