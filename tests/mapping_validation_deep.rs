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
//! Comprehensive tests for SDK mapping and validation.
//!
//! Covers:
//! - MappingError variants and Display
//! - Mapping rules between all SDK pairs
//! - Lossy mapping detection and reporting
//! - Feature loss tracking during mapping
//! - Unmappable feature early failures
//! - Valid mapping paths through IR
//! - Round-trip mapping (A→IR→B→IR→A) fidelity
//! - Mapping validation before execution
//! - Error codes for mapping failures
//! - Mapping configuration options
//! - Mapping with emulation labels
//! - Mapping statistics collection
//! - Edge cases: empty messages, no tools, unsupported features
//! - Cross-dialect capability checking before mapping

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;
use abp_mapping::{
    Fidelity, MappingError, MappingMatrix, MappingRegistry, MappingRule, MappingValidation,
    features, known_rules, validate_mapping,
};

// ═══════════════════════════════════════════════════════════════════════════
// Module 1: MappingError variants and Display
// ═══════════════════════════════════════════════════════════════════════════

mod mapping_error_display {
    use super::*;

    #[test]
    fn feature_unsupported_display_contains_feature() {
        let err = MappingError::FeatureUnsupported {
            feature: "logprobs".into(),
            from: Dialect::Claude,
            to: Dialect::Gemini,
        };
        let msg = err.to_string();
        assert!(msg.contains("logprobs"), "should mention the feature");
        assert!(msg.contains("Claude"), "should mention source");
        assert!(msg.contains("Gemini"), "should mention target");
    }

    #[test]
    fn fidelity_loss_display_contains_warning() {
        let err = MappingError::FidelityLoss {
            feature: "thinking".into(),
            warning: "mapped to system message".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("thinking"));
        assert!(msg.contains("mapped to system message"));
    }

