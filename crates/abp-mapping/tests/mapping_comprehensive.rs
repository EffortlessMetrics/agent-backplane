#![allow(clippy::all)]
#![allow(unknown_lints)]

use abp_dialect::Dialect;
use abp_mapping::{
    features, known_rules, validate_mapping, Fidelity, MappingError, MappingMatrix,
    MappingRegistry, MappingRule, MappingValidation,
};

// ── Helper ──────────────────────────────────────────────────────────────

fn make_rule(src: Dialect, tgt: Dialect, feat: &str, fidelity: Fidelity) -> MappingRule {
    MappingRule {
        source_dialect: src,
        target_dialect: tgt,
        feature: feat.into(),
        fidelity,
    }
}

// ════════════════════════════════════════════════════════════════════════
//  1. MappingError — construction, Display, equality, serialization
// ════════════════════════════════════════════════════════════════════════

#[test]
fn error_feature_unsupported_display_contains_feature() {
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

#[test]
fn error_fidelity_loss_display() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "mapped to system message".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("thinking"));
    assert!(msg.contains("mapped to system message"));
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
    assert!(err.to_string().contains("empty feature name"));
}

#[test]
fn error_feature_unsupported_eq() {
    let a = MappingError::FeatureUnsupported {
        feature: "x".into(),
        from: Dialect::OpenAi,
        to: Dialect::Claude,
    };
    let b = MappingError::FeatureUnsupported {
        feature: "x".into(),
        from: Dialect::OpenAi,
        to: Dialect::Claude,
    };
    assert_eq!(a, b);
}

#[test]
fn error_different_variants_not_eq() {
    let a = MappingError::InvalidInput {
        reason: "bad".into(),
    };
    let b = MappingError::FidelityLoss {
        feature: "bad".into(),
        warning: "bad".into(),
    };
    assert_ne!(a, b);
}

#[test]
fn error_feature_unsupported_serde_roundtrip() {
    let err = MappingError::FeatureUnsupported {
        feature: "img".into(),
        from: Dialect::Gemini,
        to: Dialect::Codex,
    };
    let json = serde_json::to_string(&err).unwrap();
    let err2: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, err2);
}

#[test]
fn error_fidelity_loss_serde_roundtrip() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "lossy".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let err2: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, err2);
}

#[test]
fn error_dialect_mismatch_serde_roundtrip() {
    let err = MappingError::DialectMismatch {
        from: Dialect::Kimi,
        to: Dialect::Copilot,
    };
    let json = serde_json::to_string(&err).unwrap();
    let err2: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, err2);
}

#[test]
fn error_invalid_input_serde_roundtrip() {
    let err = MappingError::InvalidInput {
        reason: "nope".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let err2: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, err2);
}

#[test]
fn error_clone() {
    let err = MappingError::FeatureUnsupported {
        feature: "f".into(),
        from: Dialect::OpenAi,
        to: Dialect::Claude,
    };
    let err2 = err.clone();
    assert_eq!(err, err2);
}

#[test]
fn error_debug_format() {
    let err = MappingError::InvalidInput {
        reason: "test".into(),
    };
    let dbg = format!("{:?}", err);
    assert!(dbg.contains("InvalidInput"));
}

// ════════════════════════════════════════════════════════════════════════
//  2. Fidelity — construction, helpers, serde
// ════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_lossless_helpers() {
    let f = Fidelity::Lossless;
    assert!(f.is_lossless());
    assert!(!f.is_unsupported());
}

#[test]
fn fidelity_lossy_labeled_helpers() {
    let f = Fidelity::LossyLabeled {
        warning: "some warning".into(),
    };
    assert!(!f.is_lossless());
    assert!(!f.is_unsupported());
}

#[test]
fn fidelity_unsupported_helpers() {
    let f = Fidelity::Unsupported {
        reason: "not available".into(),
    };
    assert!(!f.is_lossless());
    assert!(f.is_unsupported());
}

#[test]
fn fidelity_lossless_serde() {
    let f = Fidelity::Lossless;
    let json = serde_json::to_string(&f).unwrap();
    assert!(json.contains("lossless"));
    let f2: Fidelity = serde_json::from_str(&json).unwrap();
    assert_eq!(f, f2);
}

