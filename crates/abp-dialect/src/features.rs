// SPDX-License-Identifier: MIT OR Apache-2.0
//! Dialect feature enumeration and feature-set queries.
//!
//! `DialectFeature` enumerates capabilities that agent backends may
//! support (tool use, streaming, vision, etc.).  `DialectFeatureSet`
//! wraps a collection of `(DialectFeature, FeatureSupport)` pairs and
//! offers query helpers such as `supports`,
//! `native_features`, and
//! `emulated_features`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ── DialectFeature ──────────────────────────────────────────────────────

/// A capability that an agent-protocol dialect may support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DialectFeature {
    /// System / developer messages.
    SystemMessages,
    /// Tool / function calling with structured input/output.
    ToolUse,
    /// Server-sent-event or chunked streaming responses.
    Streaming,
    /// Image inputs (base64 or URL).
    Vision,
    /// Audio inputs / outputs.
    Audio,
    /// Extended thinking / chain-of-thought blocks.
    ExtendedThinking,
    /// Prompt caching (KV-cache read/write).
    Caching,
    /// Parallel tool-call dispatch in a single turn.
    ParallelToolCalls,
    /// Legacy function-calling interface (distinct from tool use).
    FunctionCalling,
    /// Embedding / vector generation.
    Embeddings,
    /// Sandboxed code execution (e.g. Codex, code-interpreter).
    CodeExecution,
    /// Structured / JSON output mode.
    StructuredOutput,
    /// File / document upload attachments.
    FileAttachments,
    /// Web search grounding.
    WebSearch,
    /// Multi-modal output (images, audio, etc.).
    MultimodalOutput,
}

impl DialectFeature {
    /// Returns all known features in declaration order.
    #[must_use]
    pub fn all() -> &'static [DialectFeature] {
        &[
            Self::SystemMessages,
            Self::ToolUse,
            Self::Streaming,
            Self::Vision,
            Self::Audio,
            Self::ExtendedThinking,
            Self::Caching,
            Self::ParallelToolCalls,
            Self::FunctionCalling,
            Self::Embeddings,
            Self::CodeExecution,
            Self::StructuredOutput,
            Self::FileAttachments,
            Self::WebSearch,
            Self::MultimodalOutput,
        ]
    }

    /// Human-readable label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::SystemMessages => "System Messages",
            Self::ToolUse => "Tool Use",
            Self::Streaming => "Streaming",
            Self::Vision => "Vision",
            Self::Audio => "Audio",
            Self::ExtendedThinking => "Extended Thinking",
            Self::Caching => "Caching",
            Self::ParallelToolCalls => "Parallel Tool Calls",
            Self::FunctionCalling => "Function Calling",
            Self::Embeddings => "Embeddings",
            Self::CodeExecution => "Code Execution",
            Self::StructuredOutput => "Structured Output",
            Self::FileAttachments => "File Attachments",
            Self::WebSearch => "Web Search",
            Self::MultimodalOutput => "Multimodal Output",
        }
    }
}

impl std::fmt::Display for DialectFeature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

// ── FeatureSupport ──────────────────────────────────────────────────────

/// How well a dialect supports a given feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureSupport {
    /// Feature is not available in this dialect.
    None,
    /// Feature can be approximated via translation but is not native.
    Emulated,
    /// Feature is natively supported by the dialect.
    Native,
}

impl FeatureSupport {
    /// Returns `true` for [`Native`](Self::Native) or [`Emulated`](Self::Emulated).
    #[must_use]
    pub fn is_available(self) -> bool {
        matches!(self, Self::Native | Self::Emulated)
    }

    /// Returns `true` only for [`Native`](Self::Native).
    #[must_use]
    pub fn is_native(self) -> bool {
        self == Self::Native
    }
}

impl std::fmt::Display for FeatureSupport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => f.write_str("none"),
            Self::Emulated => f.write_str("emulated"),
            Self::Native => f.write_str("native"),
        }
    }
}

