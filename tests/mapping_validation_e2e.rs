#![allow(clippy::all)]
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
//! Comprehensive end-to-end tests for `abp-mapping` cross-dialect validation.

use abp_dialect::{Dialect, DialectDetector, DialectValidator};
use abp_mapping::{
    Fidelity, MappingError, MappingMatrix, MappingRegistry, MappingRule, MappingValidation,
    features, known_rules, validate_mapping,
};

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════
// 1. MappingRegistry creation and lookup
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
fn registry_insert_single_rule() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    assert_eq!(reg.len(), 1);
    assert!(!reg.is_empty());
}

#[test]
fn registry_lookup_existing() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "streaming",
        Fidelity::Lossless,
    ));
    let rule = reg.lookup(Dialect::OpenAi, Dialect::Claude, "streaming");
    assert!(rule.is_some());
    assert!(rule.unwrap().fidelity.is_lossless());
}

#[test]
fn registry_lookup_missing_returns_none() {
    let reg = MappingRegistry::new();
    assert!(
        reg.lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
            .is_none()
    );
}

#[test]
fn registry_lookup_wrong_direction() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    // Reverse direction should not match.
    assert!(
        reg.lookup(Dialect::Claude, Dialect::OpenAi, "tool_use")
            .is_none()
    );
}

#[test]
fn registry_lookup_wrong_feature() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    assert!(
        reg.lookup(Dialect::OpenAi, Dialect::Claude, "streaming")
            .is_none()
    );
}

#[test]
fn registry_insert_replaces_same_key() {
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
    assert!(
        !reg.lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
            .unwrap()
            .fidelity
            .is_lossless()
    );
}

#[test]
fn registry_multiple_features_same_pair() {
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
    assert_eq!(reg.len(), 2);
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
    for i in 0..5 {
        reg.insert(make_rule(
            Dialect::OpenAi,
            Dialect::Claude,
            &format!("feat_{i}"),
            Fidelity::Lossless,
        ));
    }
    assert_eq!(reg.iter().count(), reg.len());
}

#[test]
fn registry_iter_returns_all_rules() {
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
    let features: Vec<String> = reg.iter().map(|r| r.feature.clone()).collect();
    assert!(features.contains(&"a".to_string()));
    assert!(features.contains(&"b".to_string()));
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Feature compatibility matrix queries
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn matrix_new_is_empty() {
    let m = MappingMatrix::new();
    assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), None);
    assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn matrix_set_and_get_true() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, true);
    assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), Some(true));
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn matrix_set_and_get_false() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, false);
    assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), Some(false));
    assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn matrix_is_directional() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, true);
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
fn matrix_from_registry_marks_supported() {
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
fn matrix_from_registry_excludes_unsupported_only() {
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
fn matrix_from_registry_lossy_counts_as_supported() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::Claude,
        Dialect::Codex,
        "thinking",
        Fidelity::LossyLabeled {
            warning: "lossy".into(),
        },
    ));
    let m = MappingMatrix::from_registry(&reg);
    assert!(m.is_supported(Dialect::Claude, Dialect::Codex));
}

#[test]
fn matrix_from_known_rules_openai_claude() {
    let reg = known_rules();
    let m = MappingMatrix::from_registry(&reg);
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
    assert!(m.is_supported(Dialect::Claude, Dialect::OpenAi));
}

#[test]
fn matrix_from_known_rules_all_self_pairs() {
    let reg = known_rules();
    let m = MappingMatrix::from_registry(&reg);
    for &d in all_dialects() {
        assert!(
            m.is_supported(d, d),
            "{d} -> {d} should be supported (self-identity)"
        );
    }
}

#[test]
fn matrix_unset_returns_none() {
    let m = MappingMatrix::new();
    for &a in all_dialects() {
        for &b in all_dialects() {
            assert_eq!(m.get(a, b), None);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Cross-dialect validation (valid and invalid pairs)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_lossless_feature() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[features::TOOL_USE.into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_lossless());
    assert!(results[0].errors.is_empty());
}

#[test]
fn validate_lossy_feature_produces_fidelity_loss_error() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::Claude,
        Dialect::OpenAi,
        &[features::THINKING.into()],
    );
    assert_eq!(results.len(), 1);
    assert!(!results[0].fidelity.is_lossless());
    assert!(
        results[0]
            .errors
            .iter()
            .any(|e| matches!(e, MappingError::FidelityLoss { .. }))
    );
}