#[test]
fn fidelity_lossy_labeled_serde() {
    let f = Fidelity::LossyLabeled {
        warning: "w".into(),
    };
    let json = serde_json::to_string(&f).unwrap();
    assert!(json.contains("lossy_labeled"));
    let f2: Fidelity = serde_json::from_str(&json).unwrap();
    assert_eq!(f, f2);
}

#[test]
fn fidelity_unsupported_serde() {
    let f = Fidelity::Unsupported { reason: "r".into() };
    let json = serde_json::to_string(&f).unwrap();
    assert!(json.contains("unsupported"));
    let f2: Fidelity = serde_json::from_str(&json).unwrap();
    assert_eq!(f, f2);
}

#[test]
fn fidelity_eq_same_variant() {
    assert_eq!(Fidelity::Lossless, Fidelity::Lossless);
}

#[test]
fn fidelity_ne_different_variant() {
    assert_ne!(
        Fidelity::Lossless,
        Fidelity::Unsupported { reason: "x".into() }
    );
}

#[test]
fn fidelity_clone() {
    let f = Fidelity::LossyLabeled {
        warning: "w".into(),
    };
    assert_eq!(f, f.clone());
}

#[test]
fn fidelity_debug() {
    let dbg = format!("{:?}", Fidelity::Lossless);
    assert!(dbg.contains("Lossless"));
}

// ════════════════════════════════════════════════════════════════════════
//  3. MappingRule — construction, fields, serde
// ════════════════════════════════════════════════════════════════════════

#[test]
fn rule_construction_and_fields() {
    let rule = make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "streaming",
        Fidelity::Lossless,
    );
    assert_eq!(rule.source_dialect, Dialect::OpenAi);
    assert_eq!(rule.target_dialect, Dialect::Claude);
    assert_eq!(rule.feature, "streaming");
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn rule_with_lossy_fidelity() {
    let rule = make_rule(
        Dialect::Claude,
        Dialect::OpenAi,
        "thinking",
        Fidelity::LossyLabeled {
            warning: "mapped to system".into(),
        },
    );
    assert!(!rule.fidelity.is_lossless());
    assert!(!rule.fidelity.is_unsupported());
}

#[test]
fn rule_with_unsupported_fidelity() {
    let rule = make_rule(
        Dialect::OpenAi,
        Dialect::Codex,
        "image_input",
        Fidelity::Unsupported {
            reason: "no images".into(),
        },
    );
    assert!(rule.fidelity.is_unsupported());
}

#[test]
fn rule_serde_roundtrip() {
    let rule = make_rule(
        Dialect::Gemini,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    );
    let json = serde_json::to_string(&rule).unwrap();
    let rule2: MappingRule = serde_json::from_str(&json).unwrap();
    assert_eq!(rule, rule2);
}

#[test]
fn rule_serde_roundtrip_lossy() {
    let rule = make_rule(
        Dialect::Codex,
        Dialect::OpenAi,
        "tool_use",
        Fidelity::LossyLabeled {
            warning: "schema differs".into(),
        },
    );
    let json = serde_json::to_string(&rule).unwrap();
    let rule2: MappingRule = serde_json::from_str(&json).unwrap();
    assert_eq!(rule, rule2);
}

#[test]
fn rule_eq() {
    let a = make_rule(Dialect::OpenAi, Dialect::Claude, "x", Fidelity::Lossless);
    let b = make_rule(Dialect::OpenAi, Dialect::Claude, "x", Fidelity::Lossless);
    assert_eq!(a, b);
}

#[test]
fn rule_ne_different_feature() {
    let a = make_rule(Dialect::OpenAi, Dialect::Claude, "x", Fidelity::Lossless);
    let b = make_rule(Dialect::OpenAi, Dialect::Claude, "y", Fidelity::Lossless);
    assert_ne!(a, b);
}

#[test]
fn rule_clone() {
    let rule = make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    );
    assert_eq!(rule, rule.clone());
}

#[test]
fn rule_debug() {
    let rule = make_rule(Dialect::OpenAi, Dialect::Claude, "f", Fidelity::Lossless);
    let dbg = format!("{:?}", rule);
    assert!(dbg.contains("MappingRule"));
}

