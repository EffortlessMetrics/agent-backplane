// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive error handling tests for the mapping layer.
//!
//! Covers MappingError variants, Fidelity edge cases, MappingRegistry
//! validation failures, error context richness, Send/Sync bounds,
//! Display/Debug contracts, serde roundtrips, chained errors, and
//! integration with the unified error taxonomy (`abp_error`).

use abp_dialect::Dialect;
use abp_error::{AbpError, AbpErrorDto, ErrorCategory, ErrorCode};
use abp_mapping::{
    Fidelity, MappingError, MappingMatrix, MappingRegistry, MappingRule, MappingValidation,
    features, known_rules, validate_mapping,
};

// ===========================================================================
// Helpers
// ===========================================================================

fn empty_registry() -> MappingRegistry {
    MappingRegistry::new()
}

fn registry_with_lossless(from: Dialect, to: Dialect, feature: &str) -> MappingRegistry {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: from,
        target_dialect: to,
        feature: feature.into(),
        fidelity: Fidelity::Lossless,
    });
    reg
}

fn registry_with_unsupported(
    from: Dialect,
    to: Dialect,
    feature: &str,
    reason: &str,
) -> MappingRegistry {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: from,
        target_dialect: to,
        feature: feature.into(),
        fidelity: Fidelity::Unsupported {
            reason: reason.into(),
        },
    });
    reg
}

fn registry_with_lossy(
    from: Dialect,
    to: Dialect,
    feature: &str,
    warning: &str,
) -> MappingRegistry {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: from,
        target_dialect: to,
        feature: feature.into(),
        fidelity: Fidelity::LossyLabeled {
            warning: warning.into(),
        },
    });
    reg
}

// ===========================================================================
// 1. Mapping with unsupported capabilities returns typed error
// ===========================================================================

#[test]
fn unsupported_capability_returns_feature_unsupported_error() {
    let reg = registry_with_unsupported(
        Dialect::OpenAi,
        Dialect::Codex,
        features::IMAGE_INPUT,
        "Codex does not support image inputs",
    );
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &[features::IMAGE_INPUT.into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
    assert_eq!(results[0].errors.len(), 1);
    assert!(matches!(
        &results[0].errors[0],
        MappingError::FeatureUnsupported { feature, from, to }
        if feature == features::IMAGE_INPUT
            && *from == Dialect::OpenAi
            && *to == Dialect::Codex
    ));
}

#[test]
fn unsupported_capability_with_known_rules_image_to_codex() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &[features::IMAGE_INPUT.into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
    assert!(!results[0].errors.is_empty());
}

#[test]
fn unsupported_capability_kimi_code_exec() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Kimi,
        Dialect::OpenAi,
        &[features::CODE_EXEC.into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
    assert!(matches!(
        &results[0].errors[0],
        MappingError::FeatureUnsupported { .. }
    ));
}

#[test]
fn unsupported_capability_kimi_image_input() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Kimi,
        Dialect::Claude,
        &[features::IMAGE_INPUT.into()],
    );
    assert!(results[0].fidelity.is_unsupported());
}

#[test]
fn unsupported_capability_copilot_image_input() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Copilot,
        Dialect::Gemini,
        &[features::IMAGE_INPUT.into()],
    );
    assert!(results[0].fidelity.is_unsupported());
}

// ===========================================================================
// 2. Mapping unknown dialect fails early with clear error
// ===========================================================================

#[test]
fn unknown_feature_in_empty_registry_produces_error() {
    let reg = empty_registry();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &["nonexistent_feature".into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
    assert_eq!(results[0].errors.len(), 1);
    assert!(matches!(
        &results[0].errors[0],
        MappingError::FeatureUnsupported { feature, .. } if feature == "nonexistent_feature"
    ));
}

#[test]
fn unknown_feature_in_populated_registry() {
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
        MappingError::FeatureUnsupported { feature, .. } if feature == "teleportation"
    ));
}

