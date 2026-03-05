// SPDX-License-Identifier: MIT OR Apache-2.0
//! Shared types used across dialect modules.

use serde::{Deserialize, Serialize};

// ── Message roles ───────────────────────────────────────────────────────

/// Message role common across most dialects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// System-level instruction.
    System,
    /// User message.
    User,
    /// Assistant (model) response.
    Assistant,
    /// Tool/function result.
    Tool,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::System => f.write_str("system"),
            Self::User => f.write_str("user"),
            Self::Assistant => f.write_str("assistant"),
            Self::Tool => f.write_str("tool"),
        }
    }
}

// ── Token counting ──────────────────────────────────────────────────────

/// Normalized token usage across all dialects.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct TokenUsage {
    /// Tokens consumed by the prompt / input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// Tokens generated in the response / output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    /// Total tokens (input + output).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
}

impl TokenUsage {
    /// Creates a usage record from input and output counts, computing total.
    #[must_use]
    pub fn from_io(input: u64, output: u64) -> Self {
        Self {
            input_tokens: Some(input),
            output_tokens: Some(output),
            total_tokens: Some(input + output),
        }
    }
}

/// Pre-generation token estimate for budget planning.
///
/// Used before a request is sent to check whether the conversation fits
/// within a model's context window and how many tokens remain for generation.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct TokenEstimate {
    /// Estimated input/prompt token count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// Maximum context window size for the target model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    /// Remaining tokens available for generation (context_window − input_tokens).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_tokens: Option<u64>,
}

impl TokenEstimate {
    /// Computes a token estimate given input count and context window size.
    #[must_use]
    pub fn new(input_tokens: u64, context_window: u64) -> Self {
        Self {
            input_tokens: Some(input_tokens),
            context_window: Some(context_window),
            remaining_tokens: Some(context_window.saturating_sub(input_tokens)),
        }
    }

    /// Returns `true` if the estimated input exceeds the context window.
    #[must_use]
    pub fn exceeds_context(&self) -> bool {
        match (self.input_tokens, self.context_window) {
            (Some(input), Some(window)) => input > window,
            _ => false,
        }
    }
}

// ── Finish reason ───────────────────────────────────────────────────────

/// Finish reason normalized across dialects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Natural stop or end-of-turn.
    Stop,
    /// Model wants to invoke one or more tools.
    ToolUse,
    /// Output hit the max_tokens limit.
    MaxTokens,
    /// A stop sequence was matched.
    StopSequence,
    /// Content was filtered for safety.
    ContentFilter,
}

// ── Model identification ────────────────────────────────────────────────

/// Structured model identifier for routing and capability lookup.
///
/// Provides a richer alternative to bare model name strings, enabling
/// the projection matrix to route requests based on provider + model family.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub struct ModelId {
    /// Provider name (e.g. `"openai"`, `"anthropic"`, `"google"`, `"moonshot"`).
    pub provider: String,
    /// Model name as used in API calls (e.g. `"gpt-4o"`, `"claude-sonnet-4-20250514"`).
    pub name: String,
    /// Optional version or snapshot tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl ModelId {
    /// Creates a new model identifier.
    #[must_use]
    pub fn new(provider: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            name: name.into(),
            version: None,
        }
    }

    /// Creates a model identifier with a version tag.
    #[must_use]
    pub fn with_version(
        provider: impl Into<String>,
        name: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            name: name.into(),
            version: Some(version.into()),
        }
    }
}

impl std::fmt::Display for ModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.provider, self.name)?;
        if let Some(v) = &self.version {
            write!(f, "@{v}")?;
        }
        Ok(())
    }
}

// ── Multimodal content ──────────────────────────────────────────────────

/// A content part in a multimodal message.
///
/// This is the shared vocabulary for multimodal content across all SDK shims.
/// Each dialect module converts its native content types to/from this enum.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    /// Plain text content.
    Text {
        /// The text payload.
        text: String,
    },
    /// Image content (base64-encoded or URL reference).
    Image {
        /// MIME type (e.g. `"image/png"`, `"image/jpeg"`).
        media_type: String,
        /// Base64-encoded image data or a URL.
        data: String,
    },
    /// Audio content (base64-encoded).
    Audio {
        /// MIME type (e.g. `"audio/wav"`, `"audio/mp3"`).
        media_type: String,
        /// Base64-encoded audio data.
        data: String,
    },
}

impl ContentPart {
    /// Creates a text content part.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// Creates an image content part.
    #[must_use]
    pub fn image(media_type: impl Into<String>, data: impl Into<String>) -> Self {
        Self::Image {
            media_type: media_type.into(),
            data: data.into(),
        }
    }

