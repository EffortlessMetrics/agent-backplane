// SPDX-License-Identifier: MIT OR Apache-2.0
//! Capability advertisement for sidecar `hello` handshake.
//!
//! A sidecar can attach a [`CapabilityAdvertisement`] to its `hello` envelope
//! to declare fine-grained information about its supported dialects, tool
//! support levels, streaming modes, maximum context length, and accepted
//! content types.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Dialect
// ---------------------------------------------------------------------------

/// A dialect (vendor SDK surface) that a sidecar can speak.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dialect {
    /// OpenAI chat completions API surface.
    OpenAi,
    /// Anthropic Messages API surface.
    Anthropic,
    /// Google Gemini API surface.
    Gemini,
    /// Generic / vendor-agnostic surface.
    Generic,
    /// A custom dialect identified by name.
    Custom(String),
}

// ---------------------------------------------------------------------------
// ToolSupportLevel
// ---------------------------------------------------------------------------

/// Granularity of tool-use support advertised by a sidecar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSupportLevel {
    /// No tool support.
    None,
    /// Single tool call per turn.
    SingleCall,
    /// Multiple parallel tool calls per turn.
    ParallelCalls,
    /// Streaming tool calls (partial arguments).
    StreamingCalls,
}

// ---------------------------------------------------------------------------
// StreamingMode
// ---------------------------------------------------------------------------

/// How the sidecar delivers its output.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamingMode {
    /// Responses arrive in a single batch after completion.
    Batch,
    /// Server-sent-event style token streaming.
    Sse,
    /// JSONL line-by-line streaming (ABP native).
    Jsonl,
}

// ---------------------------------------------------------------------------
// ContentType
// ---------------------------------------------------------------------------

/// MIME-like content types the sidecar can accept.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    /// Plain text (`text/plain`).
    Text,
    /// JSON (`application/json`).
    Json,
    /// Markdown (`text/markdown`).
    Markdown,
    /// Image input (e.g. base64-encoded).
    Image,
    /// PDF document.
    Pdf,
    /// Audio input.
    Audio,
    /// A custom content type identified by a MIME string.
    Custom(String),
}

// ---------------------------------------------------------------------------
// CapabilityAdvertisement
// ---------------------------------------------------------------------------

/// Detailed capability advertisement attached to a sidecar `hello`.
///
/// # Examples
///
/// ```
/// use abp_protocol::capability_advertisement::*;
///
/// let ad = CapabilityAdvertisement::builder()
///     .dialect(Dialect::Anthropic)
///     .dialect(Dialect::OpenAi)
///     .tool_support(ToolSupportLevel::ParallelCalls)
///     .streaming_mode(StreamingMode::Jsonl)
///     .max_context_length(200_000)
///     .content_type(ContentType::Text)
///     .content_type(ContentType::Image)
///     .build();
///
/// assert_eq!(ad.dialects().len(), 2);
/// assert_eq!(ad.max_context_length(), Some(200_000));
/// assert!(ad.supports_content_type(&ContentType::Image));
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityAdvertisement {
    /// Dialects this sidecar speaks, in preference order.
    dialects: Vec<Dialect>,
    /// Level of tool-use support.
    tool_support: ToolSupportLevel,
    /// Supported streaming modes.
    streaming_modes: Vec<StreamingMode>,
    /// Maximum context window in tokens, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_context_length: Option<u64>,
    /// Content types the sidecar can accept as input.
    content_types: Vec<ContentType>,
    /// Vendor-specific extension metadata.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    extensions: BTreeMap<String, serde_json::Value>,
}

impl CapabilityAdvertisement {
    /// Start building a new advertisement.
    #[must_use]
    pub fn builder() -> CapabilityAdvertisementBuilder {
        CapabilityAdvertisementBuilder::default()
    }

    /// The dialects this sidecar supports.
    #[must_use]
    pub fn dialects(&self) -> &[Dialect] {
        &self.dialects
    }

    /// Tool-use support level.
    #[must_use]
    pub fn tool_support(&self) -> &ToolSupportLevel {
        &self.tool_support
    }

    /// Supported streaming modes.
    #[must_use]
    pub fn streaming_modes(&self) -> &[StreamingMode] {
        &self.streaming_modes
    }

    /// Maximum context length in tokens, if declared.
    #[must_use]
    pub fn max_context_length(&self) -> Option<u64> {
        self.max_context_length
    }

    /// Content types this sidecar accepts.
    #[must_use]
    pub fn content_types(&self) -> &[ContentType] {
        &self.content_types
    }

    /// Extension metadata map.
    #[must_use]
    pub fn extensions(&self) -> &BTreeMap<String, serde_json::Value> {
        &self.extensions
    }

    /// Returns `true` if the sidecar speaks the given dialect.
    #[must_use]
    pub fn supports_dialect(&self, dialect: &Dialect) -> bool {
        self.dialects.contains(dialect)
    }

    /// Returns `true` if the sidecar accepts the given content type.
    #[must_use]
    pub fn supports_content_type(&self, ct: &ContentType) -> bool {
        self.content_types.contains(ct)
    }

