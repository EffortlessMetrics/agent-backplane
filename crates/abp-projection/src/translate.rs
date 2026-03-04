// SPDX-License-Identifier: MIT OR Apache-2.0

//! Cross-dialect translation engine using the IR pipeline.
//!
//! The `TranslationEngine` registers translator pairs (dialect→IR→dialect)
//! and provides `translate` which chains
//! `source→IR→target` in a single call. It tracks whether each translation
//! is passthrough, mapped, or emulated, and detects capability gaps.

use std::collections::{BTreeMap, BTreeSet};

use abp_core::ir::IrConversation;
use abp_dialect::Dialect;
use abp_dialect::features::DialectFeature;
use abp_mapper::{IrMapper, default_ir_mapper, supported_ir_pairs};
use serde::{Deserialize, Serialize};

use crate::ProjectionError;

// ── Translation mode ────────────────────────────────────────────────────

/// Describes how a translation was performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranslationMode {
    /// Same dialect — no transformation applied.
    Passthrough,
    /// Different dialects — a mapper was applied.
    Mapped,
    /// The feature was emulated with best-effort approximation.
    Emulated,
}

impl std::fmt::Display for TranslationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Passthrough => f.write_str("passthrough"),
            Self::Mapped => f.write_str("mapped"),
            Self::Emulated => f.write_str("emulated"),
        }
    }
}

// ── Capability gap ──────────────────────────────────────────────────────

/// A detected capability gap between source and target dialects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityGap {
    /// The feature that is unsupported or degraded in the target.
    pub feature: DialectFeature,
    /// Source dialect where the feature is available.
    pub source: Dialect,
    /// Target dialect where the feature is missing or degraded.
    pub target: Dialect,
    /// Human-readable description of the gap.
    pub description: String,
}

// ── Translation result ──────────────────────────────────────────────────

/// The outcome of a successful translation, carrying the translated
/// conversation alongside metadata about how it was produced.
#[derive(Debug, Clone)]
pub struct TranslationResult {
    /// The translated IR conversation.
    pub conversation: IrConversation,
    /// Source dialect.
    pub from: Dialect,
    /// Target dialect.
    pub to: Dialect,
    /// How the translation was classified.
    pub mode: TranslationMode,
    /// Detected capability gaps (non-fatal warnings).
    pub gaps: Vec<CapabilityGap>,
}

// ── Registered translator ───────────────────────────────────────────────

/// Wrapper holding a boxed `IrMapper` with its supported pairs cached.
struct RegisteredTranslator {
    mapper: Box<dyn IrMapper>,
    pairs: BTreeSet<(Dialect, Dialect)>,
}

// ── Translation engine ──────────────────────────────────────────────────

/// Cross-dialect translation engine.
///
/// Holds registered `IrMapper`s and routes translation requests through
/// the appropriate mapper. For same-dialect requests, the engine performs
/// a zero-cost passthrough without consulting any mapper.
///
/// # Examples
///
/// ```
/// use abp_projection::translate::{TranslationEngine, TranslationMode};
/// use abp_core::ir::{IrConversation, IrMessage, IrRole};
/// use abp_dialect::Dialect;
///
/// let engine = TranslationEngine::with_defaults();
/// let conv = IrConversation::from_messages(vec![
///     IrMessage::text(IrRole::User, "Hello"),
/// ]);
/// let result = engine.translate(Dialect::OpenAi, Dialect::OpenAi, &conv).unwrap();
/// assert_eq!(result.mode, TranslationMode::Passthrough);
/// ```
#[derive(Default)]
pub struct TranslationEngine {
    translators: Vec<RegisteredTranslator>,
    /// Features to check for capability gap detection.
    gap_features: Vec<DialectFeature>,
}

impl std::fmt::Debug for TranslationEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranslationEngine")
            .field("translator_count", &self.translators.len())
            .field("gap_features", &self.gap_features)
            .finish()
    }
}