// ════════════════════════════════════════════════════════════════════════
//  4. MappingRegistry — insert, lookup, len, is_empty, iter, rank_targets
// ════════════════════════════════════════════════════════════════════════

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
    assert_eq!(reg.len(), 1);
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn registry_lookup_miss_returns_none() {
    let reg = MappingRegistry::new();
    assert!(reg
        .lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
        .is_none());
}

#[test]
fn registry_insert_replaces_existing() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "x",
        Fidelity::Lossless,
    ));
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "x",
        Fidelity::LossyLabeled {
            warning: "changed".into(),
        },
    ));
    assert_eq!(reg.len(), 1);
    assert!(!reg
        .lookup(Dialect::OpenAi, Dialect::Claude, "x")
        .unwrap()
        .fidelity
        .is_lossless());
}

#[test]
fn registry_multiple_features_same_pair() {
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
    assert!(reg.lookup(Dialect::OpenAi, Dialect::Claude, "a").is_some());
    assert!(reg.lookup(Dialect::OpenAi, Dialect::Claude, "b").is_some());
}

#[test]
fn registry_same_feature_different_pairs() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    reg.insert(make_rule(
        Dialect::Claude,
        Dialect::Gemini,
        "tool_use",
        Fidelity::Lossless,
    ));
    assert_eq!(reg.len(), 2);
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
    reg.insert(make_rule(
        Dialect::Gemini,
        Dialect::Codex,
        "c",
        Fidelity::Lossless,
    ));
    assert_eq!(reg.iter().count(), reg.len());
    assert_eq!(reg.len(), 3);
}

#[test]
fn registry_iter_empty() {
    let reg = MappingRegistry::new();
    assert_eq!(reg.iter().count(), 0);
}

#[test]
fn registry_is_not_empty_after_insert() {
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
fn registry_lookup_direction_matters() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "x",
        Fidelity::Lossless,
    ));
    assert!(reg.lookup(Dialect::OpenAi, Dialect::Claude, "x").is_some());
    assert!(reg.lookup(Dialect::Claude, Dialect::OpenAi, "x").is_none());
}

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
        Dialect::Claude,
        "streaming",
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

    let ranked = reg.rank_targets(Dialect::OpenAi, &["tool_use", "streaming"]);
    // Claude has 2 lossless, Gemini has 0 lossless but 1 supported
    assert!(!ranked.is_empty());
    assert_eq!(ranked[0].0, Dialect::Claude);
    assert_eq!(ranked[0].1, 2);
}

#[test]
fn registry_rank_targets_empty_features() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[]);
    assert!(ranked.is_empty());
}

#[test]
fn registry_rank_targets_excludes_self() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &["tool_use"]);
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
    assert!(ranked.iter().all(|(d, _)| *d != Dialect::Codex));
}

#[test]
fn registry_rank_targets_sorted_descending() {
    let mut reg = MappingRegistry::new();
    // Claude: 2 lossless
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
    // Gemini: 1 lossless, 1 lossy
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Gemini,
        "a",
        Fidelity::Lossless,
    ));
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Gemini,
        "b",
        Fidelity::LossyLabeled {
            warning: "w".into(),
        },
    ));

    let ranked = reg.rank_targets(Dialect::OpenAi, &["a", "b"]);
    assert!(ranked.len() >= 2);
    assert!(ranked[0].1 >= ranked[1].1);
}

#[test]
fn registry_clone() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "x",
        Fidelity::Lossless,
    ));
    let reg2 = reg.clone();
    assert_eq!(reg2.len(), 1);
    assert!(reg2.lookup(Dialect::OpenAi, Dialect::Claude, "x").is_some());
}

// ════════════════════════════════════════════════════════════════════════
//  5. MappingMatrix — construction, set, get, is_supported, from_registry
// ════════════════════════════════════════════════════════════════════════

#[test]
fn matrix_new_is_empty() {
    let m = MappingMatrix::new();
    assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), None);
    assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn matrix_default_is_empty() {
    let m = MappingMatrix::default();
    assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), None);
}

#[test]
fn matrix_set_true_and_get() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, true);
    assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), Some(true));
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn matrix_set_false_and_get() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, false);
    assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), Some(false));
    assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn matrix_direction_matters() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, true);
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
    assert!(!m.is_supported(Dialect::Claude, Dialect::OpenAi));
}

