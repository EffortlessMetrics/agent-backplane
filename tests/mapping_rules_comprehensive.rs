#![allow(clippy::all)]

use abp_dialect::Dialect;
use abp_mapping::{
    Fidelity, MappingError, MappingMatrix, MappingRegistry, MappingRule, MappingValidation,
    features, known_rules, validate_mapping,
};

// ── Helpers ─────────────────────────────────────────────────────────────

fn all_features() -> [&'static str; 5] {
    [
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

// ═══════════════════════════════════════════════════════════════════════
//  1. Fidelity
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_lossless_is_lossless() {
    assert!(Fidelity::Lossless.is_lossless());
}

#[test]
fn fidelity_lossy_labeled_is_not_lossless() {
    let f = Fidelity::LossyLabeled {
        warning: "w".into(),
    };
    assert!(!f.is_lossless());
}

#[test]
fn fidelity_unsupported_is_not_lossless() {
    let f = Fidelity::Unsupported { reason: "r".into() };
    assert!(!f.is_lossless());
}

#[test]
fn fidelity_lossless_is_not_unsupported() {
    assert!(!Fidelity::Lossless.is_unsupported());
}

#[test]
fn fidelity_lossy_labeled_is_not_unsupported() {
    let f = Fidelity::LossyLabeled {
        warning: "w".into(),
    };
    assert!(!f.is_unsupported());
}

#[test]
fn fidelity_unsupported_is_unsupported() {
    let f = Fidelity::Unsupported { reason: "r".into() };
    assert!(f.is_unsupported());
}

#[test]
fn fidelity_clone_preserves_equality() {
    let f = Fidelity::LossyLabeled {
        warning: "test".into(),
    };
    assert_eq!(f.clone(), f);
}

#[test]
fn fidelity_debug_not_empty() {
    let f = Fidelity::Lossless;
    let dbg = format!("{:?}", f);
    assert!(!dbg.is_empty());
}

// ── Fidelity serde ──────────────────────────────────────────────────────

#[test]
fn fidelity_serde_roundtrip_lossless() {
    let f = Fidelity::Lossless;
    let json = serde_json::to_string(&f).unwrap();
    let f2: Fidelity = serde_json::from_str(&json).unwrap();
    assert_eq!(f, f2);
}

#[test]
fn fidelity_serde_roundtrip_lossy_labeled() {
    let f = Fidelity::LossyLabeled {
        warning: "some warning".into(),
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
fn fidelity_serde_lossless_has_type_tag() {
    let json = serde_json::to_string(&Fidelity::Lossless).unwrap();
    assert!(json.contains("\"type\""));
    assert!(json.contains("\"lossless\""));
}

#[test]
fn fidelity_serde_lossy_labeled_has_warning_field() {
    let f = Fidelity::LossyLabeled {
        warning: "hello".into(),
    };
    let json = serde_json::to_string(&f).unwrap();
    assert!(json.contains("\"warning\""));
    assert!(json.contains("hello"));
}

#[test]
fn fidelity_serde_unsupported_has_reason_field() {
    let f = Fidelity::Unsupported {
        reason: "nope".into(),
    };
    let json = serde_json::to_string(&f).unwrap();
    assert!(json.contains("\"reason\""));
    assert!(json.contains("nope"));
}

// ═══════════════════════════════════════════════════════════════════════
//  2. MappingError
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn error_feature_unsupported_display_contains_feature() {
    let err = MappingError::FeatureUnsupported {
        feature: "logprobs".into(),
        from: Dialect::Claude,
        to: Dialect::Gemini,
    };
    let msg = err.to_string();
    assert!(msg.contains("logprobs"));
}

#[test]
fn error_feature_unsupported_display_contains_dialects() {
    let err = MappingError::FeatureUnsupported {
        feature: "x".into(),
        from: Dialect::Claude,
        to: Dialect::Gemini,
    };
    let msg = err.to_string();
    assert!(msg.contains("Claude"));
    assert!(msg.contains("Gemini"));
}

#[test]
fn error_fidelity_loss_display_contains_warning() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "mapped to system".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("mapped to system"));
    assert!(msg.contains("thinking"));
}

#[test]
fn error_dialect_mismatch_display() {
    let err = MappingError::DialectMismatch {
        from: Dialect::OpenAi,
        to: Dialect::Codex,
    };
    let msg = err.to_string();
    assert!(msg.contains("OpenAI"));
    assert!(msg.contains("Codex"));
}

#[test]
fn error_invalid_input_display() {
    let err = MappingError::InvalidInput {
        reason: "empty feature name".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("empty feature name"));
}

#[test]
fn error_serde_feature_unsupported_roundtrip() {
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
fn error_serde_fidelity_loss_roundtrip() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "lossy".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let err2: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, err2);
}

#[test]
fn error_serde_dialect_mismatch_roundtrip() {
    let err = MappingError::DialectMismatch {
        from: Dialect::Kimi,
        to: Dialect::Copilot,
    };
    let json = serde_json::to_string(&err).unwrap();
    let err2: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, err2);
}

#[test]
fn error_serde_invalid_input_roundtrip() {
    let err = MappingError::InvalidInput {
        reason: "bad".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let err2: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, err2);
}

#[test]
fn error_eq_same_variant() {
    let a = MappingError::InvalidInput { reason: "x".into() };
    let b = MappingError::InvalidInput { reason: "x".into() };
    assert_eq!(a, b);
}

#[test]
fn error_ne_different_variant() {
    let a = MappingError::InvalidInput { reason: "x".into() };
    let b = MappingError::FidelityLoss {
        feature: "x".into(),
        warning: "y".into(),
    };
    assert_ne!(a, b);
}

#[test]
fn error_clone_preserves_equality() {
    let err = MappingError::FeatureUnsupported {
        feature: "f".into(),
        from: Dialect::OpenAi,
        to: Dialect::Claude,
    };
    assert_eq!(err.clone(), err);
}

