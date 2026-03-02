// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive dialect detection and handling tests (80+).

use abp_dialect::{Dialect, DialectDetector, DialectValidator};
use abp_mapping::{
    Fidelity, MappingError, MappingMatrix, MappingRegistry, MappingRule, features, known_rules,
    validate_mapping,
};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════
// 1. Dialect enum variants
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dialect_all_returns_six_variants() {
    assert_eq!(Dialect::all().len(), 6);
}

#[test]
fn dialect_all_contains_openai() {
    assert!(Dialect::all().contains(&Dialect::OpenAi));
}

#[test]
fn dialect_all_contains_claude() {
    assert!(Dialect::all().contains(&Dialect::Claude));
}

#[test]
fn dialect_all_contains_gemini() {
    assert!(Dialect::all().contains(&Dialect::Gemini));
}

#[test]
fn dialect_all_contains_codex() {
    assert!(Dialect::all().contains(&Dialect::Codex));
}

#[test]
fn dialect_all_contains_kimi() {
    assert!(Dialect::all().contains(&Dialect::Kimi));
}

#[test]
fn dialect_all_contains_copilot() {
    assert!(Dialect::all().contains(&Dialect::Copilot));
}

#[test]
fn dialect_clone_is_identical() {
    let d = Dialect::Claude;
    let d2 = d;
    assert_eq!(d, d2);
}