#[test]
fn lookup_returns_none_for_unpopulated_dialect_pair() {
    let reg = registry_with_lossless(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE);
    // Reversed direction not registered
    assert!(
        reg.lookup(Dialect::Claude, Dialect::OpenAi, features::TOOL_USE)
            .is_none()
    );
}

#[test]
fn matrix_unpopulated_pair_not_supported() {
    let m = MappingMatrix::new();
    assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
    assert!(!m.is_supported(Dialect::Claude, Dialect::OpenAi));
}

#[test]
fn dialect_mismatch_error_display_contains_dialects() {
    let err = MappingError::DialectMismatch {
        from: Dialect::OpenAi,
        to: Dialect::Gemini,
    };
    let msg = err.to_string();
    assert!(msg.contains("OpenAI"), "should contain source: {msg}");
    assert!(msg.contains("Gemini"), "should contain target: {msg}");
}

// ===========================================================================
// 3. Mapping with incompatible tool definitions
// ===========================================================================

#[test]
fn codex_tool_use_mapping_is_lossy_from_openai() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &[features::TOOL_USE.into()],
    );
    assert_eq!(results.len(), 1);
    assert!(!results[0].fidelity.is_lossless());
    assert!(matches!(
        &results[0].errors[0],
        MappingError::FidelityLoss { feature, warning }
        if feature == features::TOOL_USE && !warning.is_empty()
    ));
}

#[test]
fn codex_tool_use_mapping_is_lossy_from_claude() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Claude,
        Dialect::Codex,
        &[features::TOOL_USE.into()],
    );
    assert_eq!(results.len(), 1);
    assert!(!results[0].fidelity.is_lossless());
}

#[test]
fn codex_tool_use_mapping_is_lossy_from_gemini() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Gemini,
        Dialect::Codex,
        &[features::TOOL_USE.into()],
    );
    assert_eq!(results.len(), 1);
    assert!(!results[0].fidelity.is_lossless());
}

#[test]
fn codex_tool_use_mapping_is_lossy_from_kimi() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Kimi,
        Dialect::Codex,
        &[features::TOOL_USE.into()],
    );
    assert_eq!(results.len(), 1);
    assert!(!results[0].fidelity.is_lossless());
}

#[test]
fn codex_tool_use_mapping_is_lossy_from_copilot() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Copilot,
        Dialect::Codex,
        &[features::TOOL_USE.into()],
    );
    assert_eq!(results.len(), 1);
    assert!(!results[0].fidelity.is_lossless());
}

#[test]
fn fidelity_loss_error_contains_warning_text() {
    let reg = registry_with_lossy(
        Dialect::OpenAi,
        Dialect::Codex,
        features::TOOL_USE,
        "schema differs",
    );
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &[features::TOOL_USE.into()],
    );
    if let MappingError::FidelityLoss { warning, .. } = &results[0].errors[0] {
        assert!(
            warning.contains("schema differs"),
            "warning should contain detail: {warning}"
        );
    } else {
        panic!("expected FidelityLoss error");
    }
}

// ===========================================================================
// 4. Empty request mapping behavior
// ===========================================================================

#[test]
fn empty_features_list_returns_empty_results() {
    let reg = known_rules();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[]);
    assert!(results.is_empty());
}

#[test]
fn empty_features_list_with_empty_registry() {
    let reg = empty_registry();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[]);
    assert!(results.is_empty());
}

#[test]
fn empty_registry_validates_any_feature_as_unsupported() {
    let reg = empty_registry();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[features::TOOL_USE.into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
}

#[test]
fn rank_targets_on_empty_registry_returns_empty() {
    let reg = empty_registry();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE]);
    assert!(ranked.is_empty());
}

// ===========================================================================
// 5. Null/missing fields in mapping input
// ===========================================================================

#[test]
fn empty_feature_name_produces_invalid_input_error() {
    let reg = known_rules();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);
    assert_eq!(results.len(), 1);
    assert!(matches!(
        &results[0].errors[0],
        MappingError::InvalidInput { reason } if reason.contains("empty")
    ));
}

#[test]
fn empty_feature_name_fidelity_is_unsupported() {
    let reg = empty_registry();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
}