// ═══════════════════════════════════════════════════════════════════════
//  3. MappingRule
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn rule_construction() {
    let rule = make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    );
    assert_eq!(rule.source_dialect, Dialect::OpenAi);
    assert_eq!(rule.target_dialect, Dialect::Claude);
    assert_eq!(rule.feature, "tool_use");
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn rule_serde_roundtrip() {
    let rule = make_rule(
        Dialect::Gemini,
        Dialect::Codex,
        "streaming",
        Fidelity::LossyLabeled {
            warning: "test".into(),
        },
    );
    let json = serde_json::to_string(&rule).unwrap();
    let rule2: MappingRule = serde_json::from_str(&json).unwrap();
    assert_eq!(rule, rule2);
}

#[test]
fn rule_clone_preserves_equality() {
    let rule = make_rule(
        Dialect::Kimi,
        Dialect::Copilot,
        "code_exec",
        Fidelity::Lossless,
    );
    assert_eq!(rule.clone(), rule);
}

#[test]
fn rule_ne_different_feature() {
    let a = make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    );
    let b = make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "streaming",
        Fidelity::Lossless,
    );
    assert_ne!(a, b);
}

#[test]
fn rule_ne_different_dialect() {
    let a = make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    );
    let b = make_rule(
        Dialect::OpenAi,
        Dialect::Gemini,
        "tool_use",
        Fidelity::Lossless,
    );
    assert_ne!(a, b);
}

// ═══════════════════════════════════════════════════════════════════════
//  4. MappingValidation
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validation_construction_no_errors() {
    let v = MappingValidation {
        feature: "streaming".into(),
        fidelity: Fidelity::Lossless,
        errors: vec![],
    };
    assert_eq!(v.feature, "streaming");
    assert!(v.fidelity.is_lossless());
    assert!(v.errors.is_empty());
}

#[test]
fn validation_with_errors() {
    let v = MappingValidation {
        feature: "img".into(),
        fidelity: Fidelity::Unsupported {
            reason: "nope".into(),
        },
        errors: vec![MappingError::FeatureUnsupported {
            feature: "img".into(),
            from: Dialect::OpenAi,
            to: Dialect::Codex,
        }],
    };
    assert_eq!(v.errors.len(), 1);
    assert!(v.fidelity.is_unsupported());
}

#[test]
fn validation_serde_roundtrip() {
    let v = MappingValidation {
        feature: "tool_use".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "w".into(),
        },
        errors: vec![MappingError::FidelityLoss {
            feature: "tool_use".into(),
            warning: "w".into(),
        }],
    };
    let json = serde_json::to_string(&v).unwrap();
    let v2: MappingValidation = serde_json::from_str(&json).unwrap();
    assert_eq!(v, v2);
}

#[test]
fn validation_clone_preserves_equality() {
    let v = MappingValidation {
        feature: "x".into(),
        fidelity: Fidelity::Lossless,
        errors: vec![],
    };
    assert_eq!(v.clone(), v);
}

// ═══════════════════════════════════════════════════════════════════════
//  5. Feature constants
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn feature_tool_use_value() {
    assert_eq!(features::TOOL_USE, "tool_use");
}

#[test]
fn feature_streaming_value() {
    assert_eq!(features::STREAMING, "streaming");
}

#[test]
fn feature_thinking_value() {
    assert_eq!(features::THINKING, "thinking");
}

#[test]
fn feature_image_input_value() {
    assert_eq!(features::IMAGE_INPUT, "image_input");
}

#[test]
fn feature_code_exec_value() {
    assert_eq!(features::CODE_EXEC, "code_exec");
}

// ═══════════════════════════════════════════════════════════════════════
//  6. MappingRegistry — basics
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_new_is_empty() {
    let reg = MappingRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
}

#[test]
fn registry_default_is_empty() {
    let reg = MappingRegistry::default();
    assert!(reg.is_empty());
}

#[test]
fn registry_insert_and_lookup() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    let rule = reg.lookup(Dialect::OpenAi, Dialect::Claude, "tool_use");
    assert!(rule.is_some());
    assert!(rule.unwrap().fidelity.is_lossless());
}

#[test]
fn registry_lookup_returns_none_for_missing() {
    let reg = MappingRegistry::new();
    assert!(
        reg.lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
            .is_none()
    );
}

#[test]
fn registry_insert_replaces_existing() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::LossyLabeled {
            warning: "changed".into(),
        },
    ));
    assert_eq!(reg.len(), 1);
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
}

#[test]
fn registry_len_counts_distinct_keys() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "a",
        Fidelity::Lossless,
    ));
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "b",
        Fidelity::Lossless,
    ));
    assert_eq!(reg.len(), 2);
}

#[test]
fn registry_is_empty_after_insert_is_false() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "x",
        Fidelity::Lossless,
    ));
    assert!(!reg.is_empty());
}

#[test]
fn registry_iter_count_matches_len() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "a",
        Fidelity::Lossless,
    ));
    reg.insert(make_rule(
        Dialect::Claude,
        Dialect::Gemini,
        "b",
        Fidelity::Lossless,
    ));
    assert_eq!(reg.iter().count(), reg.len());
}

#[test]
fn registry_multiple_features_same_pair() {
    let mut reg = MappingRegistry::new();
    for feat in all_features() {
        reg.insert(make_rule(
            Dialect::OpenAi,
            Dialect::Claude,
            feat,
            Fidelity::Lossless,
        ));
    }
    assert_eq!(reg.len(), 5);
    for feat in all_features() {
        assert!(reg.lookup(Dialect::OpenAi, Dialect::Claude, feat).is_some());
    }
}

#[test]
fn registry_same_feature_multiple_pairs() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Gemini,
        "tool_use",
        Fidelity::Lossless,
    ));
    assert_eq!(reg.len(), 2);
}

#[test]
fn registry_direction_matters() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    // Reverse direction should not exist
    assert!(
        reg.lookup(Dialect::Claude, Dialect::OpenAi, "tool_use")
            .is_none()
    );
}

#[test]
fn registry_clone() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    let reg2 = reg.clone();
    assert_eq!(reg2.len(), 1);
    assert!(
        reg2.lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
            .is_some()
    );
}

#[test]
fn registry_debug_not_empty() {
    let reg = MappingRegistry::new();
    let dbg = format!("{:?}", reg);
    assert!(!dbg.is_empty());
}

// ── rank_targets ────────────────────────────────────────────────────────