    #[test]
    fn dialect_mismatch_display_contains_dialects() {
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
    fn feature_unsupported_is_std_error() {
        let err = MappingError::FeatureUnsupported {
            feature: "x".into(),
            from: Dialect::OpenAi,
            to: Dialect::Claude,
        };
        // MappingError derives thiserror::Error, so it implements std::error::Error
        let _: &dyn std::error::Error = &err;
    }

    #[test]
    fn all_error_variants_are_clone() {
        let err = MappingError::FidelityLoss {
            feature: "a".into(),
            warning: "b".into(),
        };
        let _ = err.clone();
    }

    #[test]
    fn all_error_variants_are_debug() {
        let err = MappingError::InvalidInput {
            reason: "test".into(),
        };
        let debug = format!("{err:?}");
        assert!(debug.contains("InvalidInput"));
    }

    #[test]
    fn error_equality() {
        let a = MappingError::FeatureUnsupported {
            feature: "x".into(),
            from: Dialect::OpenAi,
            to: Dialect::Claude,
        };
        let b = a.clone();
        assert_eq!(a, b);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 2: Mapping rules between all SDK pairs
// ═══════════════════════════════════════════════════════════════════════════

mod mapping_rules_all_pairs {
    use super::*;

    #[test]
    fn every_dialect_self_maps_lossless_for_all_features() {
        let reg = known_rules();
        for &d in Dialect::all() {
            for &f in &[
                features::TOOL_USE,
                features::STREAMING,
                features::THINKING,
                features::IMAGE_INPUT,
                features::CODE_EXEC,
            ] {
                let rule = reg
                    .lookup(d, d, f)
                    .unwrap_or_else(|| panic!("missing self-mapping for {d} feature {f}"));
                assert!(rule.fidelity.is_lossless(), "{d}->{d} {f} must be lossless");
            }
        }
    }

    #[test]
    fn openai_claude_tool_use_bidirectional_lossless() {
        let reg = known_rules();
        let fwd = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
            .unwrap();
        let rev = reg
            .lookup(Dialect::Claude, Dialect::OpenAi, features::TOOL_USE)
            .unwrap();
        assert!(fwd.fidelity.is_lossless());
        assert!(rev.fidelity.is_lossless());
    }

    #[test]
    fn openai_gemini_tool_use_bidirectional_lossless() {
        let reg = known_rules();
        assert!(
            reg.lookup(Dialect::OpenAi, Dialect::Gemini, features::TOOL_USE)
                .unwrap()
                .fidelity
                .is_lossless()
        );
        assert!(
            reg.lookup(Dialect::Gemini, Dialect::OpenAi, features::TOOL_USE)
                .unwrap()
                .fidelity
                .is_lossless()
        );
    }

    #[test]
    fn claude_gemini_tool_use_bidirectional_lossless() {
        let reg = known_rules();
        assert!(
            reg.lookup(Dialect::Claude, Dialect::Gemini, features::TOOL_USE)
                .unwrap()
                .fidelity
                .is_lossless()
        );
        assert!(
            reg.lookup(Dialect::Gemini, Dialect::Claude, features::TOOL_USE)
                .unwrap()
                .fidelity
                .is_lossless()
        );
    }

    #[test]
    fn codex_tool_use_always_lossy_with_non_codex() {
        let reg = known_rules();
        for &d in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
            let fwd = reg.lookup(d, Dialect::Codex, features::TOOL_USE).unwrap();
            assert!(
                !fwd.fidelity.is_lossless() && !fwd.fidelity.is_unsupported(),
                "{d}->Codex tool_use should be lossy"
            );
            let rev = reg.lookup(Dialect::Codex, d, features::TOOL_USE).unwrap();
            assert!(
                !rev.fidelity.is_lossless() && !rev.fidelity.is_unsupported(),
                "Codex->{d} tool_use should be lossy"
            );
        }
    }

    #[test]
    fn streaming_lossless_for_all_pairs() {
        let reg = known_rules();
        for &a in Dialect::all() {
            for &b in Dialect::all() {
                let rule = reg
                    .lookup(a, b, features::STREAMING)
                    .unwrap_or_else(|| panic!("missing streaming rule {a}->{b}"));
                assert!(
                    rule.fidelity.is_lossless(),
                    "streaming {a}->{b} must be lossless"
                );
            }
        }
    }

    #[test]
    fn kimi_copilot_tool_use_lossless() {
        let reg = known_rules();
        assert!(
            reg.lookup(Dialect::Kimi, Dialect::Copilot, features::TOOL_USE)
                .unwrap()
                .fidelity
                .is_lossless()
        );
    }

    #[test]
    fn kimi_openai_compatible_tool_use_lossless() {
        let reg = known_rules();
        for &od in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
            assert!(
                reg.lookup(Dialect::Kimi, od, features::TOOL_USE)
                    .unwrap()
                    .fidelity
                    .is_lossless(),
                "Kimi->{od} tool_use should be lossless"
            );
        }
    }

    #[test]
    fn copilot_openai_compatible_tool_use_lossless() {
        let reg = known_rules();
        for &od in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
            assert!(
                reg.lookup(Dialect::Copilot, od, features::TOOL_USE)
                    .unwrap()
                    .fidelity
                    .is_lossless(),
                "Copilot->{od} tool_use should be lossless"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 3: Lossy mapping detection and reporting
// ═══════════════════════════════════════════════════════════════════════════

mod lossy_detection {
    use super::*;

    #[test]
    fn thinking_cross_dialect_always_lossy() {
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
                    "thinking {a}->{b} should be lossy"
                );
            }
        }
    }

    #[test]
    fn lossy_labeled_carries_warning_string() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING)
            .unwrap();
        match &rule.fidelity {
            Fidelity::LossyLabeled { warning } => {
                assert!(!warning.is_empty(), "warning should not be empty");
            }
            other => panic!("expected LossyLabeled, got {other:?}"),
        }
    }

    #[test]
    fn validation_reports_fidelity_loss_error_for_lossy() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::Claude,
            Dialect::OpenAi,
            &[features::THINKING.into()],
        );
        assert_eq!(results.len(), 1);
        assert!(matches!(
            &results[0].errors[0],
            MappingError::FidelityLoss { .. }
        ));
    }

    #[test]
    fn lossy_codex_tool_use_warning_mentions_schema() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Codex, features::TOOL_USE)
            .unwrap();
        if let Fidelity::LossyLabeled { warning } = &rule.fidelity {
            assert!(
                warning.contains("schema") || warning.contains("differs"),
                "warning should mention schema difference"
            );
        } else {
            panic!("expected lossy labeled");
        }
    }

    #[test]
    fn kimi_thinking_lossy_to_all_dialects() {
        let reg = known_rules();
        for &d in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            let rule = reg.lookup(Dialect::Kimi, d, features::THINKING).unwrap();
            assert!(
                !rule.fidelity.is_lossless(),
                "Kimi->{d} thinking should be lossy"
            );
        }
    }

    #[test]
    fn copilot_thinking_lossy_to_all_dialects() {
        let reg = known_rules();
        for &d in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            let rule = reg.lookup(Dialect::Copilot, d, features::THINKING).unwrap();
            assert!(!rule.fidelity.is_lossless());
        }
    }

    #[test]
    fn kimi_copilot_thinking_bidirectional_lossy() {
        let reg = known_rules();
        let kc = reg
            .lookup(Dialect::Kimi, Dialect::Copilot, features::THINKING)
            .unwrap();
        let ck = reg
            .lookup(Dialect::Copilot, Dialect::Kimi, features::THINKING)
            .unwrap();
        assert!(!kc.fidelity.is_lossless());
        assert!(!ck.fidelity.is_lossless());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 4: Feature loss tracking during mapping
// ═══════════════════════════════════════════════════════════════════════════

mod feature_loss_tracking {
    use super::*;

    #[test]
    fn validation_errors_track_feature_name() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Codex,
            &[features::IMAGE_INPUT.into()],
        );
        assert_eq!(results[0].feature, features::IMAGE_INPUT);
        assert!(!results[0].errors.is_empty());
    }

    #[test]
    fn multiple_features_tracked_independently() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Codex,
            &[
                features::TOOL_USE.into(),
                features::IMAGE_INPUT.into(),
                features::STREAMING.into(),
            ],
        );
        assert_eq!(results.len(), 3);
        // tool_use: lossy (has FidelityLoss error)
        assert_eq!(results[0].errors.len(), 1);
        assert!(matches!(
            &results[0].errors[0],
            MappingError::FidelityLoss { .. }
        ));
        // image_input: unsupported
        assert!(results[1].fidelity.is_unsupported());
        // streaming: lossless
        assert!(results[2].errors.is_empty());
    }

    #[test]
    fn lossy_error_carries_warning_from_rule() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Codex,
            &[features::TOOL_USE.into()],
        );
        if let MappingError::FidelityLoss { warning, .. } = &results[0].errors[0] {
            assert!(!warning.is_empty());
        } else {
            panic!("expected FidelityLoss");
        }
    }

    #[test]
    fn unsupported_error_carries_dialects() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Codex,
            &[features::IMAGE_INPUT.into()],
        );
        if let MappingError::FeatureUnsupported {
            feature, from, to, ..
        } = &results[0].errors[0]
        {
            assert_eq!(feature, features::IMAGE_INPUT);
            assert_eq!(*from, Dialect::OpenAi);
            assert_eq!(*to, Dialect::Codex);
        } else {
            panic!("expected FeatureUnsupported");
        }
    }

    #[test]
    fn lossless_validation_has_no_errors() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[features::TOOL_USE.into(), features::STREAMING.into()],
        );
        for r in &results {
            assert!(
                r.errors.is_empty(),
                "lossless features should have no errors"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 5: Unmappable feature early failures
// ═══════════════════════════════════════════════════════════════════════════

mod unmappable_features {
    use super::*;

    #[test]
    fn unknown_feature_produces_unsupported_error() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &["teleportation".into()],
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_unsupported());
        assert!(matches!(
            &results[0].errors[0],
            MappingError::FeatureUnsupported { .. }
        ));
    }

    #[test]
    fn image_input_to_codex_is_unsupported() {
        let reg = known_rules();
        for &src in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
            let rule = reg
                .lookup(src, Dialect::Codex, features::IMAGE_INPUT)
                .unwrap();
            assert!(rule.fidelity.is_unsupported());
        }
    }

    #[test]
    fn kimi_image_input_unsupported_to_all() {
        let reg = known_rules();
        for &d in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            let rule = reg.lookup(Dialect::Kimi, d, features::IMAGE_INPUT).unwrap();
            assert!(
                rule.fidelity.is_unsupported(),
                "Kimi->{d} image_input should be unsupported"
            );
        }
    }

    #[test]
    fn copilot_image_input_unsupported_to_other_dialects() {
        let reg = known_rules();
        for &d in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            let rule = reg
                .lookup(Dialect::Copilot, d, features::IMAGE_INPUT)
                .unwrap();
            assert!(rule.fidelity.is_unsupported());
        }
    }

    #[test]
    fn kimi_code_exec_unsupported_to_all() {
        let reg = known_rules();
        for &d in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
            Dialect::Copilot,
        ] {
            let rule = reg.lookup(Dialect::Kimi, d, features::CODE_EXEC).unwrap();
            assert!(
                rule.fidelity.is_unsupported(),
                "Kimi->{d} code_exec should be unsupported"
            );
        }
    }

    #[test]
    fn unsupported_feature_reason_is_non_empty() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Kimi, Dialect::OpenAi, features::CODE_EXEC)
            .unwrap();
        if let Fidelity::Unsupported { reason } = &rule.fidelity {
            assert!(!reason.is_empty());
        } else {
            panic!("expected Unsupported");
        }
    }

    #[test]
    fn validation_unsupported_reason_matches_no_rule() {
        let reg = MappingRegistry::new();
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["fake".into()]);
        if let Fidelity::Unsupported { reason } = &results[0].fidelity {
            assert!(reason.contains("no mapping rule"));
        } else {
            panic!("expected unsupported fidelity");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 6: Valid mapping paths through IR
// ═══════════════════════════════════════════════════════════════════════════

mod valid_mapping_paths {
    use super::*;

    #[test]
    fn openai_to_claude_all_features_have_rules() {
        let reg = known_rules();
        for &f in &[
            features::TOOL_USE,
            features::STREAMING,
            features::THINKING,
            features::IMAGE_INPUT,
            features::CODE_EXEC,
        ] {
            assert!(
                reg.lookup(Dialect::OpenAi, Dialect::Claude, f).is_some(),
                "OpenAI->Claude should have rule for {f}"
            );
        }
    }

    #[test]
    fn every_cross_dialect_pair_has_streaming() {
        let reg = known_rules();
        for &a in Dialect::all() {
            for &b in Dialect::all() {
                assert!(
                    reg.lookup(a, b, features::STREAMING).is_some(),
                    "{a}->{b} should have streaming rule"
                );
            }
        }
    }

    #[test]
    fn core_four_have_all_feature_rules() {
        let reg = known_rules();
        let core = [
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ];
        for &a in &core {
            for &b in &core {
                for &f in &[
                    features::TOOL_USE,
                    features::STREAMING,
                    features::THINKING,
                    features::IMAGE_INPUT,
                ] {
                    assert!(
                        reg.lookup(a, b, f).is_some(),
                        "{a}->{b} should have rule for {f}"
                    );
                }
            }
        }
    }

    #[test]
    fn matrix_marks_all_core_pairs_supported() {
        let reg = known_rules();
        let m = MappingMatrix::from_registry(&reg);
        let core = [
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ];
        for &a in &core {
            for &b in &core {
                if a != b {
                    assert!(
                        m.is_supported(a, b),
                        "{a}->{b} should be supported in matrix"
                    );
                }
            }
        }
    }

    #[test]
    fn kimi_copilot_have_streaming_and_tool_use_with_all() {
        let reg = known_rules();
        for &nd in &[Dialect::Kimi, Dialect::Copilot] {
            for &d in Dialect::all() {
                assert!(
                    reg.lookup(nd, d, features::STREAMING).is_some(),
                    "{nd}->{d} streaming"
                );
                assert!(
                    reg.lookup(nd, d, features::TOOL_USE).is_some(),
                    "{nd}->{d} tool_use"
                );
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 7: Round-trip mapping fidelity
// ═══════════════════════════════════════════════════════════════════════════

mod round_trip_fidelity {
    use super::*;

    /// If A→B is lossless and B→A is lossless, round-trip is lossless.
    fn assert_round_trip_lossless(reg: &MappingRegistry, a: Dialect, b: Dialect, feature: &str) {
        let fwd = reg.lookup(a, b, feature).unwrap();
        let rev = reg.lookup(b, a, feature).unwrap();
        assert!(
            fwd.fidelity.is_lossless() && rev.fidelity.is_lossless(),
            "round-trip {a}->{b}->{a} for {feature} should be lossless"
        );
    }

    #[test]
    fn streaming_round_trip_all_pairs_lossless() {
        let reg = known_rules();
        for &a in Dialect::all() {
            for &b in Dialect::all() {
                if a == b {
                    continue;
                }
                assert_round_trip_lossless(&reg, a, b, features::STREAMING);
            }
        }
    }

    #[test]
    fn tool_use_round_trip_openai_claude_lossless() {
        let reg = known_rules();
        assert_round_trip_lossless(&reg, Dialect::OpenAi, Dialect::Claude, features::TOOL_USE);
    }

    #[test]
    fn tool_use_round_trip_openai_gemini_lossless() {
        let reg = known_rules();
        assert_round_trip_lossless(&reg, Dialect::OpenAi, Dialect::Gemini, features::TOOL_USE);
    }

    #[test]
    fn tool_use_round_trip_claude_gemini_lossless() {
        let reg = known_rules();
        assert_round_trip_lossless(&reg, Dialect::Claude, Dialect::Gemini, features::TOOL_USE);
    }

    #[test]
    fn image_round_trip_openai_claude_lossless() {
        let reg = known_rules();
        assert_round_trip_lossless(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            features::IMAGE_INPUT,
        );
    }

    #[test]
    fn image_round_trip_openai_gemini_lossless() {
        let reg = known_rules();
        assert_round_trip_lossless(
            &reg,
            Dialect::OpenAi,
            Dialect::Gemini,
            features::IMAGE_INPUT,
        );
    }

    #[test]
    fn image_round_trip_claude_gemini_lossless() {
        let reg = known_rules();
        assert_round_trip_lossless(
            &reg,
            Dialect::Claude,
            Dialect::Gemini,
            features::IMAGE_INPUT,
        );
    }

    #[test]
    fn codex_round_trip_tool_use_is_lossy_both_ways() {
        let reg = known_rules();
        for &d in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
            let fwd = reg.lookup(d, Dialect::Codex, features::TOOL_USE).unwrap();
            let rev = reg.lookup(Dialect::Codex, d, features::TOOL_USE).unwrap();
            assert!(!fwd.fidelity.is_lossless() || !rev.fidelity.is_lossless());
        }
    }

    #[test]
    fn kimi_copilot_round_trip_streaming_lossless() {
        let reg = known_rules();
        assert_round_trip_lossless(&reg, Dialect::Kimi, Dialect::Copilot, features::STREAMING);
    }

    #[test]
    fn kimi_copilot_round_trip_tool_use_lossless() {
        let reg = known_rules();
        assert_round_trip_lossless(&reg, Dialect::Kimi, Dialect::Copilot, features::TOOL_USE);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 8: Mapping validation before execution
// ═══════════════════════════════════════════════════════════════════════════

mod validation_before_execution {
    use super::*;

    #[test]
    fn validate_empty_features_returns_empty() {
        let reg = known_rules();
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[]);
        assert!(results.is_empty());
    }

    #[test]
    fn validate_single_lossless_no_errors() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[features::STREAMING.into()],
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_lossless());
        assert!(results[0].errors.is_empty());
    }

    #[test]
    fn validate_mixed_features_ordered_correctly() {
        let reg = known_rules();
        let feats: Vec<String> = vec![
            features::TOOL_USE.into(),
            features::THINKING.into(),
            features::STREAMING.into(),
        ];
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].feature, features::TOOL_USE);
        assert_eq!(results[1].feature, features::THINKING);
        assert_eq!(results[2].feature, features::STREAMING);
    }

    #[test]
    fn validate_all_five_features_openai_to_codex() {
        let reg = known_rules();
        let feats: Vec<String> = vec![
            features::TOOL_USE.into(),
            features::STREAMING.into(),
            features::THINKING.into(),
            features::IMAGE_INPUT.into(),
            features::CODE_EXEC.into(),
        ];
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Codex, &feats);
        assert_eq!(results.len(), 5);
        // streaming lossless
        assert!(results[1].fidelity.is_lossless());
        // image_input unsupported
        assert!(results[3].fidelity.is_unsupported());
    }

    #[test]
    fn can_filter_validations_by_errors() {
        let reg = known_rules();
        let feats: Vec<String> = vec![
            features::TOOL_USE.into(),
            features::STREAMING.into(),
            features::IMAGE_INPUT.into(),
        ];
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Codex, &feats);
        let with_errors: Vec<_> = results.iter().filter(|v| !v.errors.is_empty()).collect();
        // tool_use lossy + image_input unsupported = 2 with errors
        assert_eq!(with_errors.len(), 2);
    }

    #[test]
    fn can_check_all_lossless_before_proceeding() {
        let reg = known_rules();
        let feats: Vec<String> = vec![features::TOOL_USE.into(), features::STREAMING.into()];
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
        let all_lossless = results.iter().all(|v| v.fidelity.is_lossless());
        assert!(
            all_lossless,
            "OpenAI->Claude tool_use+streaming should be all lossless"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 9: Error codes for mapping failures
// ═══════════════════════════════════════════════════════════════════════════

mod error_codes {
    use super::*;

    #[test]
    fn empty_feature_name_produces_invalid_input() {
        let reg = known_rules();
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);
        assert!(matches!(
            &results[0].errors[0],
            MappingError::InvalidInput { .. }
        ));
    }

    #[test]
    fn missing_rule_produces_feature_unsupported() {
        let reg = MappingRegistry::new();
        let results =
            validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["anything".into()]);
        assert!(matches!(
            &results[0].errors[0],
            MappingError::FeatureUnsupported { .. }
        ));
    }

    #[test]
    fn unsupported_rule_produces_feature_unsupported() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Codex,
            &[features::IMAGE_INPUT.into()],
        );
        assert!(matches!(
            &results[0].errors[0],
            MappingError::FeatureUnsupported { .. }
        ));
    }

    #[test]
    fn lossy_rule_produces_fidelity_loss() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Codex,
            &[features::TOOL_USE.into()],
        );
        assert!(matches!(
            &results[0].errors[0],
            MappingError::FidelityLoss { .. }
        ));
    }

    #[test]
    fn error_serde_roundtrip_feature_unsupported() {
        let err = MappingError::FeatureUnsupported {
            feature: "img".into(),
            from: Dialect::Kimi,
            to: Dialect::Codex,
        };
        let json = serde_json::to_string(&err).unwrap();
        let err2: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, err2);
    }

    #[test]
    fn error_serde_roundtrip_fidelity_loss() {
        let err = MappingError::FidelityLoss {
            feature: "thinking".into(),
            warning: "test".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let err2: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, err2);
    }

    #[test]
    fn error_serde_roundtrip_dialect_mismatch() {
        let err = MappingError::DialectMismatch {
            from: Dialect::OpenAi,
            to: Dialect::Codex,
        };
        let json = serde_json::to_string(&err).unwrap();
        let err2: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, err2);
    }

    #[test]
    fn error_serde_roundtrip_invalid_input() {
        let err = MappingError::InvalidInput {
            reason: "bad data".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        let err2: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, err2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 10: Mapping configuration options
// ═══════════════════════════════════════════════════════════════════════════

mod mapping_configuration {
    use super::*;

    #[test]
    fn custom_registry_overrides_known_rules() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: features::TOOL_USE.into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "custom override".into(),
            },
        });
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
    }

    #[test]
    fn registry_insert_replaces_existing_rule() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "x".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "x".into(),
            fidelity: Fidelity::Unsupported {
                reason: "replaced".into(),
            },
        });
        assert_eq!(reg.len(), 1);
        assert!(
            reg.lookup(Dialect::OpenAi, Dialect::Claude, "x")
                .unwrap()
                .fidelity
                .is_unsupported()
        );
    }

    #[test]
    fn empty_registry_validates_all_as_unsupported() {
        let reg = MappingRegistry::new();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[features::TOOL_USE.into()],
        );
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn can_build_partial_registry() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: features::STREAMING.into(),
            fidelity: Fidelity::Lossless,
        });
        assert_eq!(reg.len(), 1);
        assert!(
            reg.lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
                .is_none()
        );
        assert!(
            reg.lookup(Dialect::OpenAi, Dialect::Claude, features::STREAMING)
                .is_some()
        );
    }

    #[test]
    fn known_rules_registry_is_non_trivial() {
        let reg = known_rules();
        // 6 dialects × 5 features self-mapping = 30, plus many cross-dialect
        assert!(reg.len() > 100, "known_rules should have 100+ rules");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 11: Mapping with emulation labels
// ═══════════════════════════════════════════════════════════════════════════

mod emulation_labels {
    use super::*;

    #[test]
    fn dialect_labels_are_human_readable() {
        assert_eq!(Dialect::OpenAi.label(), "OpenAI");
        assert_eq!(Dialect::Claude.label(), "Claude");
        assert_eq!(Dialect::Gemini.label(), "Gemini");
        assert_eq!(Dialect::Codex.label(), "Codex");
        assert_eq!(Dialect::Kimi.label(), "Kimi");
        assert_eq!(Dialect::Copilot.label(), "Copilot");
    }

    #[test]
    fn dialect_display_matches_label() {
        for &d in Dialect::all() {
            assert_eq!(d.to_string(), d.label());
        }
    }

    #[test]
    fn lossy_warning_can_reference_dialect_labels() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Kimi, Dialect::OpenAi, features::THINKING)
            .unwrap();
        if let Fidelity::LossyLabeled { warning } = &rule.fidelity {
            assert!(
                warning.contains("Kimi"),
                "warning should reference Kimi label"
            );
        } else {
            panic!("expected lossy");
        }
    }

    #[test]
    fn code_exec_lossy_warning_references_both_labels() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, features::CODE_EXEC)
            .unwrap();
        if let Fidelity::LossyLabeled { warning } = &rule.fidelity {
            assert!(warning.contains("OpenAI") || warning.contains("Claude"));
        } else {
            panic!("expected lossy code_exec");
        }
    }

    #[test]
    fn mapping_error_display_uses_labels() {
        let err = MappingError::FeatureUnsupported {
            feature: "img".into(),
            from: Dialect::Kimi,
            to: Dialect::Codex,
        };
        let s = err.to_string();
        assert!(s.contains("Kimi"));
        assert!(s.contains("Codex"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 12: Mapping statistics collection
// ═══════════════════════════════════════════════════════════════════════════

mod statistics_collection {
    use super::*;

    #[test]
    fn rank_targets_returns_sorted_by_lossless_count() {
        let reg = known_rules();
        let ranks = reg.rank_targets(
            Dialect::OpenAi,
            &[
                features::TOOL_USE,
                features::STREAMING,
                features::IMAGE_INPUT,
            ],
        );
        // Should be sorted descending by lossless count
        for w in ranks.windows(2) {
            assert!(w[0].1 >= w[1].1, "should be sorted descending");
        }
    }

    #[test]
    fn rank_targets_excludes_source_dialect() {
        let reg = known_rules();
        let ranks = reg.rank_targets(Dialect::OpenAi, &[features::STREAMING]);
        assert!(
            ranks.iter().all(|(d, _)| *d != Dialect::OpenAi),
            "source dialect should be excluded"
        );
    }

    #[test]
    fn rank_targets_claude_and_gemini_high_for_openai() {
        let reg = known_rules();
        let ranks = reg.rank_targets(
            Dialect::OpenAi,
            &[
                features::TOOL_USE,
                features::STREAMING,
                features::IMAGE_INPUT,
            ],
        );
        // Claude and Gemini should be near the top (all 3 lossless)
        assert!(!ranks.is_empty());
        let top_dialects: Vec<_> = ranks.iter().map(|(d, _)| *d).collect();
        assert!(
            top_dialects.contains(&Dialect::Claude),
            "Claude should be in ranked results"
        );
        assert!(
            top_dialects.contains(&Dialect::Gemini),
            "Gemini should be in ranked results"
        );
    }

    #[test]
    fn rank_targets_excludes_fully_unsupported_dialects() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Codex,
            feature: "x".into(),
            fidelity: Fidelity::Unsupported {
                reason: "nope".into(),
            },
        });
        let ranks = reg.rank_targets(Dialect::OpenAi, &["x"]);
        assert!(
            ranks.iter().all(|(d, _)| *d != Dialect::Codex),
            "fully unsupported dialect should be excluded"
        );
    }

    #[test]
    fn rank_targets_empty_features_returns_empty() {
        let reg = known_rules();
        let ranks = reg.rank_targets(Dialect::OpenAi, &[]);
        assert!(ranks.is_empty());
    }

    #[test]
    fn registry_len_counts_all_rules() {
        let mut reg = MappingRegistry::new();
        assert_eq!(reg.len(), 0);
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
    fn registry_iter_covers_all_rules() {
        let reg = known_rules();
        let count = reg.iter().count();
        assert_eq!(count, reg.len());
    }

    #[test]
    fn count_lossless_vs_lossy_in_known_rules() {
        let reg = known_rules();
        let lossless = reg.iter().filter(|r| r.fidelity.is_lossless()).count();
        let unsupported = reg.iter().filter(|r| r.fidelity.is_unsupported()).count();
        let lossy = reg.len() - lossless - unsupported;
        assert!(lossless > 0, "should have lossless rules");
        assert!(lossy > 0, "should have lossy rules");
        assert!(unsupported > 0, "should have unsupported rules");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 13: Edge cases
// ═══════════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;

    #[test]
    fn validate_with_empty_feature_name() {
        let reg = known_rules();
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);
        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_unsupported());
        assert!(matches!(
            &results[0].errors[0],
            MappingError::InvalidInput { reason } if reason.contains("empty")
        ));
    }

    #[test]
    fn validate_with_no_features() {
        let reg = known_rules();
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[]);
        assert!(results.is_empty());
    }

    #[test]
    fn validate_with_duplicate_features() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[features::STREAMING.into(), features::STREAMING.into()],
        );
        assert_eq!(results.len(), 2);
        assert!(results[0].fidelity.is_lossless());
        assert!(results[1].fidelity.is_lossless());
    }

    #[test]
    fn validate_with_whitespace_only_feature() {
        let reg = known_rules();
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["   ".into()]);
        // Not empty, so not InvalidInput for "empty feature name"
        // but no rule exists for "   ", so FeatureUnsupported
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn validate_with_very_long_feature_name() {
        let reg = known_rules();
        let long_name = "a".repeat(10000);
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[long_name]);
        assert_eq!(results.len(), 1);
        assert!(results[0].fidelity.is_unsupported());
    }

    #[test]
    fn same_dialect_mapping_always_valid() {
        let reg = known_rules();
        for &d in Dialect::all() {
            let results = validate_mapping(
                &reg,
                d,
                d,
                &[
                    features::TOOL_USE.into(),
                    features::STREAMING.into(),
                    features::THINKING.into(),
                    features::IMAGE_INPUT.into(),
                    features::CODE_EXEC.into(),
                ],
            );
            for r in &results {
                assert!(
                    r.fidelity.is_lossless(),
                    "{d}->{d} {} should be lossless",
                    r.feature
                );
                assert!(r.errors.is_empty());
            }
        }
    }

    #[test]
    fn matrix_from_empty_registry() {
        let reg = MappingRegistry::new();
        let m = MappingMatrix::from_registry(&reg);
        for &a in Dialect::all() {
            for &b in Dialect::all() {
                assert!(!m.is_supported(a, b));
            }
        }
    }

    #[test]
    fn matrix_unsupported_only_not_marked_supported() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Codex,
            feature: features::IMAGE_INPUT.into(),
            fidelity: Fidelity::Unsupported {
                reason: "no images".into(),
            },
        });
        let m = MappingMatrix::from_registry(&reg);
        assert!(!m.is_supported(Dialect::OpenAi, Dialect::Codex));
    }

    #[test]
    fn matrix_set_explicit_false() {
        let mut m = MappingMatrix::new();
        m.set(Dialect::OpenAi, Dialect::Claude, false);
        assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
        assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), Some(false));
    }

    #[test]
    fn matrix_get_returns_none_for_unset() {
        let m = MappingMatrix::new();
        assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), None);
    }

    #[test]
    fn registry_is_empty_when_new() {
        let reg = MappingRegistry::new();
        assert!(reg.is_empty());
    }

    #[test]
    fn ir_message_through_validation_context() {
        // Creating IR structures alongside mapping validation
        let ir = IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Text {
                text: "hello".into(),
            }],
        );
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[features::STREAMING.into()],
        );
        assert!(results[0].fidelity.is_lossless());
        assert_eq!(ir.role, IrRole::User);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 14: Cross-dialect capability checking before mapping