#[test]
fn validate_unsupported_feature_produces_error() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &[features::IMAGE_INPUT.into()],
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
    assert!(
        results[0]
            .errors
            .iter()
            .any(|e| matches!(e, MappingError::FeatureUnsupported { .. }))
    );
}

#[test]
fn validate_missing_rule_returns_unsupported() {
    let reg = MappingRegistry::new();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &["nonexistent".into()],
    );
    assert!(results[0].fidelity.is_unsupported());
}

#[test]
fn validate_empty_features_returns_empty() {
    let reg = known_rules();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[]);
    assert!(results.is_empty());
}

#[test]
fn validate_empty_feature_name_returns_invalid_input() {
    let reg = known_rules();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);
    assert_eq!(results.len(), 1);
    assert!(
        results[0]
            .errors
            .iter()
            .any(|e| matches!(e, MappingError::InvalidInput { .. }))
    );
}

#[test]
fn validate_multiple_features_mixed_fidelity() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[
            features::TOOL_USE.into(),
            features::THINKING.into(),
            features::IMAGE_INPUT.into(),
        ],
    );
    assert_eq!(results.len(), 3);
    // tool_use: lossless
    assert!(results[0].fidelity.is_lossless());
    // thinking: lossy
    assert!(!results[1].fidelity.is_lossless());
    assert!(!results[1].fidelity.is_unsupported());
    // image_input: lossless
    assert!(results[2].fidelity.is_lossless());
}

#[test]
fn validate_same_dialect_all_features_lossless() {
    let reg = known_rules();
    for &d in all_dialects() {
        let feats: Vec<String> = all_features().iter().map(|f| f.to_string()).collect();
        let results = validate_mapping(&reg, d, d, &feats);
        for r in &results {
            assert!(
                r.fidelity.is_lossless(),
                "same-dialect {d} -> {d} feature {} should be lossless",
                r.feature
            );
            assert!(r.errors.is_empty());
        }
    }
}

#[test]
fn validate_streaming_all_core_pairs_lossless() {
    let reg = known_rules();
    let core = [
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ];
    for &a in &core {
        for &b in &core {
            let results = validate_mapping(&reg, a, b, &[features::STREAMING.into()]);
            assert!(
                results[0].fidelity.is_lossless(),
                "streaming {a} -> {b} should be lossless"
            );
        }
    }
}

#[test]
fn validate_image_input_codex_always_unsupported() {
    let reg = known_rules();
    for &src in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
        let results = validate_mapping(&reg, src, Dialect::Codex, &[features::IMAGE_INPUT.into()]);
        assert!(
            results[0].fidelity.is_unsupported(),
            "image_input {src} -> Codex should be unsupported"
        );
    }
}

#[test]
fn validate_codex_image_to_others_unsupported() {
    let reg = known_rules();
    for &tgt in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
        let results = validate_mapping(&reg, Dialect::Codex, tgt, &[features::IMAGE_INPUT.into()]);
        assert!(
            results[0].fidelity.is_unsupported(),
            "image_input Codex -> {tgt} should be unsupported"
        );
    }
}

#[test]
fn validate_tool_use_openai_claude_bidirectional_lossless() {
    let reg = known_rules();
    let r1 = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[features::TOOL_USE.into()],
    );
    let r2 = validate_mapping(
        &reg,
        Dialect::Claude,
        Dialect::OpenAi,
        &[features::TOOL_USE.into()],
    );
    assert!(r1[0].fidelity.is_lossless());
    assert!(r2[0].fidelity.is_lossless());
}

#[test]
fn validate_tool_use_codex_is_lossy() {
    let reg = known_rules();
    let r = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &[features::TOOL_USE.into()],
    );
    assert!(!r[0].fidelity.is_lossless());
    assert!(!r[0].fidelity.is_unsupported());
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Fidelity tracking (what gets lost in translation)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_lossless_predicates() {
    let f = Fidelity::Lossless;
    assert!(f.is_lossless());
    assert!(!f.is_unsupported());
}

#[test]
fn fidelity_lossy_labeled_predicates() {
    let f = Fidelity::LossyLabeled {
        warning: "some loss".into(),
    };
    assert!(!f.is_lossless());
    assert!(!f.is_unsupported());
}

