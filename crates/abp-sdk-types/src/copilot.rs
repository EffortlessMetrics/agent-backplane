// SPDX-License-Identifier: MIT OR Apache-2.0
//! GitHub Copilot Extensions API type definitions.
//!
//! Mirrors the Copilot agent protocol request/response surface including
//! references, confirmations, and streaming SSE events.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ── Reference types ─────────────────────────────────────────────────────

/// The type of a Copilot reference attached to a message or response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CopilotReferenceType {
    /// A file reference (path + optional content).
    File,
    /// A code snippet with location metadata.
    Snippet,
    /// A repository reference (owner/name).
    Repository,
    /// A web search result.
    WebSearchResult,
}

/// A reference attached to a Copilot message or response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CopilotReference {
    /// The reference type discriminator.
    #[serde(rename = "type")]
    pub ref_type: CopilotReferenceType,
    /// Unique identifier for this reference.
    pub id: String,
    /// Structured data payload for this reference.
    pub data: serde_json::Value,
    /// Optional metadata (e.g. display label, URI).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, serde_json::Value>>,
}

// ── Message types ───────────────────────────────────────────────────────

/// A single message in the Copilot conversation format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CopilotMessage {
    /// Message role (`system`, `user`, or `assistant`).
    pub role: String,
    /// Text content of the message.
    pub content: String,
    /// Optional display name for the message author.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// References attached to this message.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copilot_references: Vec<CopilotReference>,
}

// ── Tool types ──────────────────────────────────────────────────────────

/// The type of a Copilot tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CopilotToolType {
    /// A standard function tool.
    Function,
    /// A confirmation prompt tool.
    Confirmation,
}

/// Copilot-style tool definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CopilotTool {
    /// The tool type.
    #[serde(rename = "type")]
    pub tool_type: CopilotToolType,
    /// The function definition (for function tools).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<CopilotFunctionDef>,
    /// The confirmation definition (for confirmation tools).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<CopilotConfirmation>,
}

/// Function definition inside a Copilot tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CopilotFunctionDef {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

/// Confirmation prompt for user approval flows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CopilotConfirmation {
    /// Unique identifier for this confirmation.
    pub id: String,
    /// Title displayed to the user.
    pub title: String,
    /// Detailed message explaining what the user is approving.
    pub message: String,
    /// Whether the confirmation has been accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted: Option<bool>,
}

/// A function call emitted by the Copilot agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CopilotFunctionCall {
    /// Name of the function to invoke.
    pub name: String,
    /// JSON-encoded arguments.
    pub arguments: String,
    /// Unique call identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// An error reported by the Copilot agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CopilotError {
    /// Error type identifier.
    #[serde(rename = "type")]
    pub error_type: String,
    /// Human-readable error message.
    pub message: String,
    /// Machine-readable error code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// An entry in the turn history for multi-turn conversations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CopilotTurnEntry {
    /// The user message for this turn.
    pub request: String,
    /// The assistant response for this turn.
    pub response: String,
}

// ── Request ─────────────────────────────────────────────────────────────

/// A request to the GitHub Copilot agent API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CopilotRequest {
    /// Model identifier (e.g. `gpt-4o`).
    pub model: String,
    /// Conversation messages.
    pub messages: Vec<CopilotMessage>,
    /// Tool definitions available to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<CopilotTool>>,
    /// Previous turns in the conversation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub turn_history: Vec<CopilotTurnEntry>,
    /// Top-level references for the request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<CopilotReference>,
}

// ── Response ────────────────────────────────────────────────────────────

/// A non-streaming response from the Copilot agent API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CopilotResponse {
    /// The assistant's reply text.
    pub message: String,
    /// References emitted in the response.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copilot_references: Vec<CopilotReference>,
    /// Errors reported during processing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copilot_errors: Vec<CopilotError>,
    /// Confirmation prompt (if the agent requests user approval).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copilot_confirmation: Option<CopilotConfirmation>,
    /// Function call request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_call: Option<CopilotFunctionCall>,
}

// ── Streaming ───────────────────────────────────────────────────────────

/// Server-sent events from the Copilot streaming API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CopilotStreamEvent {
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
    /// A confirmation prompt for user approval.
    CopilotConfirmation {
        /// The confirmation details.
        confirmation: CopilotConfirmation,
    },
    /// Stream completed.
    Done {},
}

// ── Model config ────────────────────────────────────────────────────────

/// Vendor-specific configuration for the GitHub Copilot agent API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct CopilotConfig {
    /// Base URL for the Copilot API.
    pub base_url: String,
    /// Model identifier (e.g. `gpt-4o`).
    pub model: String,
    /// System prompt override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
}

impl Default for CopilotConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.githubcopilot.com".into(),
            model: "gpt-4o".into(),
            system_prompt: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serde_roundtrip() {
        let req = CopilotRequest {
            model: "gpt-4o".into(),
            messages: vec![CopilotMessage {
                role: "user".into(),
                content: "Hello".into(),
                name: None,
                copilot_references: vec![CopilotReference {
                    ref_type: CopilotReferenceType::File,
                    id: "file-0".into(),
                    data: serde_json::json!({"path": "src/main.rs"}),
                    metadata: None,
                }],
            }],
            tools: None,
            turn_history: vec![],
            references: vec![],
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: CopilotRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn response_serde_roundtrip() {
        let resp = CopilotResponse {
            message: "Hello from Copilot!".into(),
            copilot_references: vec![],
            copilot_errors: vec![],
            copilot_confirmation: None,
            function_call: Some(CopilotFunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"src/main.rs"}"#.into(),
                id: Some("call_123".into()),
            }),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: CopilotResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn stream_event_text_delta_roundtrip() {
        let event = CopilotStreamEvent::TextDelta { text: "Hi".into() };
        let json = serde_json::to_string(&event).unwrap();
        let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }

    #[test]
    fn stream_event_done_roundtrip() {
        let event = CopilotStreamEvent::Done {};
        let json = serde_json::to_string(&event).unwrap();
        let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }

    #[test]
    fn reference_with_metadata_roundtrip() {
        let mut meta = BTreeMap::new();
        meta.insert("label".into(), serde_json::json!("Main source"));
        let reference = CopilotReference {
            ref_type: CopilotReferenceType::Snippet,
            id: "snippet-0".into(),
            data: serde_json::json!({"content": "fn main() {}"}),
            metadata: Some(meta),
        };
        let json = serde_json::to_string(&reference).unwrap();
        let back: CopilotReference = serde_json::from_str(&json).unwrap();
        assert_eq!(reference, back);
    }

    #[test]
    fn config_default_values() {
        let cfg = CopilotConfig::default();
        assert!(cfg.base_url.contains("githubcopilot"));
        assert_eq!(cfg.model, "gpt-4o");
    }
}
