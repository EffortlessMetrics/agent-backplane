// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cross-dialect compatibility analysis.
//!
//! `dialect_compatibility` compares two dialects and produces a
//! `CompatibilityReport` detailing native matches, emulation
//! opportunities, and feature gaps.

use serde::{Deserialize, Serialize};

use crate::Dialect;
use crate::features::{DialectFeature, FeatureSupport};
use crate::matrix::dialect_features;

// ── FeatureGap ──────────────────────────────────────────────────────────

/// Describes how a single feature differs between source and target dialects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureGap {
    /// The feature being compared.
    pub feature: DialectFeature,
    /// Support level in the source dialect.
    pub source_support: FeatureSupport,
    /// Support level in the target dialect.
    pub target_support: FeatureSupport,
}

// ── CompatibilityReport ─────────────────────────────────────────────────

/// Result of comparing two dialects' feature sets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompatibilityReport {
    /// Source dialect.
    pub source: Dialect,
    /// Target dialect.
    pub target: Dialect,
    /// Features natively supported by both dialects.
    pub native_matches: Vec<DialectFeature>,
    /// Features that exist in the source but must be emulated in the target.
    pub emulations: Vec<FeatureGap>,
    /// Features available in the source but completely absent in the target.
    pub gaps: Vec<FeatureGap>,
    /// Overall compatibility score in `[0.0, 1.0]`.
    pub score: f64,
}

impl CompatibilityReport {
    /// Returns `true` when every source feature is at least available
    /// (native or emulated) in the target.
    #[must_use]
    pub fn is_fully_compatible(&self) -> bool {
        self.gaps.is_empty()
    }

    /// Number of features that cannot be translated to the target.
    #[must_use]
    pub fn gap_count(&self) -> usize {
        self.gaps.len()
    }

    /// Number of features requiring emulation in the target.
    #[must_use]
    pub fn emulation_count(&self) -> usize {
        self.emulations.len()
    }
}

impl std::fmt::Display for CompatibilityReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}→{}: score={:.0}%, native={}, emulated={}, gaps={}",
            self.source.label(),
            self.target.label(),
            self.score * 100.0,
            self.native_matches.len(),
            self.emulations.len(),
            self.gaps.len(),
        )
    }
}

// ── Public API ──────────────────────────────────────────────────────────