#[test]
fn fidelity_unsupported_predicates() {
    let f = Fidelity::Unsupported {
        reason: "n/a".into(),
    };
    assert!(!f.is_lossless());
    assert!(f.is_unsupported());
}

#[test]
fn fidelity_lossy_warning_preserved_in_validation() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::Claude,
        Dialect::OpenAi,
        "thinking",
        Fidelity::LossyLabeled {
            warning: "mapped to system message".into(),
        },
    ));
    let results = validate_mapping(&reg, Dialect::Claude, Dialect::OpenAi, &["thinking".into()]);
    match &results[0].fidelity {
        Fidelity::LossyLabeled { warning } => {
            assert!(warning.contains("mapped to system message"));
        }
        other => panic!("expected LossyLabeled, got {other:?}"),
    }
}

#[test]
fn fidelity_unsupported_reason_preserved() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Codex,
        "image_input",
        Fidelity::Unsupported {
            reason: "Codex has no images".into(),
        },
    ));
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Codex,
        &["image_input".into()],
    );
    match &results[0].fidelity {
        Fidelity::Unsupported { reason } => assert!(reason.contains("no images")),
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

#[test]
fn known_rules_thinking_cross_dialect_always_lossy() {
    let reg = known_rules();
    let core = [
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ];
    for &a in &core {
        for &b in &core {
            if a == b {
                continue;
            }
            let rule = reg.lookup(a, b, features::THINKING);
            assert!(rule.is_some(), "thinking rule missing for {a} -> {b}");
            assert!(
                !rule.unwrap().fidelity.is_lossless(),
                "thinking {a} -> {b} should not be lossless"
            );
        }
    }
}

#[test]
fn known_rules_code_exec_cross_dialect_lossy_for_code_capable() {
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
            let rule = reg.lookup(a, b, features::CODE_EXEC);
            assert!(rule.is_some(), "code_exec rule missing for {a} -> {b}");
            assert!(
                !rule.unwrap().fidelity.is_lossless(),
                "code_exec {a} -> {b} should be lossy"
            );
        }
    }
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
        let rule = reg.lookup(Dialect::Kimi, tgt, features::CODE_EXEC);
        assert!(rule.is_some(), "code_exec Kimi -> {tgt} rule should exist");
        assert!(
            rule.unwrap().fidelity.is_unsupported(),
            "code_exec Kimi -> {tgt} should be unsupported"
        );
    }
}

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
        warning: "some warning".into(),
    };
    let json = serde_json::to_string(&f).unwrap();
    let f2: Fidelity = serde_json::from_str(&json).unwrap();
    assert_eq!(f, f2);
}

#[test]
fn fidelity_serde_roundtrip_unsupported() {
    let f = Fidelity::Unsupported {
        reason: "test reason".into(),
    };
    let json = serde_json::to_string(&f).unwrap();
    let f2: Fidelity = serde_json::from_str(&json).unwrap();
    assert_eq!(f, f2);
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Dialect detection from request payloads
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_openai_request() {
    let d = DialectDetector::new();
    let msg = serde_json::json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hello"}]
    });
    let result = d.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::OpenAi);
    assert!(result.confidence > 0.0);
}

#[test]
fn detect_openai_response() {
    let d = DialectDetector::new();
    let msg = serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": "hi"}}],
        "model": "gpt-4"
    });
    let result = d.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::OpenAi);
}

#[test]
fn detect_claude_message() {
    let d = DialectDetector::new();
    let msg = serde_json::json!({
        "type": "message",
        "model": "claude-3",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
    });
    let result = d.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Claude);
}

#[test]
fn detect_gemini_request() {
    let d = DialectDetector::new();
    let msg = serde_json::json!({
        "contents": [{"parts": [{"text": "hello"}]}]
    });
    let result = d.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Gemini);
}

#[test]
fn detect_gemini_response() {
    let d = DialectDetector::new();
    let msg = serde_json::json!({
        "candidates": [{"content": {"parts": [{"text": "hi"}]}}]
    });
    let result = d.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Gemini);
}

#[test]
fn detect_codex_response() {
    let d = DialectDetector::new();
    let msg = serde_json::json!({
        "items": [{"type": "message", "text": "hello"}],
        "status": "completed",
        "object": "response"
    });
    let result = d.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Codex);
}

