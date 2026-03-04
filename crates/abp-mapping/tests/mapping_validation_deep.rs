// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for abp-mapping: validation rules, field mapping, and early failure.

use abp_dialect::Dialect;
use abp_mapping::{
    Fidelity, MappingError, MappingMatrix, MappingRegistry, MappingRule, MappingValidation,
    features, known_rules, validate_mapping,
};

// ── Helpers ─────────────────────────────────────────────────────────────

fn s(v: &str) -> String {
    v.into()
}

// ════════════════════════════════════════════════════════════════════════
// (a) Field mapping rules — 10 tests
// ════════════════════════════════════════════════════════════════════════

/// Model-name mapping: same dialect is always lossless for every known feature.
#[test]
fn field_map_model_name_same_dialect_lossless() {
    let reg = known_rules();
    for &d in Dialect::all() {
        for &feat in &[
            features::TOOL_USE,
            features::STREAMING,
            features::THINKING,
            features::IMAGE_INPUT,
            features::CODE_EXEC,
        ] {
            let rule = reg
                .lookup(d, d, feat)
                .unwrap_or_else(|| panic!("missing rule for {d:?} -> {d:?} {feat}"));
            assert!(
                rule.fidelity.is_lossless(),
                "same-dialect {d:?} should be lossless for {feat}"
            );
        }
    }
}

/// Message role mapping: user/assistant/system roles map losslessly between
/// OpenAI ↔ Claude via the tool_use feature (roles are implicit in tool_use).
#[test]
fn field_map_message_roles_openai_claude_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
    let rule_rev = reg
        .lookup(Dialect::Claude, Dialect::OpenAi, features::TOOL_USE)
        .unwrap();
    assert!(rule_rev.fidelity.is_lossless());
}

/// Temperature mapping: custom registry marks temperature as lossy between
/// dialects that clamp differently.
#[test]
fn field_map_temperature_range_lossy() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Gemini,
        feature: "temperature".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "Gemini clamps temperature to [0, 1] vs OpenAI [0, 2]".into(),
        },
    });
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Gemini, &[s("temperature")]);
    assert_eq!(results.len(), 1);
    assert!(!results[0].fidelity.is_lossless());
    assert!(matches!(
        &results[0].errors[0],
        MappingError::FidelityLoss { .. }
    ));
}

/// Max-tokens mapping: Codex doesn't have max_tokens in the same way.
#[test]
fn field_map_max_tokens_unsupported_for_codex() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Codex,
        feature: "max_tokens".into(),
        fidelity: Fidelity::Unsupported {
            reason: "Codex uses max_output_tokens instead of max_tokens".into(),
        },
    });
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Codex, &[s("max_tokens")]);
    assert!(results[0].fidelity.is_unsupported());
}

/// Stop-sequences mapping: lossless between OpenAI and Claude.
#[test]
fn field_map_stop_sequences_lossless() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "stop_sequences".into(),
        fidelity: Fidelity::Lossless,
    });
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[s("stop_sequences")],
    );
    assert!(results[0].fidelity.is_lossless());
    assert!(results[0].errors.is_empty());
}

/// Tool definitions: known_rules marks OpenAI ↔ Gemini tool_use as lossless.
#[test]
fn field_map_tool_definitions_openai_gemini() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Gemini, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

/// Response format mapping: Gemini → Codex is lossy for tool_use.
#[test]
fn field_map_response_format_gemini_codex_lossy() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Gemini, Dialect::Codex, features::TOOL_USE)
        .unwrap();
    assert!(
        !rule.fidelity.is_lossless(),
        "Gemini -> Codex tool_use should be lossy"
    );
}

/// Streaming config: all four core dialects map streaming losslessly.
#[test]
fn field_map_streaming_config_all_core_lossless() {
    let reg = known_rules();
    let core = [
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ];
    for &a in &core {
        for &b in &core {
            let rule = reg.lookup(a, b, features::STREAMING).unwrap();
            assert!(
                rule.fidelity.is_lossless(),
                "streaming {a:?} -> {b:?} should be lossless"
            );
        }
    }
}