// ── DialectFeatureSet ───────────────────────────────────────────────────

/// A set of features with their support levels for a single dialect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DialectFeatureSet {
    entries: BTreeMap<DialectFeature, FeatureSupport>,
}

impl DialectFeatureSet {
    /// Build a feature set from an iterator of `(feature, support)` pairs.
    pub fn build_from_iter(
        iter: impl IntoIterator<Item = (DialectFeature, FeatureSupport)>,
    ) -> Self {
        Self {
            entries: iter.into_iter().collect(),
        }
    }

    /// Query the support level for a specific feature.
    ///
    /// Returns [`FeatureSupport::None`] when the feature is not in the set.
    #[must_use]
    pub fn supports(&self, feature: DialectFeature) -> FeatureSupport {
        self.entries
            .get(&feature)
            .copied()
            .unwrap_or(FeatureSupport::None)
    }

    /// Returns all features that are natively supported.
    #[must_use]
    pub fn native_features(&self) -> Vec<DialectFeature> {
        self.entries
            .iter()
            .filter(|(_, s)| **s == FeatureSupport::Native)
            .map(|(f, _)| *f)
            .collect()
    }

    /// Returns all features that are emulated (but not native).
    #[must_use]
    pub fn emulated_features(&self) -> Vec<DialectFeature> {
        self.entries
            .iter()
            .filter(|(_, s)| **s == FeatureSupport::Emulated)
            .map(|(f, _)| *f)
            .collect()
    }

    /// Returns all features with any level of availability (native or emulated).
    #[must_use]
    pub fn available_features(&self) -> Vec<DialectFeature> {
        self.entries
            .iter()
            .filter(|(_, s)| s.is_available())
            .map(|(f, _)| *f)
            .collect()
    }

    /// Returns all features that are unsupported.
    #[must_use]
    pub fn unsupported_features(&self) -> Vec<DialectFeature> {
        self.entries
            .iter()
            .filter(|(_, s)| **s == FeatureSupport::None)
            .map(|(f, _)| *f)
            .collect()
    }