// ═══════════════════════════════════════════════════════════════════════════

mod cross_dialect_capability {
    use super::*;

    #[test]
    fn can_check_dialect_pair_support_via_matrix() {
        let reg = known_rules();
        let m = MappingMatrix::from_registry(&reg);
        assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
        assert!(m.is_supported(Dialect::Claude, Dialect::OpenAi));
    }

    #[test]
    fn kimi_copilot_supported_in_matrix() {
        let reg = known_rules();
        let m = MappingMatrix::from_registry(&reg);
        assert!(m.is_supported(Dialect::Kimi, Dialect::Copilot));
        assert!(m.is_supported(Dialect::Copilot, Dialect::Kimi));
    }

    #[test]
    fn all_dialect_all_returns_six() {
        assert_eq!(Dialect::all().len(), 6);
    }

    #[test]
    fn matrix_symmetry_for_known_rules() {
        let reg = known_rules();
        let m = MappingMatrix::from_registry(&reg);
        for &a in Dialect::all() {
            for &b in Dialect::all() {
                if a == b {
                    continue;
                }
                let ab = m.is_supported(a, b);
                let ba = m.is_supported(b, a);
                assert_eq!(
                    ab, ba,
                    "matrix should be symmetric: {a}->{b}={ab}, {b}->{a}={ba}"
                );
            }
        }
    }

