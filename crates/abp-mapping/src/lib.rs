// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! # abp-mapping
//!
//! Cross-dialect mapping validation for the Agent Backplane.

use std::collections::HashMap;

use abp_dialect::Dialect;
use serde::{Deserialize, Serialize};

// ── Errors ──────────────────────────────────────────────────────────────

/// Errors that can occur during mapping validation.
///
/// # Examples
///
/// ```
/// use abp_mapping::MappingError;
/// use abp_dialect::Dialect;
///
/// let err = MappingError::FeatureUnsupported {
///     feature: "logprobs".into(),
///     from: Dialect::Claude,
///     to: Dialect::Gemini,
/// };
/// assert!(err.to_string().contains("logprobs"));
/// ```
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize, PartialEq, Eq)]
pub enum MappingError {
    /// The requested feature is unsupported in the target dialect.
    #[error("feature `{feature}` is unsupported for {from} -> {to}")]
    FeatureUnsupported {
        /// Feature name.
        feature: String,
        /// Source dialect.
        from: Dialect,
        /// Target dialect.
        to: Dialect,
    },
    /// The mapping incurs fidelity loss.
    #[error("fidelity loss for `{feature}`: {warning}")]
    FidelityLoss {
        /// Feature name.
        feature: String,
        /// Human-readable warning.
        warning: String,
    },
    /// Source and target dialects are incompatible.
    #[error("dialect mismatch: {from} cannot map to {to}")]
    DialectMismatch {
        /// Source dialect.
        from: Dialect,
        /// Target dialect.
        to: Dialect,
    },
    /// Invalid input was provided.
    #[error("invalid input: {reason}")]
    InvalidInput {
        /// Reason the input is invalid.
        reason: String,
    },
}

// ── Fidelity ────────────────────────────────────────────────────────────

/// Describes how faithfully a feature maps between dialects.
///
/// # Examples
///
/// ```
/// use abp_mapping::Fidelity;
///
/// let f = Fidelity::Lossless;
/// assert!(f.is_lossless());
/// assert!(!f.is_unsupported());
///
/// let u = Fidelity::Unsupported { reason: "not available".into() };
/// assert!(u.is_unsupported());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Fidelity {
    /// The feature maps perfectly with no information loss.
    Lossless,
    /// The feature maps but with labeled fidelity loss.
    LossyLabeled {
        /// Human-readable description of what is lost.
        warning: String,
    },
    /// The feature is not supported in the target dialect.
    Unsupported {
        /// Reason the feature is unsupported.
        reason: String,
    },
}

impl Fidelity {
    /// Returns `true` if the fidelity is lossless.
    #[must_use]
    pub fn is_lossless(&self) -> bool {
        matches!(self, Self::Lossless)
    }

    /// Returns `true` if the mapping is unsupported.
    #[must_use]
    pub fn is_unsupported(&self) -> bool {
        matches!(self, Self::Unsupported { .. })
    }
}

// ── MappingRule ─────────────────────────────────────────────────────────

/// A single mapping rule describing how a feature translates between dialects.
///
/// # Examples
///
/// ```
/// use abp_mapping::{MappingRule, Fidelity};
/// use abp_dialect::Dialect;
///
/// let rule = MappingRule {
///     source_dialect: Dialect::OpenAi,
///     target_dialect: Dialect::Claude,
///     feature: "streaming".into(),
///     fidelity: Fidelity::Lossless,
/// };
/// assert!(rule.fidelity.is_lossless());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingRule {
    /// Source dialect.
    pub source_dialect: Dialect,
    /// Target dialect.
    pub target_dialect: Dialect,
    /// Feature being mapped (e.g. `"tool_use"`, `"streaming"`).
    pub feature: String,
    /// Fidelity of the mapping.
    pub fidelity: Fidelity,
}

// ── MappingValidation ───────────────────────────────────────────────────

/// Per-feature validation result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingValidation {
    /// Feature that was validated.
    pub feature: String,
    /// Fidelity of the mapping (if a rule was found).
    pub fidelity: Fidelity,
    /// Any errors found during validation.
    pub errors: Vec<MappingError>,
}

// ── Rule Metadata ───────────────────────────────────────────────────────

/// Documentation and metadata attached to a mapping rule.
///
/// # Examples
///
/// ```
/// use abp_mapping::RuleMetadata;
///
/// let meta = RuleMetadata::new("Maps OpenAI function_call to Claude tool_use blocks");
/// assert_eq!(meta.description, "Maps OpenAI function_call to Claude tool_use blocks");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleMetadata {
    /// Human-readable description of the mapping rule.
    pub description: String,
    /// Version when this rule was introduced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_version: Option<String>,
    /// Additional notes or caveats.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl RuleMetadata {
    /// Creates metadata with a description.
    #[must_use]
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            since_version: None,
            notes: Vec::new(),
        }
    }

    /// Sets the version when this rule was introduced.
    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.since_version = Some(version.into());
        self
    }

    /// Adds a note.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

// ── Bidirectional Report ────────────────────────────────────────────────

/// Result of validating both directions of a mapping (A→B and B→A).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BidirectionalReport {
    /// Dialect A.
    pub dialect_a: Dialect,
    /// Dialect B.
    pub dialect_b: Dialect,
    /// Feature being validated.
    pub feature: String,
    /// Fidelity of A→B mapping, if a rule exists.
    pub forward_fidelity: Option<Fidelity>,
    /// Fidelity of B→A mapping, if a rule exists.
    pub reverse_fidelity: Option<Fidelity>,
    /// Whether both directions have rules registered.
    pub is_symmetric: bool,
    /// Detected asymmetries or issues.
    pub warnings: Vec<String>,
}

// ── Fidelity Report ─────────────────────────────────────────────────────

/// Aggregated fidelity loss report for a dialect pair across features.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FidelityReport {
    /// Source dialect.
    pub source: Dialect,
    /// Target dialect.
    pub target: Dialect,
    /// Features with lossless mapping.
    pub lossless: Vec<String>,
    /// Features with lossy mapping: `(feature, warning)`.
    pub lossy: Vec<(String, String)>,
    /// Features that are explicitly unsupported.
    pub unsupported: Vec<String>,
    /// Features with no mapping rule at all.
    pub unmapped: Vec<String>,
}

impl FidelityReport {
    /// Returns the total number of features analyzed.
    #[must_use]
    pub fn total_features(&self) -> usize {
        self.lossless.len() + self.lossy.len() + self.unsupported.len() + self.unmapped.len()
    }

    /// Returns `true` if all features are lossless.
    #[must_use]
    pub fn is_all_lossless(&self) -> bool {
        self.lossy.is_empty() && self.unsupported.is_empty() && self.unmapped.is_empty()
    }

    /// Returns `true` if any features are unsupported or unmapped.
    #[must_use]
    pub fn has_blockers(&self) -> bool {
        !self.unsupported.is_empty() || !self.unmapped.is_empty()
    }
}

// ── Chain Validation ────────────────────────────────────────────────────

/// Result of validating a mapping chain (e.g. A→B→C).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainValidation {
    /// The ordered chain of dialects.
    pub chain: Vec<Dialect>,
    /// Feature being validated.
    pub feature: String,
    /// Fidelity at each hop.
    pub hops: Vec<Fidelity>,
    /// Overall (worst-case) fidelity of the chain.
    pub overall_fidelity: Fidelity,
    /// Errors encountered along the chain.
    pub errors: Vec<MappingError>,
}

// ── Token Usage ─────────────────────────────────────────────────────────

/// Normalized token usage across dialects.
///
/// Different providers report token usage with varying field names and
/// semantics. This struct provides a common representation.
///
/// # Examples
///
/// ```
/// use abp_mapping::TokenUsage;
/// use abp_dialect::Dialect;
/// use std::collections::HashMap;
///
/// let mut fields = HashMap::new();
/// fields.insert("prompt_tokens".into(), 100u64);
/// fields.insert("completion_tokens".into(), 50);
/// let usage = TokenUsage::normalize(Dialect::OpenAi, &fields);
/// assert_eq!(usage.input_tokens, 100);
/// assert_eq!(usage.output_tokens, 50);
/// assert_eq!(usage.total_tokens, 150);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Input/prompt tokens.
    pub input_tokens: u64,
    /// Output/completion tokens.
    pub output_tokens: u64,
    /// Reasoning/thinking tokens (if reported).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    /// Cache read tokens (if reported).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    /// Cache write/creation tokens (if reported).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
    /// Total tokens.
    pub total_tokens: u64,
}

