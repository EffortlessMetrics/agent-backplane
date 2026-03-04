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
// 1  Feature support matrix — which features map across which dialects
// ════════════════════════════════════════════════════════════════════════

/// Same dialect is always lossless for every known feature.
#[test]
fn feature_matrix_same_dialect_lossless() {
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

/// tool_use: OpenAI ↔ Claude ↔ Gemini are lossless.
#[test]
fn feature_matrix_tool_use_openai_claude_gemini_lossless() {
    let reg = known_rules();
    let core = [Dialect::OpenAi, Dialect::Claude, Dialect::Gemini];
    for &a in &core {
        for &b in &core {
            if a == b {
                continue;
            }
            let rule = reg.lookup(a, b, features::TOOL_USE).unwrap();
            assert!(
                rule.fidelity.is_lossless(),
                "tool_use {a:?} -> {b:?} should be lossless"
            );
        }
    }
}

/// tool_use: anything → Codex is lossy.
#[test]
fn feature_matrix_tool_use_to_codex_lossy() {
    let reg = known_rules();
    for &src in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Kimi,
        Dialect::Copilot,
    ] {
        let rule = reg.lookup(src, Dialect::Codex, features::TOOL_USE).unwrap();
        assert!(
            !rule.fidelity.is_lossless(),
            "tool_use {src:?} -> Codex should be lossy"
        );
    }
}

/// streaming: all 6 dialects map streaming losslessly.
#[test]
fn feature_matrix_streaming_all_lossless() {
    let reg = known_rules();
    for &a in Dialect::all() {
        for &b in Dialect::all() {
            let rule = reg.lookup(a, b, features::STREAMING).unwrap();
            assert!(
                rule.fidelity.is_lossless(),
                "streaming {a:?} -> {b:?} should be lossless"
            );
        }
    }
}

/// thinking: every cross-dialect pair is lossy.
#[test]
fn feature_matrix_thinking_cross_dialect_lossy() {
    let reg = known_rules();
    for &a in Dialect::all() {
        for &b in Dialect::all() {
            if a == b {
                continue;
            }
            let rule = reg.lookup(a, b, features::THINKING).unwrap();
            assert!(
                !rule.fidelity.is_lossless(),
                "thinking {a:?} -> {b:?} should not be lossless"
            );
        }
    }
}

/// image_input: OpenAI ↔ Claude ↔ Gemini are lossless.
#[test]
fn feature_matrix_image_lossless_triple() {
    let reg = known_rules();
    let trio = [Dialect::OpenAi, Dialect::Claude, Dialect::Gemini];
    for &a in &trio {
        for &b in &trio {
            if a == b {
                continue;
            }
            let rule = reg.lookup(a, b, features::IMAGE_INPUT).unwrap();
            assert!(
                rule.fidelity.is_lossless(),
                "image_input {a:?} -> {b:?} should be lossless"
            );
        }
    }
}

/// image_input: to Codex, Kimi, and Copilot is unsupported.
#[test]
fn feature_matrix_image_unsupported_targets() {
    let reg = known_rules();
    for &src in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
        for &tgt in &[Dialect::Codex, Dialect::Kimi, Dialect::Copilot] {
            let rule = reg.lookup(src, tgt, features::IMAGE_INPUT).unwrap();
            assert!(
                rule.fidelity.is_unsupported(),
                "image_input {src:?} -> {tgt:?} should be unsupported"
            );
        }
    }
}

/// code_exec: Kimi → any code-capable dialect is unsupported.
#[test]
fn feature_matrix_code_exec_kimi_unsupported() {
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
            "code_exec Kimi -> {tgt:?} should be unsupported"
        );
    }
}

/// code_exec: cross-dialect between code-capable dialects is lossy.
#[test]
fn feature_matrix_code_exec_cross_lossy() {
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
                "code_exec {a:?} -> {b:?} should be lossy"
            );
        }
    }
}