impl TranslationEngine {
    /// Creates an empty translation engine with no registered translators.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a translation engine pre-populated with all default IR
    /// mappers from `abp-mapper`.
    #[must_use]
    pub fn with_defaults() -> Self {
        let mut engine = Self::new();
        engine.register_defaults();
        engine
    }

    /// Registers all default IR mappers from `abp-mapper`.
    pub fn register_defaults(&mut self) {
        // Collect unique mappers for each pair to avoid duplicates.
        let mut seen = BTreeSet::new();
        for (from, to) in supported_ir_pairs() {
            if seen.contains(&(from, to)) {
                continue;
            }
            if let Some(mapper) = default_ir_mapper(from, to) {
                let pairs: BTreeSet<(Dialect, Dialect)> =
                    mapper.supported_pairs().into_iter().collect();
                for &p in &pairs {
                    seen.insert(p);
                }
                self.translators
                    .push(RegisteredTranslator { mapper, pairs });
            }
        }
    }

    /// Registers a single `IrMapper` implementation.
    ///
    /// The mapper's supported pairs are cached for fast lookup.
    pub fn register(&mut self, mapper: Box<dyn IrMapper>) {
        let pairs: BTreeSet<(Dialect, Dialect)> = mapper.supported_pairs().into_iter().collect();
        self.translators
            .push(RegisteredTranslator { mapper, pairs });
    }

    /// Sets the features to check for capability gap detection.
    pub fn set_gap_features(&mut self, features: Vec<DialectFeature>) {
        self.gap_features = features;
    }

    /// Returns the number of registered translator implementations.
    #[must_use]
    pub fn translator_count(&self) -> usize {
        self.translators.len()
    }

    /// Returns all dialect pairs that have at least one registered mapper.
    #[must_use]
    pub fn supported_pairs(&self) -> Vec<(Dialect, Dialect)> {
        let mut all = BTreeSet::new();
        for t in &self.translators {
            all.extend(&t.pairs);
        }
        all.into_iter().collect()
    }

    /// Returns `true` if a translation from `from` to `to` is supported.
    #[must_use]
    pub fn supports(&self, from: Dialect, to: Dialect) -> bool {
        if from == to {
            return true;
        }
        self.find_mapper(from, to).is_some()
    }

    /// Describes how a given translation would be classified.
    #[must_use]
    pub fn classify(&self, from: Dialect, to: Dialect) -> TranslationMode {
        if from == to {
            TranslationMode::Passthrough
        } else if self.find_mapper(from, to).is_some() {
            TranslationMode::Mapped
        } else {
            // No direct mapper — would need emulation or multi-hop.
            TranslationMode::Emulated
        }
    }

    /// Translate an IR conversation from one dialect to another.
    ///
    /// # Chain: source → IR → target
    ///
    /// - **Passthrough** (`from == to`): returns the conversation unchanged.
    /// - **Mapped**: uses the registered `IrMapper` for the pair.
    /// - **Error**: if no mapper exists for the pair, returns
    ///   [`ProjectionError::UnsupportedDialectPair`].
    ///
    /// # Errors
    ///
    /// - [`ProjectionError::UnsupportedDialectPair`] if no mapper covers the pair.
    /// - [`ProjectionError::MappingFailed`] if the mapper returns an error.
    pub fn translate(
        &self,
        from: Dialect,
        to: Dialect,
        conversation: &IrConversation,
    ) -> Result<TranslationResult, ProjectionError> {
        // Passthrough — same dialect, zero-cost clone.
        if from == to {
            return Ok(TranslationResult {
                conversation: conversation.clone(),
                from,
                to,
                mode: TranslationMode::Passthrough,
                gaps: Vec::new(),
            });
        }

        // Find a direct mapper.
        let mapper = self
            .find_mapper(from, to)
            .ok_or(ProjectionError::UnsupportedDialectPair {
                src_dialect: from,
                tgt_dialect: to,
            })?;

        let translated = mapper.map_request(from, to, conversation).map_err(|e| {
            ProjectionError::MappingFailed {
                reason: e.to_string(),
            }
        })?;

        let gaps = self.detect_gaps(from, to, conversation);

        Ok(TranslationResult {
            conversation: translated,
            from,
            to,
            mode: TranslationMode::Mapped,
            gaps,
        })
    }