#[test]
fn dialect_copy_semantics() {
    let d = Dialect::Gemini;
    let d2 = d;
    // Both usable after copy.
    assert_eq!(d.label(), d2.label());
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Dialect Display and label
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn display_openai() {
    assert_eq!(format!("{}", Dialect::OpenAi), "OpenAI");
}

#[test]
fn display_claude() {
    assert_eq!(format!("{}", Dialect::Claude), "Claude");
}

#[test]
fn display_gemini() {
    assert_eq!(format!("{}", Dialect::Gemini), "Gemini");
}

#[test]
fn display_codex() {
    assert_eq!(format!("{}", Dialect::Codex), "Codex");
}

#[test]
fn display_kimi() {
    assert_eq!(format!("{}", Dialect::Kimi), "Kimi");
}

#[test]
fn display_copilot() {
    assert_eq!(format!("{}", Dialect::Copilot), "Copilot");
}

#[test]
fn label_matches_display_for_all() {
    for &d in Dialect::all() {
        assert_eq!(format!("{d}"), d.label());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Dialect serde roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn serde_roundtrip_openai() {
    let s = serde_json::to_string(&Dialect::OpenAi).unwrap();
    assert_eq!(s, "\"open_ai\"");
    let back: Dialect = serde_json::from_str(&s).unwrap();
    assert_eq!(back, Dialect::OpenAi);
}

#[test]
fn serde_roundtrip_claude() {
    let s = serde_json::to_string(&Dialect::Claude).unwrap();
    assert_eq!(s, "\"claude\"");
    let back: Dialect = serde_json::from_str(&s).unwrap();
    assert_eq!(back, Dialect::Claude);
}

#[test]
fn serde_roundtrip_gemini() {
    let s = serde_json::to_string(&Dialect::Gemini).unwrap();
    assert_eq!(s, "\"gemini\"");
    let back: Dialect = serde_json::from_str(&s).unwrap();
    assert_eq!(back, Dialect::Gemini);
}

#[test]
fn serde_roundtrip_codex() {
    let s = serde_json::to_string(&Dialect::Codex).unwrap();
    assert_eq!(s, "\"codex\"");
    let back: Dialect = serde_json::from_str(&s).unwrap();
    assert_eq!(back, Dialect::Codex);
}

#[test]
fn serde_roundtrip_kimi() {
    let s = serde_json::to_string(&Dialect::Kimi).unwrap();
    assert_eq!(s, "\"kimi\"");
    let back: Dialect = serde_json::from_str(&s).unwrap();
    assert_eq!(back, Dialect::Kimi);
}

#[test]
fn serde_roundtrip_copilot() {
    let s = serde_json::to_string(&Dialect::Copilot).unwrap();
    assert_eq!(s, "\"copilot\"");
    let back: Dialect = serde_json::from_str(&s).unwrap();
    assert_eq!(back, Dialect::Copilot);
}

#[test]
fn serde_all_variants_roundtrip() {
    for &d in Dialect::all() {
        let json = serde_json::to_value(d).unwrap();
        let back: Dialect = serde_json::from_value(json).unwrap();
        assert_eq!(d, back);
    }
}

#[test]
fn serde_reject_unknown_variant() {
    let result = serde_json::from_str::<Dialect>("\"unknown_dialect\"");
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Dialect ordering and comparison
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dialect_eq_reflexive() {
    for &d in Dialect::all() {
        assert_eq!(d, d);
    }
}

#[test]
fn dialect_ne_different_variants() {
    assert_ne!(Dialect::OpenAi, Dialect::Claude);
    assert_ne!(Dialect::Claude, Dialect::Gemini);
    assert_ne!(Dialect::Gemini, Dialect::Codex);
    assert_ne!(Dialect::Codex, Dialect::Kimi);
    assert_ne!(Dialect::Kimi, Dialect::Copilot);
    assert_ne!(Dialect::Copilot, Dialect::OpenAi);
}

#[test]
fn dialect_hash_distinct() {
    use std::collections::HashSet;
    let set: HashSet<Dialect> = Dialect::all().iter().copied().collect();
    assert_eq!(set.len(), 6);
}

#[test]
fn dialect_debug_includes_variant_name() {
    let dbg = format!("{:?}", Dialect::OpenAi);
    assert!(dbg.contains("OpenAi"));
}

#[test]
fn dialect_debug_all_variants_non_empty() {
    for &d in Dialect::all() {
        assert!(!format!("{d:?}").is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Dialect feature matrix (which supports what via known_rules)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn feature_same_dialect_always_lossless() {
    let reg = known_rules();
    for &d in Dialect::all() {
        for &f in &[
            features::TOOL_USE,
            features::STREAMING,
            features::THINKING,
            features::IMAGE_INPUT,
            features::CODE_EXEC,
        ] {
            let rule = reg.lookup(d, d, f).expect(&format!("{d} self {f}"));
            assert!(rule.fidelity.is_lossless(), "{d}->{d} {f} not lossless");
        }
    }
}

#[test]
fn feature_streaming_lossless_across_original_four() {
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
            assert!(rule.fidelity.is_lossless(), "{a}->{b} streaming");
        }
    }
}

#[test]
fn feature_image_input_to_codex_unsupported() {
    let reg = known_rules();
    for &src in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
        let rule = reg
            .lookup(src, Dialect::Codex, features::IMAGE_INPUT)
            .unwrap();
        assert!(rule.fidelity.is_unsupported());
    }
}

#[test]
fn feature_thinking_cross_dialect_is_lossy() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING)
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
    assert!(!rule.fidelity.is_unsupported());
}

#[test]
fn feature_tool_use_openai_claude_bidirectional_lossless() {
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
fn feature_code_exec_kimi_to_any_unsupported() {
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
            "Kimi->{tgt} code_exec should be unsupported"
        );
    }
}

#[test]
fn feature_kimi_copilot_image_unsupported() {
    let reg = known_rules();
    for &nd in &[Dialect::Kimi, Dialect::Copilot] {
        for &od in &[
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Codex,
        ] {
            let rule = reg.lookup(nd, od, features::IMAGE_INPUT).unwrap();
            assert!(
                rule.fidelity.is_unsupported(),
                "{nd}->{od} image_input should be unsupported"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Model name to dialect resolution (detection heuristics)
// ═══════════════════════════════════════════════════════════════════════

fn detect(value: &serde_json::Value) -> Option<Dialect> {
    DialectDetector::new().detect(value).map(|r| r.dialect)
}

#[test]
fn detect_openai_by_choices_key() {
    assert_eq!(detect(&json!({"choices": [{}]})), Some(Dialect::OpenAi));
}

#[test]
fn detect_openai_by_messages_with_string_content() {
    let msg = json!({"model": "gpt-4o", "messages": [{"role": "user", "content": "hi"}]});
    assert_eq!(detect(&msg), Some(Dialect::OpenAi));
}

#[test]
fn detect_claude_by_type_message() {
    let msg = json!({"type": "message", "model": "claude-3"});
    assert_eq!(detect(&msg), Some(Dialect::Claude));
}

#[test]
fn detect_claude_by_array_content_blocks() {
    let msg = json!({
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
    });
    assert_eq!(detect(&msg), Some(Dialect::Claude));
}

#[test]
fn detect_gemini_by_contents_with_parts() {
    let msg = json!({"contents": [{"parts": [{"text": "hello"}]}]});
    assert_eq!(detect(&msg), Some(Dialect::Gemini));
}

#[test]
fn detect_gemini_by_candidates() {
    let msg = json!({"candidates": [{}]});
    assert_eq!(detect(&msg), Some(Dialect::Gemini));
}

#[test]
fn detect_codex_by_items_with_type() {
    let msg = json!({"items": [{"type": "message"}]});
    assert_eq!(detect(&msg), Some(Dialect::Codex));
}

#[test]
fn detect_codex_by_object_response() {
    let msg = json!({"object": "response", "status": "completed"});
    assert_eq!(detect(&msg), Some(Dialect::Codex));
}

#[test]
fn detect_kimi_by_search_plus() {
    let msg = json!({"search_plus": true});
    assert_eq!(detect(&msg), Some(Dialect::Kimi));
}

#[test]
fn detect_kimi_by_refs() {
    let msg = json!({"refs": ["https://example.com"]});
    assert_eq!(detect(&msg), Some(Dialect::Kimi));
}

#[test]
fn detect_copilot_by_references() {
    let msg = json!({"references": [{"type": "file"}]});
    assert_eq!(detect(&msg), Some(Dialect::Copilot));
}

#[test]
fn detect_copilot_by_agent_mode() {
    let msg = json!({"agent_mode": true});
    assert_eq!(detect(&msg), Some(Dialect::Copilot));
}

#[test]
fn detect_none_for_integer() {
    assert!(detect(&json!(42)).is_none());
}

#[test]
fn detect_none_for_string() {
    assert!(detect(&json!("hello")).is_none());
}

#[test]
fn detect_none_for_null() {
    assert!(detect(&json!(null)).is_none());
}

#[test]
fn detect_none_for_array() {
    assert!(detect(&json!([])).is_none());
}

#[test]
fn detect_none_for_empty_object() {
    assert!(detect(&json!({})).is_none());
}

#[test]
fn detect_all_empty_for_non_object() {
    let d = DialectDetector::new();
    assert!(d.detect_all(&json!("text")).is_empty());
}

#[test]
fn detect_all_sorted_descending_confidence() {
    let d = DialectDetector::new();
    let msg = json!({"model": "x", "messages": [{"role": "user", "content": "hi"}], "refs": ["a"]});
    let results = d.detect_all(&msg);
    for w in results.windows(2) {
        assert!(w[0].confidence >= w[1].confidence);
    }
}

#[test]
fn detect_confidence_capped_at_one() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "choices": [{}],
        "temperature": 0.7,
        "top_p": 0.9,
        "max_tokens": 100
    });
    let r = DialectDetector::new().detect(&msg).unwrap();
    assert!(r.confidence <= 1.0);
}

#[test]
fn detect_evidence_non_empty() {
    let msg = json!({"choices": [{}]});
    let r = DialectDetector::new().detect(&msg).unwrap();
    assert!(!r.evidence.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Dialect pair mapping combinations
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_insert_and_lookup() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
    });
    assert!(
        reg.lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
            .is_some()
    );
}

#[test]
fn registry_lookup_miss() {
    let reg = MappingRegistry::new();
    assert!(reg.lookup(Dialect::OpenAi, Dialect::Claude, "x").is_none());
}

#[test]
fn registry_insert_replaces() {
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
        fidelity: Fidelity::LossyLabeled {
            warning: "changed".into(),
        },
    });
    assert_eq!(reg.len(), 1);
    assert!(
        !reg.lookup(Dialect::OpenAi, Dialect::Claude, "f")
            .unwrap()
            .fidelity
            .is_lossless()
    );
}

