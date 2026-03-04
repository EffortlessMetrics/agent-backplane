#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Deep comprehensive tests for `abp-mapping` cross-dialect mapping validation.
//!
//! 150+ tests covering:
//! 1. MappingRule construction and variants
//! 2. MappingRegistry registration and lookup
//! 3. Mapping validation (validate_mapping)
//! 4. Fidelity levels (Lossless, Lossy, Unsupported)
//! 5. Known mapping rules (known_rules)
//! 6. Feature detection (features)
//! 7. MappingError types and handling
//! 8. Cross-dialect mapping validation (OpenAI→Claude, etc.)
//! 9. Mapping chain composition
//! 10. Mapping metadata and documentation
//! 11. Mapping priority and conflict resolution
//! 12. Edge cases (empty rules, duplicate rules)
//! 13. Mapping serialization
//! 14. Mapping capabilities query
//! 15. Mapping coverage analysis

use abp_dialect::Dialect;
use abp_mapping::{
    Fidelity, MappingError, MappingMatrix, MappingRegistry, MappingRule, MappingValidation,
    features, known_rules, validate_mapping,
};

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn all_dialects() -> &'static [Dialect] {
    Dialect::all()
}

fn all_features() -> Vec<&'static str> {
    vec![
        features::TOOL_USE,
        features::STREAMING,
        features::THINKING,
        features::IMAGE_INPUT,
        features::CODE_EXEC,
    ]
}

fn make_rule(src: Dialect, tgt: Dialect, feat: &str, fidelity: Fidelity) -> MappingRule {
    MappingRule {
        source_dialect: src,
        target_dialect: tgt,
        feature: feat.into(),
        fidelity,
    }
}

fn make_lossless(src: Dialect, tgt: Dialect, feat: &str) -> MappingRule {
    make_rule(src, tgt, feat, Fidelity::Lossless)
}

fn make_lossy(src: Dialect, tgt: Dialect, feat: &str, warning: &str) -> MappingRule {
    make_rule(
        src,
        tgt,
        feat,
        Fidelity::LossyLabeled {
            warning: warning.into(),
        },
    )
}