/// Kimi ↔ Copilot tool_use is lossless.
#[test]
fn feature_matrix_kimi_copilot_tool_use_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

// ════════════════════════════════════════════════════════════════════════
// 2  Fidelity tracking — lossless vs lossy labels
// ════════════════════════════════════════════════════════════════════════

/// Lossless fidelity is_lossless() == true, is_unsupported() == false.
#[test]
fn fidelity_lossless_predicates() {
    let f = Fidelity::Lossless;
    assert!(f.is_lossless());
    assert!(!f.is_unsupported());
}

/// LossyLabeled is neither lossless nor unsupported.
#[test]
fn fidelity_lossy_labeled_predicates() {
    let f = Fidelity::LossyLabeled {
        warning: "w".into(),
    };
    assert!(!f.is_lossless());
    assert!(!f.is_unsupported());
}

/// Unsupported: is_unsupported() == true, is_lossless() == false.
#[test]
fn fidelity_unsupported_predicates() {
    let f = Fidelity::Unsupported { reason: "r".into() };
    assert!(!f.is_lossless());
    assert!(f.is_unsupported());
}

/// LossyLabeled warning text is preserved.
#[test]
fn fidelity_lossy_warning_preserved() {
    let f = Fidelity::LossyLabeled {
        warning: "temperature clamped to [0,1]".into(),
    };
    if let Fidelity::LossyLabeled { warning } = &f {
        assert!(warning.contains("clamped"));
    } else {
        panic!("expected LossyLabeled");
    }
}

/// Unsupported reason text is preserved.
#[test]
fn fidelity_unsupported_reason_preserved() {
    let f = Fidelity::Unsupported {
        reason: "images not available".into(),
    };
    if let Fidelity::Unsupported { reason } = &f {
        assert!(reason.contains("images"));
    } else {
        panic!("expected Unsupported");
    }
}

/// Custom lossy rule produces FidelityLoss error in validation.
#[test]
fn fidelity_lossy_validation_produces_error() {
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

// ════════════════════════════════════════════════════════════════════════
// 3  Early failure on unmappable features
// ════════════════════════════════════════════════════════════════════════

/// Unknown feature in a populated registry → FeatureUnsupported.
#[test]
fn early_fail_unknown_feature_typed_error() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[s("nonexistent_feature")],
    );
    assert_eq!(results[0].errors.len(), 1);
    assert!(matches!(
        &results[0].errors[0],
        MappingError::FeatureUnsupported { .. }
    ));
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

/// Validation reports all issues — no short-circuiting.
#[test]
fn early_fail_report_all_issues() {
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
    assert_eq!(results.len(), 4);
    assert!(results[0].errors.is_empty());
    assert_eq!(results[1].errors.len(), 1);
    assert_eq!(results[2].errors.len(), 1);
    assert_eq!(results[3].errors.len(), 1);
}

/// Empty feature name yields InvalidInput error.
#[test]
fn early_fail_empty_feature_invalid() {
    let reg = known_rules();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[s("")]);
    assert!(matches!(
        &results[0].errors[0],
        MappingError::InvalidInput { reason } if reason.contains("empty")
    ));
}

// ════════════════════════════════════════════════════════════════════════
// 4  Tool mapping — OpenAI functions ↔ Claude tools ↔ Gemini declarations
// ════════════════════════════════════════════════════════════════════════