#[test]
fn matrix_overwrite() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, true);
    m.set(Dialect::OpenAi, Dialect::Claude, false);
    assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn matrix_from_registry_lossless_marks_supported() {
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
fn matrix_from_registry_lossy_marks_supported() {
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
fn matrix_from_registry_empty_registry() {
    let reg = MappingRegistry::new();
    let m = MappingMatrix::from_registry(&reg);
    for &a in Dialect::all() {
        for &b in Dialect::all() {
            assert!(!m.is_supported(a, b));
        }
    }
}

#[test]
fn matrix_from_known_rules_has_entries() {
    let reg = known_rules();
    let m = MappingMatrix::from_registry(&reg);
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
    assert!(m.is_supported(Dialect::Claude, Dialect::Gemini));
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Gemini));
}

#[test]
fn matrix_clone() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, true);
    let m2 = m.clone();
    assert!(m2.is_supported(Dialect::OpenAi, Dialect::Claude));
}

// ════════════════════════════════════════════════════════════════════════
//  6. MappingValidation — construction, serde
// ════════════════════════════════════════════════════════════════════════

#[test]
fn validation_construction() {
    let v = MappingValidation {
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
        errors: vec![],
    };
    assert_eq!(v.feature, "tool_use");
    assert!(v.fidelity.is_lossless());
    assert!(v.errors.is_empty());
}

#[test]
fn validation_with_errors() {
    let v = MappingValidation {
        feature: "img".into(),
        fidelity: Fidelity::Unsupported {
            reason: "no images".into(),
        },
        errors: vec![MappingError::FeatureUnsupported {
            feature: "img".into(),
            from: Dialect::OpenAi,
            to: Dialect::Codex,
        }],
    };
    assert!(v.fidelity.is_unsupported());
    assert_eq!(v.errors.len(), 1);
}

#[test]
fn validation_serde_roundtrip_empty_errors() {
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
fn validation_serde_roundtrip_with_errors() {
    let v = MappingValidation {
        feature: "thinking".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "mapped".into(),
        },
        errors: vec![MappingError::FidelityLoss {
            feature: "thinking".into(),
            warning: "mapped".into(),
        }],
    };
    let json = serde_json::to_string(&v).unwrap();
    let v2: MappingValidation = serde_json::from_str(&json).unwrap();
    assert_eq!(v, v2);
}

#[test]
fn validation_eq() {
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
fn validation_clone() {
    let v = MappingValidation {
        feature: "f".into(),
        fidelity: Fidelity::Lossless,
        errors: vec![],
    };
    assert_eq!(v, v.clone());
}

// ════════════════════════════════════════════════════════════════════════
//  7. validate_mapping — the main validation function
// ════════════════════════════════════════════════════════════════════════

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
fn validate_unknown_feature_no_rule() {
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
    assert!(matches!(
        &results[0].errors[0],
        MappingError::FeatureUnsupported { .. }
    ));
}

#[test]
fn validate_empty_feature_name_is_invalid_input() {
    let reg = MappingRegistry::new();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);
    assert_eq!(results.len(), 1);
    assert!(matches!(
        &results[0].errors[0],
        MappingError::InvalidInput { .. }
    ));
    assert!(results[0].fidelity.is_unsupported());
}

#[test]
fn validate_empty_feature_list_returns_empty() {
    let reg = MappingRegistry::new();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[]);
    assert!(results.is_empty());
}

#[test]
fn validate_multiple_features_mixed() {
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
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "c",
        Fidelity::Lossless,
    ));

    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &["c".into(), "a".into(), "b".into()],
    );
    assert_eq!(results[0].feature, "c");
    assert_eq!(results[1].feature, "a");
    assert_eq!(results[2].feature, "b");
}

#[test]
fn validate_duplicate_features() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "x",
        Fidelity::Lossless,
    ));
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &["x".into(), "x".into()],
    );
    assert_eq!(results.len(), 2);
    assert!(results[0].fidelity.is_lossless());
    assert!(results[1].fidelity.is_lossless());
}

// ════════════════════════════════════════════════════════════════════════
//  8. known_rules() — pre-populated registry
// ════════════════════════════════════════════════════════════════════════