#[test]
fn registry_rank_targets_basic() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Gemini,
        "tool_use",
        Fidelity::LossyLabeled {
            warning: "w".into(),
        },
    ));
    let ranked = reg.rank_targets(Dialect::OpenAi, &["tool_use"]);
    assert_eq!(ranked.len(), 2);
    // Claude (1 lossless) should come before Gemini (0 lossless)
    assert_eq!(ranked[0].0, Dialect::Claude);
    assert_eq!(ranked[0].1, 1);
    assert_eq!(ranked[1].0, Dialect::Gemini);
    assert_eq!(ranked[1].1, 0);
}

#[test]
fn registry_rank_targets_excludes_source() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::OpenAi,
        "tool_use",
        Fidelity::Lossless,
    ));
    let ranked = reg.rank_targets(Dialect::OpenAi, &["tool_use"]);
    // Should not include self
    assert!(ranked.iter().all(|(d, _)| *d != Dialect::OpenAi));
}

#[test]
fn registry_rank_targets_excludes_all_unsupported() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Codex,
        "image_input",
        Fidelity::Unsupported {
            reason: "nope".into(),
        },
    ));
    let ranked = reg.rank_targets(Dialect::OpenAi, &["image_input"]);
    assert!(ranked.is_empty());
}

#[test]
fn registry_rank_targets_empty_features() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[]);
    assert!(ranked.is_empty());
}

#[test]
fn registry_rank_targets_no_rules() {
    let reg = MappingRegistry::new();
    let ranked = reg.rank_targets(Dialect::OpenAi, &["tool_use"]);
    assert!(ranked.is_empty());
}

#[test]
fn registry_rank_targets_sorted_descending() {
    let mut reg = MappingRegistry::new();
    // Claude: 2 lossless
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "streaming",
        Fidelity::Lossless,
    ));
    // Gemini: 1 lossless
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Gemini,
        "tool_use",
        Fidelity::Lossless,
    ));
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Gemini,
        "streaming",
        Fidelity::LossyLabeled {
            warning: "w".into(),
        },
    ));
    let ranked = reg.rank_targets(Dialect::OpenAi, &["tool_use", "streaming"]);
    assert!(ranked[0].1 >= ranked[1].1);
    assert_eq!(ranked[0].0, Dialect::Claude);
}

// ═══════════════════════════════════════════════════════════════════════
//  7. MappingMatrix
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn matrix_new_get_returns_none() {
    let m = MappingMatrix::new();
    assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), None);
}

#[test]
fn matrix_default_is_empty() {
    let m = MappingMatrix::default();
    assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn matrix_set_and_get_true() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, true);
    assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), Some(true));
}

#[test]
fn matrix_set_and_get_false() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, false);
    assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), Some(false));
}

#[test]
fn matrix_is_supported_unset_returns_false() {
    let m = MappingMatrix::new();
    assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn matrix_is_supported_set_true() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, true);
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn matrix_is_supported_set_false() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, false);
    assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn matrix_overwrite() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, true);
    m.set(Dialect::OpenAi, Dialect::Claude, false);
    assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn matrix_is_directional() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, true);
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
    assert!(!m.is_supported(Dialect::Claude, Dialect::OpenAi));
}

#[test]
fn matrix_clone() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, true);
    let m2 = m.clone();
    assert!(m2.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn matrix_debug_not_empty() {
    let m = MappingMatrix::new();
    let dbg = format!("{:?}", m);
    assert!(!dbg.is_empty());
}

// ── Matrix from_registry ────────────────────────────────────────────────

#[test]
fn matrix_from_registry_lossless_is_supported() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    let m = MappingMatrix::from_registry(&reg);
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn matrix_from_registry_lossy_is_supported() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Codex,
        "tool_use",
        Fidelity::LossyLabeled {
            warning: "w".into(),
        },
    ));
    let m = MappingMatrix::from_registry(&reg);
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Codex));
}

#[test]
fn matrix_from_registry_unsupported_not_marked() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::Gemini,
        Dialect::Codex,
        "image_input",
        Fidelity::Unsupported {
            reason: "nope".into(),
        },
    ));
    let m = MappingMatrix::from_registry(&reg);
    assert!(!m.is_supported(Dialect::Gemini, Dialect::Codex));
}

#[test]
fn matrix_from_registry_empty() {
    let reg = MappingRegistry::new();
    let m = MappingMatrix::from_registry(&reg);
    assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn matrix_from_registry_mixed_rules() {
    let mut reg = MappingRegistry::new();
    // One unsupported and one lossless for the same pair → supported
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Codex,
        "image_input",
        Fidelity::Unsupported {
            reason: "nope".into(),
        },
    ));
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Codex,
        "streaming",
        Fidelity::Lossless,
    ));
    let m = MappingMatrix::from_registry(&reg);
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Codex));
}

// ═══════════════════════════════════════════════════════════════════════
//  8. validate_mapping
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_lossless_feature() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "streaming",
        Fidelity::Lossless,
    ));
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
    reg.insert(make_rule(
        Dialect::Claude,
        Dialect::OpenAi,
        "thinking",
        Fidelity::LossyLabeled {
            warning: "mapped to system".into(),
        },
    ));
    let results = validate_mapping(&reg, Dialect::Claude, Dialect::OpenAi, &["thinking".into()]);
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
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Codex,
        "image_input",
        Fidelity::Unsupported {
            reason: "no images".into(),
        },
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
fn validate_unknown_feature_returns_unsupported() {
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
fn validate_empty_features_list_returns_empty() {
    let reg = MappingRegistry::new();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[]);
    assert!(results.is_empty());
}

#[test]
fn validate_multiple_features() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "streaming",
        Fidelity::Lossless,
    ));
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

#[test]
fn validate_preserves_feature_order() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "a",
        Fidelity::Lossless,
    ));
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "b",
        Fidelity::Lossless,
    ));
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &["b".into(), "a".into()],
    );
    assert_eq!(results[0].feature, "b");
    assert_eq!(results[1].feature, "a");
}