/// OpenAI → Claude tool_use is lossless (bidirectional).
#[test]
fn tool_map_openai_claude_lossless_bidirectional() {
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

/// OpenAI → Gemini tool_use is lossless (bidirectional).
#[test]
fn tool_map_openai_gemini_lossless_bidirectional() {
    let reg = known_rules();
    let fwd = reg
        .lookup(Dialect::OpenAi, Dialect::Gemini, features::TOOL_USE)
        .unwrap();
    let rev = reg
        .lookup(Dialect::Gemini, Dialect::OpenAi, features::TOOL_USE)
        .unwrap();
    assert!(fwd.fidelity.is_lossless());
    assert!(rev.fidelity.is_lossless());
}

/// Claude → Gemini tool_use is lossless (bidirectional).
#[test]
fn tool_map_claude_gemini_lossless_bidirectional() {
    let reg = known_rules();
    let fwd = reg
        .lookup(Dialect::Claude, Dialect::Gemini, features::TOOL_USE)
        .unwrap();
    let rev = reg
        .lookup(Dialect::Gemini, Dialect::Claude, features::TOOL_USE)
        .unwrap();
    assert!(fwd.fidelity.is_lossless());
    assert!(rev.fidelity.is_lossless());
}

/// Codex tool_use to all others is lossy with a descriptive warning.
#[test]
fn tool_map_codex_lossy_with_description() {
    let reg = known_rules();
    for &tgt in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
        let rule = reg.lookup(Dialect::Codex, tgt, features::TOOL_USE).unwrap();
        if let Fidelity::LossyLabeled { warning } = &rule.fidelity {
            assert!(
                warning.contains("Codex"),
                "warning should mention Codex: {warning}"
            );
        } else {
            panic!("Codex -> {tgt:?} tool_use should be LossyLabeled");
        }
    }
}

/// Kimi tool_use to core trio is lossless (OpenAI-compatible format).
#[test]
fn tool_map_kimi_to_core_lossless() {
    let reg = known_rules();
    for &tgt in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
        let rule = reg.lookup(Dialect::Kimi, tgt, features::TOOL_USE).unwrap();
        assert!(
            rule.fidelity.is_lossless(),
            "Kimi -> {tgt:?} tool_use should be lossless"
        );
    }
}

/// Copilot tool_use to core trio is lossless.
#[test]
fn tool_map_copilot_to_core_lossless() {
    let reg = known_rules();
    for &tgt in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
        let rule = reg
            .lookup(Dialect::Copilot, tgt, features::TOOL_USE)
            .unwrap();
        assert!(
            rule.fidelity.is_lossless(),
            "Copilot -> {tgt:?} tool_use should be lossless"
        );
    }
}

/// Validate tool_use feature for all 6×6 dialect pairs without panicking.
#[test]
fn tool_map_all_pairs_exist() {
    let reg = known_rules();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            assert!(
                reg.lookup(src, tgt, features::TOOL_USE).is_some(),
                "tool_use rule missing for {src:?} -> {tgt:?}"
            );
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
// 5  Message role mapping — system, user, assistant, tool across dialects
// ════════════════════════════════════════════════════════════════════════

/// OpenAI ↔ Claude roles map via tool_use (roles are implicit in messages).
#[test]
fn role_map_openai_claude_tool_use_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

/// All dialects have lossless streaming (which carries role semantics).
#[test]
fn role_map_streaming_carries_roles_lossless() {
    let reg = known_rules();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            let rule = reg.lookup(src, tgt, features::STREAMING).unwrap();
            assert!(rule.fidelity.is_lossless());
        }
    }
}

/// Gemini → Claude tool_use is lossless (roles map cleanly).
#[test]
fn role_map_gemini_claude_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Gemini, Dialect::Claude, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

/// Thinking messages (system-like role) are lossy cross-dialect.
#[test]
fn role_map_thinking_cross_dialect_lossy() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING)
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
}

// ════════════════════════════════════════════════════════════════════════
// 6  Token counting differences — vendor-specific tokenization
// ════════════════════════════════════════════════════════════════════════

/// Validate that thinking feature (which may affect token budgets) has
/// appropriate fidelity markers across all dialect pairs.
#[test]
fn token_counting_thinking_fidelity_per_pair() {
    let reg = known_rules();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            let rule = reg.lookup(src, tgt, features::THINKING).unwrap();
            if src == tgt {
                assert!(rule.fidelity.is_lossless());
            } else {
                assert!(!rule.fidelity.is_lossless());
            }
        }
    }
}