#[test]
fn detect_kimi_request() {
    let d = DialectDetector::new();
    let msg = serde_json::json!({
        "refs": ["ref1"],
        "search_plus": true,
        "messages": [{"role": "user", "content": "hi"}]
    });
    let result = d.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Kimi);
}

#[test]
fn detect_copilot_request() {
    let d = DialectDetector::new();
    let msg = serde_json::json!({
        "references": [{"type": "file"}],
        "agent_mode": true,
        "messages": [{"role": "user", "content": "hi"}]
    });
    let result = d.detect(&msg).unwrap();
    assert_eq!(result.dialect, Dialect::Copilot);
}

#[test]
fn detect_returns_none_for_non_object() {
    let d = DialectDetector::new();
    assert!(d.detect(&serde_json::json!("string")).is_none());
    assert!(d.detect(&serde_json::json!(42)).is_none());
    assert!(d.detect(&serde_json::json!(null)).is_none());
    assert!(d.detect(&serde_json::json!([])).is_none());
}

#[test]
fn detect_returns_none_for_empty_object() {
    let d = DialectDetector::new();
    assert!(d.detect(&serde_json::json!({})).is_none());
}

#[test]
fn detect_all_returns_multiple_candidates() {
    let d = DialectDetector::new();
    // Ambiguous payload that scores for both OpenAI and Kimi.
    let msg = serde_json::json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hello"}],
        "refs": ["ref1"]
    });
    let results = d.detect_all(&msg);
    assert!(!results.is_empty());
}

#[test]
fn detect_all_sorted_by_confidence_descending() {
    let d = DialectDetector::new();
    let msg = serde_json::json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hello"}],
        "refs": ["ref1"]
    });
    let results = d.detect_all(&msg);
    for w in results.windows(2) {
        assert!(w[0].confidence >= w[1].confidence);
    }
}

#[test]
fn detect_all_empty_for_non_object() {
    let d = DialectDetector::new();
    assert!(d.detect_all(&serde_json::json!("string")).is_empty());
}

#[test]
fn detect_evidence_is_non_empty() {
    let d = DialectDetector::new();
    let msg = serde_json::json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hello"}]
    });
    let result = d.detect(&msg).unwrap();
    assert!(!result.evidence.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Round-trip validation (serde and rule consistency)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn mapping_rule_serde_roundtrip() {
    let rule = make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    );
    let json = serde_json::to_string(&rule).unwrap();
    let rule2: MappingRule = serde_json::from_str(&json).unwrap();
    assert_eq!(rule, rule2);
}

#[test]
fn mapping_error_serde_roundtrip_feature_unsupported() {
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
fn mapping_error_serde_roundtrip_fidelity_loss() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "loss".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let err2: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, err2);
}

#[test]
fn mapping_error_serde_roundtrip_dialect_mismatch() {
    let err = MappingError::DialectMismatch {
        from: Dialect::Gemini,
        to: Dialect::Codex,
    };
    let json = serde_json::to_string(&err).unwrap();
    let err2: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, err2);
}

#[test]
fn mapping_error_serde_roundtrip_invalid_input() {
    let err = MappingError::InvalidInput {
        reason: "bad".into(),
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

#[test]
fn mapping_validation_with_errors_serde_roundtrip() {
    let v = MappingValidation {
        feature: "thinking".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "loss".into(),
        },
        errors: vec![MappingError::FidelityLoss {
            feature: "thinking".into(),
            warning: "loss".into(),
        }],
    };
    let json = serde_json::to_string(&v).unwrap();
    let v2: MappingValidation = serde_json::from_str(&json).unwrap();
    assert_eq!(v, v2);
}

#[test]
fn validate_then_re_validate_same_result() {
    let reg = known_rules();
    let feats: Vec<String> = all_features().iter().map(|f| f.to_string()).collect();
    let r1 = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
    let r2 = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
    assert_eq!(r1, r2);
}

#[test]
fn validate_symmetric_pair_has_matching_feature_count() {
    let reg = known_rules();
    let feats: Vec<String> = all_features().iter().map(|f| f.to_string()).collect();
    let r1 = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
    let r2 = validate_mapping(&reg, Dialect::Claude, Dialect::OpenAi, &feats);
    assert_eq!(r1.len(), r2.len());
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_unknown_feature_in_known_registry() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &["teleportation".into()],
    );
    assert!(results[0].fidelity.is_unsupported());
}

