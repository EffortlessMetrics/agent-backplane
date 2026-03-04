// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Normalized chat response and streaming types for the IR layer.
//!
//! `IrChatResponse` carries the full non-streaming response, while
//! `IrStreamChunk` represents a single incremental update during
//! server-sent-event streaming.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::ir::{IrContentPart, IrMessage, IrRole, IrToolCall, IrUsage};

// ── Finish reason ───────────────────────────────────────────────────────

/// Why the model stopped generating tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IrFinishReason {
    /// Natural end of turn / stop token.
    Stop,
    /// Output hit the max_tokens limit.
    Length,
    /// Model wants to invoke one or more tools.
    ToolUse,
    /// Content was filtered for safety.
    ContentFilter,
    /// An error terminated generation.
    Error,
}

impl std::fmt::Display for IrFinishReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stop => f.write_str("stop"),
            Self::Length => f.write_str("length"),
            Self::ToolUse => f.write_str("tool_use"),
            Self::ContentFilter => f.write_str("content_filter"),
            Self::Error => f.write_str("error"),
        }
    }
}

// ── Choice ──────────────────────────────────────────────────────────────

/// A single choice in a chat-completions response.
///
/// Most dialects return exactly one choice; OpenAI supports `n > 1`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrChoice {
    /// Zero-based index of this choice.
    pub index: u32,
    /// The assistant message for this choice.
    pub message: IrMessage,
    /// Why the model stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<IrFinishReason>,
}

// ── Chat response ───────────────────────────────────────────────────────

/// A normalized chat-completions response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrChatResponse {
    /// Optional response identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Model that produced this response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Response choices (typically one).
    pub choices: Vec<IrChoice>,

    /// Aggregated token-usage statistics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<IrUsage>,

    /// Vendor-opaque metadata carried through the pipeline.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}

impl IrChatResponse {
    /// Create a response from a single assistant message.
    #[must_use]
    pub fn from_message(message: IrMessage) -> Self {
        Self {
            id: None,
            model: None,
            choices: vec![IrChoice {
                index: 0,
                message,
                finish_reason: Some(IrFinishReason::Stop),
            }],
            usage: None,
            metadata: BTreeMap::new(),
        }
    }

    /// Create a simple text response.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::from_message(IrMessage::text(IrRole::Assistant, text))
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

    /// Builder: set usage statistics.
    #[must_use]
    pub fn with_usage(mut self, usage: IrUsage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Returns the first choice's message, if any.
    #[must_use]
    pub fn first_message(&self) -> Option<&IrMessage> {
        self.choices.first().map(|c| &c.message)
    }

    /// Returns the concatenated text content from the first choice.
    #[must_use]
    pub fn text_content(&self) -> String {
        self.first_message()
            .map(|m| m.text_content())
            .unwrap_or_default()
    }

    /// Returns `true` if any choice contains tool calls.
    #[must_use]
    pub fn has_tool_calls(&self) -> bool {
        self.choices
            .iter()
            .any(|c| !c.message.tool_calls.is_empty())
    }
}

// ── Stream chunk ────────────────────────────────────────────────────────

/// An incremental streaming chunk (SSE delta).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrStreamChunk {
    /// Optional response identifier (usually set in first chunk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Model identifier (usually set in first chunk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Choice index this chunk belongs to.
    #[serde(default)]
    pub index: u32,

    /// Incremental content delta.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delta_content: Vec<IrContentPart>,

    /// Incremental tool-call deltas.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delta_tool_calls: Vec<IrToolCall>,

    /// Role of the message being streamed (usually first chunk only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<IrRole>,

    /// Finish reason (set in the final chunk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<IrFinishReason>,

    /// Token-usage statistics (typically only in the final chunk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<IrUsage>,

    /// Vendor-opaque metadata.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}

impl IrStreamChunk {
    /// Create an empty chunk for the given choice index.
    #[must_use]
    pub fn new(index: u32) -> Self {
        Self {
            id: None,
            model: None,
            index,
            delta_content: Vec::new(),
            delta_tool_calls: Vec::new(),
            role: None,
            finish_reason: None,
            usage: None,
            metadata: BTreeMap::new(),
        }
    }