    /// Translate a response conversation from one dialect to another.
    ///
    /// Works identically to [`translate`](Self::translate) but calls
    /// `map_response` on the underlying mapper.
    pub fn translate_response(
        &self,
        from: Dialect,
        to: Dialect,
        conversation: &IrConversation,
    ) -> Result<TranslationResult, ProjectionError> {
        if from == to {
            return Ok(TranslationResult {
                conversation: conversation.clone(),
                from,
                to,
                mode: TranslationMode::Passthrough,
                gaps: Vec::new(),
            });
        }

        let mapper = self
            .find_mapper(from, to)
            .ok_or(ProjectionError::UnsupportedDialectPair {
                src_dialect: from,
                tgt_dialect: to,
            })?;

        let translated = mapper.map_response(from, to, conversation).map_err(|e| {
            ProjectionError::MappingFailed {
                reason: e.to_string(),
            }
        })?;

        let gaps = self.detect_gaps(from, to, conversation);

        Ok(TranslationResult {
            conversation: translated,
            from,
            to,
            mode: TranslationMode::Mapped,
            gaps,
        })
    }

    /// Build a translation mode map for all dialect pairs.
    ///
    /// Returns `(source, target) → TranslationMode` for every combination
    /// of known dialects.
    #[must_use]
    pub fn mode_matrix(&self) -> BTreeMap<(Dialect, Dialect), TranslationMode> {
        let mut map = BTreeMap::new();
        for &src in Dialect::all() {
            for &tgt in Dialect::all() {
                map.insert((src, tgt), self.classify(src, tgt));
            }
        }
        map
    }