#[test]
fn validate_empty_registry_any_feature_unsupported() {
    let reg = MappingRegistry::new();
    for &a in all_dialects() {
        for &b in all_dialects() {
            let results = validate_mapping(&reg, a, b, &["anything".into()]);
            assert!(results[0].fidelity.is_unsupported());
        }
    }
}

#[test]
fn registry_insert_many_rules_stress() {
    let mut reg = MappingRegistry::new();
    for &a in all_dialects() {
        for &b in all_dialects() {
            for i in 0..10 {
                reg.insert(make_rule(a, b, &format!("feat_{i}"), Fidelity::Lossless));
            }
        }
    }
    // 6 dialects * 6 dialects * 10 features = 360
    assert_eq!(reg.len(), 6 * 6 * 10);
}

#[test]
fn registry_replace_all_rules_preserves_count() {
    let mut reg = MappingRegistry::new();
    for _ in 0..3 {
        reg.insert(make_rule(
            Dialect::OpenAi,
            Dialect::Claude,
            "x",
            Fidelity::Lossless,
        ));
    }
    assert_eq!(reg.len(), 1);
}

#[test]
fn known_rules_is_non_empty() {
    let reg = known_rules();
    assert!(!reg.is_empty());
}

#[test]
fn known_rules_same_dialect_lossless_for_all() {
    let reg = known_rules();
    for &d in all_dialects() {
        for f in all_features() {
            let rule = reg.lookup(d, d, f);
            assert!(rule.is_some(), "self-rule missing for {d} -> {d} {f}");
            assert!(
                rule.unwrap().fidelity.is_lossless(),
                "{d} -> {d} {f} should be lossless"
            );
        }
    }
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
    let r = reg
        .lookup(Dialect::Claude, Dialect::Gemini, features::TOOL_USE)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_rules_kimi_copilot_tool_use_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::TOOL_USE)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_rules_kimi_image_unsupported() {
    let reg = known_rules();
    for &tgt in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ] {
        let rule = reg.lookup(Dialect::Kimi, tgt, features::IMAGE_INPUT);
        assert!(
            rule.is_some(),
            "image_input Kimi -> {tgt} rule should exist"
        );
        assert!(
            rule.unwrap().fidelity.is_unsupported(),
            "image_input Kimi -> {tgt} should be unsupported"
        );
    }
}

#[test]
fn known_rules_copilot_image_unsupported() {
    let reg = known_rules();
    for &tgt in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
    ] {
        let rule = reg.lookup(Dialect::Copilot, tgt, features::IMAGE_INPUT);
        assert!(
            rule.is_some(),
            "image_input Copilot -> {tgt} rule should exist"
        );
        assert!(
            rule.unwrap().fidelity.is_unsupported(),
            "image_input Copilot -> {tgt} should be unsupported"
        );
    }
}

#[test]
fn known_rules_streaming_kimi_copilot_lossless() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::STREAMING)
        .unwrap();
    assert!(r.fidelity.is_lossless());
}

#[test]
fn known_rules_kimi_copilot_thinking_lossy() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::THINKING)
        .unwrap();
    assert!(!r.fidelity.is_lossless());
}

#[test]
fn known_rules_kimi_copilot_image_unsupported() {
    let reg = known_rules();
    let r = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::IMAGE_INPUT)
        .unwrap();
    assert!(r.fidelity.is_unsupported());
}

// ── Rank targets ────────────────────────────────────────────────────

#[test]
fn rank_targets_excludes_self() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE]);
    assert!(
        ranked.iter().all(|(d, _)| *d != Dialect::OpenAi),
        "rank_targets should exclude the source dialect"
    );
}

#[test]
fn rank_targets_sorted_by_lossless_count_descending() {
    let reg = known_rules();
    let ranked = reg.rank_targets(
        Dialect::OpenAi,
        &[
            features::TOOL_USE,
            features::STREAMING,
            features::IMAGE_INPUT,
        ],
    );
    for w in ranked.windows(2) {
        assert!(w[0].1 >= w[1].1);
    }
}

#[test]
fn rank_targets_excludes_all_unsupported() {
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
    assert!(
        !ranked.iter().any(|(d, _)| *d == Dialect::Codex),
        "all-unsupported target should be excluded"
    );
}

#[test]
fn rank_targets_empty_features() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[]);
    assert!(ranked.is_empty());
}