    /// Returns `true` if the sidecar supports the given streaming mode.
    #[must_use]
    pub fn supports_streaming_mode(&self, mode: &StreamingMode) -> bool {
        self.streaming_modes.contains(mode)
    }

    /// Find the first dialect both sides support.
    #[must_use]
    pub fn negotiate_dialect(&self, other: &CapabilityAdvertisement) -> Option<Dialect> {
        self.dialects
            .iter()
            .find(|d| other.dialects.contains(d))
            .cloned()
    }
}

impl Default for CapabilityAdvertisement {
    fn default() -> Self {
        Self {
            dialects: vec![Dialect::Generic],
            tool_support: ToolSupportLevel::None,
            streaming_modes: vec![StreamingMode::Jsonl],
            max_context_length: None,
            content_types: vec![ContentType::Text],
            extensions: BTreeMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Builder for [`CapabilityAdvertisement`].
#[derive(Debug, Default)]
pub struct CapabilityAdvertisementBuilder {
    dialects: Vec<Dialect>,
    tool_support: Option<ToolSupportLevel>,
    streaming_modes: Vec<StreamingMode>,
    max_context_length: Option<u64>,
    content_types: Vec<ContentType>,
    extensions: BTreeMap<String, serde_json::Value>,
}

impl CapabilityAdvertisementBuilder {
    /// Add a supported dialect.
    pub fn dialect(&mut self, d: Dialect) -> &mut Self {
        if !self.dialects.contains(&d) {
            self.dialects.push(d);
        }
        self
    }

    /// Set tool support level.
    pub fn tool_support(&mut self, level: ToolSupportLevel) -> &mut Self {
        self.tool_support = Some(level);
        self
    }

    /// Add a supported streaming mode.
    pub fn streaming_mode(&mut self, mode: StreamingMode) -> &mut Self {
        if !self.streaming_modes.contains(&mode) {
            self.streaming_modes.push(mode);
        }
        self
    }

    /// Set the maximum context length in tokens.
    pub fn max_context_length(&mut self, tokens: u64) -> &mut Self {
        self.max_context_length = Some(tokens);
        self
    }

    /// Add a supported content type.
    pub fn content_type(&mut self, ct: ContentType) -> &mut Self {
        if !self.content_types.contains(&ct) {
            self.content_types.push(ct);
        }
        self
    }

    /// Add a vendor-specific extension key-value pair.
    pub fn extension(&mut self, key: impl Into<String>, value: serde_json::Value) -> &mut Self {
        self.extensions.insert(key.into(), value);
        self
    }

    /// Build the [`CapabilityAdvertisement`].
    #[must_use]
    pub fn build(&self) -> CapabilityAdvertisement {
        CapabilityAdvertisement {
            dialects: if self.dialects.is_empty() {
                vec![Dialect::Generic]
            } else {
                self.dialects.clone()
            },
            tool_support: self.tool_support.clone().unwrap_or(ToolSupportLevel::None),
            streaming_modes: if self.streaming_modes.is_empty() {
                vec![StreamingMode::Jsonl]
            } else {
                self.streaming_modes.clone()
            },
            max_context_length: self.max_context_length,
            content_types: if self.content_types.is_empty() {
                vec![ContentType::Text]
            } else {
                self.content_types.clone()
            },
            extensions: self.extensions.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_advertisement() {
        let ad = CapabilityAdvertisement::default();
        assert_eq!(ad.dialects(), &[Dialect::Generic]);
        assert_eq!(*ad.tool_support(), ToolSupportLevel::None);
        assert_eq!(ad.streaming_modes(), &[StreamingMode::Jsonl]);
        assert_eq!(ad.max_context_length(), None);
        assert_eq!(ad.content_types(), &[ContentType::Text]);
        assert!(ad.extensions().is_empty());
    }

    #[test]
    fn builder_basic() {
        let ad = CapabilityAdvertisement::builder()
            .dialect(Dialect::Anthropic)
            .tool_support(ToolSupportLevel::ParallelCalls)
            .streaming_mode(StreamingMode::Sse)
            .max_context_length(128_000)
            .content_type(ContentType::Image)
            .build();
        assert_eq!(ad.dialects(), &[Dialect::Anthropic]);
        assert_eq!(*ad.tool_support(), ToolSupportLevel::ParallelCalls);
        assert_eq!(ad.streaming_modes(), &[StreamingMode::Sse]);
        assert_eq!(ad.max_context_length(), Some(128_000));
        assert_eq!(ad.content_types(), &[ContentType::Image]);
    }

    #[test]
    fn builder_multiple_dialects() {
        let ad = CapabilityAdvertisement::builder()
            .dialect(Dialect::OpenAi)
            .dialect(Dialect::Anthropic)
            .dialect(Dialect::Gemini)
            .build();
        assert_eq!(ad.dialects().len(), 3);
    }

    #[test]
    fn builder_deduplicates_dialects() {
        let ad = CapabilityAdvertisement::builder()
            .dialect(Dialect::OpenAi)
            .dialect(Dialect::OpenAi)
            .build();
        assert_eq!(ad.dialects().len(), 1);
    }

    #[test]
    fn builder_empty_uses_defaults() {
        let ad = CapabilityAdvertisement::builder().build();
        assert_eq!(ad.dialects(), &[Dialect::Generic]);
        assert_eq!(ad.streaming_modes(), &[StreamingMode::Jsonl]);
        assert_eq!(ad.content_types(), &[ContentType::Text]);
    }

    #[test]
    fn supports_dialect_query() {
        let ad = CapabilityAdvertisement::builder()
            .dialect(Dialect::Anthropic)
            .build();
        assert!(ad.supports_dialect(&Dialect::Anthropic));
        assert!(!ad.supports_dialect(&Dialect::OpenAi));
    }

    #[test]
    fn supports_content_type_query() {
        let ad = CapabilityAdvertisement::builder()
            .content_type(ContentType::Json)
            .content_type(ContentType::Image)
            .build();
        assert!(ad.supports_content_type(&ContentType::Json));
        assert!(ad.supports_content_type(&ContentType::Image));
        assert!(!ad.supports_content_type(&ContentType::Audio));
    }

    #[test]
    fn supports_streaming_mode_query() {
        let ad = CapabilityAdvertisement::builder()
            .streaming_mode(StreamingMode::Sse)
            .streaming_mode(StreamingMode::Jsonl)
            .build();
        assert!(ad.supports_streaming_mode(&StreamingMode::Sse));
        assert!(!ad.supports_streaming_mode(&StreamingMode::Batch));
    }

    #[test]
    fn negotiate_dialect_finds_common() {
        let a = CapabilityAdvertisement::builder()
            .dialect(Dialect::Anthropic)
            .dialect(Dialect::OpenAi)
            .build();
        let b = CapabilityAdvertisement::builder()
            .dialect(Dialect::OpenAi)
            .dialect(Dialect::Gemini)
            .build();
        assert_eq!(a.negotiate_dialect(&b), Some(Dialect::OpenAi));
    }

    #[test]
    fn negotiate_dialect_none_when_disjoint() {
        let a = CapabilityAdvertisement::builder()
            .dialect(Dialect::Anthropic)
            .build();
        let b = CapabilityAdvertisement::builder()
            .dialect(Dialect::Gemini)
            .build();
        assert_eq!(a.negotiate_dialect(&b), None);
    }

    #[test]
    fn negotiate_dialect_prefers_first_side_order() {
        let a = CapabilityAdvertisement::builder()
            .dialect(Dialect::Anthropic)
            .dialect(Dialect::OpenAi)
            .build();
        let b = CapabilityAdvertisement::builder()
            .dialect(Dialect::OpenAi)
            .dialect(Dialect::Anthropic)
            .build();
        // `a` prefers Anthropic, and `b` has it, so Anthropic wins.
        assert_eq!(a.negotiate_dialect(&b), Some(Dialect::Anthropic));
    }

    #[test]
    fn custom_dialect() {
        let ad = CapabilityAdvertisement::builder()
            .dialect(Dialect::Custom("my-vendor".into()))
            .build();
        assert!(ad.supports_dialect(&Dialect::Custom("my-vendor".into())));
        assert!(!ad.supports_dialect(&Dialect::Custom("other".into())));
    }

    #[test]
    fn custom_content_type() {
        let ad = CapabilityAdvertisement::builder()
            .content_type(ContentType::Custom("application/x-custom".into()))
            .build();
        assert!(ad.supports_content_type(&ContentType::Custom("application/x-custom".into())));
    }

    #[test]
    fn extension_metadata() {
        let ad = CapabilityAdvertisement::builder()
            .extension("model", serde_json::json!("claude-3"))
            .extension("tier", serde_json::json!(1))
            .build();
        assert_eq!(ad.extensions().len(), 2);
        assert_eq!(ad.extensions()["model"], serde_json::json!("claude-3"));
    }

    #[test]
    fn serde_round_trip() {
        let ad = CapabilityAdvertisement::builder()
            .dialect(Dialect::Anthropic)
            .tool_support(ToolSupportLevel::StreamingCalls)
            .streaming_mode(StreamingMode::Sse)
            .max_context_length(200_000)
            .content_type(ContentType::Text)
            .content_type(ContentType::Image)
            .extension("key", serde_json::json!("val"))
            .build();
        let json = serde_json::to_string(&ad).unwrap();
        let decoded: CapabilityAdvertisement = serde_json::from_str(&json).unwrap();
        assert_eq!(ad, decoded);
    }

    #[test]
    fn serde_json_structure() {
        let ad = CapabilityAdvertisement::builder()
            .dialect(Dialect::OpenAi)
            .tool_support(ToolSupportLevel::SingleCall)
            .max_context_length(4096)
            .build();
        let v: serde_json::Value = serde_json::to_value(&ad).unwrap();
        assert_eq!(v["dialects"][0], "open_ai");
        assert_eq!(v["tool_support"], "single_call");
        assert_eq!(v["max_context_length"], 4096);
    }
}