/// Custom token_counting feature registered as lossy carries warning.
#[test]
fn token_counting_custom_lossy_rule() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "token_counting".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "tokenizers differ: cl100k_base vs claude tokenizer".into(),
        },
    });
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[s("token_counting")],
    );
    assert!(!results[0].fidelity.is_lossless());
    if let Fidelity::LossyLabeled { warning } = &results[0].fidelity {
        assert!(warning.contains("tokenizer"));
    }
}

/// Custom token_counting Gemini → Codex unsupported.
#[test]
fn token_counting_gemini_codex_unsupported() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Gemini,
        target_dialect: Dialect::Codex,
        feature: "token_counting".into(),
        fidelity: Fidelity::Unsupported {
            reason: "Gemini billable character counting incompatible with Codex tokens".into(),
        },
    });
    let results = validate_mapping(
        &reg,
        Dialect::Gemini,
        Dialect::Codex,
        &[s("token_counting")],
    );
    assert!(results[0].fidelity.is_unsupported());
}

// ════════════════════════════════════════════════════════════════════════
// 7  Model parameter mapping — temperature, top_p ranges/defaults
// ════════════════════════════════════════════════════════════════════════

/// Temperature OpenAI [0,2] → Claude [0,1] is lossy.
#[test]
fn param_map_temperature_openai_claude_lossy() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "temperature".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "Claude temperature range [0,1], OpenAI [0,2] — values > 1 clamped".into(),
        },
    });
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[s("temperature")]);
    assert!(!results[0].fidelity.is_lossless());
}

/// Top-k: Claude top_k semantics differ from Gemini.
#[test]
fn param_map_top_k_gemini_claude_lossy() {
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

/// max_tokens: Codex uses max_output_tokens → unsupported.
#[test]
fn param_map_max_tokens_openai_codex_unsupported() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Codex,
        feature: "max_tokens".into(),
        fidelity: Fidelity::Unsupported {
            reason: "Codex uses max_output_tokens instead".into(),
        },
    });
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Codex, &[s("max_tokens")]);
    assert!(results[0].fidelity.is_unsupported());
}

/// frequency_penalty: unsupported for Claude.
#[test]
fn param_map_penalty_unsupported_claude() {
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

/// stop_sequences: lossless OpenAI → Claude.
#[test]
fn param_map_stop_sequences_lossless() {
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

// ════════════════════════════════════════════════════════════════════════
// 8  Context window validation — different limits per model/vendor
// ════════════════════════════════════════════════════════════════════════

/// Custom context_window feature marked lossy for different limits.
#[test]
fn context_window_openai_claude_lossy() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "context_window".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "GPT-4 128k vs Claude 200k — truncation behavior differs".into(),
        },
    });
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[s("context_window")],
    );
    assert!(!results[0].fidelity.is_lossless());
}

/// Context window Kimi → Codex unsupported.
#[test]
fn context_window_kimi_codex_unsupported() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Kimi,
        target_dialect: Dialect::Codex,
        feature: "context_window".into(),
        fidelity: Fidelity::Unsupported {
            reason: "Kimi 128k context cannot map to Codex environment model".into(),
        },
    });
    let results = validate_mapping(&reg, Dialect::Kimi, Dialect::Codex, &[s("context_window")]);
    assert!(results[0].fidelity.is_unsupported());
}

/// Same dialect context window is lossless via known rules thinking self-mapping.
#[test]
fn context_window_same_dialect_via_thinking() {
    let reg = known_rules();
    for &d in Dialect::all() {
        let rule = reg.lookup(d, d, features::THINKING).unwrap();
        assert!(rule.fidelity.is_lossless());
    }
}

// ════════════════════════════════════════════════════════════════════════
// 9  Response format mapping — json_object, json_schema, structured output
// ════════════════════════════════════════════════════════════════════════