impl TokenUsage {
    /// Normalizes dialect-specific token usage fields into a common
    /// representation.
    ///
    /// Recognized field names per dialect:
    /// - **OpenAI**: `prompt_tokens`, `completion_tokens`,
    ///   `reasoning_tokens`, `total_tokens`
    /// - **Claude**: `input_tokens`, `output_tokens`,
    ///   `cache_creation_input_tokens`, `cache_read_input_tokens`
    /// - **Gemini**: `promptTokenCount`, `candidatesTokenCount`,
    ///   `thoughtsTokenCount`, `totalTokenCount`
    /// - **Others**: Falls back to OpenAI-style field names.
    #[must_use]
    pub fn normalize(dialect: Dialect, fields: &HashMap<String, u64>) -> Self {
        let get = |k: &str| fields.get(k).copied();

        let (input, output, reasoning, cache_read, cache_write) = match dialect {
            Dialect::Claude => (
                get("input_tokens").unwrap_or(0),
                get("output_tokens").unwrap_or(0),
                None,
                get("cache_read_input_tokens"),
                get("cache_creation_input_tokens"),
            ),
            Dialect::Gemini => (
                get("promptTokenCount").unwrap_or(0),
                get("candidatesTokenCount").unwrap_or(0),
                get("thoughtsTokenCount"),
                None,
                None,
            ),
            // OpenAI, Codex, Kimi, Copilot all use OpenAI-style fields.
            _ => (
                get("prompt_tokens").unwrap_or(0),
                get("completion_tokens").unwrap_or(0),
                get("reasoning_tokens"),
                get("cached_tokens"),
                None,
            ),
        };

        let total = get("total_tokens")
            .or_else(|| get("totalTokenCount"))
            .unwrap_or_else(|| input + output + reasoning.unwrap_or(0));

        Self {
            input_tokens: input,
            output_tokens: output,
            reasoning_tokens: reasoning,
            cache_read_tokens: cache_read,
            cache_write_tokens: cache_write,
            total_tokens: total,
        }
    }
}

// ── Streaming Events ────────────────────────────────────────────────────

/// Well-known streaming event kinds that require mapping between dialects.
pub mod streaming_events {
    /// Content text delta.
    pub const CONTENT_DELTA: &str = "content_delta";
    /// Tool/function call delta.
    pub const TOOL_CALL_DELTA: &str = "tool_call_delta";
    /// Thinking/reasoning delta.
    pub const THINKING_DELTA: &str = "thinking_delta";
    /// Message/response started.
    pub const MESSAGE_START: &str = "message_start";
    /// Message/response completed.
    pub const MESSAGE_STOP: &str = "message_stop";
    /// Stream error.
    pub const ERROR: &str = "error";
}

/// A mapping rule for streaming events between dialects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamingEventMapping {
    /// Source dialect.
    pub source_dialect: Dialect,
    /// Target dialect.
    pub target_dialect: Dialect,
    /// Canonical event kind in the source dialect.
    pub source_event: String,
    /// Corresponding event kind in the target dialect.
    pub target_event: String,
    /// Fidelity of the event mapping.
    pub fidelity: Fidelity,
}

// ── MappingRegistry ─────────────────────────────────────────────────────

/// Key for registry lookups.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RuleKey {
    source: Dialect,
    target: Dialect,
    feature: String,
}

/// Collects [`MappingRule`]s and provides lookup by source, target, and feature.
///
/// # Examples
///
/// ```
/// use abp_mapping::{MappingRegistry, MappingRule, Fidelity};
/// use abp_dialect::Dialect;
///
/// let mut reg = MappingRegistry::new();
/// reg.insert(MappingRule {
///     source_dialect: Dialect::OpenAi,
///     target_dialect: Dialect::Claude,
///     feature: "tool_use".into(),
///     fidelity: Fidelity::Lossless,
/// });
///
/// assert_eq!(reg.len(), 1);
/// let rule = reg.lookup(Dialect::OpenAi, Dialect::Claude, "tool_use");
/// assert!(rule.is_some());
/// ```
#[derive(Debug, Clone, Default)]
pub struct MappingRegistry {
    rules: HashMap<RuleKey, MappingRule>,
    metadata: HashMap<RuleKey, RuleMetadata>,
    streaming: HashMap<RuleKey, StreamingEventMapping>,
}

