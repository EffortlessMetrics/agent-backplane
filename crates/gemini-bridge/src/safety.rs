// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Extended safety helpers for Gemini API integration.
//!
//! Provides typed block reasons, safety-profile presets, and analysis
//! utilities on top of the core safety types in [`crate::gemini_types`].

use crate::gemini_types::{
    HarmBlockThreshold, HarmCategory, HarmProbability, PromptFeedback, SafetyRating, SafetySetting,
};
use serde::{Deserialize, Serialize};

// ── BlockReason (typed) ─────────────────────────────────────────────────

/// Typed prompt block reason.
///
/// The Gemini API returns `block_reason` as a string; this enum provides
/// a typed representation for matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BlockReason {
    /// Prompt blocked for safety.
    Safety,
    /// Prompt blocked for other reasons.
    Other,
    /// Blocked due to blocklist match.
    Blocklist,
    /// Blocked due to prohibited content.
    ProhibitedContent,
}

impl BlockReason {
    /// Try to parse a raw block-reason string into a typed [`BlockReason`].
    #[must_use]
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "SAFETY" => Some(Self::Safety),
            "OTHER" => Some(Self::Other),
            "BLOCKLIST" => Some(Self::Blocklist),
            "PROHIBITED_CONTENT" => Some(Self::ProhibitedContent),
            _ => None,
        }
    }
}

// ── SafetySetting helpers ───────────────────────────────────────────────

/// Create a [`SafetySetting`] for a given category and threshold.
#[must_use]
pub fn setting(category: HarmCategory, threshold: HarmBlockThreshold) -> SafetySetting {
    SafetySetting {
        category,
        threshold,
    }
}

/// Create a profile that blocks nothing for all categories.
#[must_use]
pub fn permissive_profile() -> Vec<SafetySetting> {
    all_categories()
        .into_iter()
        .map(|cat| setting(cat, HarmBlockThreshold::BlockNone))
        .collect()
}

/// Create a strict profile that blocks low-and-above for all categories.
#[must_use]
pub fn strict_profile() -> Vec<SafetySetting> {
    all_categories()
        .into_iter()
        .map(|cat| setting(cat, HarmBlockThreshold::BlockLowAndAbove))
        .collect()
}

/// Create a balanced profile that blocks medium-and-above for all categories.
#[must_use]
pub fn balanced_profile() -> Vec<SafetySetting> {
    all_categories()
        .into_iter()
        .map(|cat| setting(cat, HarmBlockThreshold::BlockMediumAndAbove))
        .collect()
}

/// Return all defined harm categories.
#[must_use]
pub fn all_categories() -> Vec<HarmCategory> {
    vec![
        HarmCategory::HarmCategoryHarassment,
        HarmCategory::HarmCategoryHateSpeech,
        HarmCategory::HarmCategorySexuallyExplicit,
        HarmCategory::HarmCategoryDangerousContent,
        HarmCategory::HarmCategoryCivicIntegrity,
    ]
}

// ── SafetyRating analysis ───────────────────────────────────────────────

/// Returns the numeric severity of a [`HarmProbability`] (0 = negligible, 3 = high).
#[must_use]
pub fn probability_severity(p: HarmProbability) -> u8 {
    match p {
        HarmProbability::Negligible => 0,
        HarmProbability::Low => 1,
        HarmProbability::Medium => 2,
        HarmProbability::High => 3,
    }
}

/// Find the highest-severity rating from a slice, if any.
#[must_use]
pub fn max_severity(ratings: &[SafetyRating]) -> Option<&SafetyRating> {
    ratings
        .iter()
        .max_by_key(|r| probability_severity(r.probability))
}

/// Returns `true` if any rating in the slice is at or above the given threshold.
#[must_use]
pub fn any_above_threshold(ratings: &[SafetyRating], min_probability: HarmProbability) -> bool {
    let min_sev = probability_severity(min_probability);
    ratings
        .iter()
        .any(|r| probability_severity(r.probability) >= min_sev)
}

/// Check if a rating would be blocked by a given threshold.
#[must_use]
pub fn is_blocked_by(rating: &SafetyRating, threshold: HarmBlockThreshold) -> bool {
    let sev = probability_severity(rating.probability);
    match threshold {
        HarmBlockThreshold::BlockNone => false,
        HarmBlockThreshold::BlockOnlyHigh => sev >= 3,
        HarmBlockThreshold::BlockMediumAndAbove => sev >= 2,
        HarmBlockThreshold::BlockLowAndAbove => sev >= 1,
    }
}