    /// Creates an audio content part.
    #[must_use]
    pub fn audio(media_type: impl Into<String>, data: impl Into<String>) -> Self {
        Self::Audio {
            media_type: media_type.into(),
            data: data.into(),
        }
    }

    /// Returns `true` if this is a text content part.
    #[must_use]
    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text { .. })
    }

    /// Returns the text content if this is a text part, `None` otherwise.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }
}

// ── Streaming ───────────────────────────────────────────────────────────

/// A vendor-agnostic streaming chunk used as a common denominator.
///
/// Each dialect has richer streaming types; this captures the minimal
/// intersection used during cross-dialect stream translation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct StreamDelta {
    /// Role of the message being streamed (usually only in the first chunk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    /// Incremental content parts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<ContentPart>,
    /// Finish reason (set in the final chunk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<FinishReason>,
    /// Token usage (typically only in the final chunk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
}

// ── Function / tool call ────────────────────────────────────────────────

/// A vendor-agnostic function/tool call emitted by a model.
///
/// Normalized representation of a tool invocation request across all dialects.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct FunctionCall {
    /// Unique identifier for this call (correlates with the tool result).
    pub id: String,
    /// Name of the function/tool to invoke.
    pub name: String,
    /// JSON-encoded arguments.
    pub arguments: String,
}