#[test]
fn validate_duplicate_features_returns_both() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
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
fn validate_mixed_fidelities() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Codex,
        "streaming",
        Fidelity::Lossless,
    ));
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Codex,
        "tool_use",
        Fidelity::LossyLabeled {
            warning: "w".into(),
        },
    ));
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Codex,
        "image_input",
        Fidelity::Unsupported { reason: "r".into() },
    ));
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &["streaming".into(), "tool_use".into(), "image_input".into()],
    );
    assert!(results[0].fidelity.is_lossless());
    assert!(!results[1].fidelity.is_lossless());
    assert!(!results[1].fidelity.is_unsupported());
    assert!(results[2].fidelity.is_unsupported());
}

#[test]
fn validate_unknown_feature_error_contains_name() {
    let reg = MappingRegistry::new();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &["teleportation".into()],
    );
    if let MappingError::FeatureUnsupported { feature, .. } = &results[0].errors[0] {
        assert_eq!(feature, "teleportation");
    } else {
        panic!("expected FeatureUnsupported");
    }
}

#[test]
fn validate_lossy_error_preserves_warning() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::Claude,
        Dialect::OpenAi,
        "thinking",
        Fidelity::LossyLabeled {
            warning: "test-warning-123".into(),
        },
    ));
    let results = validate_mapping(&reg, Dialect::Claude, Dialect::OpenAi, &["thinking".into()]);
    if let MappingError::FidelityLoss { warning, .. } = &results[0].errors[0] {
        assert_eq!(warning, "test-warning-123");
    } else {
        panic!("expected FidelityLoss");
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  9. known_rules — basics
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn known_rules_non_empty() {
    let reg = known_rules();
    assert!(!reg.is_empty());
}

#[test]
fn known_rules_has_substantial_count() {
    let reg = known_rules();
    // At least 30 same-dialect + many cross-dialect
    assert!(reg.len() > 100);
}

#[test]
fn known_rules_matrix_has_entries() {
    let reg = known_rules();
    let m = MappingMatrix::from_registry(&reg);
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
    assert!(m.is_supported(Dialect::Claude, Dialect::Gemini));
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Gemini));
}

// ── Same-dialect: all features lossless ─────────────────────────────────

#[test]
fn known_rules_openai_self_all_lossless() {
    let reg = known_rules();
    for feat in all_features() {
        let rule = reg
            .lookup(Dialect::OpenAi, Dialect::OpenAi, feat)
            .unwrap_or_else(|| panic!("missing rule: OpenAI->OpenAI {feat}"));
        assert!(
            rule.fidelity.is_lossless(),
            "OpenAI->OpenAI {feat} should be lossless"
        );
    }
}

#[test]
fn known_rules_claude_self_all_lossless() {
    let reg = known_rules();
    for feat in all_features() {
        let rule = reg
            .lookup(Dialect::Claude, Dialect::Claude, feat)
            .unwrap_or_else(|| panic!("missing rule: Claude->Claude {feat}"));
        assert!(rule.fidelity.is_lossless());
    }
}

#[test]
fn known_rules_gemini_self_all_lossless() {
    let reg = known_rules();
    for feat in all_features() {
        let rule = reg
            .lookup(Dialect::Gemini, Dialect::Gemini, feat)
            .unwrap_or_else(|| panic!("missing rule: Gemini->Gemini {feat}"));
        assert!(rule.fidelity.is_lossless());
    }
}

#[test]
fn known_rules_codex_self_all_lossless() {
    let reg = known_rules();
    for feat in all_features() {
        let rule = reg
            .lookup(Dialect::Codex, Dialect::Codex, feat)
            .unwrap_or_else(|| panic!("missing rule: Codex->Codex {feat}"));
        assert!(rule.fidelity.is_lossless());
    }
}

#[test]
fn known_rules_kimi_self_all_lossless() {
    let reg = known_rules();
    for feat in all_features() {
        let rule = reg
            .lookup(Dialect::Kimi, Dialect::Kimi, feat)
            .unwrap_or_else(|| panic!("missing rule: Kimi->Kimi {feat}"));
        assert!(rule.fidelity.is_lossless());
    }
}

#[test]
fn known_rules_copilot_self_all_lossless() {
    let reg = known_rules();
    for feat in all_features() {
        let rule = reg
            .lookup(Dialect::Copilot, Dialect::Copilot, feat)
            .unwrap_or_else(|| panic!("missing rule: Copilot->Copilot {feat}"));
        assert!(rule.fidelity.is_lossless());
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  10. known_rules — tool_use cross-dialect
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn known_tool_use_openai_claude_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_tool_use_claude_openai_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Claude, Dialect::OpenAi, features::TOOL_USE)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_tool_use_openai_gemini_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::OpenAi, Dialect::Gemini, features::TOOL_USE)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_tool_use_claude_gemini_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Claude, Dialect::Gemini, features::TOOL_USE)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_tool_use_openai_codex_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::OpenAi, Dialect::Codex, features::TOOL_USE)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
    assert!(!r.fidelity.is_unsupported());
}

#[test]
fn known_tool_use_codex_openai_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Codex, Dialect::OpenAi, features::TOOL_USE)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
    assert!(!r.fidelity.is_unsupported());
}

#[test]
fn known_tool_use_codex_claude_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Codex, Dialect::Claude, features::TOOL_USE)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
}

#[test]
fn known_tool_use_codex_gemini_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Codex, Dialect::Gemini, features::TOOL_USE)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
}

#[test]
fn known_tool_use_kimi_openai_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Kimi, Dialect::OpenAi, features::TOOL_USE)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_tool_use_kimi_claude_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Kimi, Dialect::Claude, features::TOOL_USE)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_tool_use_kimi_codex_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Kimi, Dialect::Codex, features::TOOL_USE)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
}

#[test]
fn known_tool_use_copilot_openai_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Copilot, Dialect::OpenAi, features::TOOL_USE)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_tool_use_copilot_codex_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Copilot, Dialect::Codex, features::TOOL_USE)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
}

#[test]
fn known_tool_use_kimi_copilot_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::TOOL_USE)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_tool_use_copilot_kimi_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Copilot, Dialect::Kimi, features::TOOL_USE)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

