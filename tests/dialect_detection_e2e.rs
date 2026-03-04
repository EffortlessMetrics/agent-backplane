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
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! End-to-end tests for the `abp-dialect` crate covering dialect detection,
//! validation, serde, Display/Debug, equality, and edge cases.

use std::collections::HashSet;

use abp_dialect::{Dialect, DialectDetector, DialectValidator, ValidationError};
use serde_json::{Value, json};

// ── Helpers ─────────────────────────────────────────────────────────────

fn detector() -> DialectDetector {
    DialectDetector::new()
}

fn validator() -> DialectValidator {
    DialectValidator::new()
}

fn detect(v: &Value) -> (Dialect, f64) {
    let r = detector().detect(v).expect("expected detection result");
    (r.dialect, r.confidence)
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Dialect enum — all 6 variants
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
fn dialect_all_no_duplicates() {
    let all = Dialect::all();
    let unique: HashSet<Dialect> = all.iter().copied().collect();
    assert_eq!(all.len(), unique.len());
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Dialect metadata — label()
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn label_openai() {
    assert_eq!(Dialect::OpenAi.label(), "OpenAI");
}

#[test]
fn label_claude() {
    assert_eq!(Dialect::Claude.label(), "Claude");
}

#[test]
fn label_gemini() {
    assert_eq!(Dialect::Gemini.label(), "Gemini");
}

#[test]
fn label_codex() {
    assert_eq!(Dialect::Codex.label(), "Codex");
}

#[test]
fn label_kimi() {
    assert_eq!(Dialect::Kimi.label(), "Kimi");
}

#[test]
fn label_copilot() {
    assert_eq!(Dialect::Copilot.label(), "Copilot");
}

#[test]
fn every_variant_has_non_empty_label() {
    for &d in Dialect::all() {
        assert!(!d.label().is_empty(), "{d:?} has empty label");
    }
}

#[test]
fn all_labels_are_unique() {
    let labels: HashSet<&str> = Dialect::all().iter().map(|d| d.label()).collect();
    assert_eq!(labels.len(), Dialect::all().len());
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Display and Debug traits
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
fn display_matches_label_for_all() {
    for &d in Dialect::all() {
        assert_eq!(format!("{d}"), d.label());
    }
}

#[test]
fn debug_contains_variant_name() {
    assert_eq!(format!("{:?}", Dialect::OpenAi), "OpenAi");
    assert_eq!(format!("{:?}", Dialect::Claude), "Claude");
    assert_eq!(format!("{:?}", Dialect::Gemini), "Gemini");
    assert_eq!(format!("{:?}", Dialect::Codex), "Codex");
    assert_eq!(format!("{:?}", Dialect::Kimi), "Kimi");
    assert_eq!(format!("{:?}", Dialect::Copilot), "Copilot");
}

#[test]
fn detector_implements_debug() {
    let d = detector();
    let s = format!("{d:?}");
    assert!(s.contains("DialectDetector"));
}

#[test]
fn validator_implements_debug() {
    let v = validator();
    let s = format!("{v:?}");
    assert!(s.contains("DialectValidator"));
}

#[test]
fn detection_result_implements_debug() {
    let msg = json!({"choices": [{}]});
    let r = detector().detect(&msg).unwrap();
    let s = format!("{r:?}");
    assert!(s.contains("DetectionResult"));
}

#[test]
fn validation_error_display() {
    let e = ValidationError {
        path: "/foo/bar".into(),
        message: "bad value".into(),
    };
    assert_eq!(format!("{e}"), "/foo/bar: bad value");
}

#[test]
fn validation_error_is_std_error() {
    let e = ValidationError {
        path: "/x".into(),
        message: "oops".into(),
    };
    let _: &dyn std::error::Error = &e;
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Serde serialization / deserialization
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn serde_openai_roundtrip() {
    let s = serde_json::to_string(&Dialect::OpenAi).unwrap();
    assert_eq!(s, "\"open_ai\"");
    let back: Dialect = serde_json::from_str(&s).unwrap();
    assert_eq!(back, Dialect::OpenAi);
}

#[test]
fn serde_claude_roundtrip() {
    let s = serde_json::to_string(&Dialect::Claude).unwrap();
    assert_eq!(s, "\"claude\"");
    let back: Dialect = serde_json::from_str(&s).unwrap();
    assert_eq!(back, Dialect::Claude);
}

#[test]
fn serde_gemini_roundtrip() {
    let s = serde_json::to_string(&Dialect::Gemini).unwrap();
    assert_eq!(s, "\"gemini\"");
    let back: Dialect = serde_json::from_str(&s).unwrap();
    assert_eq!(back, Dialect::Gemini);
}

#[test]
fn serde_codex_roundtrip() {
    let s = serde_json::to_string(&Dialect::Codex).unwrap();
    assert_eq!(s, "\"codex\"");
    let back: Dialect = serde_json::from_str(&s).unwrap();
    assert_eq!(back, Dialect::Codex);
}

#[test]
fn serde_kimi_roundtrip() {
    let s = serde_json::to_string(&Dialect::Kimi).unwrap();
    assert_eq!(s, "\"kimi\"");
    let back: Dialect = serde_json::from_str(&s).unwrap();
    assert_eq!(back, Dialect::Kimi);
}

#[test]
fn serde_copilot_roundtrip() {
    let s = serde_json::to_string(&Dialect::Copilot).unwrap();
    assert_eq!(s, "\"copilot\"");
    let back: Dialect = serde_json::from_str(&s).unwrap();
    assert_eq!(back, Dialect::Copilot);
}

#[test]
fn serde_all_variants_roundtrip() {
    for &d in Dialect::all() {
        let s = serde_json::to_string(&d).unwrap();
        let back: Dialect = serde_json::from_str(&s).unwrap();
        assert_eq!(back, d, "roundtrip failed for {d:?}");
    }
}

#[test]
fn serde_unknown_string_fails() {
    let result = serde_json::from_str::<Dialect>("\"unknown_dialect\"");
    assert!(result.is_err());
}

#[test]
fn serde_empty_string_fails() {
    let result = serde_json::from_str::<Dialect>("\"\"");
    assert!(result.is_err());
}

#[test]
fn serde_case_sensitive_uppercase_fails() {
    let result = serde_json::from_str::<Dialect>("\"CLAUDE\"");
    assert!(result.is_err());
}

#[test]
fn serde_case_sensitive_mixed_case_fails() {
    let result = serde_json::from_str::<Dialect>("\"Claude\"");
    assert!(result.is_err());
}

#[test]
fn serde_number_input_fails() {
    let result = serde_json::from_str::<Dialect>("42");
    assert!(result.is_err());
}

#[test]
fn serde_null_input_fails() {
    let result = serde_json::from_str::<Dialect>("null");
    assert!(result.is_err());
}

#[test]
fn serde_bool_input_fails() {
    let result = serde_json::from_str::<Dialect>("true");
    assert!(result.is_err());
}

#[test]
fn serde_in_json_object() {
    let obj = json!({"dialect": "claude"});
    #[derive(serde::Deserialize)]
    struct Wrapper {
        dialect: Dialect,
    }
    let w: Wrapper = serde_json::from_value(obj).unwrap();
    assert_eq!(w.dialect, Dialect::Claude);
}

#[test]
fn serde_in_json_array() {
    let arr = json!(["open_ai", "gemini", "kimi"]);
    let dialects: Vec<Dialect> = serde_json::from_value(arr).unwrap();
    assert_eq!(
        dialects,
        vec![Dialect::OpenAi, Dialect::Gemini, Dialect::Kimi]
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Equality, Copy, Clone, Hash
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn equality_same_variant() {
    assert_eq!(Dialect::OpenAi, Dialect::OpenAi);
    assert_eq!(Dialect::Claude, Dialect::Claude);
}

#[test]
fn inequality_different_variants() {
    assert_ne!(Dialect::OpenAi, Dialect::Claude);
    assert_ne!(Dialect::Gemini, Dialect::Codex);
    assert_ne!(Dialect::Kimi, Dialect::Copilot);
}

#[test]
fn clone_produces_equal_value() {
    for &d in Dialect::all() {
        #[allow(clippy::clone_on_copy)]
        let cloned = d.clone();
        assert_eq!(d, cloned);
    }
}

#[test]
fn copy_semantics() {
    let a = Dialect::Gemini;
    let b = a; // copy
    assert_eq!(a, b);
}

#[test]
fn hash_equal_for_same_variant() {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h1 = DefaultHasher::new();
    let mut h2 = DefaultHasher::new();
    Dialect::Claude.hash(&mut h1);
    Dialect::Claude.hash(&mut h2);
    assert_eq!(h1.finish(), h2.finish());
}

#[test]
fn hash_different_for_different_variants() {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h1 = DefaultHasher::new();
    let mut h2 = DefaultHasher::new();
    Dialect::OpenAi.hash(&mut h1);
    Dialect::Copilot.hash(&mut h2);
    // Not strictly guaranteed but extremely likely for distinct enum variants.
    assert_ne!(h1.finish(), h2.finish());
}

#[test]
fn hashset_deduplication() {
    let mut set = HashSet::new();
    set.insert(Dialect::Kimi);
    set.insert(Dialect::Kimi);
    set.insert(Dialect::Gemini);
    assert_eq!(set.len(), 2);
}

#[test]
fn all_six_in_hashset() {
    let set: HashSet<Dialect> = Dialect::all().iter().copied().collect();
    assert_eq!(set.len(), 6);
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Dialect detection — OpenAI payloads
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_openai_chat_request() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hello"}]
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
    assert!(c > 0.4);
}

#[test]
fn detect_openai_response_with_choices() {
    let msg = json!({
        "choices": [{"message": {"role": "assistant", "content": "hi"}}],
        "model": "gpt-4"
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
    assert!(c >= 0.5);
}

#[test]
fn detect_openai_with_temperature() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "x"}],
        "temperature": 0.7
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn detect_openai_with_top_p() {
    let msg = json!({
        "model": "gpt-3.5-turbo",
        "messages": [{"role": "user", "content": "test"}],
        "top_p": 0.9
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn detect_openai_with_max_tokens() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 512
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn detect_openai_minimal_choices() {
    let msg = json!({"choices": []});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn detect_openai_confidence_capped_at_one() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "choices": [{}],
        "temperature": 0.7,
        "top_p": 0.9,
        "max_tokens": 100
    });
    let (_, c) = detect(&msg);
    assert!(c <= 1.0);
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Dialect detection — Claude payloads
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_claude_request_with_array_content() {
    let msg = json!({
        "model": "claude-3-opus",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn detect_claude_response_type_message() {
    let msg = json!({
        "type": "message",
        "model": "claude-3-opus",
        "content": [{"type": "text", "text": "hi"}],
        "stop_reason": "end_turn"
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
    assert!(c > 0.5);
}

#[test]
fn detect_claude_minimal_type_message() {
    let msg = json!({"type": "message"});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn detect_claude_with_stop_reason_and_array_content() {
    let msg = json!({
        "stop_reason": "end_turn",
        "content": [{"type": "text", "text": "result"}]
    });
    let r = detector().detect(&msg).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Dialect detection — Gemini payloads
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_gemini_contents_with_parts() {
    let msg = json!({
        "contents": [{"parts": [{"text": "hello"}]}]
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
    assert!(c >= 0.5);
}

#[test]
fn detect_gemini_candidates_response() {
    let msg = json!({
        "candidates": [{"content": {"parts": [{"text": "world"}]}}]
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
    assert!(c >= 0.4);
}

#[test]
fn detect_gemini_with_generation_config_camel() {
    let msg = json!({
        "contents": [{"parts": [{"text": "hi"}]}],
        "generationConfig": {"temperature": 0.5}
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn detect_gemini_with_generation_config_snake() {
    let msg = json!({
        "contents": [{"parts": [{"text": "hi"}]}],
        "generation_config": {"temperature": 0.5}
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Dialect detection — Codex payloads
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_codex_items_with_type() {
    let msg = json!({
        "items": [{"type": "message", "content": "done"}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Codex);
}

#[test]
fn detect_codex_response_object() {
    let msg = json!({
        "object": "response",
        "status": "completed",
        "items": [{"type": "message"}]
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::Codex);
    assert!(c > 0.5);
}

#[test]
fn detect_codex_status_without_candidates() {
    let msg = json!({
        "status": "completed",
        "items": [{"type": "function_call", "name": "run"}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Codex);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Dialect detection — Kimi payloads
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_kimi_with_refs() {
    let msg = json!({
        "model": "kimi",
        "messages": [{"role": "user", "content": "search"}],
        "refs": ["https://example.com"]
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::Kimi);
    assert!(c > 0.5);
}

#[test]
fn detect_kimi_with_search_plus() {
    let msg = json!({
        "model": "kimi",
        "messages": [{"role": "user", "content": "hello"}],
        "search_plus": true
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Kimi);
}

#[test]
fn detect_kimi_refs_only() {
    let msg = json!({"refs": ["https://example.com"]});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Kimi);
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Dialect detection — Copilot payloads
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_copilot_with_references() {
    let msg = json!({
        "messages": [{"role": "user", "content": "fix bug"}],
        "references": [{"type": "file", "path": "src/main.rs"}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Copilot);
}

#[test]
fn detect_copilot_agent_mode() {
    let msg = json!({
        "messages": [{"role": "user", "content": "do it"}],
        "agent_mode": true,
        "confirmations": []
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::Copilot);
    assert!(c > 0.5);
}

#[test]
fn detect_copilot_references_only() {
    let msg = json!({"references": [{"type": "snippet"}]});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Copilot);
}

#[test]
fn detect_copilot_confirmations_only() {
    let msg = json!({"confirmations": [{"id": "c1"}]});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Copilot);
}

// ═══════════════════════════════════════════════════════════════════════
// 12. detect_all
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_all_returns_empty_for_non_object() {
    assert!(detector().detect_all(&json!(null)).is_empty());
    assert!(detector().detect_all(&json!(42)).is_empty());
    assert!(detector().detect_all(&json!("str")).is_empty());
    assert!(detector().detect_all(&json!([])).is_empty());
}

#[test]
fn detect_all_returns_empty_for_empty_object() {
    assert!(detector().detect_all(&json!({})).is_empty());
}

#[test]
fn detect_all_sorted_descending() {
    let msg = json!({
        "model": "x",
        "messages": [{"role": "user", "content": "hi"}],
        "refs": ["a"]
    });
    let results = detector().detect_all(&msg);
    for w in results.windows(2) {
        assert!(w[0].confidence >= w[1].confidence);
    }
}

#[test]
fn detect_all_multiple_matches_for_ambiguous() {
    let msg = json!({
        "model": "x",
        "messages": [{"role": "user", "content": "hi"}],
        "contents": [{"parts": [{"text": "hi"}]}]
    });
    let results = detector().detect_all(&msg);
    assert!(results.len() >= 2);
}

#[test]
fn detect_all_evidence_populated() {
    let msg = json!({"choices": [{}]});
    let results = detector().detect_all(&msg);
    assert!(!results.is_empty());
    for r in &results {
        assert!(!r.evidence.is_empty());
    }
}

#[test]
fn detect_all_first_matches_detect() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "temperature": 0.5
    });
    let best = detector().detect(&msg).unwrap();
    let all = detector().detect_all(&msg);
    assert_eq!(all[0].dialect, best.dialect);
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Edge cases — detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_none_for_integer() {
    assert!(detector().detect(&json!(42)).is_none());
}

#[test]
fn detect_none_for_string() {
    assert!(detector().detect(&json!("hello")).is_none());
}

#[test]
fn detect_none_for_null() {
    assert!(detector().detect(&json!(null)).is_none());
}

#[test]
fn detect_none_for_bool() {
    assert!(detector().detect(&json!(true)).is_none());
}

#[test]
fn detect_none_for_array() {
    assert!(detector().detect(&json!([1, 2, 3])).is_none());
}

#[test]
fn detect_none_for_empty_object() {
    assert!(detector().detect(&json!({})).is_none());
}

#[test]
fn detect_none_for_unknown_keys() {
    assert!(
        detector()
            .detect(&json!({"foo": "bar", "baz": 1}))
            .is_none()
    );
}

#[test]
fn detect_none_for_deeply_nested_signals() {
    // Detector only checks top-level keys.
    let msg = json!({"wrapper": {"choices": [{}]}});
    assert!(detector().detect(&msg).is_none());
}

#[test]
fn evidence_strings_are_nonempty() {
    let msg = json!({"choices": [{}], "model": "gpt-4"});
    let r = detector().detect(&msg).unwrap();
    for ev in &r.evidence {
        assert!(!ev.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Validation — OpenAI
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_openai_valid_request() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let r = validator().validate(&msg, Dialect::OpenAi);
    assert!(r.valid);
    assert!(r.errors.is_empty());
}

#[test]
fn validate_openai_missing_model() {
    let msg = json!({"messages": [{"role": "user", "content": "hi"}]});
    let r = validator().validate(&msg, Dialect::OpenAi);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.path == "/model"));
}

#[test]
fn validate_openai_missing_role_in_message() {
    let msg = json!({"model": "gpt-4", "messages": [{"content": "hi"}]});
    let r = validator().validate(&msg, Dialect::OpenAi);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.path.contains("role")));
}

#[test]
fn validate_openai_messages_not_array() {
    let msg = json!({"model": "gpt-4", "messages": "oops"});
    let r = validator().validate(&msg, Dialect::OpenAi);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.path == "/messages"));
}

#[test]
fn validate_openai_no_messages_gives_warning() {
    let msg = json!({"model": "gpt-4"});
    let r = validator().validate(&msg, Dialect::OpenAi);
    assert!(r.valid); // no hard error
    assert!(!r.warnings.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Validation — Claude
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_claude_valid_request() {
    let msg = json!({
        "model": "claude-3-opus",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
    });
    let r = validator().validate(&msg, Dialect::Claude);
    assert!(r.valid);
}

#[test]
fn validate_claude_response_type_message_no_model() {
    let msg = json!({
        "type": "message",
        "content": [{"type": "text", "text": "hi"}],
        "stop_reason": "end_turn"
    });
    let r = validator().validate(&msg, Dialect::Claude);
    assert!(r.valid);
}

#[test]
fn validate_claude_missing_model_without_type_message() {
    let msg = json!({"messages": [{"role": "user", "content": "hi"}]});
    let r = validator().validate(&msg, Dialect::Claude);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.path == "/model"));
}

#[test]
fn validate_claude_bad_content_type() {
    let msg = json!({
        "model": "claude-3",
        "messages": [{"role": "user", "content": 42}]
    });
    let r = validator().validate(&msg, Dialect::Claude);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.path.contains("content")));
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Validation — Gemini
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_gemini_valid_contents() {
    let msg = json!({"contents": [{"parts": [{"text": "hi"}]}]});
    let r = validator().validate(&msg, Dialect::Gemini);
    assert!(r.valid);
}

#[test]
fn validate_gemini_missing_parts() {
    let msg = json!({"contents": [{"role": "user"}]});
    let r = validator().validate(&msg, Dialect::Gemini);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.path.contains("parts")));
}

#[test]
fn validate_gemini_contents_not_array() {
    let msg = json!({"contents": "bad"});
    let r = validator().validate(&msg, Dialect::Gemini);
    assert!(!r.valid);
}

#[test]
fn validate_gemini_candidates_only_is_valid() {
    let msg = json!({"candidates": [{}]});
    let r = validator().validate(&msg, Dialect::Gemini);
    assert!(r.valid); // no contents but candidates present = no warning
}

#[test]
fn validate_gemini_no_contents_no_candidates_warns() {
    let msg = json!({"generationConfig": {}});
    let r = validator().validate(&msg, Dialect::Gemini);
    assert!(r.valid);
    assert!(!r.warnings.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Validation — Codex
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_codex_valid() {
    let msg = json!({"items": [{"type": "message"}], "status": "completed"});
    let r = validator().validate(&msg, Dialect::Codex);
    assert!(r.valid);
}

#[test]
fn validate_codex_item_missing_type() {
    let msg = json!({"items": [{"content": "x"}]});
    let r = validator().validate(&msg, Dialect::Codex);
    assert!(!r.valid);
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Validation — Kimi
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_kimi_valid() {
    let msg = json!({"messages": [{"role": "user", "content": "hi"}], "refs": []});
    let r = validator().validate(&msg, Dialect::Kimi);
    assert!(r.valid);
}

#[test]
fn validate_kimi_missing_role() {
    let msg = json!({"messages": [{"content": "hi"}]});
    let r = validator().validate(&msg, Dialect::Kimi);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.path.contains("role")));
}

// ═══════════════════════════════════════════════════════════════════════
// 19. Validation — Copilot
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_copilot_valid() {
    let msg = json!({
        "messages": [{"role": "user", "content": "hi"}],
        "references": []
    });
    let r = validator().validate(&msg, Dialect::Copilot);
    assert!(r.valid);
}

#[test]
fn validate_copilot_missing_role() {
    let msg = json!({"messages": [{"content": "hi"}]});
    let r = validator().validate(&msg, Dialect::Copilot);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.path.contains("role")));
}

// ═══════════════════════════════════════════════════════════════════════
// 20. Validation — cross-cutting
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_non_object_always_fails() {
    for &d in Dialect::all() {
        let r = validator().validate(&json!("not an object"), d);
        assert!(!r.valid, "expected failure for {d:?}");
        assert_eq!(r.errors[0].path, "/");
    }
}

#[test]
fn validate_null_always_fails() {
    for &d in Dialect::all() {
        let r = validator().validate(&json!(null), d);
        assert!(!r.valid, "expected failure for {d:?} on null");
    }
}

#[test]
fn validate_array_always_fails() {
    for &d in Dialect::all() {
        let r = validator().validate(&json!([1, 2]), d);
        assert!(!r.valid, "expected failure for {d:?} on array");
    }
}

#[test]
fn validation_error_equality() {
    let a = ValidationError {
        path: "/model".into(),
        message: "missing".into(),
    };
    let b = ValidationError {
        path: "/model".into(),
        message: "missing".into(),
    };
    let c = ValidationError {
        path: "/other".into(),
        message: "missing".into(),
    };
    assert_eq!(a, b);
    assert_ne!(a, c);
}

// ═══════════════════════════════════════════════════════════════════════
// 21. Default trait
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detector_default() {
    let d = DialectDetector::default();
    // Smoke: default detector works
    assert!(d.detect(&json!({})).is_none());
}

#[test]
fn validator_default() {
    let v = DialectValidator::default();
    let r = v.validate(&json!({}), Dialect::OpenAi);
    // Empty object → missing model error
    assert!(!r.valid);
}