#[test]
fn whitespace_only_feature_name_treated_as_unknown() {
    let reg = known_rules();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["   ".into()]);
    assert_eq!(results.len(), 1);
    // Whitespace-only is not empty, so it goes through normal lookup path
    assert!(results[0].fidelity.is_unsupported());
}

#[test]
fn multiple_empty_feature_names() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &["".into(), "".into(), "".into()],
    );
    assert_eq!(results.len(), 3);
    for r in &results {
        assert!(matches!(&r.errors[0], MappingError::InvalidInput { .. }));
    }
}

#[test]
fn invalid_input_error_reason_is_human_readable() {
    let err = MappingError::InvalidInput {
        reason: "field `name` cannot be empty".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("field `name` cannot be empty"));
    assert!(msg.contains("invalid input"));
}

// ===========================================================================
// 6. Mapping error messages contain useful context
// ===========================================================================

#[test]
fn feature_unsupported_display_contains_feature_name() {
    let err = MappingError::FeatureUnsupported {
        feature: "logprobs".into(),
        from: Dialect::Claude,
        to: Dialect::Gemini,
    };
    let msg = err.to_string();
    assert!(msg.contains("logprobs"), "feature name missing: {msg}");
    assert!(msg.contains("Claude"), "source dialect missing: {msg}");
    assert!(msg.contains("Gemini"), "target dialect missing: {msg}");
}

#[test]
fn fidelity_loss_display_contains_warning() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "mapped to system message".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("thinking"), "feature missing: {msg}");
    assert!(
        msg.contains("mapped to system message"),
        "warning missing: {msg}"
    );
}

#[test]
fn dialect_mismatch_display_contains_both_dialects() {
    let err = MappingError::DialectMismatch {
        from: Dialect::Codex,
        to: Dialect::Kimi,
    };
    let msg = err.to_string();
    assert!(msg.contains("Codex"), "source missing: {msg}");
    assert!(msg.contains("Kimi"), "target missing: {msg}");
}

#[test]
fn invalid_input_display_contains_reason() {
    let err = MappingError::InvalidInput {
        reason: "payload exceeds 1MB limit".into(),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("payload exceeds 1MB limit"),
        "reason missing: {msg}"
    );
}

#[test]
fn validation_result_unsupported_reason_contains_feature_name() {
    let reg = empty_registry();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &["my_custom_feature".into()],
    );
    if let Fidelity::Unsupported { reason } = &results[0].fidelity {
        assert!(
            reason.contains("my_custom_feature"),
            "reason should reference feature: {reason}"
        );
    } else {
        panic!("expected Unsupported fidelity");
    }
}

#[test]
fn error_display_is_non_empty_for_all_variants() {
    let errors = vec![
        MappingError::FeatureUnsupported {
            feature: "f".into(),
            from: Dialect::OpenAi,
            to: Dialect::Claude,
        },
        MappingError::FidelityLoss {
            feature: "f".into(),
            warning: "w".into(),
        },
        MappingError::DialectMismatch {
            from: Dialect::OpenAi,
            to: Dialect::Claude,
        },
        MappingError::InvalidInput { reason: "r".into() },
    ];
    for err in &errors {
        let msg = err.to_string();
        assert!(!msg.is_empty(), "Display should not be empty: {err:?}");
        assert!(msg.len() > 5, "Display should be descriptive: {msg}");
    }
}

// ===========================================================================
// 7. Error classification matches expected taxonomy
// ===========================================================================

#[test]
fn abp_error_dialect_unknown_has_dialect_category() {
    let err = AbpError::new(ErrorCode::DialectUnknown, "unrecognized dialect");
    assert_eq!(err.category(), ErrorCategory::Dialect);
}

#[test]
fn abp_error_dialect_mapping_failed_has_dialect_category() {
    let err = AbpError::new(ErrorCode::DialectMappingFailed, "mapping failed");
    assert_eq!(err.category(), ErrorCategory::Dialect);
}