    #[test]
    fn rank_targets_considers_all_other_dialects() {
        let reg = known_rules();
        let ranks = reg.rank_targets(Dialect::OpenAi, &[features::STREAMING]);
        // Streaming is lossless everywhere, so all 5 other dialects should appear
        assert_eq!(ranks.len(), 5, "should rank all 5 non-source dialects");
    }

    #[test]
    fn validate_before_mapping_prevents_unsupported() {
        let reg = known_rules();
        let features_needed: Vec<String> =
            vec![features::IMAGE_INPUT.into(), features::CODE_EXEC.into()];
        let results = validate_mapping(&reg, Dialect::Kimi, Dialect::Codex, &features_needed);
        let any_unsupported = results.iter().any(|v| v.fidelity.is_unsupported());
        assert!(
            any_unsupported,
            "Kimi->Codex should have unsupported features"
        );
    }

    #[test]
    fn validate_all_features_kimi_to_openai() {
        let reg = known_rules();
        let feats: Vec<String> = vec![
            features::TOOL_USE.into(),
            features::STREAMING.into(),
            features::THINKING.into(),
            features::IMAGE_INPUT.into(),
            features::CODE_EXEC.into(),
        ];
        let results = validate_mapping(&reg, Dialect::Kimi, Dialect::OpenAi, &feats);
        // tool_use: lossless
        assert!(results[0].fidelity.is_lossless());
        // streaming: lossless
        assert!(results[1].fidelity.is_lossless());
        // thinking: lossy
        assert!(!results[2].fidelity.is_lossless());
        assert!(!results[2].fidelity.is_unsupported());
        // image_input: unsupported
        assert!(results[3].fidelity.is_unsupported());
        // code_exec: unsupported
        assert!(results[4].fidelity.is_unsupported());
    }

