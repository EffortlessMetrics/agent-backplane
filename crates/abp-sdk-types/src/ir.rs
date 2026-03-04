// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Normalized intermediate-representation (IR) message types for cross-dialect mapping.
//!
//! These types form the canonical vocabulary that every SDK dialect is
//! lowered into and raised out of.  They deliberately carry *more* detail
//! than the per-dialect common types in [`crate::common`] so that
//! translation is as lossless as possible.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

// ── Roles ───────────────────────────────────────────────────────────────

/// Normalized message role across all dialects.
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

impl std::fmt::Display for IrRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::System => f.write_str("system"),
            Self::User => f.write_str("user"),
            Self::Assistant => f.write_str("assistant"),
            Self::Tool => f.write_str("tool"),
        }
    }
}

// ── Content parts ───────────────────────────────────────────────────────

/// A single content part inside an [`IrMessage`].
///
/// Covers text, multimodal media, tool invocations, and tool results so
/// that no information is lost when lowering a vendor message to the IR.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IrContentPart {
    /// Plain text content.
    Text {
        /// The text payload.
        text: String,
    },
    /// Image content (base64-encoded data or a URL reference).
    Image {
        /// URL to the image, if available.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        /// Base64-encoded image bytes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base64: Option<String>,
        /// MIME type (e.g. `"image/png"`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        media_type: Option<String>,
    },
    /// Audio content (base64-encoded).
    Audio {
        /// MIME type (e.g. `"audio/wav"`).
        media_type: String,
        /// Base64-encoded audio bytes.
        data: String,
    },
    /// File attachment.
    File {
        /// File name or path.
        name: String,
        /// MIME type.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        media_type: Option<String>,
        /// Base64-encoded file content.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<String>,
        /// URL to the file, if available.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    /// A tool invocation requested by the model.
    ToolUse {
        /// Unique identifier for this invocation.
        id: String,
        /// Tool name.
        name: String,
        /// JSON-encoded input arguments.
        arguments: Value,
    },
    /// The result of a prior tool invocation.
    ToolResult {
        /// Identifier of the corresponding tool call.
        call_id: String,
        /// Result content (text or structured).
        content: String,
        /// Whether the tool reported an error.
        #[serde(default)]
        is_error: bool,
    },
}

impl IrContentPart {
    /// Create a text content part.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// Create an image content part from base64 data.
    #[must_use]
    pub fn image_base64(media_type: impl Into<String>, base64: impl Into<String>) -> Self {
        Self::Image {
            url: None,
            base64: Some(base64.into()),
            media_type: Some(media_type.into()),
        }
    }

    /// Create an image content part from a URL.
    #[must_use]
    pub fn image_url(url: impl Into<String>) -> Self {
        Self::Image {
            url: Some(url.into()),
            base64: None,
            media_type: None,
        }
    }

    /// Returns `true` if this is a [`IrContentPart::Text`] part.
    #[must_use]
    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text { .. })
    }

    /// Returns the text payload if this is a text part.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }

    /// Returns `true` if this part is a tool-use block.
    #[must_use]
    pub fn is_tool_use(&self) -> bool {
        matches!(self, Self::ToolUse { .. })
    }

    /// Returns `true` if this part is a tool-result block.
    #[must_use]
    pub fn is_tool_result(&self) -> bool {
        matches!(self, Self::ToolResult { .. })
    }
}

// ── Tool call ───────────────────────────────────────────────────────────

/// A standalone tool call structure used in [`IrMessage`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrToolCall {
    /// Unique identifier for this call (correlates with the tool result).
    pub id: String,
    /// Name of the tool to invoke.
    pub name: String,
    /// JSON-encoded arguments.
    pub arguments: Value,
}

// ── Tool result ─────────────────────────────────────────────────────────

/// A standalone tool result structure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrToolResult {
    /// The call ID this result corresponds to.
    pub call_id: String,
    /// Result content (text or structured).
    pub content: String,
    /// Whether the tool execution produced an error.
    #[serde(default)]
    pub is_error: bool,
}