#[test]
fn dialect_error_codes_stable_strings() {
    assert_eq!(ErrorCode::DialectUnknown.as_str(), "DIALECT_UNKNOWN");
    assert_eq!(
        ErrorCode::DialectMappingFailed.as_str(),
        "DIALECT_MAPPING_FAILED"
    );
}

#[test]
fn capability_unsupported_error_code_category() {
    assert_eq!(
        ErrorCode::CapabilityUnsupported.category(),
        ErrorCategory::Capability
    );
}

#[test]
fn ir_lowering_error_code_category() {
    assert_eq!(ErrorCode::IrLoweringFailed.category(), ErrorCategory::Ir);
}

#[test]
fn all_dialect_codes_share_same_category() {
    let dialect_codes = [ErrorCode::DialectUnknown, ErrorCode::DialectMappingFailed];
    for code in &dialect_codes {
        assert_eq!(
            code.category(),
            ErrorCategory::Dialect,
            "{code:?} should be Dialect category"
        );
    }
}

#[test]
fn error_category_display_for_dialect() {
    assert_eq!(ErrorCategory::Dialect.to_string(), "dialect");
}

#[test]
fn error_category_display_for_capability() {
    assert_eq!(ErrorCategory::Capability.to_string(), "capability");
}

#[test]
fn error_category_display_for_ir() {
    assert_eq!(ErrorCategory::Ir.to_string(), "ir");
}

// ===========================================================================
// 8. Partial mapping failures (some fields map, others don't)
// ===========================================================================

#[test]
fn partial_failure_some_features_pass_some_fail() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &[
            features::STREAMING.into(),   // lossless
            features::IMAGE_INPUT.into(), // unsupported
            features::TOOL_USE.into(),    // lossy
        ],
    );
    assert_eq!(results.len(), 3);

    // streaming: lossless, no errors
    assert!(results[0].fidelity.is_lossless());
    assert!(results[0].errors.is_empty());

    // image_input: unsupported
    assert!(results[1].fidelity.is_unsupported());
    assert!(!results[1].errors.is_empty());

    // tool_use: lossy
    assert!(!results[2].fidelity.is_lossless());
    assert!(!results[2].fidelity.is_unsupported());
}

#[test]
fn partial_failure_mixed_known_and_unknown_features() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[
            features::TOOL_USE.into(),
            "completely_unknown".into(),
            features::STREAMING.into(),
        ],
    );
    assert_eq!(results.len(), 3);

    assert!(results[0].errors.is_empty()); // tool_use OK
    assert!(!results[1].errors.is_empty()); // unknown has error
    assert!(results[2].errors.is_empty()); // streaming OK
}

#[test]
fn partial_failure_error_count_matches_failed_features() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &[
            features::STREAMING.into(),
            features::IMAGE_INPUT.into(),
            features::TOOL_USE.into(),
            features::THINKING.into(),
            "fake_feature".into(),
        ],
    );
    let error_count: usize = results.iter().map(|r| r.errors.len()).sum();
    // streaming is lossless (0 errors), others have 1 each
    assert!(
        error_count >= 4,
        "expected at least 4 errors, got {error_count}"
    );
}

#[test]
fn partial_failure_preserves_feature_names_in_results() {
    let reg = known_rules();
    let features_input: Vec<String> = vec![
        features::TOOL_USE.into(),
        features::STREAMING.into(),
        "unknown_x".into(),
    ];
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &features_input);
    for (i, result) in results.iter().enumerate() {
        assert_eq!(
            result.feature, features_input[i],
            "feature name mismatch at index {i}"
        );
    }
}

#[test]
fn partial_failure_lossless_results_always_have_zero_errors() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[features::TOOL_USE.into(), features::STREAMING.into()],
    );
    for r in &results {
        if r.fidelity.is_lossless() {
            assert!(
                r.errors.is_empty(),
                "lossless feature `{}` should have no errors",
                r.feature
            );
        }
    }
}

// ===========================================================================
// 9. Mapping with oversized payloads
// ===========================================================================