fn make_unsupported(src: Dialect, tgt: Dialect, feat: &str, reason: &str) -> MappingRule {
    make_rule(
        src,
        tgt,
        feat,
        Fidelity::Unsupported {
            reason: reason.into(),
        },
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. MappingRule construction and variants
// ═══════════════════════════════════════════════════════════════════════════

mod mapping_rule_construction {
    use super::*;

    #[test]
    fn lossless_rule_has_correct_fields() {
        let rule = make_lossless(Dialect::OpenAi, Dialect::Claude, "tool_use");
        assert_eq!(rule.source_dialect, Dialect::OpenAi);
        assert_eq!(rule.target_dialect, Dialect::Claude);
        assert_eq!(rule.feature, "tool_use");
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn lossy_rule_preserves_warning() {
        let rule = make_lossy(
            Dialect::Claude,
            Dialect::OpenAi,
            "thinking",
            "mapped to system",
        );
        assert_eq!(rule.feature, "thinking");
        assert!(!rule.fidelity.is_lossless());
        assert!(!rule.fidelity.is_unsupported());
        if let Fidelity::LossyLabeled { warning } = &rule.fidelity {
            assert_eq!(warning, "mapped to system");
        } else {
            panic!("expected LossyLabeled");
        }
    }

    #[test]
    fn unsupported_rule_preserves_reason() {
        let rule = make_unsupported(Dialect::OpenAi, Dialect::Codex, "image_input", "no images");
        assert!(rule.fidelity.is_unsupported());
        if let Fidelity::Unsupported { reason } = &rule.fidelity {
            assert_eq!(reason, "no images");
        } else {
            panic!("expected Unsupported");
        }
    }

    #[test]
    fn rule_with_empty_feature_name() {
        let rule = make_lossless(Dialect::OpenAi, Dialect::Claude, "");
        assert_eq!(rule.feature, "");
    }

    #[test]
    fn rule_with_unicode_feature_name() {
        let rule = make_lossless(Dialect::OpenAi, Dialect::Claude, "工具使用");
        assert_eq!(rule.feature, "工具使用");
    }

    #[test]
    fn rule_with_long_feature_name() {
        let long_name = "a".repeat(1000);
        let rule = make_lossless(Dialect::OpenAi, Dialect::Claude, &long_name);
        assert_eq!(rule.feature.len(), 1000);
    }

    #[test]
    fn rule_equality_same_values() {
        let r1 = make_lossless(Dialect::OpenAi, Dialect::Claude, "streaming");
        let r2 = make_lossless(Dialect::OpenAi, Dialect::Claude, "streaming");
        assert_eq!(r1, r2);
    }

    #[test]
    fn rule_inequality_different_fidelity() {
        let r1 = make_lossless(Dialect::OpenAi, Dialect::Claude, "streaming");
        let r2 = make_lossy(Dialect::OpenAi, Dialect::Claude, "streaming", "loss");
        assert_ne!(r1, r2);
    }

    #[test]
    fn rule_inequality_different_dialects() {
        let r1 = make_lossless(Dialect::OpenAi, Dialect::Claude, "streaming");
        let r2 = make_lossless(Dialect::OpenAi, Dialect::Gemini, "streaming");
        assert_ne!(r1, r2);
    }

    #[test]
    fn rule_inequality_different_features() {
        let r1 = make_lossless(Dialect::OpenAi, Dialect::Claude, "tool_use");
        let r2 = make_lossless(Dialect::OpenAi, Dialect::Claude, "streaming");
        assert_ne!(r1, r2);
    }

    #[test]
    fn rule_clone_produces_equal_copy() {
        let rule = make_lossy(Dialect::Gemini, Dialect::Codex, "thinking", "warn");
        let cloned = rule.clone();
        assert_eq!(rule, cloned);
    }

    #[test]
    fn rule_debug_format_is_readable() {
        let rule = make_lossless(Dialect::OpenAi, Dialect::Claude, "tool_use");
        let dbg = format!("{rule:?}");
        assert!(dbg.contains("OpenAi"));
        assert!(dbg.contains("Claude"));
        assert!(dbg.contains("tool_use"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. MappingRegistry registration and lookup
// ═══════════════════════════════════════════════════════════════════════════

mod registry_basics {
    use super::*;

    #[test]
    fn new_registry_is_empty() {
        let reg = MappingRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn default_registry_is_empty() {
        let reg = MappingRegistry::default();
        assert!(reg.is_empty());
    }

    #[test]
    fn insert_single_rule_increments_len() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "tool_use"));
        assert_eq!(reg.len(), 1);
        assert!(!reg.is_empty());
    }

    #[test]
    fn insert_multiple_distinct_rules() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "tool_use"));
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "streaming"));
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Gemini, "tool_use"));
        assert_eq!(reg.len(), 3);
    }

    #[test]
    fn lookup_existing_rule_returns_some() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "tool_use"));
        assert!(
            reg.lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
                .is_some()
        );
    }

    #[test]
    fn lookup_missing_source_returns_none() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "tool_use"));
        assert!(
            reg.lookup(Dialect::Gemini, Dialect::Claude, "tool_use")
                .is_none()
        );
    }

    #[test]
    fn lookup_missing_target_returns_none() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "tool_use"));
        assert!(
            reg.lookup(Dialect::OpenAi, Dialect::Gemini, "tool_use")
                .is_none()
        );
    }

    #[test]
    fn lookup_missing_feature_returns_none() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "tool_use"));
        assert!(
            reg.lookup(Dialect::OpenAi, Dialect::Claude, "streaming")
                .is_none()
        );
    }

    #[test]
    fn lookup_is_direction_sensitive() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "tool_use"));
        // Reverse direction should not match
        assert!(
            reg.lookup(Dialect::Claude, Dialect::OpenAi, "tool_use")
                .is_none()
        );
    }

    #[test]
    fn insert_replaces_existing_rule() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "tool_use"));
        reg.insert(make_lossy(
            Dialect::OpenAi,
            Dialect::Claude,
            "tool_use",
            "changed",
        ));
        assert_eq!(reg.len(), 1);
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
    }

    #[test]
    fn iter_returns_all_rules() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "a"));
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "b"));
        reg.insert(make_lossless(Dialect::Claude, Dialect::Gemini, "a"));
        assert_eq!(reg.iter().count(), 3);
    }

    #[test]
    fn iter_empty_registry_yields_nothing() {
        let reg = MappingRegistry::new();
        assert_eq!(reg.iter().count(), 0);
    }

    #[test]
    fn registry_clone_is_independent() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "tool_use"));
        let cloned = reg.clone();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Gemini, "tool_use"));
        assert_eq!(cloned.len(), 1);
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn registry_debug_is_readable() {
        let reg = MappingRegistry::new();
        let dbg = format!("{reg:?}");
        assert!(dbg.contains("MappingRegistry"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Mapping validation (validate_mapping)
// ═══════════════════════════════════════════════════════════════════════════

mod validation {
    use super::*;

    #[test]
    fn validate_empty_features_returns_empty() {
        let reg = MappingRegistry::new();
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[]);
        assert!(results.is_empty());
    }

    #[test]
    fn validate_single_lossless_feature() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "streaming"));
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
    fn validate_single_lossy_feature_has_fidelity_loss_error() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossy(
            Dialect::Claude,
            Dialect::OpenAi,
            "thinking",
            "mapped to system msg",
        ));
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
    fn validate_unsupported_feature_has_feature_unsupported_error() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_unsupported(
            Dialect::OpenAi,
            Dialect::Codex,
            "image_input",
            "no images",
        ));
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
    fn validate_unknown_feature_generates_error() {
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
    fn validate_empty_feature_name_gives_invalid_input() {
        let reg = MappingRegistry::new();
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);
        assert_eq!(results.len(), 1);
        assert!(matches!(
            &results[0].errors[0],
            MappingError::InvalidInput { .. }
        ));
    }

    #[test]
    fn validate_multiple_features_mixed_results() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "tool_use"));
        reg.insert(make_lossy(
            Dialect::OpenAi,
            Dialect::Claude,
            "thinking",
            "lossy",
        ));
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[
                "tool_use".into(),
                "thinking".into(),
                "missing_feature".into(),
            ],
        );
        assert_eq!(results.len(), 3);
        assert!(results[0].errors.is_empty()); // lossless
        assert_eq!(results[1].errors.len(), 1); // lossy
        assert_eq!(results[2].errors.len(), 1); // unknown
    }

    #[test]
    fn validate_preserves_feature_order() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "z_feat"));
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "a_feat"));
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &["z_feat".into(), "a_feat".into()],
        );
        assert_eq!(results[0].feature, "z_feat");
        assert_eq!(results[1].feature, "a_feat");
    }

    #[test]
    fn validate_duplicate_features_in_input() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "tool_use"));
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &["tool_use".into(), "tool_use".into()],
        );
        assert_eq!(results.len(), 2);
        assert!(results[0].fidelity.is_lossless());
        assert!(results[1].fidelity.is_lossless());
    }

    #[test]
    fn validate_feature_name_is_case_sensitive() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "tool_use"));
        let results =
            validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["Tool_Use".into()]);
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn validation_result_feature_matches_input() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[features::TOOL_USE.into()],
        );
        assert_eq!(results[0].feature, features::TOOL_USE);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Fidelity levels
// ═══════════════════════════════════════════════════════════════════════════

mod fidelity_levels {
    use super::*;

    #[test]
    fn lossless_is_lossless() {
        assert!(Fidelity::Lossless.is_lossless());
    }

    #[test]
    fn lossless_is_not_unsupported() {
        assert!(!Fidelity::Lossless.is_unsupported());
    }

    #[test]
    fn lossy_labeled_is_not_lossless() {
        let f = Fidelity::LossyLabeled {
            warning: "w".into(),
        };
        assert!(!f.is_lossless());
    }

    #[test]
    fn lossy_labeled_is_not_unsupported() {
        let f = Fidelity::LossyLabeled {
            warning: "w".into(),
        };
        assert!(!f.is_unsupported());
    }

    #[test]
    fn unsupported_is_unsupported() {
        let f = Fidelity::Unsupported { reason: "r".into() };
        assert!(f.is_unsupported());
    }

    #[test]
    fn unsupported_is_not_lossless() {
        let f = Fidelity::Unsupported { reason: "r".into() };
        assert!(!f.is_lossless());
    }