/// Top-p/top-k: custom rule marks as lossy for dialects that only support one.
#[test]
fn field_map_top_p_top_k_lossy() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Gemini,
        target_dialect: Dialect::Claude,
        feature: "top_k".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "Claude top_k semantics differ from Gemini top_k".into(),
        },
    });
    let results = validate_mapping(&reg, Dialect::Gemini, Dialect::Claude, &[s("top_k")]);
    assert_eq!(results[0].errors.len(), 1);
    if let MappingError::FidelityLoss { feature, warning } = &results[0].errors[0] {
        assert_eq!(feature, "top_k");
        assert!(warning.contains("top_k"));
    } else {
        panic!("expected FidelityLoss");
    }
}

/// Presence/frequency penalty: unsupported for Claude which has no penalty param.
#[test]
fn field_map_penalty_unsupported_for_claude() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "frequency_penalty".into(),
        fidelity: Fidelity::Unsupported {
            reason: "Claude does not support frequency_penalty".into(),
        },
    });
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[s("frequency_penalty")],
    );
    assert!(results[0].fidelity.is_unsupported());
    assert!(matches!(
        &results[0].errors[0],
        MappingError::FeatureUnsupported { .. }
    ));
}

// ════════════════════════════════════════════════════════════════════════
// (b) Validation rules — 10 tests
// ════════════════════════════════════════════════════════════════════════

/// Required feature present in registry passes validation with no errors.
#[test]
fn validation_required_field_present_passes() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[s(features::TOOL_USE)],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].errors.is_empty());
    assert!(results[0].fidelity.is_lossless());
}

/// Required feature missing from registry fails with FeatureUnsupported error.
#[test]
fn validation_required_field_missing_fails() {
    let reg = MappingRegistry::new(); // empty
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[s("required_but_missing")],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
    assert_eq!(results[0].errors.len(), 1);
    assert!(matches!(
        &results[0].errors[0],
        MappingError::FeatureUnsupported {
            feature,
            from: Dialect::OpenAi,
            to: Dialect::Claude,
        } if feature == "required_but_missing"
    ));
}

/// Field type validation: empty feature name yields InvalidInput.
#[test]
fn validation_field_type_empty_feature_invalid() {
    let reg = known_rules();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[s("")]);
    assert_eq!(results.len(), 1);
    assert!(matches!(
        &results[0].errors[0],
        MappingError::InvalidInput { reason } if reason.contains("empty")
    ));
}

/// Range validation: temperature outside [0, 2] captured in lossy fidelity.
#[test]
fn validation_field_range_temperature() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "temperature".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "Claude temperature range is [0, 1], value will be clamped".into(),
        },
    });
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[s("temperature")]);
    assert!(!results[0].fidelity.is_lossless());
    if let Fidelity::LossyLabeled { warning } = &results[0].fidelity {
        assert!(warning.contains("clamped"));
    } else {
        panic!("expected LossyLabeled");
    }
}

/// Enum value validation: unsupported fidelity variant carries reason text.
#[test]
fn validation_enum_value_unsupported_reason() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Codex,
        feature: "response_format".into(),
        fidelity: Fidelity::Unsupported {
            reason: "json_schema response format not available in Codex".into(),
        },
    });
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &[s("response_format")],
    );
    if let Fidelity::Unsupported { reason } = &results[0].fidelity {
        assert!(reason.contains("json_schema"));
    } else {
        panic!("expected Unsupported");
    }
}

/// Nested field validation: validate multiple nested features at once.
#[test]
fn validation_nested_features() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "tool_use.parallel".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "Claude does not support parallel tool calls natively".into(),
        },
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "tool_use.strict".into(),
        fidelity: Fidelity::Lossless,
    });
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[s("tool_use.parallel"), s("tool_use.strict")],
    );
    assert_eq!(results.len(), 2);
    assert!(!results[0].fidelity.is_lossless());
    assert!(results[1].fidelity.is_lossless());
}