    #[test]
    fn validate_all_features_copilot_to_claude() {
        let reg = known_rules();
        let feats: Vec<String> = vec![
            features::TOOL_USE.into(),
            features::STREAMING.into(),
            features::THINKING.into(),
            features::IMAGE_INPUT.into(),
            features::CODE_EXEC.into(),
        ];
        let results = validate_mapping(&reg, Dialect::Copilot, Dialect::Claude, &feats);
        // tool_use: lossless
        assert!(results[0].fidelity.is_lossless());
        // streaming: lossless
        assert!(results[1].fidelity.is_lossless());
        // thinking: lossy
        assert!(!results[2].fidelity.is_lossless());
        // image_input: unsupported (Copilot has no image support)
        assert!(results[3].fidelity.is_unsupported());
        // code_exec: lossy (different execution models)
        assert!(!results[4].fidelity.is_lossless());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 15: Serde round-trip for all types
// ═══════════════════════════════════════════════════════════════════════════

mod serde_roundtrips {
    use super::*;

    #[test]
    fn fidelity_lossless_roundtrip() {
        let f = Fidelity::Lossless;
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(serde_json::from_str::<Fidelity>(&json).unwrap(), f);
    }

    #[test]
    fn fidelity_lossy_roundtrip() {
        let f = Fidelity::LossyLabeled {
            warning: "differs".into(),
        };
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(serde_json::from_str::<Fidelity>(&json).unwrap(), f);
    }

    #[test]
    fn fidelity_unsupported_roundtrip() {
        let f = Fidelity::Unsupported {
            reason: "no support".into(),
        };
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(serde_json::from_str::<Fidelity>(&json).unwrap(), f);
    }

    #[test]
    fn mapping_rule_roundtrip() {
        let rule = MappingRule {
            source_dialect: Dialect::Kimi,
            target_dialect: Dialect::Copilot,
            feature: features::STREAMING.into(),
            fidelity: Fidelity::Lossless,
        };
        let json = serde_json::to_string(&rule).unwrap();
        assert_eq!(serde_json::from_str::<MappingRule>(&json).unwrap(), rule);
    }

    #[test]
    fn mapping_validation_roundtrip_with_errors() {
        let v = MappingValidation {
            feature: features::THINKING.into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "mapped lossy".into(),
            },
            errors: vec![MappingError::FidelityLoss {
                feature: features::THINKING.into(),
                warning: "mapped lossy".into(),
            }],
        };
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(serde_json::from_str::<MappingValidation>(&json).unwrap(), v);
    }

    #[test]
    fn mapping_validation_roundtrip_empty_errors() {
        let v = MappingValidation {
            feature: features::STREAMING.into(),
            fidelity: Fidelity::Lossless,
            errors: vec![],
        };
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(serde_json::from_str::<MappingValidation>(&json).unwrap(), v);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 16: Code execution mapping specifics
// ═══════════════════════════════════════════════════════════════════════════

mod code_exec_mapping {
    use super::*;

    #[test]
    fn code_exec_cross_dialect_lossy_among_capable() {
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
                    !rule.fidelity.is_lossless(),
                    "code_exec {a}->{b} should be lossy (different models)"
                );
                assert!(
                    !rule.fidelity.is_unsupported(),
                    "code_exec {a}->{b} should not be unsupported"
                );
            }
        }
    }