// ═══════════════════════════════════════════════════════════════════════
//  11. known_rules — streaming (all lossless)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn known_streaming_all_cross_dialect_lossless() {
    let reg = known_rules();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            let rule = reg
                .lookup(src, tgt, features::STREAMING)
                .unwrap_or_else(|| panic!("missing rule: {src}->{tgt} streaming"));
            assert!(
                rule.fidelity.is_lossless(),
                "streaming {src}->{tgt} should be lossless"
            );
        }
    }
}

#[test]
fn known_streaming_openai_claude_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::STREAMING)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_streaming_kimi_copilot_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::STREAMING)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_streaming_codex_gemini_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Codex, Dialect::Gemini, features::STREAMING)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

// ═══════════════════════════════════════════════════════════════════════
//  12. known_rules — thinking (all cross-dialect lossy)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn known_thinking_all_cross_dialect_lossy() {
    let reg = known_rules();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            if src == tgt {
                continue;
            }
            let rule = reg
                .lookup(src, tgt, features::THINKING)
                .unwrap_or_else(|| panic!("missing rule: {src}->{tgt} thinking"));
            assert!(
                !rule.fidelity.is_lossless(),
                "thinking {src}->{tgt} should not be lossless"
            );
            assert!(
                !rule.fidelity.is_unsupported(),
                "thinking {src}->{tgt} should not be unsupported (should be lossy)"
            );
        }
    }
}

#[test]
fn known_thinking_claude_openai_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
}

#[test]
fn known_thinking_openai_claude_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::THINKING)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
}

#[test]
fn known_thinking_kimi_copilot_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::THINKING)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
}

#[test]
fn known_thinking_copilot_claude_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Copilot, Dialect::Claude, features::THINKING)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
}

#[test]
fn known_thinking_gemini_codex_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Gemini, Dialect::Codex, features::THINKING)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
}

// ═══════════════════════════════════════════════════════════════════════
//  13. known_rules — image_input
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn known_image_openai_claude_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::IMAGE_INPUT)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_image_claude_openai_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Claude, Dialect::OpenAi, features::IMAGE_INPUT)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_image_openai_gemini_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::OpenAi, Dialect::Gemini, features::IMAGE_INPUT)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_image_claude_gemini_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Claude, Dialect::Gemini, features::IMAGE_INPUT)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_image_to_codex_all_unsupported() {
    let reg = known_rules();
    for &src in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
        let r = reg
            .lookup(src, Dialect::Codex, features::IMAGE_INPUT)
            .unwrap();
        assert!(
            r.fidelity.is_unsupported(),
            "image_input {src}->Codex should be unsupported"
        );
    }
}

#[test]
fn known_image_from_codex_all_unsupported() {
    let reg = known_rules();
    for &tgt in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
        let r = reg
            .lookup(Dialect::Codex, tgt, features::IMAGE_INPUT)
            .unwrap();
        assert!(
            r.fidelity.is_unsupported(),
            "image_input Codex->{tgt} should be unsupported"
        );
    }
}

#[test]
fn known_image_kimi_to_all_unsupported() {
    let reg = known_rules();
    for &tgt in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ] {
        let r = reg
            .lookup(Dialect::Kimi, tgt, features::IMAGE_INPUT)
            .unwrap();
        assert!(
            r.fidelity.is_unsupported(),
            "image_input Kimi->{tgt} should be unsupported"
        );
    }
}

#[test]
fn known_image_copilot_to_all_unsupported() {
    let reg = known_rules();
    for &tgt in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ] {
        let r = reg
            .lookup(Dialect::Copilot, tgt, features::IMAGE_INPUT)
            .unwrap();
        assert!(
            r.fidelity.is_unsupported(),
            "image_input Copilot->{tgt} should be unsupported"
        );
    }
}

#[test]
fn known_image_to_kimi_unsupported() {
    let reg = known_rules();
    for &src in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
        let r = reg
            .lookup(src, Dialect::Kimi, features::IMAGE_INPUT)
            .unwrap();
        assert!(r.fidelity.is_unsupported());
    }
}

#[test]
fn known_image_to_copilot_unsupported() {
    let reg = known_rules();
    for &src in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
        let r = reg
            .lookup(src, Dialect::Copilot, features::IMAGE_INPUT)
            .unwrap();
        assert!(r.fidelity.is_unsupported());
    }
}

#[test]
fn known_image_kimi_copilot_unsupported() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::IMAGE_INPUT)
        .unwrap();
    assert!(r.fidelity.is_unsupported());
}

#[test]
fn known_image_copilot_kimi_unsupported() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Copilot, Dialect::Kimi, features::IMAGE_INPUT)
        .unwrap();
    assert!(r.fidelity.is_unsupported());
}

// ═══════════════════════════════════════════════════════════════════════
//  14. known_rules — code_exec
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn known_code_exec_cross_code_capable_lossy() {
    let reg = known_rules();
    let code_capable = [
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Copilot,
    ];
    for &src in &code_capable {
        for &tgt in &code_capable {
            if src == tgt {
                continue;
            }
            let r = reg
                .lookup(src, tgt, features::CODE_EXEC)
                .unwrap_or_else(|| panic!("missing code_exec: {src}->{tgt}"));
            assert!(
                !r.fidelity.is_lossless(),
                "code_exec {src}->{tgt} should be lossy"
            );
            assert!(
                !r.fidelity.is_unsupported(),
                "code_exec {src}->{tgt} should not be unsupported"
            );
        }
    }
}

#[test]
fn known_code_exec_kimi_to_others_unsupported() {
    let reg = known_rules();
    let code_capable = [
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Copilot,
    ];
    for &tgt in &code_capable {
        let r = reg
            .lookup(Dialect::Kimi, tgt, features::CODE_EXEC)
            .unwrap_or_else(|| panic!("missing code_exec: Kimi->{tgt}"));
        assert!(
            r.fidelity.is_unsupported(),
            "code_exec Kimi->{tgt} should be unsupported"
        );
    }
}

#[test]
fn known_code_exec_others_to_kimi_unsupported() {
    let reg = known_rules();
    let code_capable = [
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Copilot,
    ];
    for &src in &code_capable {
        let r = reg
            .lookup(src, Dialect::Kimi, features::CODE_EXEC)
            .unwrap_or_else(|| panic!("missing code_exec: {src}->Kimi"));
        assert!(
            r.fidelity.is_unsupported(),
            "code_exec {src}->Kimi should be unsupported"
        );
    }
}