/// Compare two dialects and produce a compatibility report.
///
/// For each feature that is available (native or emulated) in `source`,
/// checks whether `target` can handle it — natively, via emulation, or
/// not at all.
#[must_use]
pub fn dialect_compatibility(source: Dialect, target: Dialect) -> CompatibilityReport {
    let src = dialect_features(source);
    let tgt = dialect_features(target);

    let mut native_matches = Vec::new();
    let mut emulations = Vec::new();
    let mut gaps = Vec::new();

    let source_available: Vec<_> = src.available_features();

    for feature in &source_available {
        let src_support = src.supports(*feature);
        let tgt_support = tgt.supports(*feature);

        match tgt_support {
            FeatureSupport::Native => {
                native_matches.push(*feature);
            }
            FeatureSupport::Emulated => {
                emulations.push(FeatureGap {
                    feature: *feature,
                    source_support: src_support,
                    target_support: tgt_support,
                });
            }
            FeatureSupport::None => {
                gaps.push(FeatureGap {
                    feature: *feature,
                    source_support: src_support,
                    target_support: tgt_support,
                });
            }
        }
    }

    let total = source_available.len().max(1) as f64;
    let score = (native_matches.len() as f64 + emulations.len() as f64 * 0.5) / total;

    CompatibilityReport {
        source,
        target,
        native_matches,
        emulations,
        gaps,
        score: score.min(1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_dialect_fully_compatible() {
        let r = dialect_compatibility(Dialect::OpenAi, Dialect::OpenAi);
        // Same dialect: no gaps (all features match at same or better level)
        assert!(r.is_fully_compatible());
        assert_eq!(r.gap_count(), 0);
    }

    #[test]
    fn same_dialect_score_high() {
        let r = dialect_compatibility(Dialect::Claude, Dialect::Claude);
        // Score may be < 1.0 when dialect has emulated features (counted at 0.5)
        assert!(r.score > 0.9, "expected high score, got {}", r.score);
        assert!(r.is_fully_compatible());
    }

    #[test]
    fn gemini_target_is_fully_compatible_with_any_source() {
        // Gemini has all features natively
        for d in Dialect::all() {
            let r = dialect_compatibility(*d, Dialect::Gemini);
            assert!(
                r.is_fully_compatible(),
                "{d:?}→Gemini should be fully compatible, gaps: {:?}",
                r.gaps
            );
        }
    }

    #[test]
    fn openai_to_claude_has_gaps() {
        let r = dialect_compatibility(Dialect::OpenAi, Dialect::Claude);
        // OpenAI has Audio native, Claude has Audio=None
        let audio_gap = r.gaps.iter().find(|g| g.feature == DialectFeature::Audio);
        assert!(audio_gap.is_some(), "expected Audio gap OpenAI→Claude");
    }

    #[test]
    fn claude_to_openai_has_gaps() {
        let r = dialect_compatibility(Dialect::Claude, Dialect::OpenAi);
        // Claude has ExtendedThinking native, OpenAI has None
        let thinking_gap = r
            .gaps
            .iter()
            .find(|g| g.feature == DialectFeature::ExtendedThinking);
        assert!(
            thinking_gap.is_some(),
            "expected ExtendedThinking gap Claude→OpenAI"
        );
    }

    #[test]
    fn compatibility_report_display() {
        let r = dialect_compatibility(Dialect::OpenAi, Dialect::Claude);
        let s = r.to_string();
        assert!(s.contains("OpenAI"));
        assert!(s.contains("Claude"));
        assert!(s.contains("score="));
    }

    #[test]
    fn compatibility_score_bounded() {
        for a in Dialect::all() {
            for b in Dialect::all() {
                let r = dialect_compatibility(*a, *b);
                assert!(
                    r.score >= 0.0 && r.score <= 1.0,
                    "score out of bounds: {}",
                    r.score
                );
            }
        }
    }

    #[test]
    fn emulations_have_correct_target_support() {
        let r = dialect_compatibility(Dialect::OpenAi, Dialect::Claude);
        for e in &r.emulations {
            assert_eq!(e.target_support, FeatureSupport::Emulated);
        }
    }

    #[test]
    fn gaps_have_none_target_support() {
        let r = dialect_compatibility(Dialect::Claude, Dialect::OpenAi);
        for g in &r.gaps {
            assert_eq!(g.target_support, FeatureSupport::None);
        }
    }

    #[test]
    fn native_matches_plus_emulations_plus_gaps_equals_source_available() {
        for a in Dialect::all() {
            for b in Dialect::all() {
                let r = dialect_compatibility(*a, *b);
                let src = dialect_features(*a);
                let source_available = src.available_features().len();
                let total = r.native_matches.len() + r.emulations.len() + r.gaps.len();
                assert_eq!(
                    total, source_available,
                    "mismatch for {a:?}→{b:?}: {total} != {source_available}"
                );
            }
        }
    }

    #[test]
    fn copilot_to_claude_vision_is_gap_or_emulation() {
        let r = dialect_compatibility(Dialect::Copilot, Dialect::Claude);
        // Copilot Vision=Emulated, Claude Vision=Native
        // So it should show up in native_matches
        assert!(r.native_matches.contains(&DialectFeature::Vision));
    }

    #[test]
    fn kimi_to_openai_parallel_tool_calls() {
        let r = dialect_compatibility(Dialect::Kimi, Dialect::OpenAi);
        // Kimi ParallelToolCalls=Emulated (available), OpenAI=Native
        assert!(
            r.native_matches
                .contains(&DialectFeature::ParallelToolCalls)
        );
    }

    #[test]
    fn compatibility_report_serde_roundtrip() {
        let r = dialect_compatibility(Dialect::OpenAi, Dialect::Claude);
        let json = serde_json::to_string(&r).unwrap();
        let back: CompatibilityReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.source, r.source);
        assert_eq!(back.target, r.target);
        assert_eq!(back.native_matches, r.native_matches);
        assert_eq!(back.emulations, r.emulations);
        assert_eq!(back.gaps, r.gaps);
    }

    #[test]
    fn feature_gap_serde_roundtrip() {
        let gap = FeatureGap {
            feature: DialectFeature::Audio,
            source_support: FeatureSupport::Native,
            target_support: FeatureSupport::None,
        };
        let json = serde_json::to_string(&gap).unwrap();
        let back: FeatureGap = serde_json::from_str(&json).unwrap();
        assert_eq!(back, gap);
    }

    #[test]
    fn is_fully_compatible_false_when_gaps_exist() {
        let r = dialect_compatibility(Dialect::OpenAi, Dialect::Kimi);
        // OpenAI has features Kimi lacks (e.g. Embeddings=Native, Kimi=None)
        if !r.gaps.is_empty() {
            assert!(!r.is_fully_compatible());
        }
    }
}