impl MappingRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a mapping rule, replacing any existing rule for the same key.
    pub fn insert(&mut self, rule: MappingRule) {
        let key = RuleKey {
            source: rule.source_dialect,
            target: rule.target_dialect,
            feature: rule.feature.clone(),
        };
        self.rules.insert(key, rule);
    }

    /// Looks up a rule by source dialect, target dialect, and feature name.
    #[must_use]
    pub fn lookup(&self, source: Dialect, target: Dialect, feature: &str) -> Option<&MappingRule> {
        let key = RuleKey {
            source,
            target,
            feature: feature.to_owned(),
        };
        self.rules.get(&key)
    }

    /// Returns the total number of rules in the registry.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Returns `true` if the registry contains no rules.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Returns an iterator over all rules.
    pub fn iter(&self) -> impl Iterator<Item = &MappingRule> {
        self.rules.values()
    }

    /// Ranks target dialects by mapping quality for the given source and features.
    ///
    /// Returns `(dialect, lossless_count)` pairs sorted by lossless count descending.
    /// Dialects where no features are supported (all unsupported or absent) are excluded.
    #[must_use]
    pub fn rank_targets(&self, source: Dialect, features: &[&str]) -> Vec<(Dialect, usize)> {
        let mut results = Vec::new();
        for &target in Dialect::all() {
            if target == source {
                continue;
            }
            let mut lossless = 0usize;
            let mut any_supported = false;
            for &feat in features {
                if let Some(rule) = self.lookup(source, target, feat)
                    && !rule.fidelity.is_unsupported()
                {
                    any_supported = true;
                    if rule.fidelity.is_lossless() {
                        lossless += 1;
                    }
                }
            }
            if any_supported {
                results.push((target, lossless));
            }
        }
        results.sort_by_key(|b| std::cmp::Reverse(b.1));
        results
    }

    /// Attaches metadata to a rule identified by source, target, and feature.
    pub fn set_metadata(
        &mut self,
        source: Dialect,
        target: Dialect,
        feature: &str,
        meta: RuleMetadata,
    ) {
        let key = RuleKey {
            source,
            target,
            feature: feature.to_owned(),
        };
        self.metadata.insert(key, meta);
    }

    /// Retrieves metadata for a rule.
    #[must_use]
    pub fn get_metadata(
        &self,
        source: Dialect,
        target: Dialect,
        feature: &str,
    ) -> Option<&RuleMetadata> {
        let key = RuleKey {
            source,
            target,
            feature: feature.to_owned(),
        };
        self.metadata.get(&key)
    }

    /// Inserts a streaming event mapping.
    pub fn insert_streaming_rule(&mut self, rule: StreamingEventMapping) {
        let key = RuleKey {
            source: rule.source_dialect,
            target: rule.target_dialect,
            feature: rule.source_event.clone(),
        };
        self.streaming.insert(key, rule);
    }

    /// Looks up a streaming event mapping by source dialect, target dialect,
    /// and source event kind.
    #[must_use]
    pub fn lookup_streaming(
        &self,
        source: Dialect,
        target: Dialect,
        event: &str,
    ) -> Option<&StreamingEventMapping> {
        let key = RuleKey {
            source,
            target,
            feature: event.to_owned(),
        };
        self.streaming.get(&key)
    }

    /// Validates that both directions of a mapping (A→B and B→A) exist and
    /// reports any asymmetries.
    #[must_use]
    pub fn validate_bidirectional(
        &self,
        a: Dialect,
        b: Dialect,
        feature: &str,
    ) -> BidirectionalReport {
        let forward = self.lookup(a, b, feature).map(|r| r.fidelity.clone());
        let reverse = self.lookup(b, a, feature).map(|r| r.fidelity.clone());
        let is_symmetric = forward.is_some() && reverse.is_some();

        let mut warnings = Vec::new();

        if forward.is_none() && reverse.is_some() {
            warnings.push(format!(
                "missing {}->{} rule for `{feature}`, but {}->{} exists",
                a.label(),
                b.label(),
                b.label(),
                a.label(),
            ));
        } else if forward.is_some() && reverse.is_none() {
            warnings.push(format!(
                "missing {}->{} rule for `{feature}`, but {}->{} exists",
                b.label(),
                a.label(),
                a.label(),
                b.label(),
            ));
        }

        if let (Some(f), Some(r)) = (&forward, &reverse) {
            match (
                f.is_lossless(),
                r.is_lossless(),
                f.is_unsupported(),
                r.is_unsupported(),
            ) {
                (true, false, _, false) | (false, true, false, _) => {
                    warnings.push(format!(
                        "asymmetric fidelity for `{feature}`: {}->{} differs from {}->{}",
                        a.label(),
                        b.label(),
                        b.label(),
                        a.label(),
                    ));
                }
                (_, _, true, false) | (_, _, false, true) => {
                    warnings.push(format!(
                        "asymmetric support for `{feature}`: one direction is unsupported"
                    ));
                }
                _ => {}
            }
        }

        BidirectionalReport {
            dialect_a: a,
            dialect_b: b,
            feature: feature.to_owned(),
            forward_fidelity: forward,
            reverse_fidelity: reverse,
            is_symmetric,
            warnings,
        }
    }

    /// Validates a mapping chain (e.g. A→B→C) for a feature and computes
    /// the overall (worst-case) fidelity.
    #[must_use]
    pub fn validate_chain(&self, chain: &[Dialect], feature: &str) -> ChainValidation {
        let mut hops = Vec::new();
        let mut errors = Vec::new();
        let mut worst = Fidelity::Lossless;

        if chain.len() < 2 {
            return ChainValidation {
                chain: chain.to_vec(),
                feature: feature.to_owned(),
                hops,
                overall_fidelity: Fidelity::Unsupported {
                    reason: "chain must contain at least 2 dialects".into(),
                },
                errors: vec![MappingError::InvalidInput {
                    reason: "chain must contain at least 2 dialects".into(),
                }],
            };
        }

        for window in chain.windows(2) {
            let (src, tgt) = (window[0], window[1]);
            match self.lookup(src, tgt, feature) {
                Some(rule) => {
                    let fidelity = rule.fidelity.clone();
                    worst = worse_fidelity(&worst, &fidelity);
                    if let Fidelity::LossyLabeled { ref warning } = fidelity {
                        errors.push(MappingError::FidelityLoss {
                            feature: feature.to_owned(),
                            warning: format!("{}->{}: {warning}", src.label(), tgt.label()),
                        });
                    }
                    if fidelity.is_unsupported() {
                        errors.push(MappingError::FeatureUnsupported {
                            feature: feature.to_owned(),
                            from: src,
                            to: tgt,
                        });
                    }
                    hops.push(fidelity);
                }
                None => {
                    let fid = Fidelity::Unsupported {
                        reason: format!("no rule for {}->{}", src.label(), tgt.label()),
                    };
                    worst = worse_fidelity(&worst, &fid);
                    errors.push(MappingError::FeatureUnsupported {
                        feature: feature.to_owned(),
                        from: src,
                        to: tgt,
                    });
                    hops.push(fid);
                }
            }
        }

        ChainValidation {
            chain: chain.to_vec(),
            feature: feature.to_owned(),
            hops,
            overall_fidelity: worst,
            errors,
        }
    }

    /// Generates a fidelity report for a dialect pair across the given
    /// features.
    #[must_use]
    pub fn fidelity_report(
        &self,
        source: Dialect,
        target: Dialect,
        features: &[&str],
    ) -> FidelityReport {
        let mut report = FidelityReport {
            source,
            target,
            lossless: Vec::new(),
            lossy: Vec::new(),
            unsupported: Vec::new(),
            unmapped: Vec::new(),
        };

        for &feat in features {
            match self.lookup(source, target, feat) {
                Some(rule) => match &rule.fidelity {
                    Fidelity::Lossless => report.lossless.push(feat.to_owned()),
                    Fidelity::LossyLabeled { warning } => {
                        report.lossy.push((feat.to_owned(), warning.clone()));
                    }
                    Fidelity::Unsupported { .. } => {
                        report.unsupported.push(feat.to_owned());
                    }
                },
                None => report.unmapped.push(feat.to_owned()),
            }
        }

        report
    }
}

/// Returns the worse of two fidelity levels (unsupported > lossy > lossless).
fn worse_fidelity(a: &Fidelity, b: &Fidelity) -> Fidelity {
    match (a, b) {
        (Fidelity::Unsupported { .. }, _) | (_, Fidelity::Unsupported { .. }) => {
            if a.is_unsupported() {
                a.clone()
            } else {
                b.clone()
            }
        }
        (Fidelity::LossyLabeled { .. }, _) | (_, Fidelity::LossyLabeled { .. }) => {
            if matches!(a, Fidelity::LossyLabeled { .. }) {
                a.clone()
            } else {
                b.clone()
            }
        }
        _ => Fidelity::Lossless,
    }
}

// ── MappingMatrix ───────────────────────────────────────────────────────

/// 2D lookup table of Dialect×Dialect support status.
///
/// Each cell indicates whether the dialect pair has *any* mapping support.
///
/// # Examples
///
/// ```
/// use abp_mapping::MappingMatrix;
/// use abp_dialect::Dialect;
///
/// let mut matrix = MappingMatrix::new();
/// matrix.set(Dialect::OpenAi, Dialect::Claude, true);
///
/// assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
/// assert!(!matrix.is_supported(Dialect::Claude, Dialect::OpenAi));
/// ```
#[derive(Debug, Clone, Default)]
pub struct MappingMatrix {
    /// `(source, target) -> supported`
    cells: HashMap<(Dialect, Dialect), bool>,
}

impl MappingMatrix {
    /// Creates an empty matrix.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the support status for a dialect pair.
    pub fn set(&mut self, source: Dialect, target: Dialect, supported: bool) {
        self.cells.insert((source, target), supported);
    }

    /// Returns the support status for a dialect pair.
    ///
    /// Returns `None` if the pair has not been populated.
    #[must_use]
    pub fn get(&self, source: Dialect, target: Dialect) -> Option<bool> {
        self.cells.get(&(source, target)).copied()
    }

    /// Returns `true` if the dialect pair is supported.
    #[must_use]
    pub fn is_supported(&self, source: Dialect, target: Dialect) -> bool {
        self.cells.get(&(source, target)).copied().unwrap_or(false)
    }

    /// Builds a matrix from a [`MappingRegistry`], marking dialect pairs as
    /// supported when at least one lossless or lossy-labeled rule exists.
    #[must_use]
    pub fn from_registry(registry: &MappingRegistry) -> Self {
        let mut matrix = Self::new();
        for rule in registry.iter() {
            if !rule.fidelity.is_unsupported() {
                matrix.set(rule.source_dialect, rule.target_dialect, true);
            }
        }
        matrix
    }
}

// ── Validation ──────────────────────────────────────────────────────────