#[test]
fn known_code_exec_openai_claude_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::CODE_EXEC)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
    assert!(!r.fidelity.is_unsupported());
}

#[test]
fn known_code_exec_copilot_codex_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Copilot, Dialect::Codex, features::CODE_EXEC)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
    assert!(!r.fidelity.is_unsupported());
}

// ═══════════════════════════════════════════════════════════════════════
//  15. Cross-dialect mapping completeness
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn known_rules_every_dialect_pair_has_tool_use() {
    let reg = known_rules();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            assert!(
                reg.lookup(src, tgt, features::TOOL_USE).is_some(),
                "missing tool_use: {src}->{tgt}"
            );
        }
    }
}

#[test]
fn known_rules_every_dialect_pair_has_streaming() {
    let reg = known_rules();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            assert!(
                reg.lookup(src, tgt, features::STREAMING).is_some(),
                "missing streaming: {src}->{tgt}"
            );
        }
    }
}

#[test]
fn known_rules_every_dialect_pair_has_thinking() {
    let reg = known_rules();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            assert!(
                reg.lookup(src, tgt, features::THINKING).is_some(),
                "missing thinking: {src}->{tgt}"
            );
        }
    }
}

#[test]
fn known_rules_every_dialect_pair_has_image_input() {
    let reg = known_rules();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            assert!(
                reg.lookup(src, tgt, features::IMAGE_INPUT).is_some(),
                "missing image_input: {src}->{tgt}"
            );
        }
    }
}

#[test]
fn known_rules_every_dialect_pair_has_code_exec() {
    let reg = known_rules();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            assert!(
                reg.lookup(src, tgt, features::CODE_EXEC).is_some(),
                "missing code_exec: {src}->{tgt}"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  16. validate_mapping with known_rules
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_known_openai_to_claude_all_features() {
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
            features::CODE_EXEC.into(),
        ],
    );
    assert_eq!(results.len(), 5);
    // tool_use: lossless
    assert!(results[0].fidelity.is_lossless());
    assert!(results[0].errors.is_empty());
    // streaming: lossless
    assert!(results[1].fidelity.is_lossless());
    assert!(results[1].errors.is_empty());
    // thinking: lossy
    assert!(!results[2].fidelity.is_lossless());
    assert!(!results[2].errors.is_empty());
    // image_input: lossless
    assert!(results[3].fidelity.is_lossless());
    // code_exec: lossy
    assert!(!results[4].fidelity.is_lossless());
}

#[test]
fn validate_known_openai_to_codex() {
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
fn validate_known_kimi_to_openai() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Kimi,
        Dialect::OpenAi,
        &[
            features::TOOL_USE.into(),
            features::STREAMING.into(),
            features::CODE_EXEC.into(),
        ],
    );
    // tool_use: lossless
    assert!(results[0].fidelity.is_lossless());
    // streaming: lossless
    assert!(results[1].fidelity.is_lossless());
    // code_exec: unsupported (Kimi doesn't support code execution)
    assert!(results[2].fidelity.is_unsupported());
}

#[test]
fn validate_known_with_unknown_feature() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &["tool_use".into(), "not_a_feature".into()],
    );
    assert_eq!(results.len(), 2);
    assert!(results[0].errors.is_empty());
    assert!(!results[1].errors.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
//  17. Matrix from known_rules
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn known_matrix_all_dialect_pairs_supported() {
    let reg = known_rules();
    let m = MappingMatrix::from_registry(&reg);
    // Every pair should be supported because streaming is always lossless
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            if src == tgt {
                continue;
            }
            assert!(
                m.is_supported(src, tgt),
                "{src}->{tgt} should be supported in known matrix"
            );
        }
    }
}

#[test]
fn known_matrix_same_dialect_supported() {
    let reg = known_rules();
    let m = MappingMatrix::from_registry(&reg);
    for &d in Dialect::all() {
        assert!(
            m.is_supported(d, d),
            "{d}->{d} should be supported in known matrix"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  18. rank_targets with known_rules
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn known_rank_openai_streaming_all_lossless() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::STREAMING]);
    // All targets should have 1 lossless
    for (_, count) in &ranked {
        assert_eq!(*count, 1);
    }
}

#[test]
fn known_rank_openai_tool_use_claude_preferred_over_codex() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE]);
    // Claude should be ranked above Codex (lossless vs lossy)
    let claude_pos = ranked.iter().position(|(d, _)| *d == Dialect::Claude);
    let codex_pos = ranked.iter().position(|(d, _)| *d == Dialect::Codex);
    assert!(claude_pos.unwrap() < codex_pos.unwrap());
}

#[test]
fn known_rank_excludes_source() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE, features::STREAMING]);
    assert!(ranked.iter().all(|(d, _)| *d != Dialect::OpenAi));
}

// ═══════════════════════════════════════════════════════════════════════
//  19. Determinism
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn deterministic_known_rules_len() {
    let r1 = known_rules();
    let r2 = known_rules();
    assert_eq!(r1.len(), r2.len());
}

#[test]
fn deterministic_validate_same_result() {
    let reg = known_rules();
    let feats: Vec<String> = all_features().iter().map(|s| s.to_string()).collect();
    let r1 = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
    let r2 = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
    assert_eq!(r1, r2);
}

#[test]
fn deterministic_matrix_from_registry() {
    let reg = known_rules();
    let m1 = MappingMatrix::from_registry(&reg);
    let m2 = MappingMatrix::from_registry(&reg);
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            assert_eq!(m1.is_supported(src, tgt), m2.is_supported(src, tgt));
        }
    }
}

#[test]
fn deterministic_rank_targets() {
    let reg = known_rules();
    let feats: Vec<&str> = all_features().to_vec();
    let r1 = reg.rank_targets(Dialect::OpenAi, &feats);
    let r2 = reg.rank_targets(Dialect::OpenAi, &feats);
    assert_eq!(r1, r2);
}

// ═══════════════════════════════════════════════════════════════════════
//  20. Edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_handles_long_feature_name() {
    let mut reg = MappingRegistry::new();
    let long_name = "a".repeat(10000);
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        &long_name,
        Fidelity::Lossless,
    ));
    assert!(
        reg.lookup(Dialect::OpenAi, Dialect::Claude, &long_name)
            .is_some()
    );
}