/// response_format: OpenAI → Codex unsupported (json_schema not available).
#[test]
fn response_format_openai_codex_unsupported() {
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

/// response_format: OpenAI → Claude lossy (different structured output model).
#[test]
fn response_format_openai_claude_lossy() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "response_format".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "Claude uses tool_use for structured output, not json_mode".into(),
        },
    });
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[s("response_format")],
    );
    assert!(!results[0].fidelity.is_lossless());
    assert!(!results[0].fidelity.is_unsupported());
}

/// response_format: Gemini structured output → OpenAI lossy.
#[test]
fn response_format_gemini_openai_lossy() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Gemini,
        target_dialect: Dialect::OpenAi,
        feature: "response_format".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "Gemini responseMimeType differs from OpenAI response_format".into(),
        },
    });
    let results = validate_mapping(
        &reg,
        Dialect::Gemini,
        Dialect::OpenAi,
        &[s("response_format")],
    );
    assert!(!results[0].fidelity.is_lossless());
}

// ════════════════════════════════════════════════════════════════════════
// 10 Streaming semantics mapping — SSE vs JSONL differences
// ════════════════════════════════════════════════════════════════════════

/// Streaming is lossless for the four core dialects.
#[test]
fn streaming_core_four_lossless() {
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
            assert!(rule.fidelity.is_lossless());
        }
    }
}

/// Kimi and Copilot streaming → all others lossless.
#[test]
fn streaming_kimi_copilot_to_others_lossless() {
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

/// Kimi ↔ Copilot streaming is lossless.
#[test]
fn streaming_kimi_copilot_bidirectional() {
    let reg = known_rules();
    assert!(
        reg.lookup(Dialect::Kimi, Dialect::Copilot, features::STREAMING)
            .unwrap()
            .fidelity
            .is_lossless()
    );
    assert!(
        reg.lookup(Dialect::Copilot, Dialect::Kimi, features::STREAMING)
            .unwrap()
            .fidelity
            .is_lossless()
    );
}

/// Custom streaming.sse_format feature lossy between SSE and JSONL dialects.
#[test]
fn streaming_custom_sse_format_lossy() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "streaming.sse_format".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "OpenAI SSE data: prefix differs from Claude event: prefix".into(),
        },
    });
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[s("streaming.sse_format")],
    );
    assert!(!results[0].fidelity.is_lossless());
}

// ════════════════════════════════════════════════════════════════════════
// 11 Error code mapping — vendor-specific → ABP ErrorCode
// ════════════════════════════════════════════════════════════════════════

/// MappingError::FeatureUnsupported display contains all relevant info.
#[test]
fn error_code_feature_unsupported_display() {
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

/// MappingError::FidelityLoss display contains feature and warning.
#[test]
fn error_code_fidelity_loss_display() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "mapped to system message".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("thinking"));
    assert!(msg.contains("mapped to system message"));
}

/// MappingError::DialectMismatch display.
#[test]
fn error_code_dialect_mismatch_display() {
    let err = MappingError::DialectMismatch {
        from: Dialect::OpenAi,
        to: Dialect::Codex,
    };
    let msg = err.to_string();
    assert!(msg.contains("OpenAi") || msg.contains("OpenAI") || msg.contains("open_ai"));
}

/// MappingError::InvalidInput display.
#[test]
fn error_code_invalid_input_display() {
    let err = MappingError::InvalidInput {
        reason: "empty feature name".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("empty feature name"));
}

/// All four error variants are distinct.
#[test]
fn error_code_variants_distinct() {
    let e1 = MappingError::FeatureUnsupported {
        feature: "x".into(),
        from: Dialect::OpenAi,
        to: Dialect::Claude,
    };
    let e2 = MappingError::FidelityLoss {
        feature: "x".into(),
        warning: "w".into(),
    };
    let e3 = MappingError::DialectMismatch {
        from: Dialect::OpenAi,
        to: Dialect::Claude,
    };
    let e4 = MappingError::InvalidInput { reason: "r".into() };
    assert_ne!(e1, e2);
    assert_ne!(e2, e3);
    assert_ne!(e3, e4);
    assert_ne!(e1, e4);
}