#[test]
fn rank_targets_empty_registry() {
    let reg = MappingRegistry::new();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE]);
    assert!(ranked.is_empty());
}

#[test]
fn rank_targets_single_lossless_feature() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Claude,
        "tool_use",
        Fidelity::Lossless,
    ));
    let ranked = reg.rank_targets(Dialect::OpenAi, &["tool_use"]);
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0], (Dialect::Claude, 1));
}

#[test]
fn rank_targets_lossy_included_but_not_lossless_counted() {
    let mut reg = MappingRegistry::new();
    reg.insert(make_rule(
        Dialect::OpenAi,
        Dialect::Codex,
        "tool_use",
        Fidelity::LossyLabeled {
            warning: "diff schema".into(),
        },
    ));
    let ranked = reg.rank_targets(Dialect::OpenAi, &["tool_use"]);
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0], (Dialect::Codex, 0));
}

// ── Dialect validation ──────────────────────────────────────────────

#[test]
fn validator_openai_valid_request() {
    let v = DialectValidator::new();
    let msg = serde_json::json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let result = v.validate(&msg, Dialect::OpenAi);
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn validator_openai_missing_model() {
    let v = DialectValidator::new();
    let msg = serde_json::json!({
        "messages": [{"role": "user", "content": "hi"}]
    });
    let result = v.validate(&msg, Dialect::OpenAi);
    assert!(!result.valid);
}

#[test]
fn validator_claude_valid_request() {
    let v = DialectValidator::new();
    let msg = serde_json::json!({
        "model": "claude-3",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let result = v.validate(&msg, Dialect::Claude);
    assert!(result.valid);
}

#[test]
fn validator_gemini_valid_request() {
    let v = DialectValidator::new();
    let msg = serde_json::json!({
        "contents": [{"parts": [{"text": "hi"}]}]
    });
    let result = v.validate(&msg, Dialect::Gemini);
    assert!(result.valid);
}

#[test]
fn validator_non_object_input() {
    let v = DialectValidator::new();
    let result = v.validate(&serde_json::json!("string"), Dialect::OpenAi);
    assert!(!result.valid);
    assert!(!result.errors.is_empty());
}

#[test]
fn validator_gemini_missing_parts() {
    let v = DialectValidator::new();
    let msg = serde_json::json!({
        "contents": [{"role": "user"}]
    });
    let result = v.validate(&msg, Dialect::Gemini);
    assert!(!result.valid);
}

// ── Error display ───────────────────────────────────────────────────

#[test]
fn mapping_error_display_feature_unsupported() {
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
fn mapping_error_display_fidelity_loss() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "mapped differently".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("thinking"));
    assert!(msg.contains("mapped differently"));
}

#[test]
fn mapping_error_display_dialect_mismatch() {
    let err = MappingError::DialectMismatch {
        from: Dialect::OpenAi,
        to: Dialect::Codex,
    };
    let msg = err.to_string();
    assert!(msg.contains("OpenAI"));
    assert!(msg.contains("Codex"));
}

#[test]
fn mapping_error_display_invalid_input() {
    let err = MappingError::InvalidInput {
        reason: "empty name".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("empty name"));
}

// ── Dialect labels ──────────────────────────────────────────────────

#[test]
fn dialect_labels_non_empty() {
    for &d in all_dialects() {
        assert!(!d.label().is_empty());
    }
}

#[test]
fn dialect_display_matches_label() {
    for &d in all_dialects() {
        assert_eq!(format!("{d}"), d.label());
    }
}

#[test]
fn dialect_all_has_six() {
    assert_eq!(Dialect::all().len(), 6);
}

// ── Comprehensive known_rules cross-product ─────────────────────────

#[test]
fn known_rules_all_feature_pairs_have_self_rules() {
    let reg = known_rules();
    for &d in all_dialects() {
        for f in all_features() {
            assert!(
                reg.lookup(d, d, f).is_some(),
                "self-rule missing: {d} -> {d} {f}"
            );
        }
    }
}

#[test]
fn known_rules_streaming_kimi_all_lossless() {
    let reg = known_rules();
    for &tgt in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Copilot,
    ] {
        let r = reg.lookup(Dialect::Kimi, tgt, features::STREAMING).unwrap();
        assert!(
            r.fidelity.is_lossless(),
            "streaming Kimi -> {tgt} should be lossless"
        );
    }
}

#[test]
fn known_rules_streaming_copilot_all_lossless() {
    let reg = known_rules();
    for &tgt in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Kimi,
    ] {
        let r = reg
            .lookup(Dialect::Copilot, tgt, features::STREAMING)
            .unwrap();
        assert!(
            r.fidelity.is_lossless(),
            "streaming Copilot -> {tgt} should be lossless"
        );
    }
}