    /// Create a text-delta chunk.
    #[must_use]
    pub fn text_delta(text: impl Into<String>) -> Self {
        Self {
            id: None,
            model: None,
            index: 0,
            delta_content: vec![IrContentPart::text(text)],
            delta_tool_calls: Vec::new(),
            role: None,
            finish_reason: None,
            usage: None,
            metadata: BTreeMap::new(),
        }
    }

    /// Create a final chunk with a finish reason.
    #[must_use]
    pub fn final_chunk(finish_reason: IrFinishReason) -> Self {
        Self {
            id: None,
            model: None,
            index: 0,
            delta_content: Vec::new(),
            delta_tool_calls: Vec::new(),
            role: None,
            finish_reason: Some(finish_reason),
            usage: None,
            metadata: BTreeMap::new(),
        }
    }

    /// Returns `true` if this chunk has a finish reason set.
    #[must_use]
    pub fn is_final(&self) -> bool {
        self.finish_reason.is_some()
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── IrFinishReason ──────────────────────────────────────────────

    #[test]
    fn finish_reason_serde_roundtrip() {
        for reason in [
            IrFinishReason::Stop,
            IrFinishReason::Length,
            IrFinishReason::ToolUse,
            IrFinishReason::ContentFilter,
            IrFinishReason::Error,
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            let back: IrFinishReason = serde_json::from_str(&json).unwrap();
            assert_eq!(reason, back);
        }
    }

    #[test]
    fn finish_reason_display() {
        assert_eq!(IrFinishReason::Stop.to_string(), "stop");
        assert_eq!(IrFinishReason::Length.to_string(), "length");
        assert_eq!(IrFinishReason::ToolUse.to_string(), "tool_use");
        assert_eq!(IrFinishReason::ContentFilter.to_string(), "content_filter");
        assert_eq!(IrFinishReason::Error.to_string(), "error");
    }

    // ── IrChoice ────────────────────────────────────────────────────

    #[test]
    fn choice_serde_roundtrip() {
        let choice = IrChoice {
            index: 0,
            message: IrMessage::text(IrRole::Assistant, "Hi there!"),
            finish_reason: Some(IrFinishReason::Stop),
        };
        let json = serde_json::to_string(&choice).unwrap();
        let back: IrChoice = serde_json::from_str(&json).unwrap();
        assert_eq!(choice, back);
    }

    #[test]
    fn choice_no_finish_reason() {
        let choice = IrChoice {
            index: 0,
            message: IrMessage::text(IrRole::Assistant, "partial"),
            finish_reason: None,
        };
        let json = serde_json::to_string(&choice).unwrap();
        assert!(!json.contains("finish_reason"));
        let back: IrChoice = serde_json::from_str(&json).unwrap();
        assert_eq!(choice, back);
    }

    // ── IrChatResponse ──────────────────────────────────────────────

    #[test]
    fn chat_response_text() {
        let resp = IrChatResponse::text("Hello!");
        assert_eq!(resp.text_content(), "Hello!");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].index, 0);
        assert_eq!(resp.choices[0].finish_reason, Some(IrFinishReason::Stop));
        assert!(!resp.has_tool_calls());
    }