#[test]
fn registry_handles_special_chars_in_feature() {
    let mut reg = MappingRegistry::new();
    let feat = "special-chars_with.dots/slashes";
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        feat,
        Fidelity::Lossless,
    ));
    assert!(reg.lookup(Dialect::OpenAi, Dialect::Claude, feat).is_some());
}

#[test]
fn validate_many_features_at_once() {
    let reg = known_rules();
    let features: Vec<String> = (0..100).map(|i| format!("feature_{i}")).collect();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &features);
    assert_eq!(results.len(), 100);
    // All should be unsupported since they're synthetic names
    for r in &results {
        assert!(r.fidelity.is_unsupported());
    }
}

#[test]
fn registry_insert_all_dialect_pairs_all_features() {
    let mut reg = MappingRegistry::new();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            for feat in all_features() {
                reg.insert(make_rule(src, tgt, feat, Fidelity::Lossless));
            }
        }
    }
    // 6 dialects × 6 dialects × 5 features = 180
    assert_eq!(reg.len(), 180);
}

#[test]
fn matrix_set_all_pairs() {
    let mut m = MappingMatrix::new();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            m.set(src, tgt, true);
        }
    }
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            assert!(m.is_supported(src, tgt));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  21. Specific warning messages in known_rules
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn known_thinking_claude_to_openai_warning_mentions_system() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING)
        .unwrap();
    if let Fidelity::LossyLabeled { warning } = &r.fidelity {
        assert!(
            warning.contains("system"),
            "expected warning to mention 'system', got: {warning}"
        );
    } else {
        panic!("expected LossyLabeled");
    }
}

#[test]
fn known_image_openai_codex_reason_mentions_codex() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::OpenAi, Dialect::Codex, features::IMAGE_INPUT)
        .unwrap();
    if let Fidelity::Unsupported { reason } = &r.fidelity {
        assert!(
            reason.contains("Codex"),
            "expected reason to mention 'Codex', got: {reason}"
        );
    } else {
        panic!("expected Unsupported");
    }
}

#[test]
fn known_code_exec_kimi_reason_mentions_kimi() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Kimi, Dialect::OpenAi, features::CODE_EXEC)
        .unwrap();
    if let Fidelity::Unsupported { reason } = &r.fidelity {
        assert!(
            reason.contains("Kimi"),
            "expected reason to mention 'Kimi', got: {reason}"
        );
    } else {
        panic!("expected Unsupported");
    }
}

#[test]
fn known_tool_use_openai_codex_warning_mentions_codex() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::OpenAi, Dialect::Codex, features::TOOL_USE)
        .unwrap();
    if let Fidelity::LossyLabeled { warning } = &r.fidelity {
        assert!(
            warning.contains("Codex"),
            "expected warning to mention 'Codex', got: {warning}"
        );
    } else {
        panic!("expected LossyLabeled");
    }
}

#[test]
fn known_thinking_kimi_openai_warning_mentions_kimi() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Kimi, Dialect::OpenAi, features::THINKING)
        .unwrap();
    if let Fidelity::LossyLabeled { warning } = &r.fidelity {
        assert!(
            warning.contains("Kimi"),
            "expected warning to mention 'Kimi', got: {warning}"
        );
    } else {
        panic!("expected LossyLabeled");
    }
}

#[test]
fn known_thinking_copilot_gemini_warning_mentions_copilot() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Copilot, Dialect::Gemini, features::THINKING)
        .unwrap();
    if let Fidelity::LossyLabeled { warning } = &r.fidelity {
        assert!(
            warning.contains("Copilot"),
            "expected warning to mention 'Copilot', got: {warning}"
        );
    } else {
        panic!("expected LossyLabeled");
    }
}

#[test]
fn known_image_kimi_copilot_reason_mentions_neither() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::IMAGE_INPUT)
        .unwrap();
    if let Fidelity::Unsupported { reason } = &r.fidelity {
        assert!(
            reason.contains("neither"),
            "expected reason to mention 'neither', got: {reason}"
        );
    } else {
        panic!("expected Unsupported");
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  22. Additional cross-dialect tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn known_tool_use_gemini_claude_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Gemini, Dialect::Claude, features::TOOL_USE)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_tool_use_gemini_openai_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Gemini, Dialect::OpenAi, features::TOOL_USE)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_tool_use_claude_codex_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Claude, Dialect::Codex, features::TOOL_USE)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
    assert!(!r.fidelity.is_unsupported());
}

#[test]
fn known_tool_use_gemini_codex_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Gemini, Dialect::Codex, features::TOOL_USE)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
    assert!(!r.fidelity.is_unsupported());
}

#[test]
fn known_image_gemini_claude_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Gemini, Dialect::Claude, features::IMAGE_INPUT)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_image_gemini_openai_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Gemini, Dialect::OpenAi, features::IMAGE_INPUT)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_code_exec_openai_gemini_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::OpenAi, Dialect::Gemini, features::CODE_EXEC)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
    assert!(!r.fidelity.is_unsupported());
}

#[test]
fn known_code_exec_copilot_claude_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Copilot, Dialect::Claude, features::CODE_EXEC)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
    assert!(!r.fidelity.is_unsupported());
}

#[test]
fn known_thinking_codex_claude_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Codex, Dialect::Claude, features::THINKING)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
    assert!(!r.fidelity.is_unsupported());
}

#[test]
fn known_thinking_gemini_claude_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Gemini, Dialect::Claude, features::THINKING)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
}

// ═══════════════════════════════════════════════════════════════════════
//  23. Error type / message tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn error_feature_unsupported_is_an_error() {
    let err = MappingError::FeatureUnsupported {
        feature: "x".into(),
        from: Dialect::OpenAi,
        to: Dialect::Claude,
    };
    // Verify it implements std::error::Error via Display
    let _msg: String = err.to_string();
}

#[test]
fn error_invalid_input_contains_reason() {
    let err = MappingError::InvalidInput {
        reason: "test reason".into(),
    };
    assert!(err.to_string().contains("test reason"));
}