#[test]
fn registry_len_and_is_empty() {
    let reg = MappingRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
}

#[test]
fn registry_iter_count() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "a".into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::Gemini,
        feature: "b".into(),
        fidelity: Fidelity::Lossless,
    });
    assert_eq!(reg.iter().count(), 2);
}

#[test]
fn matrix_empty_default_false() {
    let m = MappingMatrix::new();
    assert!(!m.is_supported(Dialect::OpenAi, Dialect::Claude));
    assert_eq!(m.get(Dialect::OpenAi, Dialect::Claude), None);
}

#[test]
fn matrix_set_and_check() {
    let mut m = MappingMatrix::new();
    m.set(Dialect::OpenAi, Dialect::Claude, true);
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
    assert!(!m.is_supported(Dialect::Claude, Dialect::OpenAi));
}

#[test]
fn matrix_from_registry_marks_supported_pairs() {
    let reg = known_rules();
    let m = MappingMatrix::from_registry(&reg);
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Claude));
    assert!(m.is_supported(Dialect::Claude, Dialect::OpenAi));
    assert!(m.is_supported(Dialect::OpenAi, Dialect::Gemini));
}

#[test]
fn matrix_from_registry_unsupported_only_not_marked() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Gemini,
        target_dialect: Dialect::Codex,
        feature: "image_input".into(),
        fidelity: Fidelity::Unsupported {
            reason: "nope".into(),
        },
    });
    let m = MappingMatrix::from_registry(&reg);
    assert!(!m.is_supported(Dialect::Gemini, Dialect::Codex));
}