    #[test]
    fn fidelity_eq_lossless() {
        assert_eq!(Fidelity::Lossless, Fidelity::Lossless);
    }

    #[test]
    fn fidelity_eq_lossy_same_warning() {
        let a = Fidelity::LossyLabeled {
            warning: "w".into(),
        };
        let b = Fidelity::LossyLabeled {
            warning: "w".into(),
        };
        assert_eq!(a, b);
    }

    #[test]
    fn fidelity_ne_lossy_different_warning() {
        let a = Fidelity::LossyLabeled {
            warning: "a".into(),
        };
        let b = Fidelity::LossyLabeled {
            warning: "b".into(),
        };
        assert_ne!(a, b);
    }

    #[test]
    fn fidelity_ne_lossless_vs_lossy() {
        let a = Fidelity::Lossless;
        let b = Fidelity::LossyLabeled {
            warning: "w".into(),
        };
        assert_ne!(a, b);
    }

    #[test]
    fn fidelity_ne_lossless_vs_unsupported() {
        let a = Fidelity::Lossless;
        let b = Fidelity::Unsupported { reason: "r".into() };
        assert_ne!(a, b);
    }

    #[test]
    fn fidelity_clone_preserves_variant() {
        let orig = Fidelity::LossyLabeled {
            warning: "test".into(),
        };
        let cloned = orig.clone();
        assert_eq!(orig, cloned);
    }

    #[test]
    fn fidelity_debug_lossless() {
        let dbg = format!("{:?}", Fidelity::Lossless);
        assert_eq!(dbg, "Lossless");
    }

    #[test]
    fn fidelity_debug_lossy_contains_warning() {
        let dbg = format!(
            "{:?}",
            Fidelity::LossyLabeled {
                warning: "test".into()
            }
        );
        assert!(dbg.contains("test"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Known mapping rules (known_rules)
// ═══════════════════════════════════════════════════════════════════════════

mod known_rules_tests {
    use super::*;

    #[test]
    fn known_rules_is_non_empty() {
        let reg = known_rules();
        assert!(!reg.is_empty());
    }

    #[test]
    fn known_rules_has_substantial_count() {
        let reg = known_rules();
        // 6 dialects × 5 features self-mappings = 30, plus many cross-dialect
        assert!(reg.len() > 30, "expected >30 rules, got {}", reg.len());
    }

    #[test]
    fn same_dialect_all_features_are_lossless() {
        let reg = known_rules();
        for &d in all_dialects() {
            for &f in &all_features() {
                let rule = reg.lookup(d, d, f);
                assert!(rule.is_some(), "missing self-mapping for {d} feature {f}");
                assert!(
                    rule.unwrap().fidelity.is_lossless(),
                    "{d} -> {d} {f} should be lossless"
                );
            }
        }
    }

    #[test]
    fn openai_to_claude_tool_use_is_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn claude_to_openai_tool_use_is_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Claude, Dialect::OpenAi, features::TOOL_USE)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn openai_to_gemini_tool_use_is_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Gemini, features::TOOL_USE)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn openai_to_codex_tool_use_is_lossy() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Codex, features::TOOL_USE)
            .unwrap();
        assert!(
            !rule.fidelity.is_lossless() && !rule.fidelity.is_unsupported(),
            "openai->codex tool_use should be lossy"
        );
    }

    #[test]
    fn streaming_all_cross_dialect_lossless_among_original_four() {
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
    fn thinking_claude_to_openai_is_lossy() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING)
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
        assert!(!rule.fidelity.is_unsupported());
    }

    #[test]
    fn thinking_all_cross_dialect_is_lossy() {
        let reg = known_rules();
        let dialects = [
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ];
        for &a in &dialects {
            for &b in &dialects {
                if a == b {
                    continue;
                }
                let rule = reg.lookup(a, b, features::THINKING).unwrap();
                assert!(
                    !rule.fidelity.is_lossless(),
                    "thinking {a} -> {b} should not be lossless"
                );
            }
        }
    }

    #[test]
    fn image_input_to_codex_is_unsupported() {
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
    fn image_input_codex_to_others_is_unsupported() {
        let reg = known_rules();
        for &tgt in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
            let rule = reg
                .lookup(Dialect::Codex, tgt, features::IMAGE_INPUT)
                .unwrap();
            assert!(rule.fidelity.is_unsupported());
        }
    }

    #[test]
    fn image_input_openai_claude_is_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, features::IMAGE_INPUT)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn image_input_openai_gemini_is_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Gemini, features::IMAGE_INPUT)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn image_input_claude_gemini_is_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Claude, Dialect::Gemini, features::IMAGE_INPUT)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn kimi_tool_use_to_openai_compatible_is_lossless() {
        let reg = known_rules();
        for &tgt in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
            let rule = reg.lookup(Dialect::Kimi, tgt, features::TOOL_USE).unwrap();
            assert!(
                rule.fidelity.is_lossless(),
                "Kimi -> {tgt} tool_use should be lossless"
            );
        }
    }

    #[test]
    fn copilot_tool_use_to_openai_compatible_is_lossless() {
        let reg = known_rules();
        for &tgt in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
            let rule = reg
                .lookup(Dialect::Copilot, tgt, features::TOOL_USE)
                .unwrap();
            assert!(
                rule.fidelity.is_lossless(),
                "Copilot -> {tgt} tool_use should be lossless"
            );
        }
    }

    #[test]
    fn kimi_to_codex_tool_use_is_lossy() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Kimi, Dialect::Codex, features::TOOL_USE)
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
        assert!(!rule.fidelity.is_unsupported());
    }

    #[test]
    fn kimi_copilot_streaming_is_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Kimi, Dialect::Copilot, features::STREAMING)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn kimi_thinking_cross_dialect_is_lossy() {
        let reg = known_rules();
        for &tgt in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            let rule = reg.lookup(Dialect::Kimi, tgt, features::THINKING).unwrap();
            assert!(
                !rule.fidelity.is_lossless(),
                "Kimi -> {tgt} thinking should be lossy"
            );
        }
    }

    #[test]
    fn kimi_image_input_cross_dialect_is_unsupported() {
        let reg = known_rules();
        for &tgt in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            let rule = reg
                .lookup(Dialect::Kimi, tgt, features::IMAGE_INPUT)
                .unwrap();
            assert!(
                rule.fidelity.is_unsupported(),
                "Kimi -> {tgt} image_input should be unsupported"
            );
        }
    }

    #[test]
    fn kimi_code_exec_to_all_is_unsupported() {
        let reg = known_rules();
        for &tgt in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
            Dialect::Copilot,
        ] {
            let rule = reg.lookup(Dialect::Kimi, tgt, features::CODE_EXEC).unwrap();
            assert!(
                rule.fidelity.is_unsupported(),
                "Kimi -> {tgt} code_exec should be unsupported"
            );
        }
    }

    #[test]
    fn code_exec_among_capable_dialects_is_lossy() {
        let reg = known_rules();
        let capable = [
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
            Dialect::Copilot,
        ];
        for &a in &capable {
            for &b in &capable {
                if a == b {
                    continue;
                }
                let rule = reg.lookup(a, b, features::CODE_EXEC).unwrap();
                assert!(
                    !rule.fidelity.is_lossless() && !rule.fidelity.is_unsupported(),
                    "code_exec {a} -> {b} should be lossy"
                );
            }
        }
    }

    #[test]
    fn kimi_copilot_tool_use_is_lossless() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Kimi, Dialect::Copilot, features::TOOL_USE)
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn copilot_kimi_thinking_is_lossy() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Copilot, Dialect::Kimi, features::THINKING)
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Feature detection (features module)
// ═══════════════════════════════════════════════════════════════════════════

