// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Kimi-specific shim types.
//!
//! Contains the convenience [`Message`] wrapper, [`Usage`] statistics,
//! [`KimiRequestBuilder`] for constructing [`KimiRequest`]s, and Kimi-specific
//! extension types for file references, plugins, web search, and streaming.

use abp_kimi_sdk::dialect::{KimiMessage, KimiRequest, KimiTool, KimiToolCall, KimiToolDef};
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
    n: Option<u32>,
    stop: Option<Vec<String>>,
    presence_penalty: Option<f64>,
    frequency_penalty: Option<f64>,
    response_format: Option<KimiResponseFormat>,
    tool_choice: Option<serde_json::Value>,
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

    /// Set the number of completions to generate.
    #[must_use]
    pub fn n(mut self, n: u32) -> Self {
        self.n = Some(n);
        self
    }

    /// Set stop sequences.
    #[must_use]
    pub fn stop(mut self, stop: Vec<String>) -> Self {
        self.stop = Some(stop);
        self
    }

    /// Set the presence penalty.
    #[must_use]
    pub fn presence_penalty(mut self, penalty: f64) -> Self {
        self.presence_penalty = Some(penalty);
        self
    }

    /// Set the frequency penalty.
    #[must_use]
    pub fn frequency_penalty(mut self, penalty: f64) -> Self {
        self.frequency_penalty = Some(penalty);
        self
    }

    /// Set the response format.
    #[must_use]
    pub fn response_format(mut self, format: KimiResponseFormat) -> Self {
        self.response_format = Some(format);
        self
    }

    /// Set the tool choice.
    #[must_use]
    pub fn tool_choice(mut self, choice: serde_json::Value) -> Self {
        self.tool_choice = Some(choice);
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
    /// Number of completions to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    /// Stop sequences — the model will stop generating when it encounters one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    /// Presence penalty (−2.0 to 2.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    /// Frequency penalty (−2.0 to 2.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    /// Tool definitions available to the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<KimiToolDef>>,
    /// Controls which tool the model should call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    /// Response format constraint (e.g. JSON mode).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<KimiResponseFormat>,

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

impl Default for KimiChatRequest {
    fn default() -> Self {
        Self {
            model: "moonshot-v1-8k".into(),
            messages: Vec::new(),
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            n: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
            tools: None,
            tool_choice: None,
            response_format: None,
            use_search: None,
            ref_file_ids: None,
            plugin_ids: None,
            plugins: None,
        }
    }
}

/// Response format constraint for Kimi Chat Completions.
///
/// Used to request structured output (e.g. JSON mode) from the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiResponseFormat {
    /// Format type: `"text"` (default) or `"json_object"` for JSON mode.
    #[serde(rename = "type")]
    pub format_type: String,
}

impl KimiResponseFormat {
    /// Create a text response format (the default).
    #[must_use]
    pub fn text() -> Self {
        Self {
            format_type: "text".into(),
        }
    }

    /// Create a JSON object response format.
    #[must_use]
    pub fn json_object() -> Self {
        Self {
            format_type: "json_object".into(),
        }
    }
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

/// A streaming tool call fragment inside a delta.
///
/// In the SSE stream, tool calls arrive incrementally: the first fragment
/// carries the `id` and function `name`; subsequent fragments carry argument
/// chunks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiStreamToolCallFragment {
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
    pub function: Option<KimiStreamFunctionFragment>,
}

/// Incremental function call data inside a streaming tool call fragment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KimiStreamFunctionFragment {
    /// Function name (first fragment only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Incremental arguments fragment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}