#[test]
fn rank_targets_excludes_source() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::STREAMING]);
    assert!(!ranked.iter().any(|(d, _)| *d == Dialect::OpenAi));
}

#[test]
fn rank_targets_sorted_by_lossless_descending() {
    let reg = known_rules();
    let ranked = reg.rank_targets(
        Dialect::OpenAi,
        &[features::TOOL_USE, features::STREAMING, features::THINKING],
    );
    for w in ranked.windows(2) {
        assert!(w[0].1 >= w[1].1);
    }
}

#[test]
fn rank_targets_nonexistent_feature_returns_empty() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &["teleportation"]);
    assert!(ranked.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Compatibility checks between dialects
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_mapping_lossless_no_errors() {
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
fn validate_mapping_lossy_has_fidelity_loss() {
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
fn validate_mapping_unsupported_reports_error() {
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
fn validate_mapping_unknown_feature_unsupported() {
    let reg = known_rules();
    let results = validate_mapping(
        &reg,
        Dialect::OpenAi,
        Dialect::Claude,
        &["nonexistent".into()],
    );
    assert!(results[0].fidelity.is_unsupported());
}

#[test]
fn validate_mapping_empty_feature_name_invalid_input() {
    let reg = MappingRegistry::new();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &["".into()]);
    assert!(matches!(
        &results[0].errors[0],
        MappingError::InvalidInput { .. }
    ));
}

#[test]
fn validate_mapping_empty_features_list() {
    let reg = known_rules();
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &[]);
    assert!(results.is_empty());
}

#[test]
fn validate_mapping_multiple_features_mixed() {
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
        ],
    );
    assert_eq!(results.len(), 4);
    // tool_use and streaming are lossless
    assert!(results[0].errors.is_empty());
    assert!(results[1].errors.is_empty());
}

#[test]
fn fidelity_lossless_helpers() {
    assert!(Fidelity::Lossless.is_lossless());
    assert!(!Fidelity::Lossless.is_unsupported());
}

#[test]
fn fidelity_lossy_helpers() {
    let f = Fidelity::LossyLabeled {
        warning: "w".into(),
    };
    assert!(!f.is_lossless());
    assert!(!f.is_unsupported());
}

#[test]
fn fidelity_unsupported_helpers() {
    let f = Fidelity::Unsupported { reason: "r".into() };
    assert!(!f.is_lossless());
    assert!(f.is_unsupported());
}

#[test]
fn mapping_error_display_feature_unsupported() {
    let err = MappingError::FeatureUnsupported {
        feature: "logprobs".into(),
        from: Dialect::Claude,
        to: Dialect::Gemini,
    };
    let s = err.to_string();
    assert!(s.contains("logprobs"));
    assert!(s.contains("unsupported"));
}

#[test]
fn mapping_error_display_fidelity_loss() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "mapped lossy".into(),
    };
    assert!(err.to_string().contains("fidelity loss"));
}