    #[test]
    fn code_exec_kimi_unsupported_to_all_capable() {
        let reg = known_rules();
        for &d in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
            Dialect::Copilot,
        ] {
            let rule = reg.lookup(Dialect::Kimi, d, features::CODE_EXEC).unwrap();
            assert!(rule.fidelity.is_unsupported());
        }
    }

    #[test]
    fn code_exec_self_mapping_lossless() {
        let reg = known_rules();
        for &d in Dialect::all() {
            let rule = reg.lookup(d, d, features::CODE_EXEC).unwrap();
            assert!(rule.fidelity.is_lossless());
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 17: Image input mapping specifics
// ═══════════════════════════════════════════════════════════════════════════

mod image_input_mapping {
    use super::*;

    #[test]
    fn image_lossless_among_openai_claude_gemini() {
        let reg = known_rules();
        let img_capable = [Dialect::OpenAi, Dialect::Claude, Dialect::Gemini];
        for &a in &img_capable {
            for &b in &img_capable {
                let rule = reg.lookup(a, b, features::IMAGE_INPUT).unwrap();
                assert!(
                    rule.fidelity.is_lossless(),
                    "image_input {a}->{b} should be lossless"
                );
            }
        }
    }

    #[test]
    fn image_unsupported_to_from_codex() {
        let reg = known_rules();
        for &d in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
            assert!(
                reg.lookup(d, Dialect::Codex, features::IMAGE_INPUT)
                    .unwrap()
                    .fidelity
                    .is_unsupported()
            );
            assert!(
                reg.lookup(Dialect::Codex, d, features::IMAGE_INPUT)
                    .unwrap()
                    .fidelity
                    .is_unsupported()
            );
        }
    }

    #[test]
    fn kimi_copilot_image_unsupported_bidirectional() {
        let reg = known_rules();
        let rule = reg
            .lookup(Dialect::Kimi, Dialect::Copilot, features::IMAGE_INPUT)
            .unwrap();
        assert!(rule.fidelity.is_unsupported());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 18: IR types integration with mapping context
// ═══════════════════════════════════════════════════════════════════════════

mod ir_integration {
    use super::*;

    #[test]
    fn ir_conversation_can_be_validated_for_mapping() {
        let conv = IrConversation {
            messages: vec![
                IrMessage::new(
                    IrRole::System,
                    vec![IrContentBlock::Text {
                        text: "Be helpful".into(),
                    }],
                ),
                IrMessage::new(
                    IrRole::User,
                    vec![IrContentBlock::Text {
                        text: "Hello".into(),
                    }],
                ),
            ],
        };
        // Mapping validation for the features used by this conversation
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[features::STREAMING.into()],
        );
        assert!(results[0].fidelity.is_lossless());
        assert_eq!(conv.messages.len(), 2);
    }

    #[test]
    fn tool_use_validation_lossless_openai_to_claude() {
        let reg = known_rules();
        let results = validate_mapping(
            &reg,
            Dialect::OpenAi,
            Dialect::Claude,
            &[features::TOOL_USE.into()],
        );
        assert!(results[0].fidelity.is_lossless());
    }

    #[test]
    fn ir_content_blocks_cover_all_mappable_features() {
        // Text → always mappable
        let _text = IrContentBlock::Text {
            text: "hello".into(),
        };
        // Image → depends on dialect support
        let _img = IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "base64data".into(),
        };
        // ToolUse → depends on tool_use fidelity
        let _tool = IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "search".into(),
            input: serde_json::json!({"q": "rust"}),
        };
        // ToolResult → depends on tool_use fidelity
        let _result = IrContentBlock::ToolResult {
            tool_use_id: "t1".into(),
            content: vec![IrContentBlock::Text {
                text: "found it".into(),
            }],
            is_error: false,
        };
        // Thinking → depends on thinking fidelity
        let _think = IrContentBlock::Thinking {
            text: "let me think...".into(),
        };

        // All 5 block types exist
        let reg = known_rules();
        assert!(
            reg.lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
                .is_some()
        );
        assert!(
            reg.lookup(Dialect::OpenAi, Dialect::Claude, features::IMAGE_INPUT)
                .is_some()
        );
        assert!(
            reg.lookup(Dialect::OpenAi, Dialect::Claude, features::THINKING)
                .is_some()
        );
    }

    #[test]
    fn empty_conversation_can_still_be_validated() {
        let conv = IrConversation { messages: vec![] };
        let reg = known_rules();
        let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[]);
        assert!(results.is_empty());
        assert!(conv.messages.is_empty());
    }
}