#[test]
fn known_rules_non_empty() {
    let reg = known_rules();
    assert!(!reg.is_empty());
}

#[test]
fn known_rules_same_dialect_all_lossless() {
    let reg = known_rules();
    for &d in Dialect::all() {
        for &f in &[
            features::TOOL_USE,
            features::STREAMING,
            features::THINKING,
            features::IMAGE_INPUT,
            features::CODE_EXEC,
        ] {
            let rule = reg.lookup(d, d, f);
            assert!(
                rule.is_some(),
                "missing self-mapping {d:?} -> {d:?} for {f}"
            );
            assert!(
                rule.unwrap().fidelity.is_lossless(),
                "{d:?} -> {d:?} {f} should be lossless"
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
fn known_rules_openai_gemini_tool_use_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Gemini, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn known_rules_claude_gemini_tool_use_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Claude, Dialect::Gemini, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn known_rules_openai_codex_tool_use_lossy() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Codex, features::TOOL_USE)
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
    assert!(!rule.fidelity.is_unsupported());
}

#[test]
fn known_rules_streaming_all_lossless_among_core_four() {
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
                "streaming {a:?} -> {b:?} should be lossless"
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
    assert!(!rule.fidelity.is_lossless());
}

#[test]
fn known_rules_thinking_claude_gemini_lossy() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Claude, Dialect::Gemini, features::THINKING)
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
    assert!(!rule.fidelity.is_unsupported());
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
            "image_input {src:?} -> Codex should be unsupported"
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
            "image_input Codex -> {tgt:?} should be unsupported"
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
fn known_rules_openai_claude_image_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::IMAGE_INPUT)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn known_rules_claude_gemini_image_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Claude, Dialect::Gemini, features::IMAGE_INPUT)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn known_rules_unknown_feature_returns_none() {
    let reg = known_rules();
    assert!(reg
        .lookup(Dialect::OpenAi, Dialect::Claude, "teleportation")
        .is_none());
}

// ════════════════════════════════════════════════════════════════════════
//  9. Cross-dialect validation with known_rules
// ════════════════════════════════════════════════════════════════════════

#[test]
fn validate_known_openai_to_claude_all_features() {
    let reg = known_rules();
    let feats: Vec<String> = vec![
        features::TOOL_USE.into(),
        features::STREAMING.into(),
        features::THINKING.into(),
        features::IMAGE_INPUT.into(),
        features::CODE_EXEC.into(),
    ];
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
    assert_eq!(results.len(), 5);
    // tool_use: lossless
    assert!(results[0].errors.is_empty());
    // streaming: lossless
    assert!(results[1].errors.is_empty());
}

#[test]
fn validate_known_claude_to_gemini() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Claude,
        Dialect::Gemini,
        &[features::TOOL_USE.into(), features::STREAMING.into()],
    );
    assert_eq!(results.len(), 2);
    assert!(results[0].fidelity.is_lossless());
    assert!(results[1].fidelity.is_lossless());
}

#[test]
fn validate_known_openai_to_codex_image_unsupported() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &[features::IMAGE_INPUT.into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
    assert!(matches!(
        &results[0].errors[0],
        MappingError::FeatureUnsupported { .. }
    ));
}

#[test]
fn validate_known_gemini_to_codex_image_unsupported() {
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
fn validate_known_claude_to_openai_thinking_lossy() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Claude,
        Dialect::OpenAi,
        &[features::THINKING.into()],
    );
    assert_eq!(results.len(), 1);
    assert!(!results[0].fidelity.is_lossless());
    assert!(matches!(
        &results[0].errors[0],
        MappingError::FidelityLoss { .. }
    ));
}

#[test]
fn validate_known_openai_to_claude_thinking_lossy() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[features::THINKING.into()],
    );
    assert!(!results[0].fidelity.is_lossless());
}

// ════════════════════════════════════════════════════════════════════════
//  10. Kimi & Copilot dialect rules
// ════════════════════════════════════════════════════════════════════════

#[test]
fn known_rules_kimi_tool_use_lossless_with_openai() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Kimi, Dialect::OpenAi, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn known_rules_copilot_tool_use_lossless_with_claude() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Copilot, Dialect::Claude, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn known_rules_kimi_copilot_tool_use_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn known_rules_kimi_codex_tool_use_lossy() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Kimi, Dialect::Codex, features::TOOL_USE)
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
    assert!(!rule.fidelity.is_unsupported());
}

