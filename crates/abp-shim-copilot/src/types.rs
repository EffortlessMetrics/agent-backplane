// SPDX-License-Identifier: MIT OR Apache-2.0
//! Copilot SDK–specific types used by the shim.
//!
//! Contains convenience wrappers and builders that present a Copilot-native
//! surface while routing through ABP's intermediate representation internally.

use abp_copilot_sdk::dialect::{
    CopilotMessage, CopilotReference, CopilotRequest, CopilotTool, CopilotTurnEntry,
};
use serde::{Deserialize, Serialize};

// ── Message constructors ────────────────────────────────────────────────

/// A chat message in the Copilot format (convenience wrapper).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Message role.
    pub role: String,
    /// Text content of the message.
    pub content: String,
    /// Optional display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// References attached to this message.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copilot_references: Vec<CopilotReference>,
}

impl Message {
    /// Create a system message.
    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
            name: None,
            copilot_references: Vec::new(),
        }
    }

    /// Create a user message.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
            name: None,
            copilot_references: Vec::new(),
        }
    }

    /// Create an assistant message.
    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
            name: None,
            copilot_references: Vec::new(),
        }
    }

    /// Create a user message with references.
    #[must_use]
    pub fn user_with_refs(content: impl Into<String>, refs: Vec<CopilotReference>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
            name: None,
            copilot_references: refs,
        }
    }
}

// ── Request builder ─────────────────────────────────────────────────────

/// Builder for [`CopilotRequest`].
#[derive(Debug, Default)]
pub struct CopilotRequestBuilder {
    model: Option<String>,
    messages: Vec<Message>,
    tools: Option<Vec<CopilotTool>>,
    turn_history: Vec<CopilotTurnEntry>,
    references: Vec<CopilotReference>,
}

impl CopilotRequestBuilder {
    /// Create a new builder for a Copilot request.
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

    /// Set the tools.
    #[must_use]
    pub fn tools(mut self, tools: Vec<CopilotTool>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set the turn history.
    #[must_use]
    pub fn turn_history(mut self, history: Vec<CopilotTurnEntry>) -> Self {
        self.turn_history = history;
        self
    }

    /// Set the references.
    #[must_use]
    pub fn references(mut self, refs: Vec<CopilotReference>) -> Self {
        self.references = refs;
        self
    }

    /// Build the request, defaulting model to `"gpt-4o"` if unset.
    #[must_use]
    pub fn build(self) -> CopilotRequest {
        CopilotRequest {
            model: self.model.unwrap_or_else(|| "gpt-4o".into()),
            messages: self.messages.into_iter().map(to_copilot_message).collect(),
            tools: self.tools,
            turn_history: self.turn_history,
            references: self.references,
        }
    }
}

/// Convert a shim [`Message`] to a [`CopilotMessage`].
pub(crate) fn to_copilot_message(msg: Message) -> CopilotMessage {
    CopilotMessage {
        role: msg.role,
        content: msg.content,
        name: msg.name,
        copilot_references: msg.copilot_references,
    }
}