// ── Tool definition ─────────────────────────────────────────────────────

/// A normalized tool definition with JSON Schema parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrToolDefinition {
    /// Unique tool name.
    pub name: String,
    /// Human-readable description of the tool.
    pub description: String,
    /// JSON Schema describing the tool's input parameters.
    pub parameters: Value,
}

// ── Messages ────────────────────────────────────────────────────────────

/// A normalized message in a conversation.
///
/// Carries role, multimodal content parts, and optional tool calls in a
/// single structure that every dialect can be mapped into.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrMessage {
    /// The role of the message author.
    pub role: IrRole,
    /// Ordered content parts that make up the message body.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<IrContentPart>,
    /// Tool calls emitted by the assistant in this turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<IrToolCall>,
    /// Optional vendor-opaque metadata carried through the pipeline.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}

impl IrMessage {
    /// Create a new message with the given role and content parts.
    #[must_use]
    pub fn new(role: IrRole, content: Vec<IrContentPart>) -> Self {
        Self {
            role,
            content,
            tool_calls: Vec::new(),
            metadata: BTreeMap::new(),
        }
    }

    /// Create a simple text message.
    #[must_use]
    pub fn text(role: IrRole, text: impl Into<String>) -> Self {
        Self::new(role, vec![IrContentPart::text(text)])
    }

    /// Returns `true` if every content part is text.
    #[must_use]
    pub fn is_text_only(&self) -> bool {
        self.tool_calls.is_empty() && self.content.iter().all(|p| p.is_text())
    }

    /// Concatenate all text content parts into a single string.
    #[must_use]
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|p| p.as_text())
            .collect::<Vec<_>>()
            .join("")
    }
}

// ── Usage ───────────────────────────────────────────────────────────────

/// Normalized token-usage counters across all dialects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IrUsage {
    /// Number of prompt / input tokens.
    pub prompt_tokens: u64,
    /// Number of completion / output tokens.
    pub completion_tokens: u64,
    /// Sum of prompt + completion tokens.
    pub total_tokens: u64,
    /// Tokens served from a KV-cache read.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cached_tokens: u64,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

impl IrUsage {
    /// Construct usage from prompt/completion counts, computing `total_tokens`.
    #[must_use]
    pub fn from_counts(prompt: u64, completion: u64) -> Self {
        Self {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
            cached_tokens: 0,
        }
    }

    /// Construct usage with a cache-read count.
    #[must_use]
    pub fn with_cached(prompt: u64, completion: u64, cached: u64) -> Self {
        Self {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
            cached_tokens: cached,
        }
    }

    /// Merge two usage records by summing all fields.
    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        Self {
            prompt_tokens: self.prompt_tokens + other.prompt_tokens,
            completion_tokens: self.completion_tokens + other.completion_tokens,
            total_tokens: self.total_tokens + other.total_tokens,
            cached_tokens: self.cached_tokens + other.cached_tokens,
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── IrRole ──────────────────────────────────────────────────────

    #[test]
    fn ir_role_serde_roundtrip() {
        for role in [IrRole::System, IrRole::User, IrRole::Assistant, IrRole::Tool] {
            let json = serde_json::to_string(&role).unwrap();
            let back: IrRole = serde_json::from_str(&json).unwrap();
            assert_eq!(role, back);
        }
    }

    #[test]
    fn ir_role_display() {
        assert_eq!(IrRole::System.to_string(), "system");
        assert_eq!(IrRole::User.to_string(), "user");
        assert_eq!(IrRole::Assistant.to_string(), "assistant");
        assert_eq!(IrRole::Tool.to_string(), "tool");
    }

    // ── IrContentPart ───────────────────────────────────────────────

    #[test]
    fn content_part_text_roundtrip() {
        let part = IrContentPart::text("Hello world");
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        let back: IrContentPart = serde_json::from_str(&json).unwrap();
        assert_eq!(part, back);
        assert!(back.is_text());
        assert_eq!(back.as_text(), Some("Hello world"));
    }

    #[test]
    fn content_part_image_base64_roundtrip() {
        let part = IrContentPart::image_base64("image/png", "iVBOR...");
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains("\"type\":\"image\""));
        let back: IrContentPart = serde_json::from_str(&json).unwrap();
        assert_eq!(part, back);
        assert!(!back.is_text());
    }