#[test]
fn known_rules_kimi_streaming_lossless() {
    let reg = known_rules();
    for &tgt in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ] {
        let rule = reg.lookup(Dialect::Kimi, tgt, features::STREAMING).unwrap();
        assert!(
            rule.fidelity.is_lossless(),
            "Kimi -> {tgt:?} streaming should be lossless"
        );
    }
}

#[test]
fn known_rules_copilot_streaming_lossless() {
    let reg = known_rules();
    for &tgt in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ] {
        let rule = reg
            .lookup(Dialect::Copilot, tgt, features::STREAMING)
            .unwrap();
        assert!(
            rule.fidelity.is_lossless(),
            "Copilot -> {tgt:?} streaming should be lossless"
        );
    }
}

#[test]
fn known_rules_kimi_thinking_lossy() {
    let reg = known_rules();
    for &tgt in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ] {
        let rule = reg.lookup(Dialect::Kimi, tgt, features::THINKING).unwrap();
        assert!(!rule.fidelity.is_lossless());
    }
}

#[test]
fn known_rules_copilot_thinking_lossy() {
    let reg = known_rules();
    for &tgt in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ] {
        let rule = reg
            .lookup(Dialect::Copilot, tgt, features::THINKING)
            .unwrap();
        assert!(!rule.fidelity.is_lossless());
    }
}

#[test]
fn known_rules_kimi_image_input_unsupported() {
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
        assert!(rule.fidelity.is_unsupported());
    }
}

#[test]
fn known_rules_kimi_copilot_image_unsupported() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::IMAGE_INPUT)
        .unwrap();
    assert!(rule.fidelity.is_unsupported());
}

#[test]
fn known_rules_kimi_code_exec_unsupported() {
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
            "Kimi -> {tgt:?} code_exec should be unsupported"
        );
    }
}

#[test]
fn known_rules_code_exec_cross_dialect_lossy() {
    let reg = known_rules();
    let code_capable = [
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Copilot,
    ];
    for &a in &code_capable {
        for &b in &code_capable {
            if a == b {
                continue;
            }
            let rule = reg.lookup(a, b, features::CODE_EXEC).unwrap();
            assert!(
                !rule.fidelity.is_lossless(),
                "code_exec {a:?} -> {b:?} should not be lossless"
            );
            assert!(
                !rule.fidelity.is_unsupported(),
                "code_exec {a:?} -> {b:?} should not be unsupported"
            );
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
//  11. Edge cases
// ════════════════════════════════════════════════════════════════════════

#[test]
fn edge_validate_with_empty_registry() {
    let reg = MappingRegistry::new();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["tool_use".into()]);
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
}

#[test]
fn edge_identity_mapping_always_lossless() {
    let reg = known_rules();
    for &d in Dialect::all() {
        for &f in &[
            features::TOOL_USE,
            features::STREAMING,
            features::THINKING,
            features::IMAGE_INPUT,
            features::CODE_EXEC,
        ] {
            let results = validate_mapping(&reg, d, d, &[f.into()]);
            assert_eq!(results.len(), 1);
            assert!(
                results[0].fidelity.is_lossless(),
                "{d:?} -> {d:?} {f} should validate lossless"
            );
            assert!(results[0].errors.is_empty());
        }
    }
}

#[test]
fn edge_many_unknown_features() {
    let reg = MappingRegistry::new();
    let feats: Vec<String> = (0..50).map(|i| format!("unknown_{i}")).collect();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
    assert_eq!(results.len(), 50);
    for r in &results {
        assert!(r.fidelity.is_unsupported());
        assert_eq!(r.errors.len(), 1);
    }
}

#[test]
fn edge_feature_name_with_special_chars() {
    let mut reg = MappingRegistry::new();
    let feat = "my-feature_v2.0";
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        feat,
        Fidelity::Lossless,
    ));
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[feat.into()]);
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_lossless());
}

#[test]
fn edge_feature_name_with_spaces() {
    let mut reg = MappingRegistry::new();
    let feat = "feature with spaces";
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        feat,
        Fidelity::Lossless,
    ));
    assert!(reg.lookup(Dialect::OpenAi, Dialect::Claude, feat).is_some());
}