/// Validates a set of features for a source→target dialect mapping.
///
/// Returns a [`MappingValidation`] for each requested feature.
///
/// # Examples
///
/// ```
/// use abp_mapping::{validate_mapping, known_rules, Fidelity};
/// use abp_dialect::Dialect;
///
/// let registry = known_rules();
/// let results = validate_mapping(
///     &registry,
///     Dialect::OpenAi,
///     Dialect::Claude,
///     &["tool_use".into(), "streaming".into()],
/// );
/// assert_eq!(results.len(), 2);
/// assert!(results[0].fidelity.is_lossless());
/// ```
#[must_use]
pub fn validate_mapping(
    registry: &MappingRegistry,
    source: Dialect,
    target: Dialect,
    features: &[String],
) -> Vec<MappingValidation> {
    features
        .iter()
        .map(|feature| {
            if feature.is_empty() {
                return MappingValidation {
                    feature: feature.clone(),
                    fidelity: Fidelity::Unsupported {
                        reason: "empty feature name".into(),
                    },
                    errors: vec![MappingError::InvalidInput {
                        reason: "empty feature name".into(),
                    }],
                };
            }

            match registry.lookup(source, target, feature) {
                Some(rule) => {
                    let mut errors = Vec::new();
                    match &rule.fidelity {
                        Fidelity::Unsupported { reason } => {
                            errors.push(MappingError::FeatureUnsupported {
                                feature: feature.clone(),
                                from: source,
                                to: target,
                            });
                            MappingValidation {
                                feature: feature.clone(),
                                fidelity: Fidelity::Unsupported {
                                    reason: reason.clone(),
                                },
                                errors,
                            }
                        }
                        Fidelity::LossyLabeled { warning } => {
                            errors.push(MappingError::FidelityLoss {
                                feature: feature.clone(),
                                warning: warning.clone(),
                            });
                            MappingValidation {
                                feature: feature.clone(),
                                fidelity: Fidelity::LossyLabeled {
                                    warning: warning.clone(),
                                },
                                errors,
                            }
                        }
                        Fidelity::Lossless => MappingValidation {
                            feature: feature.clone(),
                            fidelity: Fidelity::Lossless,
                            errors,
                        },
                    }
                }
                None => MappingValidation {
                    feature: feature.clone(),
                    fidelity: Fidelity::Unsupported {
                        reason: format!("no mapping rule for `{feature}`"),
                    },
                    errors: vec![MappingError::FeatureUnsupported {
                        feature: feature.clone(),
                        from: source,
                        to: target,
                    }],
                },
            }
        })
        .collect()
}

// ── Known rules ─────────────────────────────────────────────────────────

/// Well-known feature names.
pub mod features {
    /// Tool use / function calling.
    pub const TOOL_USE: &str = "tool_use";
    /// Streaming responses.
    pub const STREAMING: &str = "streaming";
    /// Extended thinking / chain-of-thought.
    pub const THINKING: &str = "thinking";
    /// Image input support.
    pub const IMAGE_INPUT: &str = "image_input";
    /// Code execution / bash tool.
    pub const CODE_EXEC: &str = "code_exec";
}

/// Pre-populates a [`MappingRegistry`] with known mapping rules for major
/// features across OpenAI, Claude, Gemini, and Codex.
///
/// # Examples
///
/// ```
/// use abp_mapping::known_rules;
///
/// let registry = known_rules();
/// assert!(!registry.is_empty());
/// ```
#[must_use]
pub fn known_rules() -> MappingRegistry {
    let mut reg = MappingRegistry::new();

    let dialects = Dialect::all();
    let feats = [
        features::TOOL_USE,
        features::STREAMING,
        features::THINKING,
        features::IMAGE_INPUT,
        features::CODE_EXEC,
    ];

    // Same-dialect is always lossless for all features.
    for &d in dialects {
        for &f in &feats {
            reg.insert(MappingRule {
                source_dialect: d,
                target_dialect: d,
                feature: f.into(),
                fidelity: Fidelity::Lossless,
            });
        }
    }

    // ── tool_use ────────────────────────────────────────────────────────
    // All four dialects support tool use with varying fidelity.
    insert_pair_lossless(
        &mut reg,
        Dialect::OpenAi,
        Dialect::Claude,
        features::TOOL_USE,
    );
    insert_pair_lossless(
        &mut reg,
        Dialect::OpenAi,
        Dialect::Gemini,
        features::TOOL_USE,
    );
    insert_pair_lossy(
        &mut reg,
        Dialect::OpenAi,
        Dialect::Codex,
        features::TOOL_USE,
        "Codex tool_use schema differs from chat-completions function calling",
    );
    insert_pair_lossless(
        &mut reg,
        Dialect::Claude,
        Dialect::Gemini,
        features::TOOL_USE,
    );
    insert_pair_lossy(
        &mut reg,
        Dialect::Claude,
        Dialect::Codex,
        features::TOOL_USE,
        "Codex tool_use schema differs from Claude tool_use blocks",
    );
    insert_pair_lossy(
        &mut reg,
        Dialect::Gemini,
        Dialect::Codex,
        features::TOOL_USE,
        "Codex tool_use schema differs from Gemini function declarations",
    );
    insert_pair_lossy(
        &mut reg,
        Dialect::Codex,
        Dialect::OpenAi,
        features::TOOL_USE,
        "Codex tool_use schema differs from chat-completions function calling",
    );
    insert_pair_lossy(
        &mut reg,
        Dialect::Codex,
        Dialect::Claude,
        features::TOOL_USE,
        "Codex tool_use schema differs from Claude tool_use blocks",
    );
    insert_pair_lossy(
        &mut reg,
        Dialect::Codex,
        Dialect::Gemini,
        features::TOOL_USE,
        "Codex tool_use schema differs from Gemini function declarations",
    );

    // ── streaming ───────────────────────────────────────────────────────
    insert_pair_lossless(
        &mut reg,
        Dialect::OpenAi,
        Dialect::Claude,
        features::STREAMING,
    );
    insert_pair_lossless(
        &mut reg,
        Dialect::OpenAi,
        Dialect::Gemini,
        features::STREAMING,
    );
    insert_pair_lossless(
        &mut reg,
        Dialect::OpenAi,
        Dialect::Codex,
        features::STREAMING,
    );
    insert_pair_lossless(
        &mut reg,
        Dialect::Claude,
        Dialect::Gemini,
        features::STREAMING,
    );
    insert_pair_lossless(
        &mut reg,
        Dialect::Claude,
        Dialect::Codex,
        features::STREAMING,
    );
    insert_pair_lossless(
        &mut reg,
        Dialect::Gemini,
        Dialect::Codex,
        features::STREAMING,
    );

    // ── thinking ────────────────────────────────────────────────────────
    insert_pair_lossy(
        &mut reg,
        Dialect::Claude,
        Dialect::OpenAi,
        features::THINKING,
        "OpenAI does not have a native thinking block; mapped to system message",
    );
    insert_pair_lossy(
        &mut reg,
        Dialect::Claude,
        Dialect::Gemini,
        features::THINKING,
        "Gemini thinkingConfig differs from Claude extended thinking",
    );
    insert_pair_lossy(
        &mut reg,
        Dialect::Claude,
        Dialect::Codex,
        features::THINKING,
        "Codex reasoning effort maps loosely to Claude thinking budget",
    );
    insert_pair_lossy(
        &mut reg,
        Dialect::OpenAi,
        Dialect::Claude,
        features::THINKING,
        "OpenAI reasoning_effort maps loosely to Claude thinking budget",
    );
    insert_pair_lossy(
        &mut reg,
        Dialect::OpenAi,
        Dialect::Gemini,
        features::THINKING,
        "OpenAI reasoning tokens have no direct Gemini equivalent",
    );
    insert_pair_lossy(
        &mut reg,
        Dialect::OpenAi,
        Dialect::Codex,
        features::THINKING,
        "reasoning_effort semantics differ between chat-completions and Codex",
    );
    insert_pair_lossy(
        &mut reg,
        Dialect::Gemini,
        Dialect::Claude,
        features::THINKING,
        "Gemini thinkingConfig maps loosely to Claude extended thinking",
    );
    insert_pair_lossy(
        &mut reg,
        Dialect::Gemini,
        Dialect::OpenAi,
        features::THINKING,
        "Gemini thinkingConfig has no direct OpenAI equivalent",
    );
    insert_pair_lossy(
        &mut reg,
        Dialect::Gemini,
        Dialect::Codex,
        features::THINKING,
        "Gemini thinkingConfig maps loosely to Codex reasoning_effort",
    );
    insert_pair_lossy(
        &mut reg,
        Dialect::Codex,
        Dialect::Claude,
        features::THINKING,
        "Codex reasoning effort maps loosely to Claude thinking budget",
    );
    insert_pair_lossy(
        &mut reg,
        Dialect::Codex,
        Dialect::OpenAi,
        features::THINKING,
        "Codex reasoning_effort semantics differ from chat-completions",
    );
    insert_pair_lossy(
        &mut reg,
        Dialect::Codex,
        Dialect::Gemini,
        features::THINKING,
        "Codex reasoning_effort maps loosely to Gemini thinkingConfig",
    );

    // ── image_input ─────────────────────────────────────────────────────
    insert_pair_lossless(
        &mut reg,
        Dialect::OpenAi,
        Dialect::Claude,
        features::IMAGE_INPUT,
    );
    insert_pair_lossless(
        &mut reg,
        Dialect::OpenAi,
        Dialect::Gemini,
        features::IMAGE_INPUT,
    );
    insert_pair_unsupported(
        &mut reg,
        Dialect::OpenAi,
        Dialect::Codex,
        features::IMAGE_INPUT,
        "Codex does not support image inputs",
    );
    insert_pair_lossless(
        &mut reg,
        Dialect::Claude,
        Dialect::Gemini,
        features::IMAGE_INPUT,
    );
    insert_pair_unsupported(
        &mut reg,
        Dialect::Claude,
        Dialect::Codex,
        features::IMAGE_INPUT,
        "Codex does not support image inputs",
    );
    insert_pair_unsupported(
        &mut reg,
        Dialect::Gemini,
        Dialect::Codex,
        features::IMAGE_INPUT,
        "Codex does not support image inputs",
    );

    // ── Kimi & Copilot: tool_use ────────────────────────────────────
    // Both are OpenAI-compatible; lossless with most, lossy with Codex.
    for &nd in &[Dialect::Kimi, Dialect::Copilot] {
        for &od in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
            insert_pair_lossless(&mut reg, nd, od, features::TOOL_USE);
        }
        insert_pair_lossy(
            &mut reg,
            nd,
            Dialect::Codex,
            features::TOOL_USE,
            "Codex tool_use schema differs from OpenAI-compatible format",
        );
        insert_pair_lossy(
            &mut reg,
            Dialect::Codex,
            nd,
            features::TOOL_USE,
            "Codex tool_use schema differs from OpenAI-compatible format",
        );
    }
    insert_pair_lossless(
        &mut reg,
        Dialect::Kimi,
        Dialect::Copilot,
        features::TOOL_USE,
    );

    // ── Kimi & Copilot: streaming ───────────────────────────────────
    // All SSE-based; lossless with all dialects.
    for &nd in &[Dialect::Kimi, Dialect::Copilot] {
        for &od in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            insert_pair_lossless(&mut reg, nd, od, features::STREAMING);
        }
    }
    insert_pair_lossless(
        &mut reg,
        Dialect::Kimi,
        Dialect::Copilot,
        features::STREAMING,
    );

    // ── Kimi & Copilot: thinking ────────────────────────────────────
    // Neither has native thinking; all cross-dialect is lossy.
    for &nd in &[Dialect::Kimi, Dialect::Copilot] {
        for &od in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            let w = format!("{} does not have native thinking blocks", nd.label());
            insert_pair_lossy(&mut reg, nd, od, features::THINKING, &w);
            insert_pair_lossy(&mut reg, od, nd, features::THINKING, &w);
        }
    }
    insert_pair_lossy(
        &mut reg,
        Dialect::Kimi,
        Dialect::Copilot,
        features::THINKING,
        "neither Kimi nor Copilot has native thinking blocks",
    );
    insert_pair_lossy(
        &mut reg,
        Dialect::Copilot,
        Dialect::Kimi,
        features::THINKING,
        "neither Kimi nor Copilot has native thinking blocks",
    );

    // ── Kimi & Copilot: image_input ─────────────────────────────────
    // Neither supports image inputs.
    for &nd in &[Dialect::Kimi, Dialect::Copilot] {
        for &od in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            insert_pair_unsupported(
                &mut reg,
                nd,
                od,
                features::IMAGE_INPUT,
                &format!("{} does not support image inputs", nd.label()),
            );
        }
    }
    insert_pair_unsupported(
        &mut reg,
        Dialect::Kimi,
        Dialect::Copilot,
        features::IMAGE_INPUT,
        "neither Kimi nor Copilot supports image inputs",
    );

    // ── code_exec (all dialects) ────────────────────────────────────
    // Kimi does not support code execution at all.
    // All other cross-dialect code_exec is lossy (different execution models).
    let code_capable = [
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Copilot,
    ];
    for i in 0..code_capable.len() {
        for j in (i + 1)..code_capable.len() {
            let a = code_capable[i];
            let b = code_capable[j];
            let w = format!(
                "code execution models differ between {} and {}",
                a.label(),
                b.label(),
            );
            insert_pair_lossy(&mut reg, a, b, features::CODE_EXEC, &w);
            insert_pair_lossy(&mut reg, b, a, features::CODE_EXEC, &w);
        }
    }
    for &od in &code_capable {
        insert_pair_unsupported(
            &mut reg,
            Dialect::Kimi,
            od,
            features::CODE_EXEC,
            "Kimi does not support code execution",
        );
    }

    reg
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn insert_pair_lossless(reg: &mut MappingRegistry, a: Dialect, b: Dialect, feature: &str) {
    reg.insert(MappingRule {
        source_dialect: a,
        target_dialect: b,
        feature: feature.into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: b,
        target_dialect: a,
        feature: feature.into(),
        fidelity: Fidelity::Lossless,
    });
}

