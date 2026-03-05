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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for request fingerprinting in `abp_dialect::detect`.

use std::collections::BTreeMap;

use abp_dialect::detect::{
    builtin_fingerprints, detect_dialect, detect_from_headers, DialectDetectionResult,
    DialectFingerprint,
};
use abp_dialect::Dialect;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════
// Helper
// ═══════════════════════════════════════════════════════════════════════

fn headers(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Model-prefix detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_openai_from_gpt4o_model() {
    let v = json!({"model": "gpt-4o", "messages": [{"role": "user", "content": "hi"}]});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::OpenAi);
    assert!(r.confidence > 0.4);
}

#[test]
fn detect_openai_from_gpt4_turbo_model() {
    let v =
        json!({"model": "gpt-4-turbo-2024-04-09", "messages": [{"role": "user", "content": "x"}]});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn detect_openai_from_o1_model() {
    let v = json!({"model": "o1-preview", "messages": [{"role": "user", "content": "x"}]});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn detect_claude_from_claude3_model() {
    let v =
        json!({"model": "claude-3-opus-20240229", "messages": [{"role": "user", "content": "x"}]});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn detect_claude_from_claude_sonnet_model() {
    let v = json!({"model": "claude-sonnet-4-20250514", "system": "be nice", "messages": [{"role": "user", "content": "x"}]});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn detect_gemini_from_gemini_model() {
    let v = json!({"model": "gemini-1.5-pro", "contents": [{"parts": [{"text": "hi"}]}]});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_gemini_from_models_prefix() {
    let v = json!({"model": "models/gemini-1.5-flash", "contents": [{"parts": [{"text": "hi"}]}]});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_codex_from_codex_model() {
    let v = json!({"model": "codex-mini", "items": [{"type": "message"}]});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::Codex);
}

#[test]
fn detect_kimi_from_moonshot_model() {
    let v = json!({"model": "moonshot-v1", "refs": ["https://example.com"]});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::Kimi);
}

#[test]
fn detect_kimi_from_kimi_model() {
    let v = json!({"model": "kimi", "refs": ["https://example.com"]});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::Kimi);
}

#[test]
fn detect_copilot_from_copilot_model() {
    let v = json!({"model": "copilot-gpt-4", "references": [{"type": "file"}]});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::Copilot);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Field-marker detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_openai_from_choices_field() {
    let v = json!({"choices": [{"message": {"role": "assistant", "content": "hi"}}]});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn detect_openai_from_messages_field() {
    let v = json!({"messages": [{"role": "user", "content": "hi"}], "frequency_penalty": 0.5});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn detect_gemini_from_contents_field() {
    let v = json!({"contents": [{"parts": [{"text": "hi"}]}]});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_gemini_from_candidates_field() {
    let v = json!({"candidates": [{"content": {"parts": [{"text": "hi"}]}}]});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_gemini_from_generation_config() {
    let v =
        json!({"contents": [{"parts": [{"text": "x"}]}], "generationConfig": {"temperature": 0.5}});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_codex_from_items_field() {
    let v = json!({"items": [{"type": "message"}], "instructions": "do it"});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::Codex);
}

#[test]
fn detect_kimi_from_refs_field() {
    let v = json!({"refs": ["https://example.com"], "search_plus": true});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::Kimi);
}

#[test]
fn detect_copilot_from_references_field() {
    let v = json!({"references": [{"type": "file", "path": "main.rs"}]});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn detect_copilot_from_agent_mode_field() {
    let v = json!({"agent_mode": true, "confirmations": []});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::Copilot);
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Header-based detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_claude_from_anthropic_version_header() {
    let h = headers(&[("anthropic-version", "2023-06-01")]);
    let r = detect_from_headers(&h).expect("should detect");
    assert_eq!(r.dialect, Dialect::Claude);
    assert!(r.confidence > 0.0);
}

#[test]
fn detect_claude_from_x_api_key_header() {
    let h = headers(&[("x-api-key", "sk-ant-api03-xxx")]);
    let r = detect_from_headers(&h).expect("should detect");
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn detect_gemini_from_goog_api_key_header() {
    let h = headers(&[("x-goog-api-key", "AIza-xxx")]);
    let r = detect_from_headers(&h).expect("should detect");
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_copilot_from_github_token_header() {
    let h = headers(&[("x-github-token", "ghu_xxxx")]);
    let r = detect_from_headers(&h).expect("should detect");
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn detect_copilot_from_integration_id_header() {
    let h = headers(&[("copilot-integration-id", "my-ext")]);
    let r = detect_from_headers(&h).expect("should detect");
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn headers_none_for_empty() {
    assert!(detect_from_headers(&BTreeMap::new()).is_none());
}

#[test]
fn headers_none_for_unknown() {
    let h = headers(&[("x-custom-header", "value")]);
    assert!(detect_from_headers(&h).is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Ambiguous / low-confidence
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ambiguous_messages_only_returns_low_confidence() {
    // "messages" is shared by OpenAI, Claude, Kimi — confidence should be modest.
    let v = json!({"messages": [{"role": "user", "content": "hi"}]});
    let r = detect_dialect(&v).expect("should detect something");
    assert!(
        r.confidence < 0.6,
        "ambiguous request confidence {} too high",
        r.confidence
    );
}

#[test]
fn unknown_request_returns_none() {
    let v = json!({"foo": "bar", "baz": 42});
    assert!(detect_dialect(&v).is_none());
}

#[test]
fn non_object_returns_none() {
    assert!(detect_dialect(&json!(42)).is_none());
    assert!(detect_dialect(&json!("hello")).is_none());
    assert!(detect_dialect(&json!(null)).is_none());
    assert!(detect_dialect(&json!([1, 2, 3])).is_none());
}

#[test]
fn empty_object_returns_none() {
    assert!(detect_dialect(&json!({})).is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Combined evidence increases confidence
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn combined_model_and_fields_increases_openai_confidence() {
    let minimal = json!({"model": "gpt-4"});
    let full = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "choices": [{}],
        "frequency_penalty": 0.0
    });
    let c_min = detect_dialect(&minimal).expect("should detect").confidence;
    let c_full = detect_dialect(&full).expect("should detect").confidence;
    assert!(
        c_full > c_min,
        "full ({c_full}) should exceed minimal ({c_min})"
    );
}

#[test]
fn combined_model_and_fields_increases_gemini_confidence() {
    let minimal = json!({"model": "gemini-1.5-pro"});
    let full = json!({
        "model": "gemini-1.5-pro",
        "contents": [{"parts": [{"text": "hi"}]}],
        "generationConfig": {"temperature": 0.5},
        "safetySettings": []
    });
    let c_min = detect_dialect(&minimal).expect("should detect").confidence;
    let c_full = detect_dialect(&full).expect("should detect").confidence;
    assert!(c_full > c_min);
}

#[test]
fn combined_model_and_fields_increases_claude_confidence() {
    let minimal = json!({"model": "claude-3-opus"});
    let full = json!({
        "model": "claude-3-opus",
        "system": "be nice",
        "stop_reason": "end_turn"
    });
    let c_min = detect_dialect(&minimal).expect("should detect").confidence;
    let c_full = detect_dialect(&full).expect("should detect").confidence;
    assert!(c_full > c_min);
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Evidence population
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn evidence_contains_model_match() {
    let v = json!({"model": "gpt-4o"});
    let r = detect_dialect(&v).expect("should detect");
    assert!(r.evidence.iter().any(|e| e.contains("gpt-")));
}

#[test]
fn evidence_contains_field_match() {
    let v = json!({"contents": [{"parts": [{"text": "hi"}]}]});
    let r = detect_dialect(&v).expect("should detect");
    assert!(r.evidence.iter().any(|e| e.contains("contents")));
}

#[test]
fn header_evidence_contains_key() {
    let h = headers(&[("anthropic-version", "2023-06-01")]);
    let r = detect_from_headers(&h).expect("should detect");
    assert!(r.evidence.iter().any(|e| e.contains("anthropic-version")));
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Confidence bounds
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn confidence_never_exceeds_one() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}],
        "choices": [{}],
        "frequency_penalty": 0.0,
        "presence_penalty": 0.0
    });
    let r = detect_dialect(&v).expect("should detect");
    assert!(r.confidence <= 1.0);
}

#[test]
fn confidence_always_positive_when_detected() {
    let v = json!({"choices": [{}]});
    let r = detect_dialect(&v).expect("should detect");
    assert!(r.confidence > 0.0);
}

// ═══════════════════════════════════════════════════════════════════════
// 8. DialectFingerprint & DialectDetectionResult serialization
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fingerprint_serde_roundtrip() {
    let fp = DialectFingerprint {
        model_prefix_patterns: vec!["gpt-".into()],
        field_markers: vec!["messages".into()],
        header_markers: vec![("authorization".into(), "Bearer ".into())],
    };
    let json = serde_json::to_string(&fp).unwrap();
    let back: DialectFingerprint = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model_prefix_patterns, fp.model_prefix_patterns);
    assert_eq!(back.field_markers, fp.field_markers);
    assert_eq!(back.header_markers, fp.header_markers);
}

#[test]
fn detection_result_serde_roundtrip() {
    let r = DialectDetectionResult {
        dialect: Dialect::Claude,
        confidence: 0.85,
        evidence: vec!["model match".into()],
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: DialectDetectionResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.dialect, Dialect::Claude);
    assert!((back.confidence - 0.85).abs() < f64::EPSILON);
    assert_eq!(back.evidence, r.evidence);
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Builtin fingerprints coverage
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn builtin_fingerprints_cover_all_dialects() {
    let fps = builtin_fingerprints();
    for d in Dialect::all() {
        assert!(fps.contains_key(d), "missing fingerprint for {d:?}");
    }
}

#[test]
fn every_fingerprint_has_at_least_one_marker() {
    let fps = builtin_fingerprints();
    for (dialect, fp) in &fps {
        let total =
            fp.model_prefix_patterns.len() + fp.field_markers.len() + fp.header_markers.len();
        assert!(total > 0, "{dialect:?} fingerprint has no markers");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Model prefix is case-insensitive
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn model_prefix_case_insensitive() {
    let v = json!({"model": "GPT-4o"});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn model_prefix_case_insensitive_claude() {
    let v = json!({"model": "Claude-3-Opus"});
    let r = detect_dialect(&v).expect("should detect");
    assert_eq!(r.dialect, Dialect::Claude);
}