#[test]
fn very_long_feature_name_handled_gracefully() {
    let reg = known_rules();
    let long_name: String = "x".repeat(10_000);
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, std::slice::from_ref(&long_name));
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
    assert_eq!(results[0].feature, long_name);
}

#[test]
fn many_features_validated_without_panic() {
    let reg = known_rules();
    let features: Vec<String> = (0..1_000).map(|i| format!("feature_{i}")).collect();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &features);
    assert_eq!(results.len(), 1_000);
}

#[test]
fn oversized_warning_in_lossy_rule() {
    let long_warning: String = "w".repeat(50_000);
    let reg = registry_with_lossy(
        Dialect::OpenAi,
        Dialect::Claude,
        features::TOOL_USE,
        &long_warning,
    );
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[features::TOOL_USE.into()],
    );
    if let MappingError::FidelityLoss { warning, .. } = &results[0].errors[0] {
        assert_eq!(warning.len(), 50_000);
    } else {
        panic!("expected FidelityLoss");
    }
}

#[test]
fn oversized_reason_in_unsupported_rule() {
    let long_reason: String = "r".repeat(50_000);
    let reg = registry_with_unsupported(
        Dialect::OpenAi,
        Dialect::Codex,
        features::IMAGE_INPUT,
        &long_reason,
    );
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &[features::IMAGE_INPUT.into()],
    );
    if let Fidelity::Unsupported { reason } = &results[0].fidelity {
        assert_eq!(reason.len(), 50_000);
    } else {
        panic!("expected Unsupported fidelity");
    }
}

// ===========================================================================
// 10. Chained mapping errors (error from mapping feeds into next stage)
// ===========================================================================

#[test]
fn mapping_error_wraps_into_abp_error_as_source() {
    let mapping_err = MappingError::FeatureUnsupported {
        feature: "logprobs".into(),
        from: Dialect::Claude,
        to: Dialect::Gemini,
    };
    let abp_err =
        AbpError::new(ErrorCode::DialectMappingFailed, "mapping failed").with_source(mapping_err);
    assert_eq!(abp_err.code, ErrorCode::DialectMappingFailed);
    let source = std::error::Error::source(&abp_err).unwrap();
    assert!(
        source.to_string().contains("logprobs"),
        "source should reference feature"
    );
}

#[test]
fn chained_mapping_error_preserves_context() {
    let mapping_err = MappingError::DialectMismatch {
        from: Dialect::OpenAi,
        to: Dialect::Gemini,
    };
    let abp_err = AbpError::new(ErrorCode::DialectMappingFailed, "stage 1 failed")
        .with_context("stage", "dialect_translation")
        .with_source(mapping_err);
    assert_eq!(
        abp_err.context["stage"],
        serde_json::json!("dialect_translation")
    );
    assert!(std::error::Error::source(&abp_err).is_some());
}

#[test]
fn chained_error_dto_captures_source_message() {
    let mapping_err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "lossy translation".into(),
    };
    let abp_err = AbpError::new(ErrorCode::DialectMappingFailed, "mapping incomplete")
        .with_source(mapping_err);
    let dto: AbpErrorDto = (&abp_err).into();
    assert!(dto.source_message.is_some());
    assert!(dto.source_message.unwrap().contains("lossy translation"));
}

#[test]
fn chained_error_display_includes_code_and_message() {
    let abp_err = AbpError::new(ErrorCode::DialectMappingFailed, "translation failed");
    let display = abp_err.to_string();
    assert!(display.contains("DIALECT_MAPPING_FAILED"));
    assert!(display.contains("translation failed"));
}

#[test]
fn multiple_mapping_errors_collected_into_single_abp_error() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &[features::IMAGE_INPUT.into(), "unknown_feat".into()],
    );
    let all_errors: Vec<&MappingError> = results.iter().flat_map(|r| &r.errors).collect();
    assert!(all_errors.len() >= 2);

    // Wrap aggregated errors into a single AbpError with context
    let abp_err = AbpError::new(
        ErrorCode::DialectMappingFailed,
        format!("{} features failed to map", all_errors.len()),
    )
    .with_context("failed_count", all_errors.len());
    assert!(abp_err.to_string().contains("features failed to map"));
}