mod feature_constants {
    use super::*;

    #[test]
    fn tool_use_constant_value() {
        assert_eq!(features::TOOL_USE, "tool_use");
    }

    #[test]
    fn streaming_constant_value() {
        assert_eq!(features::STREAMING, "streaming");
    }

    #[test]
    fn thinking_constant_value() {
        assert_eq!(features::THINKING, "thinking");
    }

    #[test]
    fn image_input_constant_value() {
        assert_eq!(features::IMAGE_INPUT, "image_input");
    }

    #[test]
    fn code_exec_constant_value() {
        assert_eq!(features::CODE_EXEC, "code_exec");
    }

    #[test]
    fn all_feature_constants_are_distinct() {
        let feats = all_features();
        let unique: std::collections::HashSet<_> = feats.iter().collect();
        assert_eq!(feats.len(), unique.len());
    }

    #[test]
    fn feature_constants_are_snake_case() {
        for f in all_features() {
            assert!(
                f.chars().all(|c| c.is_lowercase() || c == '_'),
                "{f} should be snake_case"
            );
        }
    }

    #[test]
    fn known_rules_covers_all_feature_constants() {
        let reg = known_rules();
        for &f in &all_features() {
            // Each feature should exist for at least OpenAI self-mapping
            assert!(
                reg.lookup(Dialect::OpenAi, Dialect::OpenAi, f).is_some(),
                "missing known rule for OpenAI->OpenAI {f}"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. MappingError types and handling
// ═══════════════════════════════════════════════════════════════════════════

mod mapping_errors {
    use super::*;

    #[test]
    fn feature_unsupported_display_contains_feature_name() {
        let err = MappingError::FeatureUnsupported {
            feature: "logprobs".into(),
            from: Dialect::Claude,
            to: Dialect::Gemini,
        };
        let msg = err.to_string();
        assert!(msg.contains("logprobs"));
    }

    #[test]
    fn feature_unsupported_display_contains_dialects() {
        let err = MappingError::FeatureUnsupported {
            feature: "test".into(),
            from: Dialect::Claude,
            to: Dialect::Gemini,
        };
        let msg = err.to_string();
        assert!(msg.contains("Claude"));
        assert!(msg.contains("Gemini"));
    }

    #[test]
    fn fidelity_loss_display_contains_warning() {
        let err = MappingError::FidelityLoss {
            feature: "thinking".into(),
            warning: "mapped to system message".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("mapped to system message"));
    }

    #[test]
    fn fidelity_loss_display_contains_feature() {
        let err = MappingError::FidelityLoss {
            feature: "thinking".into(),
            warning: "loss".into(),
        };
        assert!(err.to_string().contains("thinking"));
    }

    #[test]
    fn dialect_mismatch_display_contains_both_dialects() {
        let err = MappingError::DialectMismatch {
            from: Dialect::OpenAi,
            to: Dialect::Codex,
        };
        let msg = err.to_string();
        assert!(msg.contains("OpenAI"));
        assert!(msg.contains("Codex"));
    }

    #[test]
    fn invalid_input_display_contains_reason() {
        let err = MappingError::InvalidInput {
            reason: "empty feature name".into(),
        };
        assert!(err.to_string().contains("empty feature name"));
    }

    #[test]
    fn error_equality_same_variant() {
        let a = MappingError::InvalidInput { reason: "x".into() };
        let b = MappingError::InvalidInput { reason: "x".into() };
        assert_eq!(a, b);
    }

    #[test]
    fn error_inequality_different_variants() {
        let a = MappingError::InvalidInput { reason: "x".into() };
        let b = MappingError::DialectMismatch {
            from: Dialect::OpenAi,
            to: Dialect::Claude,
        };
        assert_ne!(a, b);
    }

    #[test]
    fn error_clone_equals_original() {
        let err = MappingError::FeatureUnsupported {
            feature: "test".into(),
            from: Dialect::OpenAi,
            to: Dialect::Claude,
        };
        assert_eq!(err, err.clone());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Cross-dialect mapping validation
// ═══════════════════════════════════════════════════════════════════════════

mod cross_dialect {
    use super::*;

    #[test]
    fn openai_to_claude_all_known_features() {
        let reg = known_rules();
        let feats: Vec<String> = all_features().iter().map(|s| s.to_string()).collect();
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
        assert_eq!(results.len(), 5);
        // tool_use and streaming should be lossless
        assert!(results[0].fidelity.is_lossless()); // tool_use
        assert!(results[1].fidelity.is_lossless()); // streaming
    }

    #[test]
    fn claude_to_gemini_all_known_features() {
        let reg = known_rules();
        let feats: Vec<String> = all_features().iter().map(|s| s.to_string()).collect();
        let results = validate_mapping(&reg, Dialect::Claude, Dialect::Gemini, &feats);
        assert_eq!(results.len(), 5);
        assert!(results[0].fidelity.is_lossless()); // tool_use
        assert!(results[1].fidelity.is_lossless()); // streaming
    }

    #[test]
    fn openai_to_codex_image_input_unsupported() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Codex,
            &[features::IMAGE_INPUT.into()],
        );
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn gemini_to_codex_image_input_unsupported() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::Gemini,
            Dialect::Codex,
            &[features::IMAGE_INPUT.into()],
        );
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn all_self_mappings_have_no_errors() {
        let reg = known_rules();
        for &d in all_dialects() {
            let feats: Vec<String> = all_features().iter().map(|s| s.to_string()).collect();
            let results = validate_mapping(&reg, d, d, &feats);
            for r in &results {
                assert!(
                    r.errors.is_empty(),
                    "self-mapping {d}->{d} feature {} should have no errors",
                    r.feature
                );
            }
        }
    }

    #[test]
    fn all_cross_dialect_pairs_have_tool_use_rule() {
        let reg = known_rules();
        for &a in all_dialects() {
            for &b in all_dialects() {
                assert!(
                    reg.lookup(a, b, features::TOOL_USE).is_some(),
                    "missing tool_use rule for {a} -> {b}"
                );
            }
        }
    }

    #[test]
    fn all_cross_dialect_pairs_have_streaming_rule() {
        let reg = known_rules();
        for &a in all_dialects() {
            for &b in all_dialects() {
                assert!(
                    reg.lookup(a, b, features::STREAMING).is_some(),
                    "missing streaming rule for {a} -> {b}"
                );
            }
        }
    }

    #[test]
    fn all_cross_dialect_pairs_have_thinking_rule() {
        let reg = known_rules();
        for &a in all_dialects() {
            for &b in all_dialects() {
                assert!(
                    reg.lookup(a, b, features::THINKING).is_some(),
                    "missing thinking rule for {a} -> {b}"
                );
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Mapping chain composition
// ═══════════════════════════════════════════════════════════════════════════

mod mapping_chain {
    use super::*;

    /// If A->B is lossless and B->C is lossless, the chain should be lossless.
    #[test]
    fn lossless_chain_openai_claude_gemini_streaming() {
        let reg = known_rules();
        let ab = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, features::STREAMING)
            .unwrap();
        let bc = reg
            .lookup(Dialect::Claude, Dialect::Gemini, features::STREAMING)
            .unwrap();
        assert!(ab.fidelity.is_lossless());
        assert!(bc.fidelity.is_lossless());
    }

    #[test]
    fn lossy_chain_if_any_hop_lossy() {
        let reg = known_rules();
        let ab = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, features::THINKING)
            .unwrap();
        let bc = reg
            .lookup(Dialect::Claude, Dialect::Gemini, features::THINKING)
            .unwrap();
        // At least one hop should be lossy for thinking
        let chain_lossy = !ab.fidelity.is_lossless() || !bc.fidelity.is_lossless();
        assert!(
            chain_lossy,
            "thinking chain should involve at least one lossy hop"
        );
    }

    #[test]
    fn chain_unsupported_if_any_hop_unsupported() {
        let reg = known_rules();
        // OpenAI->Codex image is unsupported
        let hop = reg
            .lookup(Dialect::OpenAi, Dialect::Codex, features::IMAGE_INPUT)
            .unwrap();
        assert!(hop.fidelity.is_unsupported());
    }

    #[test]
    fn three_hop_chain_streaming_all_lossless() {
        let reg = known_rules();
        let hops = [
            (Dialect::OpenAi, Dialect::Claude),
            (Dialect::Claude, Dialect::Gemini),
            (Dialect::Gemini, Dialect::Codex),
        ];
        for (src, tgt) in &hops {
            let rule = reg.lookup(*src, *tgt, features::STREAMING).unwrap();
            assert!(rule.fidelity.is_lossless());
        }
    }

    #[test]
    fn roundtrip_same_dialect_always_lossless() {
        let reg = known_rules();
        for &d in all_dialects() {
            for &f in &all_features() {
                let fwd = reg.lookup(d, d, f).unwrap();
                assert!(fwd.fidelity.is_lossless());
            }
        }
    }

    #[test]
    fn tool_use_roundtrip_openai_claude() {
        let reg = known_rules();
        let fwd = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
            .unwrap();
        let back = reg
            .lookup(Dialect::Claude, Dialect::OpenAi, features::TOOL_USE)
            .unwrap();
        assert!(fwd.fidelity.is_lossless());
        assert!(back.fidelity.is_lossless());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Mapping metadata and documentation
// ═══════════════════════════════════════════════════════════════════════════

mod mapping_metadata {
    use super::*;

    #[test]
    fn lossy_rule_contains_warning_text() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING)
            .unwrap();
        if let Fidelity::LossyLabeled { warning } = &rule.fidelity {
            assert!(!warning.is_empty(), "warning should not be empty");
        } else {
            panic!("expected LossyLabeled for thinking Claude->OpenAI");
        }
    }

    #[test]
    fn unsupported_rule_contains_reason_text() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Codex, features::IMAGE_INPUT)
            .unwrap();
        if let Fidelity::Unsupported { reason } = &rule.fidelity {
            assert!(!reason.is_empty(), "reason should not be empty");
        } else {
            panic!("expected Unsupported for image_input OpenAI->Codex");
        }
    }

    #[test]
    fn validation_error_preserves_feature_name() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::Claude,
            Dialect::OpenAi,
            &[features::THINKING.into()],
        );
        assert_eq!(results[0].feature, features::THINKING);
    }

    #[test]
    fn validation_error_for_lossy_contains_warning() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::Claude,
            Dialect::OpenAi,
            &[features::THINKING.into()],
        );
        if let MappingError::FidelityLoss { warning, .. } = &results[0].errors[0] {
            assert!(!warning.is_empty());
        } else {
            panic!("expected FidelityLoss error");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Mapping priority and conflict resolution
// ═══════════════════════════════════════════════════════════════════════════

mod priority_conflict {
    use super::*;

    #[test]
    fn later_insert_wins_over_earlier() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "feat"));
        reg.insert(make_lossy(
            Dialect::OpenAi,
            Dialect::Claude,
            "feat",
            "updated",
        ));
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, "feat")
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
    }

    #[test]
    fn overwrite_lossy_with_lossless() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossy(
            Dialect::OpenAi,
            Dialect::Claude,
            "feat",
            "warning",
        ));
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "feat"));
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, "feat")
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn overwrite_unsupported_with_lossless() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_unsupported(
            Dialect::OpenAi,
            Dialect::Claude,
            "feat",
            "reason",
        ));
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "feat"));
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, "feat")
            .unwrap();
        assert!(rule.fidelity.is_lossless());
    }

    #[test]
    fn overwrite_does_not_affect_other_keys() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "a"));
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "b"));
        reg.insert(make_lossy(Dialect::OpenAi, Dialect::Claude, "a", "warn"));
        // b should be unaffected
        let rule_b = reg.lookup(Dialect::OpenAi, Dialect::Claude, "b").unwrap();
        assert!(rule_b.fidelity.is_lossless());
    }

    #[test]
    fn count_remains_stable_on_overwrite() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "feat"));
        assert_eq!(reg.len(), 1);
        reg.insert(make_lossy(Dialect::OpenAi, Dialect::Claude, "feat", "warn"));
        assert_eq!(reg.len(), 1);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Edge cases