// ════════════════════════════════════════════════════════════════════════
// 12 Capability feature registry — feature → support level per backend
// ════════════════════════════════════════════════════════════════════════

/// rank_targets excludes self-dialect.
#[test]
fn capability_rank_excludes_self() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE]);
    assert!(!ranked.iter().any(|(d, _)| *d == Dialect::OpenAi));
}

/// rank_targets excludes dialects where all features are unsupported.
#[test]
fn capability_rank_excludes_unsupported() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::IMAGE_INPUT]);
    // Codex, Kimi, Copilot don't support image_input
    assert!(!ranked.iter().any(|(d, _)| *d == Dialect::Codex));
    assert!(!ranked.iter().any(|(d, _)| *d == Dialect::Kimi));
    assert!(!ranked.iter().any(|(d, _)| *d == Dialect::Copilot));
}

/// rank_targets: Claude ranks high for tool_use + streaming + image_input.
#[test]
fn capability_rank_claude_high_for_core_features() {
    let reg = known_rules();
    let ranked = reg.rank_targets(
        Dialect::OpenAi,
        &[
            features::TOOL_USE,
            features::STREAMING,
            features::IMAGE_INPUT,
        ],
    );
    let claude_entry = ranked.iter().find(|(d, _)| *d == Dialect::Claude);
    assert!(claude_entry.is_some());
    assert_eq!(claude_entry.unwrap().1, 3, "Claude should be 3/3 lossless");
}

/// rank_targets sorted descending by lossless count.
#[test]
fn capability_rank_sorted_descending() {
    let reg = known_rules();
    let ranked = reg.rank_targets(
        Dialect::OpenAi,
        &[
            features::TOOL_USE,
            features::STREAMING,
            features::THINKING,
            features::IMAGE_INPUT,
        ],
    );
    for w in ranked.windows(2) {
        assert!(w[0].1 >= w[1].1, "rank must be sorted descending");
    }
}

/// MappingMatrix from known_rules marks supported pairs.
#[test]
fn capability_matrix_from_known_rules() {
    let reg = known_rules();
    let m = MappingMatrix::from_registry(&reg);
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
    assert!(m.is_supported(Dialect::Claude, Dialect::Gemini));
    assert!(m.is_supported(Dialect::Kimi, Dialect::OpenAi));
}

/// MappingMatrix excludes unsupported-only pairs.
#[test]
fn capability_matrix_excludes_unsupported_only() {
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
    assert!(!m.is_supported(Dialect::OpenAi, Dialect::Codex));
}

/// MappingMatrix: lossy pair IS marked supported.
#[test]
fn capability_matrix_lossy_is_supported() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Codex,
        feature: "tool_use".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "schema differs".into(),
        },
    });
    let m = MappingMatrix::from_registry(&reg);
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Codex));
}

/// MappingMatrix set/get/overwrite.
#[test]
fn capability_matrix_set_overwrite() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, true);
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
    m.set(Dialect::OpenAi, Dialect::Claude, false);
    assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
}

/// Empty matrix returns None for get.
#[test]
fn capability_matrix_empty_get_none() {
    let m = MappingMatrix::new();
    assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), None);
}

// ════════════════════════════════════════════════════════════════════════
// 13 Serde roundtrip for mapping types
// ════════════════════════════════════════════════════════════════════════

/// Fidelity::Lossless serde roundtrip.
#[test]
fn serde_fidelity_lossless() {
    let f = Fidelity::Lossless;
    let json = serde_json::to_string(&f).unwrap();
    let f2: Fidelity = serde_json::from_str(&json).unwrap();
    assert_eq!(f, f2);
}

/// Fidelity::LossyLabeled serde roundtrip.
#[test]
fn serde_fidelity_lossy_labeled() {
    let f = Fidelity::LossyLabeled {
        warning: "temperature clamped".into(),
    };
    let json = serde_json::to_string(&f).unwrap();
    let f2: Fidelity = serde_json::from_str(&json).unwrap();
    assert_eq!(f, f2);
}