#[test]
fn error_fidelity_loss_contains_feature_and_warning() {
    let err = MappingError::FidelityLoss {
        feature: "my_feature".into(),
        warning: "my_warning".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("my_feature"));
    assert!(msg.contains("my_warning"));
}

#[test]
fn error_dialect_mismatch_mentions_both() {
    let err = MappingError::DialectMismatch {
        from: Dialect::Kimi,
        to: Dialect::Copilot,
    };
    let msg = err.to_string();
    assert!(msg.contains("Kimi"));
    assert!(msg.contains("Copilot"));
}

// ═══════════════════════════════════════════════════════════════════════
//  24. Additional validation edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_returns_feature_name_in_each_result() {
    let reg = known_rules();
    let features = vec!["tool_use".to_string(), "streaming".to_string()];
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &features);
    assert_eq!(results[0].feature, "tool_use");
    assert_eq!(results[1].feature, "streaming");
}

#[test]
fn validate_unsupported_reason_mentions_feature_name() {
    let reg = MappingRegistry::new();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &["my_special_feature".into()],
    );
    if let Fidelity::Unsupported { reason } = &results[0].fidelity {
        assert!(
            reason.contains("my_special_feature"),
            "expected reason to mention feature, got: {reason}"
        );
    } else {
        panic!("expected Unsupported");
    }
}

#[test]
fn validate_empty_name_fidelity_is_unsupported() {
    let reg = known_rules();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);
    assert!(results[0].fidelity.is_unsupported());
}

#[test]
fn validate_empty_name_error_is_invalid_input() {
    let reg = known_rules();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);
    assert!(matches!(
        &results[0].errors[0],
        MappingError::InvalidInput { reason } if reason.contains("empty")
    ));
}

// ═══════════════════════════════════════════════════════════════════════
//  25. Serde roundtrip for full workflow
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn mapping_validation_full_roundtrip() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &["tool_use".into(), "thinking".into()],
    );
    for v in &results {
        let json = serde_json::to_string(v).unwrap();
        let v2: MappingValidation = serde_json::from_str(&json).unwrap();
        assert_eq!(*v, v2);
    }
}

#[test]
fn mapping_rule_json_has_expected_fields() {
    let rule = make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    );
    let json = serde_json::to_string(&rule).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.get("source_dialect").is_some());
    assert!(parsed.get("target_dialect").is_some());
    assert!(parsed.get("feature").is_some());
    assert!(parsed.get("fidelity").is_some());
}

#[test]
fn mapping_error_all_variants_are_serializable() {
    let errors = vec![
        MappingError::FeatureUnsupported {
            feature: "x".into(),
            from: Dialect::OpenAi,
            to: Dialect::Claude,
        },
        MappingError::FidelityLoss {
            feature: "y".into(),
            warning: "w".into(),
        },
        MappingError::DialectMismatch {
            from: Dialect::Gemini,
            to: Dialect::Codex,
        },
        MappingError::InvalidInput { reason: "r".into() },
    ];
    for err in &errors {
        let json = serde_json::to_string(err).unwrap();
        let err2: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(*err, err2);
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  26. Copilot<->Codex tool_use bidirectional
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn known_tool_use_codex_copilot_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Codex, Dialect::Copilot, features::TOOL_USE)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
    assert!(!r.fidelity.is_unsupported());
}

#[test]
fn known_tool_use_copilot_gemini_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Copilot, Dialect::Gemini, features::TOOL_USE)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_tool_use_copilot_claude_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Copilot, Dialect::Claude, features::TOOL_USE)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_tool_use_kimi_gemini_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Kimi, Dialect::Gemini, features::TOOL_USE)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

// ═══════════════════════════════════════════════════════════════════════
//  27. Kimi/Copilot-specific thinking warnings
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn known_thinking_kimi_copilot_warning_mentions_neither() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::THINKING)
        .unwrap();
    if let Fidelity::LossyLabeled { warning } = &r.fidelity {
        assert!(warning.contains("neither"));
    } else {
        panic!("expected LossyLabeled");
    }
}

#[test]
fn known_thinking_copilot_kimi_warning_mentions_neither() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Copilot, Dialect::Kimi, features::THINKING)
        .unwrap();
    if let Fidelity::LossyLabeled { warning } = &r.fidelity {
        assert!(warning.contains("neither"));
    } else {
        panic!("expected LossyLabeled");
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  28. Code exec warning messages
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn known_code_exec_warning_mentions_both_dialects() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::CODE_EXEC)
        .unwrap();
    if let Fidelity::LossyLabeled { warning } = &r.fidelity {
        assert!(warning.contains("OpenAI"));
        assert!(warning.contains("Claude"));
    } else {
        panic!("expected LossyLabeled");
    }
}

#[test]
fn known_code_exec_kimi_unsupported_reason() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Kimi, Dialect::Claude, features::CODE_EXEC)
        .unwrap();
    if let Fidelity::Unsupported { reason } = &r.fidelity {
        assert!(reason.contains("Kimi"));
    } else {
        panic!("expected Unsupported");
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  29. Registry rank_targets with known rules
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn known_rank_targets_all_features_openai() {
    let reg = known_rules();
    let feats: Vec<&str> = all_features().to_vec();
    let ranked = reg.rank_targets(Dialect::OpenAi, &feats);
    // Should have 5 targets (all except OpenAI)
    assert_eq!(ranked.len(), 5);
    // Claude and Gemini should be first (most lossless features)
    let top = ranked[0].0;
    assert!(
        top == Dialect::Claude || top == Dialect::Gemini,
        "expected Claude or Gemini at top, got {top}"
    );
}

#[test]
fn known_rank_targets_image_input_only() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::IMAGE_INPUT]);
    // Codex, Kimi, Copilot are unsupported → excluded
    // Claude and Gemini are lossless
    assert!(ranked.iter().any(|(d, _)| *d == Dialect::Claude));
    assert!(ranked.iter().any(|(d, _)| *d == Dialect::Gemini));
    assert!(!ranked.iter().any(|(d, _)| *d == Dialect::Codex));
    assert!(!ranked.iter().any(|(d, _)| *d == Dialect::Kimi));
    assert!(!ranked.iter().any(|(d, _)| *d == Dialect::Copilot));
}