    /// Number of entries in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` when the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over all `(feature, support)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (DialectFeature, FeatureSupport)> + '_ {
        self.entries.iter().map(|(f, s)| (*f, *s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_all_returns_every_variant() {
        assert_eq!(DialectFeature::all().len(), 15);
    }

    #[test]
    fn feature_label_non_empty() {
        for f in DialectFeature::all() {
            assert!(!f.label().is_empty(), "empty label for {f:?}");
        }
    }

    #[test]
    fn feature_display_matches_label() {
        for f in DialectFeature::all() {
            assert_eq!(f.to_string(), f.label());
        }
    }

    #[test]
    fn feature_serde_roundtrip() {
        let f = DialectFeature::ExtendedThinking;
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(json, "\"extended_thinking\"");
        let back: DialectFeature = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn support_is_available() {
        assert!(!FeatureSupport::None.is_available());
        assert!(FeatureSupport::Emulated.is_available());
        assert!(FeatureSupport::Native.is_available());
    }

    #[test]
    fn support_is_native() {
        assert!(!FeatureSupport::None.is_native());
        assert!(!FeatureSupport::Emulated.is_native());
        assert!(FeatureSupport::Native.is_native());
    }

    #[test]
    fn support_display() {
        assert_eq!(FeatureSupport::None.to_string(), "none");
        assert_eq!(FeatureSupport::Emulated.to_string(), "emulated");
        assert_eq!(FeatureSupport::Native.to_string(), "native");
    }

    #[test]
    fn support_serde_roundtrip() {
        let s = FeatureSupport::Emulated;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"emulated\"");
        let back: FeatureSupport = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn feature_set_supports_known() {
        let set = DialectFeatureSet::build_from_iter([
            (DialectFeature::ToolUse, FeatureSupport::Native),
            (DialectFeature::Vision, FeatureSupport::Emulated),
        ]);
        assert_eq!(
            set.supports(DialectFeature::ToolUse),
            FeatureSupport::Native
        );
        assert_eq!(
            set.supports(DialectFeature::Vision),
            FeatureSupport::Emulated
        );
    }

    #[test]
    fn feature_set_supports_unknown_returns_none() {
        let set = DialectFeatureSet::build_from_iter([]);
        assert_eq!(set.supports(DialectFeature::Audio), FeatureSupport::None);
    }

    #[test]
    fn feature_set_native_features() {
        let set = DialectFeatureSet::build_from_iter([
            (DialectFeature::ToolUse, FeatureSupport::Native),
            (DialectFeature::Vision, FeatureSupport::Emulated),
            (DialectFeature::Audio, FeatureSupport::None),
        ]);
        let native = set.native_features();
        assert_eq!(native, vec![DialectFeature::ToolUse]);
    }

    #[test]
    fn feature_set_emulated_features() {
        let set = DialectFeatureSet::build_from_iter([
            (DialectFeature::ToolUse, FeatureSupport::Native),
            (DialectFeature::Vision, FeatureSupport::Emulated),
            (DialectFeature::Audio, FeatureSupport::None),
        ]);
        let emulated = set.emulated_features();
        assert_eq!(emulated, vec![DialectFeature::Vision]);
    }

    #[test]
    fn feature_set_available_features() {
        let set = DialectFeatureSet::build_from_iter([
            (DialectFeature::ToolUse, FeatureSupport::Native),
            (DialectFeature::Vision, FeatureSupport::Emulated),
            (DialectFeature::Audio, FeatureSupport::None),
        ]);
        let available = set.available_features();
        assert_eq!(available.len(), 2);
        assert!(available.contains(&DialectFeature::ToolUse));
        assert!(available.contains(&DialectFeature::Vision));
    }

    #[test]
    fn feature_set_unsupported_features() {
        let set = DialectFeatureSet::build_from_iter([
            (DialectFeature::ToolUse, FeatureSupport::Native),
            (DialectFeature::Audio, FeatureSupport::None),
        ]);
        assert_eq!(set.unsupported_features(), vec![DialectFeature::Audio]);
    }

    #[test]
    fn feature_set_len_and_empty() {
        let empty = DialectFeatureSet::build_from_iter([]);
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);

        let set = DialectFeatureSet::build_from_iter([(
            DialectFeature::Streaming,
            FeatureSupport::Native,
        )]);
        assert!(!set.is_empty());
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn feature_set_iter() {
        let set = DialectFeatureSet::build_from_iter([(
            DialectFeature::Caching,
            FeatureSupport::Emulated,
        )]);
        let pairs: Vec<_> = set.iter().collect();
        assert_eq!(
            pairs,
            vec![(DialectFeature::Caching, FeatureSupport::Emulated)]
        );
    }

    #[test]
    fn feature_set_serde_roundtrip() {
        let set = DialectFeatureSet::build_from_iter([
            (DialectFeature::ToolUse, FeatureSupport::Native),
            (DialectFeature::Vision, FeatureSupport::Emulated),
        ]);
        let json = serde_json::to_string(&set).unwrap();
        let back: DialectFeatureSet = serde_json::from_str(&json).unwrap();
        assert_eq!(back, set);
    }

    #[test]
    fn feature_ordering_is_stable() {
        // BTreeMap ordering means features come out in Ord order
        let set = DialectFeatureSet::build_from_iter([
            (DialectFeature::WebSearch, FeatureSupport::Native),
            (DialectFeature::SystemMessages, FeatureSupport::Native),
        ]);
        let native = set.native_features();
        assert_eq!(native[0], DialectFeature::SystemMessages);
        assert_eq!(native[1], DialectFeature::WebSearch);
    }
}