fn insert_pair_lossy(
    reg: &mut MappingRegistry,
    source: Dialect,
    target: Dialect,
    feature: &str,
    warning: &str,
) {
    reg.insert(MappingRule {
        source_dialect: source,
        target_dialect: target,
        feature: feature.into(),
        fidelity: Fidelity::LossyLabeled {
            warning: warning.into(),
        },
    });
}

fn insert_pair_unsupported(
    reg: &mut MappingRegistry,
    source: Dialect,
    target: Dialect,
    feature: &str,
    reason: &str,
) {
    reg.insert(MappingRule {
        source_dialect: source,
        target_dialect: target,
        feature: feature.into(),
        fidelity: Fidelity::Unsupported {
            reason: reason.into(),
        },
    });
    reg.insert(MappingRule {
        source_dialect: target,
        target_dialect: source,
        feature: feature.into(),
        fidelity: Fidelity::Unsupported {
            reason: reason.into(),
        },
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Registry basics ─────────────────────────────────────────────────

    #[test]
    fn empty_registry() {
        let reg = MappingRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn insert_and_lookup() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        let rule = reg.lookup(Dialect::OpenAi, Dialect::Claude, "tool_use");
        assert!(rule.is_some());
        assert!(rule.unwrap().fidelity.is_lossless());
    }

    #[test]
    fn lookup_miss() {
        let reg = MappingRegistry::new();
        assert!(
            reg.lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
                .is_none()
        );
    }

    #[test]
    fn insert_replaces_existing() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "changed".into(),
            },
        });
        assert_eq!(reg.len(), 1);
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
    }

    #[test]
    fn registry_len() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "a".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "b".into(),
            fidelity: Fidelity::Lossless,
        });
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn registry_iter() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "x".into(),
            fidelity: Fidelity::Lossless,
        });
        assert_eq!(reg.iter().count(), 1);
    }

    // ── Fidelity helpers ────────────────────────────────────────────────

    #[test]
    fn fidelity_is_lossless() {
        assert!(Fidelity::Lossless.is_lossless());
        assert!(
            !Fidelity::LossyLabeled {
                warning: "w".into()
            }
            .is_lossless()
        );
        assert!(!Fidelity::Unsupported { reason: "r".into() }.is_lossless());
    }

    #[test]
    fn fidelity_is_unsupported() {
        assert!(!Fidelity::Lossless.is_unsupported());
        assert!(
            !Fidelity::LossyLabeled {
                warning: "w".into()
            }
            .is_unsupported()
        );
        assert!(Fidelity::Unsupported { reason: "r".into() }.is_unsupported());
    }

    // ── Validation ──────────────────────────────────────────────────────

    #[test]
    fn validate_lossless_feature() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "streaming".into(),
            fidelity: Fidelity::Lossless,
        });
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &["streaming".into()],
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_lossless());
        assert!(results[0].errors.is_empty());
    }

    #[test]
    fn validate_lossy_feature() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::OpenAi,
            feature: "thinking".into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "mapped to system".into(),
            },
        });
        let results =
            validate_mapping(&reg, Dialect::Claude, Dialect::OpenAi, &["thinking".into()]);
        assert_eq!(results.len(), 1);
        assert!(!results[0].fidelity.is_lossless());
        assert_eq!(results[0].errors.len(), 1);
        assert!(matches!(
            &results[0].errors[0],
            MappingError::FidelityLoss { .. }
        ));
    }

    #[test]
    fn validate_unsupported_feature() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Codex,
            feature: "image_input".into(),
            fidelity: Fidelity::Unsupported {
                reason: "no images".into(),
            },
        });
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Codex,
            &["image_input".into()],
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_unsupported());
        assert!(matches!(
            &results[0].errors[0],
            MappingError::FeatureUnsupported { .. }
        ));
    }

    #[test]
    fn validate_unknown_feature() {
        let reg = MappingRegistry::new();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &["nonexistent".into()],
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_unsupported());
        assert_eq!(results[0].errors.len(), 1);
    }

    #[test]
    fn validate_empty_feature_name() {
        let reg = MappingRegistry::new();
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);
        assert_eq!(results.len(), 1);
        assert!(matches!(
            &results[0].errors[0],
            MappingError::InvalidInput { .. }
        ));
    }

    #[test]
    fn validate_empty_features_list() {
        let reg = MappingRegistry::new();
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[]);
        assert!(results.is_empty());
    }

    #[test]
    fn validate_multiple_features() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "streaming".into(),
            fidelity: Fidelity::Lossless,
        });
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &["tool_use".into(), "streaming".into(), "unknown".into()],
        );
        assert_eq!(results.len(), 3);
        assert!(results[0].errors.is_empty());
        assert!(results[1].errors.is_empty());
        assert_eq!(results[2].errors.len(), 1);
    }

    // ── Matrix ──────────────────────────────────────────────────────────

    #[test]
    fn matrix_empty() {
        let m = MappingMatrix::new();
        assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), None);
    }

    #[test]
    fn matrix_set_and_get() {
        let mut m = MappingMatrix::new();
        m.set(Dialect::OpenAi, Dialect::Claude, true);
        assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), Some(true));
    }

    #[test]
    fn matrix_is_supported_default_false() {
        let m = MappingMatrix::new();
        assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
    }

    #[test]
    fn matrix_from_registry() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::Gemini,
            target_dialect: Dialect::Codex,
            feature: "image_input".into(),
            fidelity: Fidelity::Unsupported {
                reason: "nope".into(),
            },
        });
        let m = MappingMatrix::from_registry(&reg);
        assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
        // Unsupported-only pair should NOT be marked supported.
        assert!(!m.is_supported(Dialect::Gemini, Dialect::Codex));
    }

    #[test]
    fn matrix_set_overwrite() {
        let mut m = MappingMatrix::new();
        m.set(Dialect::OpenAi, Dialect::Claude, true);
        m.set(Dialect::OpenAi, Dialect::Claude, false);
        assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
    }

    // ── Known rules ─────────────────────────────────────────────────────

    #[test]
    fn known_rules_non_empty() {
        let reg = known_rules();
        assert!(!reg.is_empty());
    }

    #[test]
    fn known_rules_same_dialect_lossless() {
        let reg = known_rules();
        for &d in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            for &f in &[
                features::TOOL_USE,
                features::STREAMING,
                features::THINKING,
                features::IMAGE_INPUT,
            ] {
                let rule = reg.lookup(d, d, f).unwrap();
                assert!(
                    rule.fidelity.is_lossless(),
                    "{d} -> {d} {f} should be lossless"
                );
            }
        }
    }

    #[test]
    fn known_rules_openai_claude_tool_use_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn known_rules_claude_openai_tool_use_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Claude, Dialect::OpenAi, features::TOOL_USE)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn known_rules_streaming_all_lossless() {
        let reg = known_rules();
        let dialects = [
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ];
        for &a in &dialects {
            for &b in &dialects {
                let rule = reg.lookup(a, b, features::STREAMING).unwrap();
                assert!(
                    rule.fidelity.is_lossless(),
                    "streaming {a} -> {b} should be lossless"
                );
            }
        }
    }

    #[test]
    fn known_rules_thinking_cross_dialect_lossy() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING)
            .unwrap();
        assert!(
            !rule.fidelity.is_lossless(),
            "thinking Claude -> OpenAI should be lossy"
        );
    }

    #[test]
    fn known_rules_image_input_to_codex_unsupported() {
        let reg = known_rules();
        for &src in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
            let rule = reg
                .lookup(src, Dialect::Codex, features::IMAGE_INPUT)
                .unwrap();
            assert!(
                rule.fidelity.is_unsupported(),
                "image_input {src} -> Codex should be unsupported"
            );
        }
    }

    #[test]
    fn known_rules_codex_to_others_image_unsupported() {
        let reg = known_rules();
        for &tgt in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
            let rule = reg
                .lookup(Dialect::Codex, tgt, features::IMAGE_INPUT)
                .unwrap();
            assert!(
                rule.fidelity.is_unsupported(),
                "image_input Codex -> {tgt} should be unsupported"
            );
        }
    }

    #[test]
    fn known_rules_openai_gemini_image_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Gemini, features::IMAGE_INPUT)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn known_rules_matrix_has_entries() {
        let reg = known_rules();
        let m = MappingMatrix::from_registry(&reg);
        assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
        assert!(m.is_supported(Dialect::Claude, Dialect::Gemini));
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn same_dialect_lookup() {
        let reg = known_rules();
        for &d in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            assert!(reg.lookup(d, d, features::TOOL_USE).is_some());
        }
    }

    #[test]
    fn unknown_feature_in_known_registry() {
        let reg = known_rules();
        assert!(
            reg.lookup(Dialect::OpenAi, Dialect::Claude, "teleportation")
                .is_none()
        );
    }

    #[test]
    fn validate_with_known_registry() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[
                features::TOOL_USE.into(),
                features::STREAMING.into(),
                features::THINKING.into(),
                features::IMAGE_INPUT.into(),
            ],
        );
        assert_eq!(results.len(), 4);
        // tool_use: lossless
        assert!(results[0].errors.is_empty());
        // streaming: lossless
        assert!(results[1].errors.is_empty());
    }

    // ── Serde round-trip ────────────────────────────────────────────────

    #[test]
    fn fidelity_serde_roundtrip_lossless() {
        let f = Fidelity::Lossless;
        let json = serde_json::to_string(&f).unwrap();
        let f2: Fidelity = serde_json::from_str(&json).unwrap();
        assert_eq!(f, f2);
    }

    #[test]
    fn fidelity_serde_roundtrip_lossy() {
        let f = Fidelity::LossyLabeled {
            warning: "test warning".into(),
        };
        let json = serde_json::to_string(&f).unwrap();
        let f2: Fidelity = serde_json::from_str(&json).unwrap();
        assert_eq!(f, f2);
    }

    #[test]
    fn fidelity_serde_roundtrip_unsupported() {
        let f = Fidelity::Unsupported {
            reason: "no support".into(),
        };
        let json = serde_json::to_string(&f).unwrap();
        let f2: Fidelity = serde_json::from_str(&json).unwrap();
        assert_eq!(f, f2);
    }

    #[test]
    fn mapping_rule_serde_roundtrip() {
        let rule = MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let rule2: MappingRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule, rule2);
    }

    #[test]
    fn mapping_error_serde_roundtrip() {
        let err = MappingError::FeatureUnsupported {
            feature: "img".into(),
            from: Dialect::OpenAi,
            to: Dialect::Codex,
        };
        let json = serde_json::to_string(&err).unwrap();
        let err2: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, err2);
    }

    #[test]
    fn mapping_validation_serde_roundtrip() {
        let v = MappingValidation {
            feature: "streaming".into(),
            fidelity: Fidelity::Lossless,
            errors: vec![],
        };
        let json = serde_json::to_string(&v).unwrap();
        let v2: MappingValidation = serde_json::from_str(&json).unwrap();
        assert_eq!(v, v2);
    }

    // ── Bidirectional validation ────────────────────────────────────────

    #[test]
    fn bidirectional_symmetric_lossless() {
        let reg = known_rules();
        let report =
            reg.validate_bidirectional(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE);
        assert!(report.is_symmetric);
        assert!(report.forward_fidelity.as_ref().unwrap().is_lossless());
        assert!(report.reverse_fidelity.as_ref().unwrap().is_lossless());
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn bidirectional_asymmetric_fidelity() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "test_feat".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::OpenAi,
            feature: "test_feat".into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "lossy reverse".into(),
            },
        });
        let report = reg.validate_bidirectional(Dialect::OpenAi, Dialect::Claude, "test_feat");
        assert!(report.is_symmetric);
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn bidirectional_missing_reverse() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "one_way".into(),
            fidelity: Fidelity::Lossless,
        });
        let report = reg.validate_bidirectional(Dialect::OpenAi, Dialect::Claude, "one_way");
        assert!(!report.is_symmetric);
        assert!(report.forward_fidelity.is_some());
        assert!(report.reverse_fidelity.is_none());
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn bidirectional_missing_forward() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::OpenAi,
            feature: "one_way".into(),
            fidelity: Fidelity::Lossless,
        });
        let report = reg.validate_bidirectional(Dialect::OpenAi, Dialect::Claude, "one_way");
        assert!(!report.is_symmetric);
        assert!(report.forward_fidelity.is_none());
        assert!(report.reverse_fidelity.is_some());
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn bidirectional_both_missing() {
        let reg = MappingRegistry::new();
        let report = reg.validate_bidirectional(Dialect::OpenAi, Dialect::Claude, "ghost");
        assert!(!report.is_symmetric);
        assert!(report.forward_fidelity.is_none());
        assert!(report.reverse_fidelity.is_none());
    }

    #[test]
    fn bidirectional_asymmetric_unsupported() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Codex,
            feature: "img".into(),
            fidelity: Fidelity::Unsupported {
                reason: "no images".into(),
            },
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::Codex,
            target_dialect: Dialect::OpenAi,
            feature: "img".into(),
            fidelity: Fidelity::Lossless,
        });
        let report = reg.validate_bidirectional(Dialect::OpenAi, Dialect::Codex, "img");
        assert!(report.is_symmetric);
        assert!(!report.warnings.is_empty());
        assert!(
            report.warnings[0].contains("asymmetric"),
            "warning should mention asymmetry"
        );
    }

    // ── Chain validation ────────────────────────────────────────────────

    #[test]
    fn chain_all_lossless() {
        let reg = known_rules();
        let result = reg.validate_chain(
            &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini],
            features::STREAMING,
        );
        assert!(result.overall_fidelity.is_lossless());
        assert_eq!(result.hops.len(), 2);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn chain_with_lossy_hop() {
        let reg = known_rules();
        let result = reg.validate_chain(
            &[Dialect::Claude, Dialect::OpenAi, Dialect::Gemini],
            features::THINKING,
        );
        assert!(!result.overall_fidelity.is_lossless());
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn chain_with_unsupported_hop() {
        let reg = known_rules();
        let result = reg.validate_chain(
            &[Dialect::OpenAi, Dialect::Codex, Dialect::Claude],
            features::IMAGE_INPUT,
        );
        assert!(result.overall_fidelity.is_unsupported());
    }

    #[test]
    fn chain_too_short() {
        let reg = known_rules();
        let result = reg.validate_chain(&[Dialect::OpenAi], features::STREAMING);
        assert!(result.overall_fidelity.is_unsupported());
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn chain_four_hops() {
        let reg = known_rules();
        let result = reg.validate_chain(
            &[
                Dialect::OpenAi,
                Dialect::Claude,
                Dialect::Gemini,
                Dialect::Codex,
            ],
            features::STREAMING,
        );
        assert!(result.overall_fidelity.is_lossless());
        assert_eq!(result.hops.len(), 3);
    }

    #[test]
    fn chain_lossy_then_unsupported() {
        let reg = known_rules();
        // thinking is lossy between Claude→OpenAI, and image_input is
        // unsupported to Codex; use image_input for a mixed chain.
        let result = reg.validate_chain(
            &[Dialect::OpenAi, Dialect::Claude, Dialect::Codex],
            features::IMAGE_INPUT,
        );
        assert!(result.overall_fidelity.is_unsupported());
    }

    // ── Token normalization ─────────────────────────────────────────────

    #[test]
    fn token_normalize_openai() {
        let mut fields = HashMap::new();
        fields.insert("prompt_tokens".into(), 100);
        fields.insert("completion_tokens".into(), 50);
        fields.insert("total_tokens".into(), 150);
        let usage = TokenUsage::normalize(Dialect::OpenAi, &fields);
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn token_normalize_openai_with_reasoning() {
        let mut fields = HashMap::new();
        fields.insert("prompt_tokens".into(), 100);
        fields.insert("completion_tokens".into(), 50);
        fields.insert("reasoning_tokens".into(), 30);
        fields.insert("total_tokens".into(), 180);
        let usage = TokenUsage::normalize(Dialect::OpenAi, &fields);
        assert_eq!(usage.reasoning_tokens, Some(30));
        assert_eq!(usage.total_tokens, 180);
    }

    #[test]
    fn token_normalize_claude() {
        let mut fields = HashMap::new();
        fields.insert("input_tokens".into(), 200);
        fields.insert("output_tokens".into(), 80);
        fields.insert("cache_read_input_tokens".into(), 10);
        let usage = TokenUsage::normalize(Dialect::Claude, &fields);
        assert_eq!(usage.input_tokens, 200);
        assert_eq!(usage.output_tokens, 80);
        assert_eq!(usage.cache_read_tokens, Some(10));
        assert_eq!(usage.total_tokens, 280);
    }

    #[test]
    fn token_normalize_claude_with_cache_write() {
        let mut fields = HashMap::new();
        fields.insert("input_tokens".into(), 200);
        fields.insert("output_tokens".into(), 80);
        fields.insert("cache_creation_input_tokens".into(), 15);
        let usage = TokenUsage::normalize(Dialect::Claude, &fields);
        assert_eq!(usage.cache_write_tokens, Some(15));
    }

    #[test]
    fn token_normalize_gemini() {
        let mut fields = HashMap::new();
        fields.insert("promptTokenCount".into(), 300);
        fields.insert("candidatesTokenCount".into(), 120);
        fields.insert("totalTokenCount".into(), 420);
        let usage = TokenUsage::normalize(Dialect::Gemini, &fields);
        assert_eq!(usage.input_tokens, 300);
        assert_eq!(usage.output_tokens, 120);
        assert_eq!(usage.total_tokens, 420);
    }

    #[test]
    fn token_normalize_gemini_with_thoughts() {
        let mut fields = HashMap::new();
        fields.insert("promptTokenCount".into(), 300);
        fields.insert("candidatesTokenCount".into(), 120);
        fields.insert("thoughtsTokenCount".into(), 50);
        let usage = TokenUsage::normalize(Dialect::Gemini, &fields);
        assert_eq!(usage.reasoning_tokens, Some(50));
        // total computed from input + output + reasoning
        assert_eq!(usage.total_tokens, 470);
    }

    #[test]
    fn token_normalize_empty_fields() {
        let fields = HashMap::new();
        let usage = TokenUsage::normalize(Dialect::OpenAi, &fields);
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
    }

    #[test]
    fn token_normalize_codex_uses_openai_fields() {
        let mut fields = HashMap::new();
        fields.insert("prompt_tokens".into(), 50);
        fields.insert("completion_tokens".into(), 25);
        let usage = TokenUsage::normalize(Dialect::Codex, &fields);
        assert_eq!(usage.input_tokens, 50);
        assert_eq!(usage.output_tokens, 25);
        assert_eq!(usage.total_tokens, 75);
    }

    // ── Fidelity report ─────────────────────────────────────────────────

    #[test]
    fn fidelity_report_mixed() {
        let reg = known_rules();
        let all_feats = [
            features::TOOL_USE,
            features::STREAMING,
            features::THINKING,
            features::IMAGE_INPUT,
            features::CODE_EXEC,
        ];
        let report = reg.fidelity_report(Dialect::OpenAi, Dialect::Codex, &all_feats);
        assert!(!report.lossless.is_empty() || !report.lossy.is_empty());
        assert!(!report.unsupported.is_empty()); // image_input
        assert_eq!(report.total_features(), all_feats.len());
    }

    #[test]
    fn fidelity_report_all_lossless() {
        let reg = known_rules();
        let report = reg.fidelity_report(Dialect::OpenAi, Dialect::Claude, &[features::STREAMING]);
        assert!(report.is_all_lossless());
        assert!(!report.has_blockers());
    }

    #[test]
    fn fidelity_report_has_blockers() {
        let reg = known_rules();
        let report = reg.fidelity_report(Dialect::OpenAi, Dialect::Codex, &[features::IMAGE_INPUT]);
        assert!(report.has_blockers());
    }

    #[test]
    fn fidelity_report_unmapped() {
        let reg = MappingRegistry::new();
        let report = reg.fidelity_report(Dialect::OpenAi, Dialect::Claude, &["unknown_feat"]);
        assert_eq!(report.unmapped.len(), 1);
        assert!(report.has_blockers());
    }

    #[test]
    fn fidelity_report_total_features() {
        let reg = known_rules();
        let feats = [features::TOOL_USE, features::STREAMING];
        let report = reg.fidelity_report(Dialect::OpenAi, Dialect::Claude, &feats);
        assert_eq!(report.total_features(), 2);
    }

    // ── Streaming event mapping ─────────────────────────────────────────

    #[test]
    fn streaming_rule_insert_lookup() {
        let mut reg = MappingRegistry::new();
        reg.insert_streaming_rule(StreamingEventMapping {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            source_event: streaming_events::CONTENT_DELTA.into(),
            target_event: "content_block_delta".into(),
            fidelity: Fidelity::Lossless,
        });
        let rule = reg.lookup_streaming(
            Dialect::OpenAi,
            Dialect::Claude,
            streaming_events::CONTENT_DELTA,
        );
        assert!(rule.is_some());
        assert_eq!(rule.unwrap().target_event, "content_block_delta");
    }

    #[test]
    fn streaming_rule_miss() {
        let reg = MappingRegistry::new();
        assert!(
            reg.lookup_streaming(Dialect::OpenAi, Dialect::Claude, "nonexistent")
                .is_none()
        );
    }

    #[test]
    fn streaming_multiple_events() {
        let mut reg = MappingRegistry::new();
        reg.insert_streaming_rule(StreamingEventMapping {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            source_event: streaming_events::CONTENT_DELTA.into(),
            target_event: "content_block_delta".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.insert_streaming_rule(StreamingEventMapping {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            source_event: streaming_events::TOOL_CALL_DELTA.into(),
            target_event: "input_json_delta".into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "schema differs".into(),
            },
        });
        assert!(
            reg.lookup_streaming(
                Dialect::OpenAi,
                Dialect::Claude,
                streaming_events::CONTENT_DELTA,
            )
            .is_some()
        );
        assert!(
            reg.lookup_streaming(
                Dialect::OpenAi,
                Dialect::Claude,
                streaming_events::TOOL_CALL_DELTA,
            )
            .is_some()
        );
    }

    #[test]
    fn streaming_event_fidelity() {
        let mut reg = MappingRegistry::new();
        reg.insert_streaming_rule(StreamingEventMapping {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::OpenAi,
            source_event: streaming_events::THINKING_DELTA.into(),
            target_event: "reasoning_delta".into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "no native thinking block in OpenAI".into(),
            },
        });
        let rule = reg
            .lookup_streaming(
                Dialect::Claude,
                Dialect::OpenAi,
                streaming_events::THINKING_DELTA,
            )
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
    }

    // ── Rule metadata ───────────────────────────────────────────────────

    #[test]
    fn metadata_set_and_get() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.set_metadata(
            Dialect::OpenAi,
            Dialect::Claude,
            "tool_use",
            RuleMetadata::new("Maps function_call to tool_use blocks")
                .with_version("0.1.0")
                .with_note("Schema differences are normalized"),
        );
        let meta = reg
            .get_metadata(Dialect::OpenAi, Dialect::Claude, "tool_use")
            .unwrap();
        assert_eq!(meta.description, "Maps function_call to tool_use blocks");
        assert_eq!(meta.since_version.as_deref(), Some("0.1.0"));
        assert_eq!(meta.notes.len(), 1);
    }

    #[test]
    fn metadata_miss() {
        let reg = MappingRegistry::new();
        assert!(
            reg.get_metadata(Dialect::OpenAi, Dialect::Claude, "tool_use")
                .is_none()
        );
    }

    #[test]
    fn metadata_builder_chain() {
        let meta = RuleMetadata::new("desc")
            .with_version("1.0")
            .with_note("note1")
            .with_note("note2");
        assert_eq!(meta.description, "desc");
        assert_eq!(meta.since_version.as_deref(), Some("1.0"));
        assert_eq!(meta.notes.len(), 2);
    }

    #[test]
    fn metadata_serde_roundtrip() {
        let meta = RuleMetadata::new("test desc")
            .with_version("0.1.0")
            .with_note("important");
        let json = serde_json::to_string(&meta).unwrap();
        let meta2: RuleMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, meta2);
    }

    // ── worse_fidelity helper ───────────────────────────────────────────

    #[test]
    fn worse_fidelity_both_lossless() {
        assert_eq!(
            worse_fidelity(&Fidelity::Lossless, &Fidelity::Lossless),
            Fidelity::Lossless
        );
    }

    #[test]
    fn worse_fidelity_lossy_wins() {
        let lossy = Fidelity::LossyLabeled {
            warning: "w".into(),
        };
        assert!(!worse_fidelity(&Fidelity::Lossless, &lossy).is_lossless());
        assert!(!worse_fidelity(&lossy, &Fidelity::Lossless).is_lossless());
    }

    #[test]
    fn worse_fidelity_unsupported_wins() {
        let unsup = Fidelity::Unsupported { reason: "r".into() };
        let lossy = Fidelity::LossyLabeled {
            warning: "w".into(),
        };
        assert!(worse_fidelity(&lossy, &unsup).is_unsupported());
        assert!(worse_fidelity(&unsup, &Fidelity::Lossless).is_unsupported());
    }

    // ── Token usage serde ───────────────────────────────────────────────

    #[test]
    fn token_usage_serde_roundtrip() {
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            reasoning_tokens: Some(20),
            cache_read_tokens: None,
            cache_write_tokens: None,
            total_tokens: 170,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let usage2: TokenUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(usage, usage2);
    }

    #[test]
    fn token_usage_serde_omits_none() {
        let usage = TokenUsage {
            input_tokens: 10,
            output_tokens: 5,
            reasoning_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            total_tokens: 15,
        };
        let json = serde_json::to_string(&usage).unwrap();
        assert!(!json.contains("reasoning_tokens"));
        assert!(!json.contains("cache_read_tokens"));
    }

    // ── Streaming event mapping serde ───────────────────────────────────

    #[test]
    fn streaming_event_mapping_serde_roundtrip() {
        let mapping = StreamingEventMapping {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            source_event: "content_delta".into(),
            target_event: "content_block_delta".into(),
            fidelity: Fidelity::Lossless,
        };
        let json = serde_json::to_string(&mapping).unwrap();
        let mapping2: StreamingEventMapping = serde_json::from_str(&json).unwrap();
        assert_eq!(mapping, mapping2);
    }

    // ── BidirectionalReport serde ───────────────────────────────────────

    #[test]
    fn bidirectional_report_serde_roundtrip() {
        let report = BidirectionalReport {
            dialect_a: Dialect::OpenAi,
            dialect_b: Dialect::Claude,
            feature: "tool_use".into(),
            forward_fidelity: Some(Fidelity::Lossless),
            reverse_fidelity: Some(Fidelity::Lossless),
            is_symmetric: true,
            warnings: vec![],
        };
        let json = serde_json::to_string(&report).unwrap();
        let report2: BidirectionalReport = serde_json::from_str(&json).unwrap();
        assert_eq!(report, report2);
    }

    // ── ChainValidation serde ───────────────────────────────────────────

    #[test]
    fn chain_validation_serde_roundtrip() {
        let cv = ChainValidation {
            chain: vec![Dialect::OpenAi, Dialect::Claude],
            feature: "streaming".into(),
            hops: vec![Fidelity::Lossless],
            overall_fidelity: Fidelity::Lossless,
            errors: vec![],
        };
        let json = serde_json::to_string(&cv).unwrap();
        let cv2: ChainValidation = serde_json::from_str(&json).unwrap();
        assert_eq!(cv, cv2);
    }

    // ── FidelityReport serde ────────────────────────────────────────────

    #[test]
    fn fidelity_report_serde_roundtrip() {
        let fr = FidelityReport {
            source: Dialect::OpenAi,
            target: Dialect::Claude,
            lossless: vec!["streaming".into()],
            lossy: vec![("thinking".into(), "mapped to system".into())],
            unsupported: vec![],
            unmapped: vec![],
        };
        let json = serde_json::to_string(&fr).unwrap();
        let fr2: FidelityReport = serde_json::from_str(&json).unwrap();
        assert_eq!(fr, fr2);
    }
}