/// Fidelity::Unsupported serde roundtrip.
#[test]
fn serde_fidelity_unsupported() {
    let f = Fidelity::Unsupported {
        reason: "not available".into(),
    };
    let json = serde_json::to_string(&f).unwrap();
    let f2: Fidelity = serde_json::from_str(&json).unwrap();
    assert_eq!(f, f2);
}

/// MappingRule serde roundtrip.
#[test]
fn serde_mapping_rule() {
    let rule = MappingRule {
        source_dialect: Dialect::Kimi,
        target_dialect: Dialect::Copilot,
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
    };
    let json = serde_json::to_string(&rule).unwrap();
    let rule2: MappingRule = serde_json::from_str(&json).unwrap();
    assert_eq!(rule, rule2);
}

/// MappingError::FeatureUnsupported serde roundtrip.
#[test]
fn serde_error_feature_unsupported() {
    let err = MappingError::FeatureUnsupported {
        feature: "img".into(),
        from: Dialect::OpenAi,
        to: Dialect::Codex,
    };
    let json = serde_json::to_string(&err).unwrap();
    let err2: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, err2);
}

/// MappingError::FidelityLoss serde roundtrip.
#[test]
fn serde_error_fidelity_loss() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "mapped".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let err2: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, err2);
}

/// MappingError::DialectMismatch serde roundtrip.
#[test]
fn serde_error_dialect_mismatch() {
    let err = MappingError::DialectMismatch {
        from: Dialect::Gemini,
        to: Dialect::Kimi,
    };
    let json = serde_json::to_string(&err).unwrap();
    let err2: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, err2);
}

/// MappingError::InvalidInput serde roundtrip.
#[test]
fn serde_error_invalid_input() {
    let err = MappingError::InvalidInput {
        reason: "empty".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let err2: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, err2);
}

/// MappingValidation serde roundtrip (empty errors).
#[test]
fn serde_validation_empty_errors() {
    let v = MappingValidation {
        feature: "streaming".into(),
        fidelity: Fidelity::Lossless,
        errors: vec![],
    };
    let json = serde_json::to_string(&v).unwrap();
    let v2: MappingValidation = serde_json::from_str(&json).unwrap();
    assert_eq!(v, v2);
}

/// MappingValidation serde roundtrip (with errors).
#[test]
fn serde_validation_with_errors() {
    let v = MappingValidation {
        feature: "image_input".into(),
        fidelity: Fidelity::Unsupported {
            reason: "no images".into(),
        },
        errors: vec![MappingError::FeatureUnsupported {
            feature: "image_input".into(),
            from: Dialect::OpenAi,
            to: Dialect::Codex,
        }],
    };
    let json = serde_json::to_string(&v).unwrap();
    let v2: MappingValidation = serde_json::from_str(&json).unwrap();
    assert_eq!(v, v2);
}

/// Fidelity JSON shape: tagged with "type" field.
#[test]
fn serde_fidelity_json_shape() {
    let f = Fidelity::Lossless;
    let val: serde_json::Value = serde_json::to_value(&f).unwrap();
    assert_eq!(val["type"], "lossless");

    let f2 = Fidelity::LossyLabeled {
        warning: "w".into(),
    };
    let val2: serde_json::Value = serde_json::to_value(&f2).unwrap();
    assert_eq!(val2["type"], "lossy_labeled");
    assert_eq!(val2["warning"], "w");

    let f3 = Fidelity::Unsupported { reason: "r".into() };
    let val3: serde_json::Value = serde_json::to_value(&f3).unwrap();
    assert_eq!(val3["type"], "unsupported");
    assert_eq!(val3["reason"], "r");
}

// ════════════════════════════════════════════════════════════════════════
// 14 Edge cases — unknown features, partial support, version-dependent
// ════════════════════════════════════════════════════════════════════════