// ===========================================================================
// 11. All error types are Send + Sync
// ===========================================================================

fn assert_send<T: Send>() {}
fn assert_sync<T: Sync>() {}

#[test]
fn mapping_error_is_send() {
    assert_send::<MappingError>();
}

#[test]
fn mapping_error_is_sync() {
    assert_sync::<MappingError>();
}

#[test]
fn fidelity_is_send() {
    assert_send::<Fidelity>();
}

#[test]
fn fidelity_is_sync() {
    assert_sync::<Fidelity>();
}

#[test]
fn mapping_rule_is_send() {
    assert_send::<MappingRule>();
}

#[test]
fn mapping_rule_is_sync() {
    assert_sync::<MappingRule>();
}

#[test]
fn mapping_validation_is_send() {
    assert_send::<MappingValidation>();
}

#[test]
fn mapping_validation_is_sync() {
    assert_sync::<MappingValidation>();
}

#[test]
fn mapping_registry_is_send() {
    assert_send::<MappingRegistry>();
}

#[test]
fn mapping_registry_is_sync() {
    assert_sync::<MappingRegistry>();
}

#[test]
fn abp_error_is_send() {
    assert_send::<AbpError>();
}

#[test]
fn abp_error_is_sync() {
    assert_sync::<AbpError>();
}

// ===========================================================================
// 12. Error Display implementations are human-readable
// ===========================================================================

#[test]
fn feature_unsupported_display_is_human_readable() {
    let err = MappingError::FeatureUnsupported {
        feature: "video_input".into(),
        from: Dialect::OpenAi,
        to: Dialect::Codex,
    };
    let msg = err.to_string();
    // Should form a complete English-like sentence/phrase
    assert!(
        msg.contains("unsupported"),
        "should describe unsupported: {msg}"
    );
    assert!(msg.contains("video_input"));
}

#[test]
fn fidelity_loss_display_is_human_readable() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "no native equivalent in target".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("fidelity"), "should mention fidelity: {msg}");
    assert!(msg.contains("thinking"));
}

#[test]
fn dialect_mismatch_display_is_human_readable() {
    let err = MappingError::DialectMismatch {
        from: Dialect::Claude,
        to: Dialect::Codex,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("mismatch") || msg.contains("cannot"),
        "should describe mismatch: {msg}"
    );
}

#[test]
fn invalid_input_display_is_human_readable() {
    let err = MappingError::InvalidInput {
        reason: "feature name must be non-empty".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("invalid"), "should say invalid: {msg}");
    assert!(msg.contains("feature name must be non-empty"));
}

#[test]
fn debug_output_includes_variant_names() {
    let errors = vec![
        MappingError::FeatureUnsupported {
            feature: "f".into(),
            from: Dialect::OpenAi,
            to: Dialect::Claude,
        },
        MappingError::FidelityLoss {
            feature: "f".into(),
            warning: "w".into(),
        },
        MappingError::DialectMismatch {
            from: Dialect::OpenAi,
            to: Dialect::Claude,
        },
        MappingError::InvalidInput { reason: "r".into() },
    ];
    for err in &errors {
        let dbg = format!("{err:?}");
        // Debug should include the variant name
        let has_variant = dbg.contains("FeatureUnsupported")
            || dbg.contains("FidelityLoss")
            || dbg.contains("DialectMismatch")
            || dbg.contains("InvalidInput");
        assert!(has_variant, "Debug should include variant name: {dbg}");
    }
}

// ===========================================================================
// 13. Error downcasting works correctly
// ===========================================================================

#[test]
fn mapping_error_downcasts_from_dyn_error() {
    let err: Box<dyn std::error::Error> = Box::new(MappingError::InvalidInput {
        reason: "bad data".into(),
    });
    let downcasted = err.downcast_ref::<MappingError>();
    assert!(downcasted.is_some());
    assert!(matches!(
        downcasted.unwrap(),
        MappingError::InvalidInput { reason } if reason == "bad data"
    ));
}