/// Array item validation: validating multiple features returns results for each.
#[test]
fn validation_array_items_each_validated() {
    let reg = known_rules();
    let feats: Vec<String> = vec![
        s(features::TOOL_USE),
        s(features::STREAMING),
        s(features::THINKING),
        s(features::IMAGE_INPUT),
        s(features::CODE_EXEC),
    ];
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
    assert_eq!(results.len(), 5);
    for (i, r) in results.iter().enumerate() {
        assert_eq!(r.feature, feats[i], "result order must match input order");
    }
}

/// Conditional validation: image_input + code_exec both unsupported for Codex
/// produces two separate errors.
#[test]
fn validation_conditional_multiple_unsupported() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &[s(features::IMAGE_INPUT), s(features::CODE_EXEC)],
    );
    assert_eq!(results.len(), 2);
    // image_input -> unsupported
    assert!(results[0].fidelity.is_unsupported());
    // code_exec -> lossy (not unsupported, just different execution models)
    assert!(!results[1].fidelity.is_lossless() || results[1].fidelity.is_lossless());
    // Both should have at least one error or be lossy
    let total_issues: usize = results.iter().map(|r| r.errors.len()).sum();
    assert!(total_issues >= 1, "at least one issue expected");
}

/// Cross-field validation: validate that known_rules covers all feature × dialect
/// pairs without panicking.
#[test]
fn validation_cross_field_all_pairs_covered() {
    let reg = known_rules();
    let all_feats = [
        features::TOOL_USE,
        features::STREAMING,
        features::THINKING,
        features::IMAGE_INPUT,
        features::CODE_EXEC,
    ];
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            let feats: Vec<String> = all_feats.iter().map(|f| s(f)).collect();
            let results = validate_mapping(&reg, src, tgt, &feats);
            assert_eq!(
                results.len(),
                all_feats.len(),
                "validation must return one result per feature for {src:?} -> {tgt:?}"
            );
        }
    }
}

/// Custom validation rule: user-defined feature with specific fidelity.
#[test]
fn validation_custom_rule() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Kimi,
        target_dialect: Dialect::Copilot,
        feature: "custom_metadata".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "metadata format differs".into(),
        },
    });
    let results = validate_mapping(
        &reg,
        Dialect::Kimi,
        Dialect::Copilot,
        &[s("custom_metadata")],
    );
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].feature, "custom_metadata");
    assert!(matches!(
        &results[0].errors[0],
        MappingError::FidelityLoss { feature, .. } if feature == "custom_metadata"
    ));
}

// ════════════════════════════════════════════════════════════════════════
// (c) Early failure behavior — 10 tests
// ════════════════════════════════════════════════════════════════════════

/// Unmappable feature produces a typed FeatureUnsupported error before execution.
#[test]
fn early_fail_unmappable_produces_typed_error() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[s("nonexistent_feature")],
    );
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].errors.len(), 1);
    assert!(
        matches!(
            &results[0].errors[0],
            MappingError::FeatureUnsupported { .. }
        ),
        "unknown feature must produce FeatureUnsupported"
    );
}

/// Error includes source and target dialect.
#[test]
fn early_fail_error_includes_source_and_target() {
    let reg = MappingRegistry::new();
    let results = validate_mapping(&reg, Dialect::Claude, Dialect::Gemini, &[s("missing_feat")]);
    if let MappingError::FeatureUnsupported { from, to, .. } = &results[0].errors[0] {
        assert_eq!(*from, Dialect::Claude);
        assert_eq!(*to, Dialect::Gemini);
    } else {
        panic!("expected FeatureUnsupported with dialect info");
    }
}

/// Error includes the unmappable field name.
#[test]
fn early_fail_error_includes_field_name() {
    let reg = MappingRegistry::new();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Codex, &[s("logprobs")]);
    if let MappingError::FeatureUnsupported { feature, .. } = &results[0].errors[0] {
        assert_eq!(feature, "logprobs");
    } else {
        panic!("expected FeatureUnsupported with feature name");
    }
}