// ── PromptFeedback analysis ─────────────────────────────────────────────

/// Returns `true` if the prompt feedback indicates the prompt was blocked.
#[must_use]
pub fn is_prompt_blocked(feedback: &PromptFeedback) -> bool {
    feedback.block_reason.is_some()
}

/// Parse the typed [`BlockReason`] from a [`PromptFeedback`], if present.
#[must_use]
pub fn prompt_block_reason(feedback: &PromptFeedback) -> Option<BlockReason> {
    feedback
        .block_reason
        .as_deref()
        .and_then(BlockReason::from_str_opt)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── BlockReason ─────────────────────────────────────────────────

    #[test]
    fn block_reason_serde_roundtrip() {
        for reason in [
            BlockReason::Safety,
            BlockReason::Other,
            BlockReason::Blocklist,
            BlockReason::ProhibitedContent,
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            let back: BlockReason = serde_json::from_str(&json).unwrap();
            assert_eq!(reason, back);
        }
    }

    #[test]
    fn block_reason_from_str_opt() {
        assert_eq!(
            BlockReason::from_str_opt("SAFETY"),
            Some(BlockReason::Safety)
        );
        assert_eq!(BlockReason::from_str_opt("OTHER"), Some(BlockReason::Other));
        assert_eq!(
            BlockReason::from_str_opt("BLOCKLIST"),
            Some(BlockReason::Blocklist)
        );
        assert_eq!(
            BlockReason::from_str_opt("PROHIBITED_CONTENT"),
            Some(BlockReason::ProhibitedContent)
        );
        assert_eq!(BlockReason::from_str_opt("UNKNOWN"), None);
    }

    // ── Safety profiles ─────────────────────────────────────────────

    #[test]
    fn permissive_profile_all_block_none() {
        let profile = permissive_profile();
        assert_eq!(profile.len(), 5);
        for s in &profile {
            assert_eq!(s.threshold, HarmBlockThreshold::BlockNone);
        }
    }

    #[test]
    fn strict_profile_all_block_low() {
        let profile = strict_profile();
        assert_eq!(profile.len(), 5);
        for s in &profile {
            assert_eq!(s.threshold, HarmBlockThreshold::BlockLowAndAbove);
        }
    }

    #[test]
    fn balanced_profile_all_block_medium() {
        let profile = balanced_profile();
        assert_eq!(profile.len(), 5);
        for s in &profile {
            assert_eq!(s.threshold, HarmBlockThreshold::BlockMediumAndAbove);
        }
    }

    #[test]
    fn all_categories_count() {
        assert_eq!(all_categories().len(), 5);
    }

    // ── Probability severity ────────────────────────────────────────

    #[test]
    fn probability_severity_ordering() {
        assert!(
            probability_severity(HarmProbability::Negligible)
                < probability_severity(HarmProbability::Low)
        );
        assert!(
            probability_severity(HarmProbability::Low)
                < probability_severity(HarmProbability::Medium)
        );
        assert!(
            probability_severity(HarmProbability::Medium)
                < probability_severity(HarmProbability::High)
        );
    }

    // ── max_severity ────────────────────────────────────────────────

    #[test]
    fn max_severity_finds_highest() {
        let ratings = vec![
            SafetyRating {
                category: HarmCategory::HarmCategoryHarassment,
                probability: HarmProbability::Low,
            },
            SafetyRating {
                category: HarmCategory::HarmCategoryHateSpeech,
                probability: HarmProbability::High,
            },
            SafetyRating {
                category: HarmCategory::HarmCategoryDangerousContent,
                probability: HarmProbability::Negligible,
            },
        ];
        let max = max_severity(&ratings).unwrap();
        assert_eq!(max.probability, HarmProbability::High);
        assert_eq!(max.category, HarmCategory::HarmCategoryHateSpeech);
    }

    #[test]
    fn max_severity_empty() {
        assert!(max_severity(&[]).is_none());
    }

    // ── any_above_threshold ─────────────────────────────────────────

    #[test]
    fn any_above_threshold_true() {
        let ratings = vec![SafetyRating {
            category: HarmCategory::HarmCategoryHarassment,
            probability: HarmProbability::High,
        }];
        assert!(any_above_threshold(&ratings, HarmProbability::Medium));
    }

    #[test]
    fn any_above_threshold_false() {
        let ratings = vec![SafetyRating {
            category: HarmCategory::HarmCategoryHarassment,
            probability: HarmProbability::Low,
        }];
        assert!(!any_above_threshold(&ratings, HarmProbability::Medium));
    }

    #[test]
    fn any_above_threshold_equal() {
        let ratings = vec![SafetyRating {
            category: HarmCategory::HarmCategoryHarassment,
            probability: HarmProbability::Medium,
        }];
        assert!(any_above_threshold(&ratings, HarmProbability::Medium));
    }

    // ── is_blocked_by ───────────────────────────────────────────────

    #[test]
    fn is_blocked_by_block_none_never_blocks() {
        let rating = SafetyRating {
            category: HarmCategory::HarmCategoryHarassment,
            probability: HarmProbability::High,
        };
        assert!(!is_blocked_by(&rating, HarmBlockThreshold::BlockNone));
    }

    #[test]
    fn is_blocked_by_high_only() {
        let high = SafetyRating {
            category: HarmCategory::HarmCategoryHarassment,
            probability: HarmProbability::High,
        };
        let medium = SafetyRating {
            category: HarmCategory::HarmCategoryHarassment,
            probability: HarmProbability::Medium,
        };
        assert!(is_blocked_by(&high, HarmBlockThreshold::BlockOnlyHigh));
        assert!(!is_blocked_by(&medium, HarmBlockThreshold::BlockOnlyHigh));
    }

    #[test]
    fn is_blocked_by_medium_and_above() {
        let medium = SafetyRating {
            category: HarmCategory::HarmCategoryHarassment,
            probability: HarmProbability::Medium,
        };
        let low = SafetyRating {
            category: HarmCategory::HarmCategoryHarassment,
            probability: HarmProbability::Low,
        };
        assert!(is_blocked_by(
            &medium,
            HarmBlockThreshold::BlockMediumAndAbove
        ));
        assert!(!is_blocked_by(
            &low,
            HarmBlockThreshold::BlockMediumAndAbove
        ));
    }

    #[test]
    fn is_blocked_by_low_and_above() {
        let low = SafetyRating {
            category: HarmCategory::HarmCategoryHarassment,
            probability: HarmProbability::Low,
        };
        let negligible = SafetyRating {
            category: HarmCategory::HarmCategoryHarassment,
            probability: HarmProbability::Negligible,
        };
        assert!(is_blocked_by(&low, HarmBlockThreshold::BlockLowAndAbove));
        assert!(!is_blocked_by(
            &negligible,
            HarmBlockThreshold::BlockLowAndAbove
        ));
    }

    // ── Prompt feedback ─────────────────────────────────────────────

    #[test]
    fn is_prompt_blocked_true() {
        let fb = PromptFeedback {
            block_reason: Some("SAFETY".into()),
            safety_ratings: None,
        };
        assert!(is_prompt_blocked(&fb));
    }

    #[test]
    fn is_prompt_blocked_false() {
        let fb = PromptFeedback {
            block_reason: None,
            safety_ratings: Some(vec![SafetyRating {
                category: HarmCategory::HarmCategoryHarassment,
                probability: HarmProbability::Negligible,
            }]),
        };
        assert!(!is_prompt_blocked(&fb));
    }

    #[test]
    fn prompt_block_reason_safety() {
        let fb = PromptFeedback {
            block_reason: Some("SAFETY".into()),
            safety_ratings: None,
        };
        assert_eq!(prompt_block_reason(&fb), Some(BlockReason::Safety));
    }

    #[test]
    fn prompt_block_reason_none() {
        let fb = PromptFeedback {
            block_reason: None,
            safety_ratings: None,
        };
        assert_eq!(prompt_block_reason(&fb), None);
    }

    #[test]
    fn prompt_block_reason_unknown_string() {
        let fb = PromptFeedback {
            block_reason: Some("FUTURE_REASON".into()),
            safety_ratings: None,
        };
        assert_eq!(prompt_block_reason(&fb), None);
    }

    // ── SafetySetting serde ─────────────────────────────────────────

    #[test]
    fn safety_setting_serde_all_combos() {
        for cat in all_categories() {
            for threshold in [
                HarmBlockThreshold::BlockNone,
                HarmBlockThreshold::BlockLowAndAbove,
                HarmBlockThreshold::BlockMediumAndAbove,
                HarmBlockThreshold::BlockOnlyHigh,
            ] {
                let s = setting(cat, threshold);
                let json = serde_json::to_string(&s).unwrap();
                let back: SafetySetting = serde_json::from_str(&json).unwrap();
                assert_eq!(s, back);
            }
        }
    }
}