#[test]
fn mapping_error_downcast_wrong_type_returns_none() {
    let err: Box<dyn std::error::Error> = Box::new(std::io::Error::other(
        "not a mapping error",
    ));
    let downcasted = err.downcast_ref::<MappingError>();
    assert!(downcasted.is_none());
}

#[test]
fn feature_unsupported_downcasts_correctly() {
    let err: Box<dyn std::error::Error + Send + Sync> =
        Box::new(MappingError::FeatureUnsupported {
            feature: "streaming".into(),
            from: Dialect::OpenAi,
            to: Dialect::Claude,
        });
    let downcasted = err.downcast::<MappingError>().unwrap();
    assert!(matches!(
        *downcasted,
        MappingError::FeatureUnsupported { .. }
    ));
}

#[test]
fn fidelity_loss_downcasts_correctly() {
    let err: Box<dyn std::error::Error + Send + Sync> = Box::new(MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "lossy".into(),
    });
    let downcasted = err.downcast::<MappingError>().unwrap();
    assert!(matches!(*downcasted, MappingError::FidelityLoss { .. }));
}

#[test]
fn dialect_mismatch_downcasts_correctly() {
    let err: Box<dyn std::error::Error + Send + Sync> = Box::new(MappingError::DialectMismatch {
        from: Dialect::Gemini,
        to: Dialect::Kimi,
    });
    let downcasted = err.downcast::<MappingError>().unwrap();
    assert!(matches!(*downcasted, MappingError::DialectMismatch { .. }));
}

#[test]
fn abp_error_source_chain_downcast_to_mapping_error() {
    let mapping_err = MappingError::FeatureUnsupported {
        feature: "code_exec".into(),
        from: Dialect::Kimi,
        to: Dialect::OpenAi,
    };
    let abp_err =
        AbpError::new(ErrorCode::DialectMappingFailed, "mapping failed").with_source(mapping_err);

    let source = std::error::Error::source(&abp_err).unwrap();
    let downcasted = source.downcast_ref::<MappingError>();
    assert!(downcasted.is_some());
    assert!(matches!(
        downcasted.unwrap(),
        MappingError::FeatureUnsupported { feature, .. } if feature == "code_exec"
    ));
}

// ===========================================================================
// Additional: Serde roundtrip for MappingError
// ===========================================================================

#[test]
fn mapping_error_serde_roundtrip_feature_unsupported() {
    let err = MappingError::FeatureUnsupported {
        feature: "tool_use".into(),
        from: Dialect::OpenAi,
        to: Dialect::Codex,
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn mapping_error_serde_roundtrip_fidelity_loss() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "no equivalent".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn mapping_error_serde_roundtrip_dialect_mismatch() {
    let err = MappingError::DialectMismatch {
        from: Dialect::Claude,
        to: Dialect::Gemini,
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn mapping_error_serde_roundtrip_invalid_input() {
    let err = MappingError::InvalidInput {
        reason: "empty".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn fidelity_serde_roundtrip_lossless() {
    let f = Fidelity::Lossless;
    let json = serde_json::to_string(&f).unwrap();
    let back: Fidelity = serde_json::from_str(&json).unwrap();
    assert_eq!(f, back);
}

#[test]
fn fidelity_serde_roundtrip_lossy_labeled() {
    let f = Fidelity::LossyLabeled {
        warning: "mapped approximately".into(),
    };
    let json = serde_json::to_string(&f).unwrap();
    let back: Fidelity = serde_json::from_str(&json).unwrap();
    assert_eq!(f, back);
}

#[test]
fn fidelity_serde_roundtrip_unsupported() {
    let f = Fidelity::Unsupported {
        reason: "not available".into(),
    };
    let json = serde_json::to_string(&f).unwrap();
    let back: Fidelity = serde_json::from_str(&json).unwrap();
    assert_eq!(f, back);
}

// ===========================================================================
// Additional: MappingValidation error aggregation
// ===========================================================================

#[test]
fn validation_result_feature_name_preserved() {
    let v = MappingValidation {
        feature: "streaming".into(),
        fidelity: Fidelity::Lossless,
        errors: vec![],
    };
    assert_eq!(v.feature, "streaming");
}

#[test]
fn validation_result_can_hold_multiple_errors() {
    let v = MappingValidation {
        feature: "tool_use".into(),
        fidelity: Fidelity::Unsupported {
            reason: "no mapping".into(),
        },
        errors: vec![
            MappingError::FeatureUnsupported {
                feature: "tool_use".into(),
                from: Dialect::OpenAi,
                to: Dialect::Codex,
            },
            MappingError::InvalidInput {
                reason: "additional constraint violated".into(),
            },
        ],
    };
    assert_eq!(v.errors.len(), 2);
}

// ===========================================================================
// Additional: Matrix and registry error scenarios
// ===========================================================================

#[test]
fn matrix_unsupported_only_pair_not_marked_supported() {
    let reg = registry_with_unsupported(
        Dialect::Gemini,
        Dialect::Codex,
        features::IMAGE_INPUT,
        "not supported",
    );
    let matrix = MappingMatrix::from_registry(&reg);
    assert!(!matrix.is_supported(Dialect::Gemini, Dialect::Codex));
}

#[test]
fn rank_targets_excludes_self() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE]);
    for (dialect, _) in &ranked {
        assert_ne!(*dialect, Dialect::OpenAi);
    }
}

#[test]
fn rank_targets_excludes_unsupported_only_targets() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Codex,
        feature: features::IMAGE_INPUT.into(),
        fidelity: Fidelity::Unsupported {
            reason: "nope".into(),
        },
    });
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::IMAGE_INPUT]);
    assert!(
        ranked.is_empty(),
        "unsupported-only target should be excluded"
    );
}