#[test]
fn known_rules_kimi_thinking_lossy_all_targets() {
    let reg = known_rules();
    for &tgt in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Copilot,
    ] {
        let r = reg.lookup(Dialect::Kimi, tgt, features::THINKING).unwrap();
        assert!(
            !r.fidelity.is_lossless(),
            "thinking Kimi -> {tgt} should be lossy"
        );
    }
}

#[test]
fn known_rules_copilot_thinking_lossy_all_targets() {
    let reg = known_rules();
    for &tgt in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Kimi,
    ] {
        let r = reg
            .lookup(Dialect::Copilot, tgt, features::THINKING)
            .unwrap();
        assert!(
            !r.fidelity.is_lossless(),
            "thinking Copilot -> {tgt} should be lossy"
        );
    }
}

// ── Validate all known features for all core pairs ──────────────────

#[test]
fn validate_all_core_pairs_all_features_no_panic() {
    let reg = known_rules();
    let feats: Vec<String> = all_features().iter().map(|f| f.to_string()).collect();
    for &a in all_dialects() {
        for &b in all_dialects() {
            let results = validate_mapping(&reg, a, b, &feats);
            assert_eq!(results.len(), feats.len(), "wrong count for {a} -> {b}");
        }
    }
}

#[test]
fn validate_all_core_pairs_each_result_has_feature_name() {
    let reg = known_rules();
    let feats: Vec<String> = all_features().iter().map(|f| f.to_string()).collect();
    for &a in all_dialects() {
        for &b in all_dialects() {
            let results = validate_mapping(&reg, a, b, &feats);
            for (i, r) in results.iter().enumerate() {
                assert_eq!(r.feature, feats[i]);
            }
        }
    }
}

// ── Matrix completeness from known_rules ────────────────────────────

#[test]
fn matrix_from_known_rules_core_four_fully_connected() {
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
            assert!(
                m.is_supported(a, b),
                "core pair {a} -> {b} should be supported"
            );
        }
    }
}

#[test]
fn matrix_from_known_rules_kimi_supported_outbound() {
    let reg = known_rules();
    let m = MappingMatrix::from_registry(&reg);
    for &tgt in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Copilot,
    ] {
        assert!(
            m.is_supported(Dialect::Kimi, tgt),
            "Kimi -> {tgt} should be supported"
        );
    }
}

#[test]
fn matrix_from_known_rules_copilot_supported_outbound() {
    let reg = known_rules();
    let m = MappingMatrix::from_registry(&reg);
    for &tgt in &[
        Dialect::OpenAi,
        Dialect::Claude,
        Dialect::Gemini,
        Dialect::Codex,
        Dialect::Kimi,
    ] {
        assert!(
            m.is_supported(Dialect::Copilot, tgt),
            "Copilot -> {tgt} should be supported"
        );
    }
}

// ── Feature constants ───────────────────────────────────────────────

#[test]
fn feature_constants_are_distinct() {
    let feats = all_features();
    let mut seen = std::collections::HashSet::new();
    for f in &feats {
        assert!(seen.insert(*f), "duplicate feature constant: {f}");
    }
}

#[test]
fn feature_constants_are_non_empty() {
    for f in all_features() {
        assert!(!f.is_empty());
    }
}

// ── Validate preserves feature order ────────────────────────────────

#[test]
fn validate_preserves_feature_order() {
    let reg = known_rules();
    let feats = vec![
        features::CODE_EXEC.to_string(),
        features::THINKING.to_string(),
        features::TOOL_USE.to_string(),
        features::STREAMING.to_string(),
    ];
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
    for (i, r) in results.iter().enumerate() {
        assert_eq!(r.feature, feats[i]);
    }
}

// ── Duplicate feature in validation ─────────────────────────────────

#[test]
fn validate_duplicate_features_returns_duplicate_results() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &[features::TOOL_USE.into(), features::TOOL_USE.into()],
    );
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].feature, results[1].feature);
    assert_eq!(results[0].fidelity, results[1].fidelity);
}