// ═══════════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;

    #[test]
    fn validate_with_only_empty_feature_names() {
        let reg = MappingRegistry::new();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &["".into(), "".into()],
        );
        assert_eq!(results.len(), 2);
        for r in &results {
            assert!(matches!(&r.errors[0], MappingError::InvalidInput { .. }));
        }
    }

    #[test]
    fn validate_large_feature_list() {
        let reg = known_rules();
        let feats: Vec<String> = (0..100).map(|i| format!("feature_{i}")).collect();
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
        assert_eq!(results.len(), 100);
        // All should be unsupported (unknown features)
        for r in &results {
            assert!(r.fidelity.is_unsupported());
        }
    }

    #[test]
    fn registry_with_many_features() {
        let mut reg = MappingRegistry::new();
        for i in 0..200 {
            reg.insert(make_lossless(
                Dialect::OpenAi,
                Dialect::Claude,
                &format!("feat_{i}"),
            ));
        }
        assert_eq!(reg.len(), 200);
        assert!(
            reg.lookup(Dialect::OpenAi, Dialect::Claude, "feat_199")
                .is_some()
        );
    }

    #[test]
    fn registry_with_all_dialect_pairs_single_feature() {
        let mut reg = MappingRegistry::new();
        for &src in all_dialects() {
            for &tgt in all_dialects() {
                reg.insert(make_lossless(src, tgt, "universal"));
            }
        }
        let n = all_dialects().len();
        assert_eq!(reg.len(), n * n);
    }

    #[test]
    fn whitespace_feature_name_is_treated_as_unknown() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[" tool_use".into()],
        );
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn feature_name_with_special_chars() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(
            Dialect::OpenAi,
            Dialect::Claude,
            "feat-with-dashes",
        ));
        assert!(
            reg.lookup(Dialect::OpenAi, Dialect::Claude, "feat-with-dashes")
                .is_some()
        );
    }

    #[test]
    fn same_source_and_target_dialect() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::OpenAi,
            &[features::TOOL_USE.into()],
        );
        assert!(results[0].fidelity.is_lossless());
        assert!(results[0].errors.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Mapping serialization
// ═══════════════════════════════════════════════════════════════════════════

mod serialization {
    use super::*;

    #[test]
    fn fidelity_lossless_roundtrip() {
        let f = Fidelity::Lossless;
        let json = serde_json::to_string(&f).unwrap();
        let f2: Fidelity = serde_json::from_str(&json).unwrap();
        assert_eq!(f, f2);
    }

    #[test]
    fn fidelity_lossy_roundtrip() {
        let f = Fidelity::LossyLabeled {
            warning: "test warning".into(),
        };
        let json = serde_json::to_string(&f).unwrap();
        let f2: Fidelity = serde_json::from_str(&json).unwrap();
        assert_eq!(f, f2);
    }

    #[test]
    fn fidelity_unsupported_roundtrip() {
        let f = Fidelity::Unsupported {
            reason: "no support".into(),
        };
        let json = serde_json::to_string(&f).unwrap();
        let f2: Fidelity = serde_json::from_str(&json).unwrap();
        assert_eq!(f, f2);
    }

    #[test]
    fn mapping_rule_roundtrip() {
        let rule = make_lossless(Dialect::OpenAi, Dialect::Claude, "tool_use");
        let json = serde_json::to_string(&rule).unwrap();
        let rule2: MappingRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule, rule2);
    }

    #[test]
    fn mapping_rule_lossy_roundtrip() {
        let rule = make_lossy(Dialect::Claude, Dialect::OpenAi, "thinking", "loss");
        let json = serde_json::to_string(&rule).unwrap();
        let rule2: MappingRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule, rule2);
    }

    #[test]
    fn mapping_rule_unsupported_roundtrip() {
        let rule = make_unsupported(Dialect::OpenAi, Dialect::Codex, "image_input", "no");
        let json = serde_json::to_string(&rule).unwrap();
        let rule2: MappingRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule, rule2);
    }

    #[test]
    fn mapping_error_feature_unsupported_roundtrip() {
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
    fn mapping_error_fidelity_loss_roundtrip() {
        let err = MappingError::FidelityLoss {
            feature: "thinking".into(),
            warning: "loss".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let err2: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, err2);
    }

    #[test]
    fn mapping_error_dialect_mismatch_roundtrip() {
        let err = MappingError::DialectMismatch {
            from: Dialect::Claude,
            to: Dialect::Gemini,
        };
        let json = serde_json::to_string(&err).unwrap();
        let err2: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, err2);
    }

    #[test]
    fn mapping_error_invalid_input_roundtrip() {
        let err = MappingError::InvalidInput {
            reason: "bad".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let err2: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, err2);
    }

    #[test]
    fn mapping_validation_roundtrip() {
        let v = MappingValidation {
            feature: "streaming".into(),
            fidelity: Fidelity::Lossless,
            errors: vec![],
        };
        let json = serde_json::to_string(&v).unwrap();
        let v2: MappingValidation = serde_json::from_str(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn mapping_validation_with_errors_roundtrip() {
        let v = MappingValidation {
            feature: "thinking".into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "lossy".into(),
            },
            errors: vec![MappingError::FidelityLoss {
                feature: "thinking".into(),
                warning: "lossy".into(),
            }],
        };
        let json = serde_json::to_string(&v).unwrap();
        let v2: MappingValidation = serde_json::from_str(&json).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn fidelity_lossless_json_has_type_tag() {
        let json = serde_json::to_string(&Fidelity::Lossless).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["type"], "lossless");
    }

    #[test]
    fn fidelity_lossy_json_has_type_tag() {
        let f = Fidelity::LossyLabeled {
            warning: "w".into(),
        };
        let json = serde_json::to_string(&f).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["type"], "lossy_labeled");
    }

    #[test]
    fn fidelity_unsupported_json_has_type_tag() {
        let f = Fidelity::Unsupported { reason: "r".into() };
        let json = serde_json::to_string(&f).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["type"], "unsupported");
    }

    #[test]
    fn fidelity_deserialize_from_explicit_json() {
        let json = r#"{"type":"lossless"}"#;
        let f: Fidelity = serde_json::from_str(json).unwrap();
        assert!(f.is_lossless());
    }

    #[test]
    fn fidelity_lossy_deserialize_from_explicit_json() {
        let json = r#"{"type":"lossy_labeled","warning":"test"}"#;
        let f: Fidelity = serde_json::from_str(json).unwrap();
        assert!(!f.is_lossless());
        assert!(!f.is_unsupported());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Mapping capabilities query (MappingMatrix)
// ═══════════════════════════════════════════════════════════════════════════

mod mapping_matrix {
    use super::*;

    #[test]
    fn new_matrix_is_empty() {
        let m = MappingMatrix::new();
        assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), None);
    }

    #[test]
    fn default_matrix_is_empty() {
        let m = MappingMatrix::default();
        assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), None);
    }

    #[test]
    fn set_and_get_supported() {
        let mut m = MappingMatrix::new();
        m.set(Dialect::OpenAi, Dialect::Claude, true);
        assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), Some(true));
    }

    #[test]
    fn set_and_get_unsupported() {
        let mut m = MappingMatrix::new();
        m.set(Dialect::OpenAi, Dialect::Claude, false);
        assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), Some(false));
    }

    #[test]
    fn is_supported_returns_false_for_unset() {
        let m = MappingMatrix::new();
        assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
    }

    #[test]
    fn is_supported_returns_true_when_set() {
        let mut m = MappingMatrix::new();
        m.set(Dialect::OpenAi, Dialect::Claude, true);
        assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
    }

    #[test]
    fn set_is_directional() {
        let mut m = MappingMatrix::new();
        m.set(Dialect::OpenAi, Dialect::Claude, true);
        assert!(!m.is_supported(Dialect::Claude, Dialect::OpenAi));
    }

    #[test]
    fn overwrite_true_with_false() {
        let mut m = MappingMatrix::new();
        m.set(Dialect::OpenAi, Dialect::Claude, true);
        m.set(Dialect::OpenAi, Dialect::Claude, false);
        assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
    }

    #[test]
    fn from_registry_marks_lossless_as_supported() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "tool_use"));
        let m = MappingMatrix::from_registry(&reg);
        assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
    }

    #[test]
    fn from_registry_marks_lossy_as_supported() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossy(
            Dialect::OpenAi,
            Dialect::Claude,
            "thinking",
            "w",
        ));
        let m = MappingMatrix::from_registry(&reg);
        assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
    }

    #[test]
    fn from_registry_does_not_mark_unsupported_only_as_supported() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_unsupported(
            Dialect::Gemini,
            Dialect::Codex,
            "image",
            "nope",
        ));
        let m = MappingMatrix::from_registry(&reg);
        assert!(!m.is_supported(Dialect::Gemini, Dialect::Codex));
    }

    #[test]
    fn from_known_rules_has_openai_claude_support() {
        let reg = known_rules();
        let m = MappingMatrix::from_registry(&reg);
        assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
    }

    #[test]
    fn from_known_rules_has_claude_gemini_support() {
        let reg = known_rules();
        let m = MappingMatrix::from_registry(&reg);
        assert!(m.is_supported(Dialect::Claude, Dialect::Gemini));
    }

    #[test]
    fn from_known_rules_all_self_mappings_supported() {
        let reg = known_rules();
        let m = MappingMatrix::from_registry(&reg);
        for &d in all_dialects() {
            assert!(
                m.is_supported(d, d),
                "self-mapping {d}->{d} should be supported"
            );
        }
    }

    #[test]
    fn matrix_clone_is_independent() {
        let mut m = MappingMatrix::new();
        m.set(Dialect::OpenAi, Dialect::Claude, true);
        let cloned = m.clone();
        m.set(Dialect::OpenAi, Dialect::Claude, false);
        assert!(cloned.is_supported(Dialect::OpenAi, Dialect::Claude));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Mapping coverage analysis
// ═══════════════════════════════════════════════════════════════════════════

mod coverage_analysis {
    use super::*;

    #[test]
    fn known_rules_covers_all_self_mappings() {
        let reg = known_rules();
        for &d in all_dialects() {
            for &f in &all_features() {
                assert!(
                    reg.lookup(d, d, f).is_some(),
                    "self-mapping {d}->{d} feature {f} missing"
                );
            }
        }
    }

    #[test]
    fn known_rules_tool_use_coverage_complete() {
        let reg = known_rules();
        for &a in all_dialects() {
            for &b in all_dialects() {
                assert!(
                    reg.lookup(a, b, features::TOOL_USE).is_some(),
                    "tool_use {a}->{b} missing"
                );
            }
        }
    }

    #[test]
    fn known_rules_streaming_coverage_complete() {
        let reg = known_rules();
        for &a in all_dialects() {
            for &b in all_dialects() {
                assert!(
                    reg.lookup(a, b, features::STREAMING).is_some(),
                    "streaming {a}->{b} missing"
                );
            }
        }
    }

    #[test]
    fn known_rules_thinking_coverage_complete() {
        let reg = known_rules();
        for &a in all_dialects() {
            for &b in all_dialects() {
                assert!(
                    reg.lookup(a, b, features::THINKING).is_some(),
                    "thinking {a}->{b} missing"
                );
            }
        }
    }

    #[test]
    fn count_lossless_rules_in_known_registry() {
        let reg = known_rules();
        let lossless_count = reg.iter().filter(|r| r.fidelity.is_lossless()).count();
        assert!(
            lossless_count > 0,
            "should have at least some lossless rules"
        );
    }

    #[test]
    fn count_lossy_rules_in_known_registry() {
        let reg = known_rules();
        let lossy_count = reg
            .iter()
            .filter(|r| !r.fidelity.is_lossless() && !r.fidelity.is_unsupported())
            .count();
        assert!(lossy_count > 0, "should have at least some lossy rules");
    }

    #[test]
    fn count_unsupported_rules_in_known_registry() {
        let reg = known_rules();
        let unsupported_count = reg.iter().filter(|r| r.fidelity.is_unsupported()).count();
        assert!(
            unsupported_count > 0,
            "should have at least some unsupported rules"
        );
    }

    #[test]
    fn matrix_from_known_rules_non_trivial() {
        let reg = known_rules();
        let m = MappingMatrix::from_registry(&reg);
        let mut supported_count = 0;
        for &a in all_dialects() {
            for &b in all_dialects() {
                if m.is_supported(a, b) {
                    supported_count += 1;
                }
            }
        }
        // All 36 pairs should be supported (each has at least one non-unsupported rule)
        assert!(supported_count >= 30, "expected many supported pairs");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. rank_targets
// ═══════════════════════════════════════════════════════════════════════════

mod rank_targets {
    use super::*;

    #[test]
    fn rank_targets_empty_registry_returns_empty() {
        let reg = MappingRegistry::new();
        let ranked = reg.rank_targets(Dialect::OpenAi, &["tool_use"]);
        assert!(ranked.is_empty());
    }

    #[test]
    fn rank_targets_excludes_source_dialect() {
        let reg = known_rules();
        let ranked = reg.rank_targets(Dialect::OpenAi, &[features::STREAMING]);
        for (d, _) in &ranked {
            assert_ne!(*d, Dialect::OpenAi);
        }
    }

    #[test]
    fn rank_targets_returns_sorted_by_lossless_descending() {
        let reg = known_rules();
        let ranked = reg.rank_targets(
            Dialect::OpenAi,
            &[features::TOOL_USE, features::STREAMING, features::THINKING],
        );
        // Should be sorted descending by lossless count
        for w in ranked.windows(2) {
            assert!(w[0].1 >= w[1].1);
        }
    }

    #[test]
    fn rank_targets_includes_dialects_with_any_supported_feature() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossy(Dialect::OpenAi, Dialect::Claude, "feat", "warn"));
        let ranked = reg.rank_targets(Dialect::OpenAi, &["feat"]);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].0, Dialect::Claude);
        assert_eq!(ranked[0].1, 0); // lossy, not lossless
    }

    #[test]
    fn rank_targets_excludes_dialects_with_only_unsupported() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_unsupported(
            Dialect::OpenAi,
            Dialect::Codex,
            "feat",
            "no",
        ));
        let ranked = reg.rank_targets(Dialect::OpenAi, &["feat"]);
        assert!(ranked.is_empty());
    }

    #[test]
    fn rank_targets_lossless_count_is_correct() {
        let mut reg = MappingRegistry::new();
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "a"));
        reg.insert(make_lossless(Dialect::OpenAi, Dialect::Claude, "b"));
        reg.insert(make_lossy(Dialect::OpenAi, Dialect::Claude, "c", "w"));
        let ranked = reg.rank_targets(Dialect::OpenAi, &["a", "b", "c"]);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].0, Dialect::Claude);
        assert_eq!(ranked[0].1, 2); // 2 lossless out of 3
    }

    #[test]
    fn rank_targets_with_no_features_returns_empty() {
        let reg = known_rules();
        let ranked = reg.rank_targets(Dialect::OpenAi, &[]);
        assert!(ranked.is_empty());
    }

    #[test]
    fn rank_targets_streaming_all_dialects_appear() {
        let reg = known_rules();
        let ranked = reg.rank_targets(Dialect::OpenAi, &[features::STREAMING]);
        // streaming is lossless everywhere, so all other dialects should appear
        assert!(ranked.len() >= 5);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. MappingValidation struct
// ═══════════════════════════════════════════════════════════════════════════

mod validation_struct {
    use super::*;

    #[test]
    fn validation_struct_fields_accessible() {
        let v = MappingValidation {
            feature: "test".into(),
            fidelity: Fidelity::Lossless,
            errors: vec![],
        };
        assert_eq!(v.feature, "test");
        assert!(v.fidelity.is_lossless());
        assert!(v.errors.is_empty());
    }

    #[test]
    fn validation_equality() {
        let a = MappingValidation {
            feature: "x".into(),
            fidelity: Fidelity::Lossless,
            errors: vec![],
        };
        let b = MappingValidation {
            feature: "x".into(),
            fidelity: Fidelity::Lossless,
            errors: vec![],
        };
        assert_eq!(a, b);
    }

    #[test]
    fn validation_inequality_different_feature() {
        let a = MappingValidation {
            feature: "x".into(),
            fidelity: Fidelity::Lossless,
            errors: vec![],
        };
        let b = MappingValidation {
            feature: "y".into(),
            fidelity: Fidelity::Lossless,
            errors: vec![],
        };
        assert_ne!(a, b);
    }

    #[test]
    fn validation_clone_equals_original() {
        let v = MappingValidation {
            feature: "streaming".into(),
            fidelity: Fidelity::Lossless,
            errors: vec![],
        };
        assert_eq!(v, v.clone());
    }

    #[test]
    fn validation_debug_is_readable() {
        let v = MappingValidation {
            feature: "streaming".into(),
            fidelity: Fidelity::Lossless,
            errors: vec![],
        };
        let dbg = format!("{v:?}");
        assert!(dbg.contains("streaming"));
    }

    #[test]
    fn validation_with_multiple_errors() {
        let v = MappingValidation {
            feature: "test".into(),
            fidelity: Fidelity::Unsupported {
                reason: "nope".into(),
            },
            errors: vec![
                MappingError::FeatureUnsupported {
                    feature: "test".into(),
                    from: Dialect::OpenAi,
                    to: Dialect::Codex,
                },
                MappingError::InvalidInput {
                    reason: "also bad".into(),
                },
            ],
        };
        assert_eq!(v.errors.len(), 2);
    }
}