#[test]
fn registry_insert_replaces_preserving_count() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "f".into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "f".into(),
        fidelity: Fidelity::Unsupported {
            reason: "changed".into(),
        },
    });
    assert_eq!(reg.len(), 1);
    let rule = reg.lookup(Dialect::OpenAi, Dialect::Claude, "f").unwrap();
    assert!(rule.fidelity.is_unsupported());
}

// ===========================================================================
// Additional: AbpError integration with mapping context
// ===========================================================================

#[test]
fn abp_error_with_mapping_context_serializes_cleanly() {
    let err = AbpError::new(ErrorCode::DialectMappingFailed, "failed")
        .with_context("source_dialect", "openai")
        .with_context("target_dialect", "codex")
        .with_context("feature", "image_input");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    assert!(json.contains("openai"));
    assert!(json.contains("codex"));
    assert!(json.contains("image_input"));
}

#[test]
fn abp_error_dto_roundtrip_for_dialect_error() {
    let err = AbpError::new(ErrorCode::DialectUnknown, "unknown dialect 'foo'")
        .with_context("dialect_name", "foo");
    let dto: AbpErrorDto = (&err).into();
    let json = serde_json::to_string(&dto).unwrap();
    let back: AbpErrorDto = serde_json::from_str(&json).unwrap();
    assert_eq!(back.code, ErrorCode::DialectUnknown);
    assert!(back.message.contains("foo"));
}

#[test]
fn mapping_error_equality() {
    let a = MappingError::FeatureUnsupported {
        feature: "tool_use".into(),
        from: Dialect::OpenAi,
        to: Dialect::Claude,
    };
    let b = MappingError::FeatureUnsupported {
        feature: "tool_use".into(),
        from: Dialect::OpenAi,
        to: Dialect::Claude,
    };
    assert_eq!(a, b);
}

#[test]
fn mapping_error_inequality_different_variants() {
    let a = MappingError::FeatureUnsupported {
        feature: "tool_use".into(),
        from: Dialect::OpenAi,
        to: Dialect::Claude,
    };
    let b = MappingError::InvalidInput {
        reason: "tool_use".into(),
    };
    assert_ne!(a, b);
}

#[test]
fn mapping_error_clone_produces_equal_value() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "approximate mapping".into(),
    };
    let cloned = err.clone();
    assert_eq!(err, cloned);
}