/// A vendor-agnostic function/tool result returned to the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct FunctionResult {
    /// The call ID this result corresponds to.
    pub call_id: String,
    /// The result content (typically JSON or plain text).
    pub content: String,
    /// Whether the tool execution produced an error.
    #[serde(default)]
    pub is_error: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_serde_roundtrip() {
        for role in [Role::System, Role::User, Role::Assistant, Role::Tool] {
            let json = serde_json::to_string(&role).unwrap();
            let back: Role = serde_json::from_str(&json).unwrap();
            assert_eq!(role, back);
        }
    }

    #[test]
    fn role_display() {
        assert_eq!(Role::System.to_string(), "system");
        assert_eq!(Role::User.to_string(), "user");
        assert_eq!(Role::Assistant.to_string(), "assistant");
        assert_eq!(Role::Tool.to_string(), "tool");
    }

    #[test]
    fn token_usage_serde_roundtrip() {
        let usage = TokenUsage {
            input_tokens: Some(100),
            output_tokens: Some(50),
            total_tokens: Some(150),
        };
        let json = serde_json::to_string(&usage).unwrap();
        let back: TokenUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(usage, back);
    }

    #[test]
    fn token_usage_from_io() {
        let usage = TokenUsage::from_io(100, 50);
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.output_tokens, Some(50));
        assert_eq!(usage.total_tokens, Some(150));
    }

    #[test]
    fn token_usage_default_is_all_none() {
        let usage = TokenUsage::default();
        assert_eq!(usage.input_tokens, None);
        assert_eq!(usage.output_tokens, None);
        assert_eq!(usage.total_tokens, None);
    }

    #[test]
    fn token_estimate_new() {
        let est = TokenEstimate::new(1000, 4096);
        assert_eq!(est.input_tokens, Some(1000));
        assert_eq!(est.context_window, Some(4096));
        assert_eq!(est.remaining_tokens, Some(3096));
        assert!(!est.exceeds_context());
    }

    #[test]
    fn token_estimate_exceeds_context() {
        let est = TokenEstimate::new(5000, 4096);
        assert!(est.exceeds_context());
        assert_eq!(est.remaining_tokens, Some(0));
    }

    #[test]
    fn token_estimate_default_does_not_exceed() {
        let est = TokenEstimate::default();
        assert!(!est.exceeds_context());
    }

    #[test]
    fn token_estimate_serde_roundtrip() {
        let est = TokenEstimate::new(500, 8192);
        let json = serde_json::to_string(&est).unwrap();
        let back: TokenEstimate = serde_json::from_str(&json).unwrap();
        assert_eq!(est, back);
    }

    #[test]
    fn finish_reason_serde_roundtrip() {
        for reason in [
            FinishReason::Stop,
            FinishReason::ToolUse,
            FinishReason::MaxTokens,
            FinishReason::StopSequence,
            FinishReason::ContentFilter,
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            let back: FinishReason = serde_json::from_str(&json).unwrap();
            assert_eq!(reason, back);
        }
    }

    #[test]
    fn model_id_new() {
        let id = ModelId::new("openai", "gpt-4o");
        assert_eq!(id.provider, "openai");
        assert_eq!(id.name, "gpt-4o");
        assert_eq!(id.version, None);
    }

    #[test]
    fn model_id_with_version() {
        let id = ModelId::with_version("anthropic", "claude-sonnet-4-20250514", "2025-05-14");
        assert_eq!(id.provider, "anthropic");
        assert_eq!(id.version, Some("2025-05-14".into()));
    }

    #[test]
    fn model_id_display() {
        let id = ModelId::new("openai", "gpt-4o");
        assert_eq!(id.to_string(), "openai/gpt-4o");

        let id_v = ModelId::with_version("anthropic", "claude-sonnet-4-20250514", "2025-05-14");
        assert_eq!(
            id_v.to_string(),
            "anthropic/claude-sonnet-4-20250514@2025-05-14"
        );
    }

    #[test]
    fn model_id_serde_roundtrip() {
        let id = ModelId::with_version("google", "gemini-2.5-flash", "001");
        let json = serde_json::to_string(&id).unwrap();
        let back: ModelId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn model_id_serde_omits_none_version() {
        let id = ModelId::new("openai", "gpt-4o");
        let json = serde_json::to_string(&id).unwrap();
        assert!(!json.contains("version"));
    }

    #[test]
    fn model_id_eq_and_hash() {
        use std::collections::HashSet;
        let a = ModelId::new("openai", "gpt-4o");
        let b = ModelId::new("openai", "gpt-4o");
        let c = ModelId::new("anthropic", "claude-sonnet-4-20250514");
        assert_eq!(a, b);
        assert_ne!(a, c);
        let mut set = HashSet::new();
        set.insert(a.clone());
        set.insert(b);
        assert_eq!(set.len(), 1);
        set.insert(c);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn content_part_text() {
        let part = ContentPart::text("Hello");
        assert!(part.is_text());
        assert_eq!(part.as_text(), Some("Hello"));
    }

    #[test]
    fn content_part_image() {
        let part = ContentPart::image("image/png", "iVBOR...");
        assert!(!part.is_text());
        assert_eq!(part.as_text(), None);
    }

    #[test]
    fn content_part_audio() {
        let part = ContentPart::audio("audio/wav", "RIFF...");
        assert!(!part.is_text());
        assert_eq!(part.as_text(), None);
    }

    #[test]
    fn content_part_serde_roundtrip_text() {
        let part = ContentPart::text("Hello world");
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        let back: ContentPart = serde_json::from_str(&json).unwrap();
        assert_eq!(part, back);
    }

    #[test]
    fn content_part_serde_roundtrip_image() {
        let part = ContentPart::image("image/jpeg", "base64data");
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains("\"type\":\"image\""));
        let back: ContentPart = serde_json::from_str(&json).unwrap();
        assert_eq!(part, back);
    }

    #[test]
    fn content_part_serde_roundtrip_audio() {
        let part = ContentPart::audio("audio/mp3", "base64audio");
        let json = serde_json::to_string(&part).unwrap();
        assert!(json.contains("\"type\":\"audio\""));
        let back: ContentPart = serde_json::from_str(&json).unwrap();
        assert_eq!(part, back);
    }

    #[test]
    fn stream_delta_serde_roundtrip() {
        let delta = StreamDelta {
            role: Some(Role::Assistant),
            content: vec![ContentPart::text("Hi")],
            finish_reason: None,
            usage: None,
        };
        let json = serde_json::to_string(&delta).unwrap();
        let back: StreamDelta = serde_json::from_str(&json).unwrap();
        assert_eq!(delta, back);
    }

    #[test]
    fn stream_delta_final_chunk() {
        let delta = StreamDelta {
            role: None,
            content: vec![],
            finish_reason: Some(FinishReason::Stop),
            usage: Some(TokenUsage::from_io(100, 50)),
        };
        let json = serde_json::to_string(&delta).unwrap();
        let back: StreamDelta = serde_json::from_str(&json).unwrap();
        assert_eq!(delta, back);
    }

    #[test]
    fn function_call_serde_roundtrip() {
        let call = FunctionCall {
            id: "call_123".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"src/main.rs"}"#.into(),
        };
        let json = serde_json::to_string(&call).unwrap();
        let back: FunctionCall = serde_json::from_str(&json).unwrap();
        assert_eq!(call, back);
    }

    #[test]
    fn function_result_serde_roundtrip() {
        let result = FunctionResult {
            call_id: "call_123".into(),
            content: "file contents here".into(),
            is_error: false,
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: FunctionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result, back);
    }

    #[test]
    fn function_result_error_serde_roundtrip() {
        let result = FunctionResult {
            call_id: "call_456".into(),
            content: "file not found".into(),
            is_error: true,
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: FunctionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result, back);
        assert!(back.is_error);
    }
}