#[test]
fn mapping_error_display_dialect_mismatch() {
    let err = MappingError::DialectMismatch {
        from: Dialect::OpenAi,
        to: Dialect::Codex,
    };
    assert!(err.to_string().contains("mismatch"));
}

#[test]
fn mapping_error_display_invalid_input() {
    let err = MappingError::InvalidInput {
        reason: "bad".into(),
    };
    assert!(err.to_string().contains("invalid input"));
}

#[test]
fn mapping_error_serde_roundtrip_all_variants() {
    let errors = vec![
        MappingError::FeatureUnsupported {
            feature: "img".into(),
            from: Dialect::OpenAi,
            to: Dialect::Codex,
        },
        MappingError::FidelityLoss {
            feature: "think".into(),
            warning: "lossy".into(),
        },
        MappingError::DialectMismatch {
            from: Dialect::Kimi,
            to: Dialect::Copilot,
        },
        MappingError::InvalidInput {
            reason: "empty".into(),
        },
    ];
    for err in errors {
        let json = serde_json::to_string(&err).unwrap();
        let back: MappingError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }
}

#[test]
fn fidelity_serde_roundtrip_all_variants() {
    let variants: Vec<Fidelity> = vec![
        Fidelity::Lossless,
        Fidelity::LossyLabeled {
            warning: "w".into(),
        },
        Fidelity::Unsupported { reason: "r".into() },
    ];
    for f in variants {
        let json = serde_json::to_string(&f).unwrap();
        let back: Fidelity = serde_json::from_str(&json).unwrap();
        assert_eq!(f, back);
    }
}

#[test]
fn mapping_rule_serde_roundtrip() {
    let rule = MappingRule {
        source_dialect: Dialect::Kimi,
        target_dialect: Dialect::Copilot,
        feature: "streaming".into(),
        fidelity: Fidelity::Lossless,
    };
    let json = serde_json::to_string(&rule).unwrap();
    let back: MappingRule = serde_json::from_str(&json).unwrap();
    assert_eq!(rule, back);
}

// ── Validation ──────────────────────────────────────────────────────

#[test]
fn validate_openai_valid_request() {
    let v = DialectValidator::new();
    let msg = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let r = v.validate(&msg, Dialect::OpenAi);
    assert!(r.valid);
}

#[test]
fn validate_openai_missing_model() {
    let v = DialectValidator::new();
    let msg = json!({"messages": [{"role": "user", "content": "hi"}]});
    let r = v.validate(&msg, Dialect::OpenAi);
    assert!(!r.valid);
}

#[test]
fn validate_claude_response_allows_missing_model() {
    let v = DialectValidator::new();
    let msg = json!({"type": "message", "content": [{"type": "text", "text": "hi"}]});
    let r = v.validate(&msg, Dialect::Claude);
    assert!(r.valid);
}

#[test]
fn validate_gemini_missing_parts_errors() {
    let v = DialectValidator::new();
    let msg = json!({"contents": [{"role": "user"}]});
    let r = v.validate(&msg, Dialect::Gemini);
    assert!(!r.valid);
}

#[test]
fn validate_codex_items_missing_type() {
    let v = DialectValidator::new();
    let msg = json!({"items": [{"content": "done"}]});
    let r = v.validate(&msg, Dialect::Codex);
    assert!(!r.valid);
}

#[test]
fn validate_non_object_always_fails() {
    let v = DialectValidator::new();
    for &d in Dialect::all() {
        let r = v.validate(&json!("not an object"), d);
        assert!(!r.valid, "non-object should fail for {d}");
    }
}

#[test]
fn known_rules_is_non_empty() {
    let reg = known_rules();
    assert!(!reg.is_empty());
    assert!(reg.len() > 30);
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
fn known_rules_kimi_copilot_streaming_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::STREAMING)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn known_rules_kimi_copilot_thinking_lossy() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Kimi, Dialect::Copilot, features::THINKING)
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
}

#[test]
fn features_constants_are_distinct() {
    let feats = [
        features::TOOL_USE,
        features::STREAMING,
        features::THINKING,
        features::IMAGE_INPUT,
        features::CODE_EXEC,
    ];
    let set: std::collections::HashSet<&str> = feats.iter().copied().collect();
    assert_eq!(set.len(), feats.len());
}