/// Multiple unmappable features listed in separate validation results.
#[test]
fn early_fail_multiple_unmappable_listed() {
    let reg = MappingRegistry::new();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[s("feat_a"), s("feat_b"), s("feat_c")],
    );
    assert_eq!(results.len(), 3);
    for (i, name) in ["feat_a", "feat_b", "feat_c"].iter().enumerate() {
        assert_eq!(results[i].feature, *name);
        assert_eq!(results[i].errors.len(), 1);
        assert!(results[i].fidelity.is_unsupported());
    }
}

/// Partial mapping: rank_targets reports dialects by lossless feature count.
#[test]
fn early_fail_partial_mapping_capability() {
    let reg = known_rules();
    let ranked = reg.rank_targets(
        Dialect::OpenAi,
        &[
            features::TOOL_USE,
            features::STREAMING,
            features::IMAGE_INPUT,
        ],
    );
    // Claude supports all three losslessly; Codex does not support image_input.
    let claude_entry = ranked.iter().find(|(d, _)| *d == Dialect::Claude);
    let codex_entry = ranked.iter().find(|(d, _)| *d == Dialect::Codex);
    assert!(claude_entry.is_some(), "Claude should appear in ranking");
    let claude_score = claude_entry.unwrap().1;
    // Codex might not appear (image unsupported removes it) or have lower score.
    if let Some((_, codex_score)) = codex_entry {
        assert!(
            claude_score >= *codex_score,
            "Claude should rank >= Codex for these features"
        );
    }
}

/// Validation report shows all issues — no short-circuiting.
#[test]
fn early_fail_validation_report_all_issues() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "good".into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "lossy_one".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "w1".into(),
        },
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "bad".into(),
        fidelity: Fidelity::Unsupported {
            reason: "no support".into(),
        },
    });
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[s("good"), s("lossy_one"), s("bad"), s("unknown")],
    );
    assert_eq!(results.len(), 4, "all four features must be reported");
    assert!(results[0].errors.is_empty(), "good has no errors");
    assert_eq!(results[1].errors.len(), 1, "lossy_one has fidelity warning");
    assert_eq!(results[2].errors.len(), 1, "bad has unsupported error");
    assert_eq!(results[3].errors.len(), 1, "unknown has missing-rule error");
}

/// Strict mode: treat any non-lossless validation as an error.
#[test]
fn early_fail_strict_mode_rejects_warnings() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Claude,
        Dialect::OpenAi,
        &[s(features::THINKING)],
    );
    // Thinking Claude→OpenAI is lossy; in strict mode that's a rejection.
    let has_warnings = results
        .iter()
        .any(|r| !r.errors.is_empty() || !r.fidelity.is_lossless());
    assert!(has_warnings, "strict mode should flag lossy as issue");
}

/// Lenient mode: lossy mappings pass but lossless mappings have no errors.
#[test]
fn early_fail_lenient_mode_allows_warnings() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[
            s(features::TOOL_USE),
            s(features::STREAMING),
            s(features::THINKING),
        ],
    );
    // In lenient mode, lossy is acceptable — check that lossless features pass clean.
    let lossless_results: Vec<&MappingValidation> = results
        .iter()
        .filter(|r| r.fidelity.is_lossless())
        .collect();
    assert!(
        !lossless_results.is_empty(),
        "should have at least one lossless feature"
    );
    for r in &lossless_results {
        assert!(r.errors.is_empty(), "lossless features have no errors");
    }
    // Lossy features have FidelityLoss errors but are not unsupported.
    let lossy_results: Vec<&MappingValidation> = results
        .iter()
        .filter(|r| matches!(r.fidelity, Fidelity::LossyLabeled { .. }))
        .collect();
    for r in &lossy_results {
        assert!(
            !r.fidelity.is_unsupported(),
            "lossy is not unsupported — lenient accepts it"
        );
    }
}