#[test]
fn edge_long_feature_name() {
    let mut reg = MappingRegistry::new();
    let feat = "a".repeat(1000);
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        &feat,
        Fidelity::Lossless,
    ));
    assert!(reg
        .lookup(Dialect::OpenAi, Dialect::Claude, &feat)
        .is_some());
}

#[test]
fn edge_matrix_self_mapping() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::OpenAi, true);
    assert!(m.is_supported(Dialect::OpenAi, Dialect::OpenAi));
}

#[test]
fn edge_rank_targets_with_no_rules_for_source() {
    let reg = MappingRegistry::new();
    let ranked = reg.rank_targets(Dialect::OpenAi, &["tool_use"]);
    assert!(ranked.is_empty());
}

#[test]
fn edge_rank_targets_all_unsupported() {
    let mut reg = MappingRegistry::new();
    for &tgt in Dialect::all() {
        if tgt != Dialect::OpenAi {
            reg.insert(make_rule(
                Dialect::OpenAi,
                tgt,
                "x",
                Fidelity::Unsupported { reason: "n".into() },
            ));
        }
    }
    let ranked = reg.rank_targets(Dialect::OpenAi, &["x"]);
    assert!(ranked.is_empty());
}

// ════════════════════════════════════════════════════════════════════════
//  12. features module constants
// ════════════════════════════════════════════════════════════════════════

#[test]
fn feature_constants_values() {
    assert_eq!(features::TOOL_USE, "tool_use");
    assert_eq!(features::STREAMING, "streaming");
    assert_eq!(features::THINKING, "thinking");
    assert_eq!(features::IMAGE_INPUT, "image_input");
    assert_eq!(features::CODE_EXEC, "code_exec");
}

#[test]
fn feature_constants_unique() {
    let all = [
        features::TOOL_USE,
        features::STREAMING,
        features::THINKING,
        features::IMAGE_INPUT,
        features::CODE_EXEC,
    ];
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            assert_ne!(all[i], all[j], "features should be unique");
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
//  13. Serialization / JSON structure checks
// ════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_json_tag_lossless() {
    let f = Fidelity::Lossless;
    let json = serde_json::to_value(&f).unwrap();
    assert_eq!(json["type"], "lossless");
}

#[test]
fn fidelity_json_tag_lossy_labeled() {
    let f = Fidelity::LossyLabeled {
        warning: "w".into(),
    };
    let json = serde_json::to_value(&f).unwrap();
    assert_eq!(json["type"], "lossy_labeled");
    assert_eq!(json["warning"], "w");
}

#[test]
fn fidelity_json_tag_unsupported() {
    let f = Fidelity::Unsupported { reason: "r".into() };
    let json = serde_json::to_value(&f).unwrap();
    assert_eq!(json["type"], "unsupported");
    assert_eq!(json["reason"], "r");
}

#[test]
fn mapping_rule_json_structure() {
    let rule = make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    );
    let json = serde_json::to_value(&rule).unwrap();
    assert!(json.get("source_dialect").is_some());
    assert!(json.get("target_dialect").is_some());
    assert!(json.get("feature").is_some());
    assert!(json.get("fidelity").is_some());
}

#[test]
fn mapping_error_json_feature_unsupported_structure() {
    let err = MappingError::FeatureUnsupported {
        feature: "x".into(),
        from: Dialect::OpenAi,
        to: Dialect::Claude,
    };
    let json = serde_json::to_value(&err).unwrap();
    // Externally tagged enum: { "FeatureUnsupported": { "feature": ..., "from": ..., "to": ... } }
    let inner = json.get("FeatureUnsupported").unwrap();
    assert!(inner.get("feature").is_some());
    assert!(inner.get("from").is_some());
    assert!(inner.get("to").is_some());
}

#[test]
fn validation_json_roundtrip_with_multiple_errors() {
    let v = MappingValidation {
        feature: "test".into(),
        fidelity: Fidelity::Unsupported {
            reason: "bad".into(),
        },
        errors: vec![
            MappingError::FeatureUnsupported {
                feature: "test".into(),
                from: Dialect::OpenAi,
                to: Dialect::Claude,
            },
            MappingError::InvalidInput {
                reason: "also bad".into(),
            },
        ],
    };
    let json = serde_json::to_string(&v).unwrap();
    let v2: MappingValidation = serde_json::from_str(&json).unwrap();
    assert_eq!(v, v2);
    assert_eq!(v2.errors.len(), 2);
}