    #[test]
    fn content_part_image_url_roundtrip() {
        let part = IrContentPart::image_url("https://example.com/img.png");
        let json = serde_json::to_string(&part).unwrap();
        let back: IrContentPart = serde_json::from_str(&json).unwrap();
        assert_eq!(part, back);
    }

    #[test]
    fn content_part_audio_roundtrip() {
        let part = IrContentPart::Audio {
            media_type: "audio/wav".into(),
            data: "RIFF...".into(),
        };
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains("\"type\":\"audio\""));
        let back: IrContentPart = serde_json::from_str(&json).unwrap();
        assert_eq!(part, back);
    }

    #[test]
    fn content_part_file_roundtrip() {
        let part = IrContentPart::File {
            name: "readme.md".into(),
            media_type: Some("text/markdown".into()),
            data: Some("base64data".into()),
            url: None,
        };
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains("\"type\":\"file\""));
        let back: IrContentPart = serde_json::from_str(&json).unwrap();
        assert_eq!(part, back);
    }

    #[test]
    fn content_part_tool_use_roundtrip() {
        let part = IrContentPart::ToolUse {
            id: "call_1".into(),
            name: "read_file".into(),
            arguments: serde_json::json!({"path": "src/main.rs"}),
        };
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains("\"type\":\"tool_use\""));
        let back: IrContentPart = serde_json::from_str(&json).unwrap();
        assert_eq!(part, back);
        assert!(back.is_tool_use());
        assert!(!back.is_tool_result());
    }

    #[test]
    fn content_part_tool_result_roundtrip() {
        let part = IrContentPart::ToolResult {
            call_id: "call_1".into(),
            content: "file contents".into(),
            is_error: false,
        };
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains("\"type\":\"tool_result\""));
        let back: IrContentPart = serde_json::from_str(&json).unwrap();
        assert_eq!(part, back);
        assert!(back.is_tool_result());
        assert!(!back.is_tool_use());
    }

    #[test]
    fn content_part_tool_result_error_roundtrip() {
        let part = IrContentPart::ToolResult {
            call_id: "call_2".into(),
            content: "file not found".into(),
            is_error: true,
        };
        let json = serde_json::to_string(&part).unwrap();
        let back: IrContentPart = serde_json::from_str(&json).unwrap();
        assert_eq!(part, back);
    }

    // ── IrToolCall ──────────────────────────────────────────────────

    #[test]
    fn tool_call_serde_roundtrip() {
        let tc = IrToolCall {
            id: "call_abc".into(),
            name: "search".into(),
            arguments: serde_json::json!({"query": "rust"}),
        };
        let json = serde_json::to_string(&tc).unwrap();
        let back: IrToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(tc, back);
    }

    // ── IrToolResult ────────────────────────────────────────────────

    #[test]
    fn tool_result_serde_roundtrip() {
        let tr = IrToolResult {
            call_id: "call_abc".into(),
            content: "42 results".into(),
            is_error: false,
        };
        let json = serde_json::to_string(&tr).unwrap();
        let back: IrToolResult = serde_json::from_str(&json).unwrap();
        assert_eq!(tr, back);
    }

    #[test]
    fn tool_result_error_serde_roundtrip() {
        let tr = IrToolResult {
            call_id: "call_xyz".into(),
            content: "timeout".into(),
            is_error: true,
        };
        let json = serde_json::to_string(&tr).unwrap();
        let back: IrToolResult = serde_json::from_str(&json).unwrap();
        assert_eq!(tr, back);
        assert!(back.is_error);
    }

    // ── IrToolDefinition ────────────────────────────────────────────

    #[test]
    fn tool_definition_serde_roundtrip() {
        let td = IrToolDefinition {
            name: "read_file".into(),
            description: "Read a file from disk".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        };
        let json = serde_json::to_string(&td).unwrap();
        let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(td, back);
    }

    #[test]
    fn tool_definition_empty_params() {
        let td = IrToolDefinition {
            name: "noop".into(),
            description: "Does nothing".into(),
            parameters: serde_json::json!({}),
        };
        let json = serde_json::to_string(&td).unwrap();
        let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(td, back);
    }

    // ── IrMessage ───────────────────────────────────────────────────

    #[test]
    fn message_text_roundtrip() {
        let msg = IrMessage::text(IrRole::User, "Hello");
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
        assert!(back.is_text_only());
        assert_eq!(back.text_content(), "Hello");
    }

    #[test]
    fn message_with_tool_calls() {
        let msg = IrMessage {
            role: IrRole::Assistant,
            content: vec![IrContentPart::text("Let me check that.")],
            tool_calls: vec![IrToolCall {
                id: "call_1".into(),
                name: "search".into(),
                arguments: serde_json::json!({"q": "rust"}),
            }],
            metadata: BTreeMap::new(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
        assert!(!back.is_text_only());
    }

    #[test]
    fn message_with_metadata() {
        let mut meta = BTreeMap::new();
        meta.insert("vendor_id".into(), serde_json::json!("msg_123"));
        let msg = IrMessage {
            role: IrRole::Assistant,
            content: vec![IrContentPart::text("Hi")],
            tool_calls: Vec::new(),
            metadata: meta,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn message_empty_content_text() {
        let msg = IrMessage::new(IrRole::User, vec![]);
        assert_eq!(msg.text_content(), "");
        assert!(msg.is_text_only());
    }

    #[test]
    fn message_multipart_text_content() {
        let msg = IrMessage::new(
            IrRole::User,
            vec![
                IrContentPart::text("Hello "),
                IrContentPart::text("world"),
            ],
        );
        assert_eq!(msg.text_content(), "Hello world");
    }

    // ── IrUsage ─────────────────────────────────────────────────────

    #[test]
    fn usage_from_counts() {
        let u = IrUsage::from_counts(100, 50);
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
        assert_eq!(u.total_tokens, 150);
        assert_eq!(u.cached_tokens, 0);
    }

    #[test]
    fn usage_with_cached() {
        let u = IrUsage::with_cached(200, 80, 50);
        assert_eq!(u.prompt_tokens, 200);
        assert_eq!(u.completion_tokens, 80);
        assert_eq!(u.total_tokens, 280);
        assert_eq!(u.cached_tokens, 50);
    }

    #[test]
    fn usage_default() {
        let u = IrUsage::default();
        assert_eq!(u.prompt_tokens, 0);
        assert_eq!(u.completion_tokens, 0);
        assert_eq!(u.total_tokens, 0);
        assert_eq!(u.cached_tokens, 0);
    }

    #[test]
    fn usage_merge() {
        let a = IrUsage::from_counts(100, 50);
        let b = IrUsage::with_cached(200, 80, 30);
        let merged = a.merge(b);
        assert_eq!(merged.prompt_tokens, 300);
        assert_eq!(merged.completion_tokens, 130);
        assert_eq!(merged.total_tokens, 430);
        assert_eq!(merged.cached_tokens, 30);
    }

    #[test]
    fn usage_serde_roundtrip() {
        let u = IrUsage::with_cached(100, 50, 25);
        let json = serde_json::to_string(&u).unwrap();
        let back: IrUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(u, back);
    }

    #[test]
    fn usage_serde_omits_zero_cached() {
        let u = IrUsage::from_counts(100, 50);
        let json = serde_json::to_string(&u).unwrap();
        assert!(!json.contains("cached_tokens"));
    }
}