/// Empty features list returns empty results.
#[test]
fn edge_empty_features_list() {
    let reg = known_rules();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[]);
    assert!(results.is_empty());
}

/// Whitespace-only feature name is not empty but returns unsupported.
#[test]
fn edge_whitespace_feature_name() {
    let reg = known_rules();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[s("  ")]);
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
}

/// Very long feature name handled gracefully.
#[test]
fn edge_long_feature_name() {
    let reg = known_rules();
    let long_name = "a".repeat(10_000);
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[long_name]);
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
}

/// Duplicate features in input produce duplicate results.
#[test]
fn edge_duplicate_features() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[s(features::TOOL_USE), s(features::TOOL_USE)],
    );
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].feature, results[1].feature);
}

/// Nested dotted feature name treated as opaque string.
#[test]
fn edge_nested_dotted_feature() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "tool_use.parallel".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "parallel not supported".into(),
        },
    });
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[s("tool_use.parallel")],
    );
    assert_eq!(results[0].feature, "tool_use.parallel");
    assert!(!results[0].fidelity.is_lossless());
}

/// Registry insert replaces existing rule for same key.
#[test]
fn edge_registry_insert_replaces() {
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
            reason: "changed".into(),
        },
    });
    assert_eq!(reg.len(), 1);
    let rule = reg.lookup(Dialect::OpenAi, Dialect::Claude, "x").unwrap();
    assert!(rule.fidelity.is_unsupported());
}

/// Registry iter returns all inserted rules.
#[test]
fn edge_registry_iter_count() {
    let mut reg = MappingRegistry::new();
    for i in 0..5 {
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: format!("f{i}"),
            fidelity: Fidelity::Lossless,
        });
    }
    assert_eq!(reg.iter().count(), 5);
}

/// known_rules registry is non-empty and has many rules.
#[test]
fn edge_known_rules_substantial() {
    let reg = known_rules();
    // 6 dialects × 5 features for self-mapping = 30, plus many cross-dialect
    assert!(reg.len() > 30, "known_rules should have >30 rules");
}

/// Cross-field validation covers all 6×6 dialect pairs for all features.
#[test]
fn edge_validation_all_pairs_covered() {
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
                "must return one result per feature for {src:?} -> {tgt:?}"
            );
        }
    }
}

/// Validation is deterministic: same inputs always produce same output.
#[test]
fn edge_validation_deterministic() {
    let reg = known_rules();
    let feats: Vec<String> = vec![
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

/// Validation is fast — 1000 iterations in <2s.
#[test]
fn edge_validation_performance() {
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

/// Lossless features have no errors, lossy features have exactly one.
#[test]
fn edge_lossless_no_errors_lossy_one_error() {
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
    for r in &results {
        if r.fidelity.is_lossless() {
            assert!(r.errors.is_empty());
        } else if matches!(r.fidelity, Fidelity::LossyLabeled { .. }) {
            assert_eq!(r.errors.len(), 1);
        }
    }
}

/// Copilot image_input unsupported to/from Kimi.
#[test]
fn edge_copilot_kimi_image_unsupported() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::IMAGE_INPUT)
        .unwrap();
    assert!(rule.fidelity.is_unsupported());
}

/// rank_targets with multiple mixed features returns correct ordering.
#[test]
fn edge_rank_targets_mixed_features() {
    let reg = known_rules();
    let ranked = reg.rank_targets(
        Dialect::OpenAi,
        &[
            features::TOOL_USE,
            features::STREAMING,
            features::IMAGE_INPUT,
            features::THINKING,
            features::CODE_EXEC,
        ],
    );
    // Claude & Gemini support image_input losslessly; others don't
    assert!(!ranked.is_empty());
    // First entry should have the highest score
    if ranked.len() > 1 {
        assert!(ranked[0].1 >= ranked[ranked.len() - 1].1);
    }
}
