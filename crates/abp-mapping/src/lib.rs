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

// ── MappingRegistry ─────────────────────────────────────────────────────

/// Key for registry lookups.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RuleKey {
    source: Dialect,
    target: Dialect,
    feature: String,
}

/// Collects [`MappingRule`]s and provides lookup by source, target, and feature.
#[derive(Debug, Clone, Default)]
pub struct MappingRegistry {
    rules: HashMap<RuleKey, MappingRule>,
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
}

// ── MappingMatrix ───────────────────────────────────────────────────────

/// 2D lookup table of Dialect×Dialect support status.
///
/// Each cell indicates whether the dialect pair has *any* mapping support.
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
}

/// Pre-populates a [`MappingRegistry`] with known mapping rules for major
/// features across OpenAI, Claude, Gemini, and Codex.
#[must_use]
pub fn known_rules() -> MappingRegistry {
    let mut reg = MappingRegistry::new();

    let dialects = [
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ];
    let feats = [
        features::TOOL_USE,
        features::STREAMING,
        features::THINKING,
        features::IMAGE_INPUT,
    ];

    // Same-dialect is always lossless for all features.
    for &d in &dialects {
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
}