#[test]
fn fidelity_deserialize_from_json_string() {
    let json = r#"{"type":"lossless"}"#;
    let f: Fidelity = serde_json::from_str(json).unwrap();
    assert!(f.is_lossless());
}

#[test]
fn fidelity_deserialize_lossy_labeled_from_json() {
    let json = r#"{"type":"lossy_labeled","warning":"test"}"#;
    let f: Fidelity = serde_json::from_str(json).unwrap();
    assert!(!f.is_lossless());
    assert!(!f.is_unsupported());
}

#[test]
fn fidelity_deserialize_unsupported_from_json() {
    let json = r#"{"type":"unsupported","reason":"nope"}"#;
    let f: Fidelity = serde_json::from_str(json).unwrap();
    assert!(f.is_unsupported());
}

// ════════════════════════════════════════════════════════════════════════
//  14. rank_targets with known_rules
// ════════════════════════════════════════════════════════════════════════

#[test]
fn rank_targets_known_openai_streaming() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::STREAMING]);
    // All streaming is lossless, so all targets should have lossless_count = 1
    assert!(!ranked.is_empty());
    for (_, count) in &ranked {
        assert_eq!(*count, 1);
    }
}

#[test]
fn rank_targets_known_openai_tool_use_and_streaming() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE, features::STREAMING]);
    // Claude should have 2 lossless (tool_use + streaming)
    let claude_entry = ranked.iter().find(|(d, _)| *d == Dialect::Claude);
    assert!(claude_entry.is_some());
    assert_eq!(claude_entry.unwrap().1, 2);
}

#[test]
fn rank_targets_known_excludes_source() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE]);
    assert!(ranked.iter().all(|(d, _)| *d != Dialect::OpenAi));
}

// ════════════════════════════════════════════════════════════════════════
//  15. Additional serde roundtrips and edge cases
// ════════════════════════════════════════════════════════════════════════

#[test]
fn mapping_rule_serde_with_unsupported_fidelity() {
    let rule = make_rule(
        Dialect::Kimi,
        Dialect::Codex,
        "image_input",
        Fidelity::Unsupported {
            reason: "Kimi does not support image inputs".into(),
        },
    );
    let json = serde_json::to_string(&rule).unwrap();
    let rule2: MappingRule = serde_json::from_str(&json).unwrap();
    assert_eq!(rule, rule2);
    assert!(rule2.fidelity.is_unsupported());
}

#[test]
fn error_all_variants_are_display() {
    let errors: Vec<MappingError> = vec![
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
        let s = err.to_string();
        assert!(!s.is_empty());
    }
}

#[test]
fn error_all_variants_debug() {
    let errors: Vec<MappingError> = vec![
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
        let dbg = format!("{:?}", err);
        assert!(!dbg.is_empty());
    }
}

#[test]
fn matrix_multiple_pairs() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, true);
    m.set(Dialect::Claude, Dialect::Gemini, true);
    m.set(Dialect::Gemini, Dialect::Codex, false);

    assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
    assert!(m.is_supported(Dialect::Claude, Dialect::Gemini));
    assert!(!m.is_supported(Dialect::Gemini, Dialect::Codex));
    assert!(!m.is_supported(Dialect::Codex, Dialect::Gemini));
}

#[test]
fn registry_debug() {
    let reg = MappingRegistry::new();
    let dbg = format!("{:?}", reg);
    assert!(dbg.contains("MappingRegistry"));
}

#[test]
fn matrix_debug() {
    let m = MappingMatrix::new();
    let dbg = format!("{:?}", m);
    assert!(dbg.contains("MappingMatrix"));
}

#[test]
fn known_rules_kimi_copilot_thinking_lossy() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::THINKING)
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
    assert!(!rule.fidelity.is_unsupported());
}

#[test]
fn known_rules_copilot_kimi_thinking_lossy() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Copilot, Dialect::Kimi, features::THINKING)
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
    assert!(!rule.fidelity.is_unsupported());
}