    #[test]
    fn chat_response_serde_roundtrip() {
        let resp = IrChatResponse::text("Hi")
            .with_id("resp_123")
            .with_model("gpt-4o")
            .with_usage(IrUsage::from_counts(50, 20));
        let json = serde_json::to_string(&resp).unwrap();
        let back: IrChatResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn chat_response_serde_omits_defaults() {
        let resp = IrChatResponse::text("Ok");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(!json.contains("\"id\""));
        assert!(!json.contains("\"model\""));
        assert!(!json.contains("\"usage\""));
        assert!(!json.contains("\"metadata\""));
    }

    #[test]
    fn chat_response_with_tool_calls() {
        let msg = IrMessage {
            role: IrRole::Assistant,
            content: vec![],
            tool_calls: vec![IrToolCall {
                id: "call_1".into(),
                name: "search".into(),
                arguments: serde_json::json!({"q": "rust"}),
            }],
            metadata: BTreeMap::new(),
        };
        let resp = IrChatResponse::from_message(msg);
        assert!(resp.has_tool_calls());
    }

    #[test]
    fn chat_response_empty_choices() {
        let resp = IrChatResponse {
            id: None,
            model: None,
            choices: vec![],
            usage: None,
            metadata: BTreeMap::new(),
        };
        assert_eq!(resp.first_message(), None);
        assert_eq!(resp.text_content(), "");
        assert!(!resp.has_tool_calls());
    }

    #[test]
    fn chat_response_full_roundtrip() {
        let mut meta = BTreeMap::new();
        meta.insert("system_fingerprint".into(), serde_json::json!("fp_abc123"));
        let resp = IrChatResponse {
            id: Some("chatcmpl-xyz".into()),
            model: Some("gpt-4o-2024-08-06".into()),
            choices: vec![
                IrChoice {
                    index: 0,
                    message: IrMessage::text(IrRole::Assistant, "First"),
                    finish_reason: Some(IrFinishReason::Stop),
                },
                IrChoice {
                    index: 1,
                    message: IrMessage::text(IrRole::Assistant, "Second"),
                    finish_reason: Some(IrFinishReason::Length),
                },
            ],
            usage: Some(IrUsage::from_counts(100, 200)),
            metadata: meta,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: IrChatResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(resp, back);
    }

    // ── IrStreamChunk ───────────────────────────────────────────────

    #[test]
    fn stream_chunk_new() {
        let chunk = IrStreamChunk::new(0);
        assert!(!chunk.is_final());
        assert!(chunk.delta_content.is_empty());
        assert!(chunk.delta_tool_calls.is_empty());
    }

    #[test]
    fn stream_chunk_text_delta() {
        let chunk = IrStreamChunk::text_delta("Hello");
        assert!(!chunk.is_final());
        assert_eq!(chunk.delta_content.len(), 1);
        assert_eq!(chunk.delta_content[0].as_text(), Some("Hello"));
    }

    #[test]
    fn stream_chunk_final() {
        let chunk = IrStreamChunk::final_chunk(IrFinishReason::Stop);
        assert!(chunk.is_final());
        assert_eq!(chunk.finish_reason, Some(IrFinishReason::Stop));
    }

    #[test]
    fn stream_chunk_serde_roundtrip() {
        let chunk = IrStreamChunk {
            id: Some("chunk_1".into()),
            model: Some("gpt-4o".into()),
            index: 0,
            delta_content: vec![IrContentPart::text("Hi")],
            delta_tool_calls: Vec::new(),
            role: Some(IrRole::Assistant),
            finish_reason: None,
            usage: None,
            metadata: BTreeMap::new(),
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let back: IrStreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(chunk, back);
    }

    #[test]
    fn stream_chunk_final_with_usage() {
        let chunk = IrStreamChunk {
            id: None,
            model: None,
            index: 0,
            delta_content: Vec::new(),
            delta_tool_calls: Vec::new(),
            role: None,
            finish_reason: Some(IrFinishReason::Stop),
            usage: Some(IrUsage::from_counts(100, 50)),
            metadata: BTreeMap::new(),
        };
        assert!(chunk.is_final());
        let json = serde_json::to_string(&chunk).unwrap();
        let back: IrStreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(chunk, back);
    }

    #[test]
    fn stream_chunk_serde_omits_defaults() {
        let chunk = IrStreamChunk::new(0);
        let json = serde_json::to_string(&chunk).unwrap();
        assert!(!json.contains("\"id\""));
        assert!(!json.contains("\"model\""));
        assert!(!json.contains("delta_content"));
        assert!(!json.contains("delta_tool_calls"));
        assert!(!json.contains("role"));
        assert!(!json.contains("finish_reason"));
        assert!(!json.contains("usage"));
        assert!(!json.contains("metadata"));
    }

    #[test]
    fn stream_chunk_with_tool_call_delta() {
        let chunk = IrStreamChunk {
            id: None,
            model: None,
            index: 0,
            delta_content: Vec::new(),
            delta_tool_calls: vec![IrToolCall {
                id: "call_1".into(),
                name: "search".into(),
                arguments: serde_json::json!({"q": "rust"}),
            }],
            role: None,
            finish_reason: None,
            usage: None,
            metadata: BTreeMap::new(),
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let back: IrStreamChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(chunk, back);
    }
}