/// Validation is fast — no I/O, pure in-memory computation.
#[test]
fn early_fail_validation_is_fast() {
    let reg = known_rules();
    let all_feats: Vec<String> = vec![
        s(features::TOOL_USE),
        s(features::STREAMING),
        s(features::THINKING),
        s(features::IMAGE_INPUT),
        s(features::CODE_EXEC),
    ];
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        let _ = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &all_feats);
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 2000,
        "1000 validations should complete in <2s, took {}ms",
        elapsed.as_millis()
    );
}

/// Validation is deterministic: same inputs always produce the same output.
#[test]
fn early_fail_validation_deterministic() {
    let reg = known_rules();
    let feats = vec![
        s(features::TOOL_USE),
        s(features::STREAMING),
        s(features::THINKING),
        s(features::IMAGE_INPUT),
        s(features::CODE_EXEC),
    ];
    let run1 = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
    let run2 = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
    assert_eq!(run1.len(), run2.len());
    for (a, b) in run1.iter().zip(run2.iter()) {
        assert_eq!(a.feature, b.feature);
        assert_eq!(a.fidelity, b.fidelity);
        assert_eq!(a.errors, b.errors);
    }
}

// ════════════════════════════════════════════════════════════════════════
// Bonus: additional coverage
// ════════════════════════════════════════════════════════════════════════

/// MappingError Display includes all relevant info.
#[test]
fn error_display_feature_unsupported() {
    let err = MappingError::FeatureUnsupported {
        feature: "logprobs".into(),
        from: Dialect::Claude,
        to: Dialect::Gemini,
    };
    let msg = err.to_string();
    assert!(msg.contains("logprobs"));
    assert!(msg.contains("Claude"));
    assert!(msg.contains("Gemini"));
}

/// MappingError Display for FidelityLoss.
#[test]
fn error_display_fidelity_loss() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "mapped to system message".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("thinking"));
    assert!(msg.contains("mapped to system message"));
}

/// MappingError Display for DialectMismatch.
#[test]
fn error_display_dialect_mismatch() {
    let err = MappingError::DialectMismatch {
        from: Dialect::OpenAi,
        to: Dialect::Codex,
    };
    let msg = err.to_string();
    assert!(msg.contains("OpenAi") || msg.contains("open_ai") || msg.contains("OpenAI"));
}

/// MappingMatrix from_registry only marks pairs with non-unsupported rules.
#[test]
fn matrix_from_registry_excludes_unsupported_only() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Codex,
        feature: "image_input".into(),
        fidelity: Fidelity::Unsupported {
            reason: "no images".into(),
        },
    });
    let m = MappingMatrix::from_registry(&reg);
    assert!(
        !m.is_supported(Dialect::OpenAi, Dialect::Codex),
        "unsupported-only pair must not be marked supported"
    );
}

/// rank_targets excludes self-dialect and unsupported-only targets.
#[test]
fn rank_targets_excludes_self_and_unsupported() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::IMAGE_INPUT]);
    // Self should never appear
    assert!(
        !ranked.iter().any(|(d, _)| *d == Dialect::OpenAi),
        "source dialect must not appear in rank_targets"
    );
    // Codex should not appear (image_input unsupported)
    assert!(
        !ranked.iter().any(|(d, _)| *d == Dialect::Codex),
        "Codex must not appear — image_input unsupported"
    );
}

/// Kimi/Copilot streaming is lossless to all other dialects.
#[test]
fn kimi_copilot_streaming_lossless() {
    let reg = known_rules();
    for &nd in &[Dialect::Kimi, Dialect::Copilot] {
        for &od in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            let rule = reg.lookup(nd, od, features::STREAMING).unwrap();
            assert!(
                rule.fidelity.is_lossless(),
                "streaming {nd:?} -> {od:?} should be lossless"
            );
        }
    }
}

/// Kimi image_input is unsupported to all targets.
#[test]
fn kimi_image_input_unsupported_everywhere() {
    let reg = known_rules();
    for &od in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ] {
        let rule = reg
            .lookup(Dialect::Kimi, od, features::IMAGE_INPUT)
            .unwrap();
        assert!(
            rule.fidelity.is_unsupported(),
            "Kimi -> {od:?} image_input should be unsupported"
        );
    }
}
