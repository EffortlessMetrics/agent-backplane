// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Copilot SDK–specific types used by the shim.
//!
//! Contains convenience wrappers and builders that present a Copilot-native
//! surface while routing through ABP's intermediate representation internally.
//!
//! ## Copilot-specific extensions
//!
//! Beyond the base OpenAI-compatible types, this module defines Copilot-specific
//! extensions: [`CopilotIntent`], [`CopilotDocContext`], [`CopilotSkill`],
//! [`CopilotChatRequest`], [`CopilotChatResponse`], and [`CopilotLocalStreamEvent`].

use std::collections::BTreeMap;

use abp_copilot_sdk::dialect::{
    CopilotConfirmation, CopilotError, CopilotFunctionCall, CopilotMessage, CopilotReference,
    CopilotRequest, CopilotTool, CopilotTurnEntry,
};
use serde::{Deserialize, Serialize};

// ── Copilot intent ──────────────────────────────────────────────────────

/// The intent behind a Copilot request.
///
/// Maps to the slash-command namespace in the Copilot UI (e.g. `/explain`,
/// `/generate`, `/fix`, `/test`) as well as free-form custom intents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CopilotIntent {
    /// Explain selected code or concepts.
    Explain,
    /// Generate new code from a description.
    Generate,
    /// Fix errors or bugs in existing code.
    Fix,
    /// Generate or improve tests.
    Test,
    /// A free-form custom intent.
    Custom(String),
}

impl std::fmt::Display for CopilotIntent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Explain => write!(f, "explain"),
            Self::Generate => write!(f, "generate"),
            Self::Fix => write!(f, "fix"),
            Self::Test => write!(f, "test"),
            Self::Custom(s) => write!(f, "{s}"),
        }
    }
}

// ── Editor / document context ───────────────────────────────────────────

/// Editor context provided by the Copilot client.
///
/// Captures the current file, cursor position, and optional selection so that
/// the agent can produce contextually relevant responses.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotDocContext {
    /// URI or path of the file currently open in the editor.
    pub uri: String,
    /// Programming language identifier (e.g. `"rust"`, `"typescript"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Zero-based cursor line position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor_line: Option<u32>,
    /// Zero-based cursor column position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor_column: Option<u32>,
    /// Selected text range, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<SelectionRange>,
    /// The full content of the file (optional, may be omitted for large files).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// A text selection range within a document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SelectionRange {
    /// Zero-based start line.
    pub start_line: u32,
    /// Zero-based start column.
    pub start_column: u32,
    /// Zero-based end line.
    pub end_line: u32,
    /// Zero-based end column.
    pub end_column: u32,
    /// The selected text content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

// ── Code references ─────────────────────────────────────────────────────

/// A code reference attached to a Copilot request.
///
/// Richer than a generic [`CopilotReference`] — carries typed file path,
/// language, and selection range metadata suitable for IDE integrations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotCodeReference {
    /// File path (relative or absolute).
    pub path: String,
    /// Programming language identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Selection range within the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<SelectionRange>,
    /// Snippet of the referenced code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

// ── Skills ──────────────────────────────────────────────────────────────

/// An extensibility agent skill available to Copilot.
///
/// Skills represent capabilities that can be invoked by the agent, similar
/// to plugins or extensions in the Copilot Extensions framework.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotSkill {
    /// Unique identifier for the skill.
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Description of what the skill does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema for the skill's input parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters_schema: Option<serde_json::Value>,
}

// ── Copilot chat request (extended) ─────────────────────────────────────

/// Extended Copilot chat request with Copilot-specific fields.
///
/// This is an OpenAI-compatible request augmented with Copilot extensions
/// such as intent, editor context, typed code references, and skills.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotChatRequest {
    /// Model identifier (e.g. `"gpt-4o"`).
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<CopilotMessage>,
    /// Tool definitions available to the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<CopilotTool>>,
    /// The intent behind the request (e.g. explain, generate, fix, test).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<CopilotIntent>,
    /// Editor/document context from the IDE.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_context: Option<CopilotDocContext>,
    /// Typed code references attached to the request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<CopilotCodeReference>,
    /// Extensibility agent skills available to the model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<CopilotSkill>,
    /// Previous turns in the conversation (for multi-turn agents).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub turn_history: Vec<CopilotTurnEntry>,
    /// Sampling temperature (0.0–2.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Maximum tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Whether to stream the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

// ── Copilot chat response (extended) ────────────────────────────────────

/// Copilot-specific metadata attached to a response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CopilotResponseMetadata {
    /// The intent that was handled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<CopilotIntent>,
    /// Model that actually served the request (may differ from requested).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Vendor-specific extension data.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ext: BTreeMap<String, serde_json::Value>,
}

/// Extended Copilot chat response with Copilot metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CopilotChatResponse {
    /// The assistant's reply text.
    pub message: String,
    /// References emitted in the response.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copilot_references: Vec<CopilotReference>,
    /// Errors reported during processing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copilot_errors: Vec<CopilotError>,
    /// Function call request, if the agent wants to invoke a tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_call: Option<CopilotFunctionCall>,
    /// Copilot-specific response metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<CopilotResponseMetadata>,
}

// ── Copilot stream event (local) ────────────────────────────────────────

/// Streaming event from a Copilot chat completion.
///
/// This is a local mirror of the SDK's [`CopilotStreamEvent`](abp_copilot_sdk::dialect::CopilotStreamEvent)
/// with additional metadata fields for richer streaming.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CopilotLocalStreamEvent {
    /// References emitted at the start of a response.
    CopilotReferences {
        /// The references payload.
        references: Vec<CopilotReference>,
    },
    /// Errors encountered during processing.
    CopilotErrors {
        /// The errors payload.
        errors: Vec<CopilotError>,
    },
    /// A text delta in the assistant's streaming reply.
    TextDelta {
        /// The text fragment.
        text: String,
    },
    /// A function call emitted during streaming.
    FunctionCall {
        /// The function call details.
        function_call: CopilotFunctionCall,
    },
    /// Copilot-specific metadata for the stream.
    Metadata {
        /// Response metadata.
        metadata: CopilotResponseMetadata,
    },
    /// Stream completed.
    Done {},
}

// ── Message constructors ────────────────────────────────────────────────

/// A chat message in the Copilot format (convenience wrapper).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    /// Confirmations attached to this message.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copilot_confirmations: Vec<CopilotConfirmation>,
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
            copilot_confirmations: Vec::new(),
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
            copilot_confirmations: Vec::new(),
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
            copilot_confirmations: Vec::new(),
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
            copilot_confirmations: Vec::new(),
        }
    }

    /// Create a user message with confirmations.
    #[must_use]
    pub fn user_with_confirmations(
        content: impl Into<String>,
        confirmations: Vec<CopilotConfirmation>,
    ) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
            name: None,
            copilot_references: Vec::new(),
            copilot_confirmations: confirmations,
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