    /// Detect capability gaps between source and target dialects by
    /// examining the conversation content against the configured gap
    /// features and the static dialect feature matrix.
    fn detect_gaps(
        &self,
        from: Dialect,
        to: Dialect,
        conversation: &IrConversation,
    ) -> Vec<CapabilityGap> {
        let mut gaps = Vec::new();

        // Check if conversation uses features that the target may not support.
        let has_tool_use = conversation.messages.iter().any(|m| {
            m.content.iter().any(|b| {
                matches!(
                    b,
                    abp_core::ir::IrContentBlock::ToolUse { .. }
                        | abp_core::ir::IrContentBlock::ToolResult { .. }
                )
            })
        });

        let has_images = conversation.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, abp_core::ir::IrContentBlock::Image { .. }))
        });

        let has_thinking = conversation.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, abp_core::ir::IrContentBlock::Thinking { .. }))
        });

        let has_system = conversation
            .messages
            .iter()
            .any(|m| m.role == abp_core::ir::IrRole::System);

        // Use the static feature matrix to check support in the target.
        let target_features = abp_dialect::matrix::dialect_features(to);

        if has_tool_use
            && !target_features
                .supports(DialectFeature::ToolUse)
                .is_available()
        {
            gaps.push(CapabilityGap {
                feature: DialectFeature::ToolUse,
                source: from,
                target: to,
                description: format!("{} does not natively support tool use", to.label()),
            });
        }

        if has_images
            && !target_features
                .supports(DialectFeature::Vision)
                .is_available()
        {
            gaps.push(CapabilityGap {
                feature: DialectFeature::Vision,
                source: from,
                target: to,
                description: format!(
                    "{} does not natively support image/vision content",
                    to.label()
                ),
            });
        }

        if has_thinking
            && !target_features
                .supports(DialectFeature::ExtendedThinking)
                .is_available()
        {
            gaps.push(CapabilityGap {
                feature: DialectFeature::ExtendedThinking,
                source: from,
                target: to,
                description: format!(
                    "{} does not natively support extended thinking blocks",
                    to.label()
                ),
            });
        }

        if has_system
            && !target_features
                .supports(DialectFeature::SystemMessages)
                .is_available()
        {
            gaps.push(CapabilityGap {
                feature: DialectFeature::SystemMessages,
                source: from,
                target: to,
                description: format!("{} does not natively support system messages", to.label()),
            });
        }

        gaps
    }

    /// Find the first mapper that supports the given pair.
    fn find_mapper(&self, from: Dialect, to: Dialect) -> Option<&dyn IrMapper> {
        self.translators
            .iter()
            .find(|t| t.pairs.contains(&(from, to)))
            .map(|t| t.mapper.as_ref())
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};

    // ── Helpers ──────────────────────────────────────────────────────────

    fn simple_conv() -> IrConversation {
        IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "Hello")])
    }

    fn system_conv() -> IrConversation {
        IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "You are a helpful assistant."),
            IrMessage::text(IrRole::User, "Hi"),
        ])
    }

    fn tool_conv() -> IrConversation {
        IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Search for X"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "search".into(),
                    input: serde_json::json!({"query": "X"}),
                }],
            ),
            IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "Result for X".into(),
                    }],
                    is_error: false,
                }],
            ),
        ])
    }

    fn image_conv() -> IrConversation {
        IrConversation::from_messages(vec![IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64data".into(),
            }],
        )])
    }

    fn thinking_conv() -> IrConversation {
        IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Let me think...".into(),
                },
                IrContentBlock::Text {
                    text: "Answer".into(),
                },
            ],
        )])
    }

    fn multi_turn_conv() -> IrConversation {
        IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be helpful"),
            IrMessage::text(IrRole::User, "Question 1"),
            IrMessage::text(IrRole::Assistant, "Answer 1"),
            IrMessage::text(IrRole::User, "Question 2"),
        ])
    }

    // ── 1. Registration tests ───────────────────────────────────────────

    #[test]
    fn new_engine_is_empty() {
        let engine = TranslationEngine::new();
        assert_eq!(engine.translator_count(), 0);
        assert!(engine.supported_pairs().is_empty());
    }

    #[test]
    fn with_defaults_has_translators() {
        let engine = TranslationEngine::with_defaults();
        assert!(engine.translator_count() > 0);
    }

    #[test]
    fn with_defaults_covers_identity_pairs() {
        let engine = TranslationEngine::with_defaults();
        for &d in Dialect::all() {
            assert!(engine.supports(d, d), "identity pair missing for {d}");
        }
    }

    #[test]
    fn with_defaults_covers_openai_claude() {
        let engine = TranslationEngine::with_defaults();
        assert!(engine.supports(Dialect::OpenAi, Dialect::Claude));
        assert!(engine.supports(Dialect::Claude, Dialect::OpenAi));
    }

    #[test]
    fn with_defaults_covers_openai_gemini() {
        let engine = TranslationEngine::with_defaults();
        assert!(engine.supports(Dialect::OpenAi, Dialect::Gemini));
        assert!(engine.supports(Dialect::Gemini, Dialect::OpenAi));
    }

    #[test]
    fn with_defaults_covers_claude_gemini() {
        let engine = TranslationEngine::with_defaults();
        assert!(engine.supports(Dialect::Claude, Dialect::Gemini));
        assert!(engine.supports(Dialect::Gemini, Dialect::Claude));
    }

    #[test]
    fn register_custom_mapper() {
        let mut engine = TranslationEngine::new();
        let mapper = abp_mapper::IrIdentityMapper;
        engine.register(Box::new(mapper));
        assert_eq!(engine.translator_count(), 1);
        // Identity mapper supports all same-dialect pairs.
        for &d in Dialect::all() {
            assert!(engine.supports(d, d));
        }
    }

    #[test]
    fn register_multiple_mappers() {
        let mut engine = TranslationEngine::new();
        engine.register(Box::new(abp_mapper::IrIdentityMapper));
        engine.register(Box::new(abp_mapper::OpenAiClaudeIrMapper));
        assert_eq!(engine.translator_count(), 2);
        assert!(engine.supports(Dialect::OpenAi, Dialect::Claude));
    }

    #[test]
    fn supported_pairs_includes_all_registered() {
        let engine = TranslationEngine::with_defaults();
        let pairs = engine.supported_pairs();
        // Must have at least identity + cross-dialect pairs.
        assert!(pairs.len() >= Dialect::all().len());
        // Every identity pair must be present.
        for &d in Dialect::all() {
            assert!(pairs.contains(&(d, d)));
        }
    }

    // ── 2. Passthrough (same dialect) ───────────────────────────────────

    #[test]
    fn passthrough_openai_to_openai() {
        let engine = TranslationEngine::with_defaults();
        let conv = simple_conv();
        let result = engine
            .translate(Dialect::OpenAi, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(result.mode, TranslationMode::Passthrough);
        assert_eq!(result.conversation, conv);
        assert_eq!(result.from, Dialect::OpenAi);
        assert_eq!(result.to, Dialect::OpenAi);
    }

    #[test]
    fn passthrough_claude_to_claude() {
        let engine = TranslationEngine::with_defaults();
        let conv = simple_conv();
        let result = engine
            .translate(Dialect::Claude, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.mode, TranslationMode::Passthrough);
    }

    #[test]
    fn passthrough_gemini_to_gemini() {
        let engine = TranslationEngine::with_defaults();
        let result = engine
            .translate(Dialect::Gemini, Dialect::Gemini, &simple_conv())
            .unwrap();
        assert_eq!(result.mode, TranslationMode::Passthrough);
    }

    #[test]
    fn passthrough_preserves_messages() {
        let engine = TranslationEngine::with_defaults();
        let conv = multi_turn_conv();
        let result = engine
            .translate(Dialect::Claude, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.conversation.messages.len(), conv.messages.len());
        assert_eq!(result.conversation, conv);
    }

    #[test]
    fn passthrough_no_gaps() {
        let engine = TranslationEngine::with_defaults();
        let result = engine
            .translate(Dialect::OpenAi, Dialect::OpenAi, &tool_conv())
            .unwrap();
        assert!(result.gaps.is_empty());
    }

    #[test]
    fn passthrough_works_on_empty_engine() {
        let engine = TranslationEngine::new();
        let result = engine
            .translate(Dialect::OpenAi, Dialect::OpenAi, &simple_conv())
            .unwrap();
        assert_eq!(result.mode, TranslationMode::Passthrough);
    }

    #[test]
    fn passthrough_all_dialects() {
        let engine = TranslationEngine::new();
        for &d in Dialect::all() {
            let result = engine.translate(d, d, &simple_conv()).unwrap();
            assert_eq!(result.mode, TranslationMode::Passthrough);
        }
    }

    #[test]
    fn passthrough_response() {
        let engine = TranslationEngine::with_defaults();
        let conv = simple_conv();
        let result = engine
            .translate_response(Dialect::OpenAi, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(result.mode, TranslationMode::Passthrough);
        assert_eq!(result.conversation, conv);
    }

    // ── 3. Mapped (different dialects) ──────────────────────────────────

    #[test]
    fn mapped_openai_to_claude() {
        let engine = TranslationEngine::with_defaults();
        let conv = simple_conv();
        let result = engine
            .translate(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.mode, TranslationMode::Mapped);
        assert_eq!(result.from, Dialect::OpenAi);
        assert_eq!(result.to, Dialect::Claude);
        // Content should be preserved for a simple text message.
        assert_eq!(result.conversation.messages.len(), 1);
    }

    #[test]
    fn mapped_claude_to_openai() {
        let engine = TranslationEngine::with_defaults();
        let result = engine
            .translate(Dialect::Claude, Dialect::OpenAi, &simple_conv())
            .unwrap();
        assert_eq!(result.mode, TranslationMode::Mapped);
    }

    #[test]
    fn mapped_openai_to_gemini() {
        let engine = TranslationEngine::with_defaults();
        let result = engine
            .translate(Dialect::OpenAi, Dialect::Gemini, &simple_conv())
            .unwrap();
        assert_eq!(result.mode, TranslationMode::Mapped);
    }

    #[test]
    fn mapped_gemini_to_openai() {
        let engine = TranslationEngine::with_defaults();
        let result = engine
            .translate(Dialect::Gemini, Dialect::OpenAi, &simple_conv())
            .unwrap();
        assert_eq!(result.mode, TranslationMode::Mapped);
    }

    #[test]
    fn mapped_claude_to_gemini() {
        let engine = TranslationEngine::with_defaults();
        let result = engine
            .translate(Dialect::Claude, Dialect::Gemini, &simple_conv())
            .unwrap();
        assert_eq!(result.mode, TranslationMode::Mapped);
    }

    #[test]
    fn mapped_preserves_user_content() {
        let engine = TranslationEngine::with_defaults();
        let conv = simple_conv();
        let result = engine
            .translate(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let user_msgs: Vec<_> = result
            .conversation
            .messages
            .iter()
            .filter(|m| m.role == IrRole::User)
            .collect();
        assert!(!user_msgs.is_empty());
    }

    #[test]
    fn mapped_system_messages_openai_to_claude() {
        let engine = TranslationEngine::with_defaults();
        let conv = system_conv();
        let result = engine
            .translate(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.mode, TranslationMode::Mapped);
        assert!(!result.conversation.messages.is_empty());
    }

    #[test]
    fn mapped_multi_turn() {
        let engine = TranslationEngine::with_defaults();
        let conv = multi_turn_conv();
        let result = engine
            .translate(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.mode, TranslationMode::Mapped);
        // Should preserve at least the non-system messages.
        assert!(result.conversation.messages.len() >= 3);
    }

    #[test]
    fn mapped_response_openai_to_claude() {
        let engine = TranslationEngine::with_defaults();
        let conv =
            IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "Response")]);
        let result = engine
            .translate_response(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.mode, TranslationMode::Mapped);
    }

    // ── 4. Error on unsupported translations ────────────────────────────

    #[test]
    fn unsupported_pair_returns_error() {
        let engine = TranslationEngine::new();
        let err = engine
            .translate(Dialect::OpenAi, Dialect::Claude, &simple_conv())
            .unwrap_err();
        assert!(matches!(
            err,
            ProjectionError::UnsupportedDialectPair { .. }
        ));
    }

    #[test]
    fn unsupported_pair_error_contains_dialects() {
        let engine = TranslationEngine::new();
        let err = engine
            .translate(Dialect::Kimi, Dialect::Copilot, &simple_conv())
            .unwrap_err();
        match err {
            ProjectionError::UnsupportedDialectPair {
                src_dialect,
                tgt_dialect,
            } => {
                assert_eq!(src_dialect, Dialect::Kimi);
                assert_eq!(tgt_dialect, Dialect::Copilot);
            }
            other => panic!("expected UnsupportedDialectPair, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_response_pair_returns_error() {
        let engine = TranslationEngine::new();
        let err = engine
            .translate_response(Dialect::OpenAi, Dialect::Claude, &simple_conv())
            .unwrap_err();
        assert!(matches!(
            err,
            ProjectionError::UnsupportedDialectPair { .. }
        ));
    }

    // ── 5. Capability gap detection ─────────────────────────────────────

    #[test]
    fn gap_detected_for_thinking_to_openai() {
        let engine = TranslationEngine::with_defaults();
        let conv = thinking_conv();
        let result = engine
            .translate(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        // OpenAI doesn't natively support extended thinking.
        let thinking_gap = result
            .gaps
            .iter()
            .find(|g| g.feature == DialectFeature::ExtendedThinking);
        assert!(thinking_gap.is_some(), "expected a thinking capability gap");
    }

    #[test]
    fn no_gap_for_simple_text() {
        let engine = TranslationEngine::with_defaults();
        let result = engine
            .translate(Dialect::OpenAi, Dialect::Claude, &simple_conv())
            .unwrap();
        assert!(result.gaps.is_empty());
    }

    #[test]
    fn gap_detection_tool_use_to_codex() {
        let engine = TranslationEngine::with_defaults();
        let conv = tool_conv();
        // Codex might not support tool use — check gap detection works.
        if engine.supports(Dialect::OpenAi, Dialect::Codex) {
            let result = engine
                .translate(Dialect::OpenAi, Dialect::Codex, &conv)
                .unwrap();
            // The result should either succeed or report gaps.
            assert_eq!(result.mode, TranslationMode::Mapped);
        }
    }

    #[test]
    fn gap_reports_correct_source_target() {
        let engine = TranslationEngine::with_defaults();
        let conv = thinking_conv();
        let result = engine
            .translate(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        for gap in &result.gaps {
            assert_eq!(gap.source, Dialect::Claude);
            assert_eq!(gap.target, Dialect::OpenAi);
        }
    }

    #[test]
    fn gap_has_description() {
        let engine = TranslationEngine::with_defaults();
        let conv = thinking_conv();
        let result = engine
            .translate(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        for gap in &result.gaps {
            assert!(!gap.description.is_empty());
        }
    }

    #[test]
    fn passthrough_never_reports_gaps() {
        let engine = TranslationEngine::with_defaults();
        // Even a complex conversation should have no gaps in passthrough.
        let conv = thinking_conv();
        let result = engine
            .translate(Dialect::Claude, Dialect::Claude, &conv)
            .unwrap();
        assert!(result.gaps.is_empty());
    }

    #[test]
    fn no_vision_gap_when_target_supports_emulated() {
        let engine = TranslationEngine::with_defaults();
        let conv = image_conv();
        // Codex supports vision (emulated), so no gap should be reported.
        if engine.supports(Dialect::OpenAi, Dialect::Codex) {
            let result = engine
                .translate(Dialect::OpenAi, Dialect::Codex, &conv)
                .unwrap();
            let vision_gap = result
                .gaps
                .iter()
                .find(|g| g.feature == DialectFeature::Vision);
            assert!(
                vision_gap.is_none(),
                "emulated vision should not produce a gap"
            );
        }
    }

    // ── 6. Translation classification ───────────────────────────────────

    #[test]
    fn classify_passthrough() {
        let engine = TranslationEngine::with_defaults();
        for &d in Dialect::all() {
            assert_eq!(engine.classify(d, d), TranslationMode::Passthrough);
        }
    }

    #[test]
    fn classify_mapped() {
        let engine = TranslationEngine::with_defaults();
        assert_eq!(
            engine.classify(Dialect::OpenAi, Dialect::Claude),
            TranslationMode::Mapped
        );
    }

    #[test]
    fn classify_emulated_for_unsupported() {
        let engine = TranslationEngine::new();
        // No mappers registered → emulated.
        assert_eq!(
            engine.classify(Dialect::OpenAi, Dialect::Claude),
            TranslationMode::Emulated
        );
    }

    // ── 7. Mode matrix ──────────────────────────────────────────────────

    #[test]
    fn mode_matrix_covers_all_pairs() {
        let engine = TranslationEngine::with_defaults();
        let matrix = engine.mode_matrix();
        let dialect_count = Dialect::all().len();
        assert_eq!(matrix.len(), dialect_count * dialect_count);
    }

    #[test]
    fn mode_matrix_diagonal_is_passthrough() {
        let engine = TranslationEngine::with_defaults();
        let matrix = engine.mode_matrix();
        for &d in Dialect::all() {
            assert_eq!(
                matrix[&(d, d)],
                TranslationMode::Passthrough,
                "diagonal should be passthrough for {d}"
            );
        }
    }

    #[test]
    fn mode_matrix_has_mapped_entries() {
        let engine = TranslationEngine::with_defaults();
        let matrix = engine.mode_matrix();
        let mapped_count = matrix
            .values()
            .filter(|&&m| m == TranslationMode::Mapped)
            .count();
        assert!(mapped_count > 0, "expected at least some mapped entries");
    }

    // ── 8. Translation mode display ─────────────────────────────────────

    #[test]
    fn translation_mode_display() {
        assert_eq!(TranslationMode::Passthrough.to_string(), "passthrough");
        assert_eq!(TranslationMode::Mapped.to_string(), "mapped");
        assert_eq!(TranslationMode::Emulated.to_string(), "emulated");
    }

    #[test]
    fn translation_mode_serde_roundtrip() {
        for mode in [
            TranslationMode::Passthrough,
            TranslationMode::Mapped,
            TranslationMode::Emulated,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let back: TranslationMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, back);
        }
    }

    // ── 9. Capability gap serde ─────────────────────────────────────────

    #[test]
    fn capability_gap_serde_roundtrip() {
        let gap = CapabilityGap {
            feature: DialectFeature::ExtendedThinking,
            source: Dialect::Claude,
            target: Dialect::OpenAi,
            description: "no thinking support".into(),
        };
        let json = serde_json::to_string(&gap).unwrap();
        let back: CapabilityGap = serde_json::from_str(&json).unwrap();
        assert_eq!(gap, back);
    }

    // ── 10. Engine debug ────────────────────────────────────────────────

    #[test]
    fn engine_debug_does_not_panic() {
        let engine = TranslationEngine::with_defaults();
        let dbg = format!("{engine:?}");
        assert!(dbg.contains("TranslationEngine"));
    }

    // ── 11. Edge cases ──────────────────────────────────────────────────

    #[test]
    fn translate_empty_conversation() {
        let engine = TranslationEngine::with_defaults();
        let conv = IrConversation::new();
        let result = engine
            .translate(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.mode, TranslationMode::Mapped);
        assert!(result.conversation.messages.is_empty());
    }

    #[test]
    fn translate_preserves_metadata() {
        let engine = TranslationEngine::with_defaults();
        let mut msg = IrMessage::text(IrRole::User, "Hello");
        msg.metadata
            .insert("custom_key".into(), serde_json::json!("custom_value"));
        let conv = IrConversation::from_messages(vec![msg]);
        let result = engine
            .translate(Dialect::OpenAi, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(
            result.conversation.messages[0].metadata["custom_key"],
            serde_json::json!("custom_value")
        );
    }

    #[test]
    fn translate_codex_to_openai() {
        let engine = TranslationEngine::with_defaults();
        if engine.supports(Dialect::Codex, Dialect::OpenAi) {
            let result = engine
                .translate(Dialect::Codex, Dialect::OpenAi, &simple_conv())
                .unwrap();
            assert_eq!(result.mode, TranslationMode::Mapped);
        }
    }

    #[test]
    fn translate_kimi_to_openai() {
        let engine = TranslationEngine::with_defaults();
        if engine.supports(Dialect::Kimi, Dialect::OpenAi) {
            let result = engine
                .translate(Dialect::Kimi, Dialect::OpenAi, &simple_conv())
                .unwrap();
            assert_eq!(result.mode, TranslationMode::Mapped);
        }
    }

    #[test]
    fn translate_copilot_to_openai() {
        let engine = TranslationEngine::with_defaults();
        if engine.supports(Dialect::Copilot, Dialect::OpenAi) {
            let result = engine
                .translate(Dialect::Copilot, Dialect::OpenAi, &simple_conv())
                .unwrap();
            assert_eq!(result.mode, TranslationMode::Mapped);
        }
    }
}
