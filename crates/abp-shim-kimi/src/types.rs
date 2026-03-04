// SPDX-License-Identifier: MIT OR Apache-2.0
//! Kimi-specific shim types.
//!
//! Contains the convenience [`Message`] wrapper, [`Usage`] statistics, and the
//! [`KimiRequestBuilder`] for constructing [`KimiRequest`]s.

use abp_kimi_sdk::dialect::{KimiMessage, KimiRequest, KimiTool, KimiToolCall};
use serde::{Deserialize, Serialize};

// ── Message constructors ────────────────────────────────────────────────

/// A chat message in the Kimi format (convenience wrapper).
#[derive(Debug, Clone, Serialize, Deserialize)]
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
