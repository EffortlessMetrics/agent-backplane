#![allow(clippy::all)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive dialect detection, validation, and conversion tests (120+).

use std::collections::BTreeMap;

use abp_dialect::detect::{
    DialectDetectionResult, DialectFingerprint, builtin_fingerprints, detect_dialect,
    detect_from_headers,
};
use abp_dialect::ir::{
    IrContentBlock, IrGenerationConfig, IrMessage, IrRequest, IrResponse, IrRole, IrStopReason,
    IrToolDefinition, IrUsage,
};
use abp_dialect::registry::{DialectError, DialectRegistry};
use abp_dialect::validate::{RequestValidator, Severity, ValidationIssue};
use abp_dialect::{Dialect, DialectDetector, DialectValidator};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════
// 1. Dialect enum variants (10 tests)
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
    assert_eq!(d.label(), d2.label());
}

#[test]
fn dialect_hash_distinct() {
    use std::collections::HashSet;
    let set: HashSet<Dialect> = Dialect::all().iter().copied().collect();
    assert_eq!(set.len(), 6);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Dialect Display and label (8 tests)
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

#[test]
fn dialect_debug_includes_variant_name() {
    let dbg = format!("{:?}", Dialect::OpenAi);
    assert!(dbg.contains("OpenAi"));
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Dialect serde roundtrip (9 tests)
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

#[test]
fn dialect_ne_different_variants() {
    assert_ne!(Dialect::OpenAi, Dialect::Claude);
    assert_ne!(Dialect::Claude, Dialect::Gemini);
    assert_ne!(Dialect::Gemini, Dialect::Codex);
    assert_ne!(Dialect::Codex, Dialect::Kimi);
    assert_ne!(Dialect::Kimi, Dialect::Copilot);
    assert_ne!(Dialect::Copilot, Dialect::OpenAi);
}

// ═══════════════════════════════════════════════════════════════════════
// 4. DialectDetector — detection from request payloads (24 tests)
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
fn detect_openai_gpt_model_prefix() {
    let msg = json!({"model": "gpt-3.5-turbo", "messages": [{"role": "user", "content": "x"}]});
    assert_eq!(detect(&msg), Some(Dialect::OpenAi));
}

#[test]
fn detect_openai_chatgpt_model_prefix() {
    let msg = json!({"model": "chatgpt-4o-latest", "messages": [{"role": "user", "content": "x"}]});
    assert_eq!(detect(&msg), Some(Dialect::OpenAi));
}

#[test]
fn detect_claude_by_type_message() {
    let msg = json!({"type": "message", "model": "claude-3"});
    assert_eq!(detect(&msg), Some(Dialect::Claude));
}

#[test]
fn detect_claude_by_model_prefix() {
    let msg = json!({"model": "claude-3-opus-20240229", "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]});
    assert_eq!(detect(&msg), Some(Dialect::Claude));
}

#[test]
fn detect_claude_by_array_content_blocks() {
    let msg = json!({"messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]});
    assert_eq!(detect(&msg), Some(Dialect::Claude));
}

#[test]
fn detect_claude_response_with_stop_reason() {
    let msg = json!({"type": "message", "model": "claude-3", "content": [{"type": "text", "text": "hi"}], "stop_reason": "end_turn"});
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
fn detect_gemini_by_model_prefix() {
    let msg = json!({"model": "gemini-1.5-pro", "contents": [{"parts": [{"text": "hi"}]}]});
    assert_eq!(detect(&msg), Some(Dialect::Gemini));
}

#[test]
fn detect_gemini_models_slash_prefix() {
    let msg =
        json!({"model": "models/gemini-1.5-flash", "contents": [{"parts": [{"text": "hi"}]}]});
    assert_eq!(detect(&msg), Some(Dialect::Gemini));
}

#[test]
fn detect_gemini_generation_config() {
    let msg = json!({"contents": [{"parts": [{"text": "hi"}]}], "generationConfig": {"temperature": 0.5}});
    assert_eq!(detect(&msg), Some(Dialect::Gemini));
}

#[test]
fn detect_codex_by_items_with_type() {
    let msg = json!({"items": [{"type": "message"}]});
    assert_eq!(detect(&msg), Some(Dialect::Codex));
}

#[test]
fn detect_codex_by_object_response() {
    let msg = json!({"object": "response", "status": "completed", "items": [{"type": "msg"}]});
    assert_eq!(detect(&msg), Some(Dialect::Codex));
}

#[test]
fn detect_codex_by_model_prefix() {
    let msg = json!({"model": "codex-mini-latest", "items": [{"type": "message"}], "status": "completed"});
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
fn detect_kimi_by_moonshot_model() {
    let msg = json!({"model": "moonshot-v1-32k", "messages": [{"role": "user", "content": "hi"}], "refs": ["a"]});
    assert_eq!(detect(&msg), Some(Dialect::Kimi));
}

#[test]
fn detect_kimi_by_kimi_model() {
    let msg = json!({"model": "kimi", "messages": [{"role": "user", "content": "hi"}], "search_plus": true});
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
fn detect_copilot_by_confirmations() {
    let msg = json!({"confirmations": [], "references": [{"type": "file"}]});
    assert_eq!(detect(&msg), Some(Dialect::Copilot));
}

#[test]
fn detect_copilot_model_prefix() {
    let msg = json!({"model": "copilot-chat", "messages": [{"role": "user", "content": "x"}], "references": [{"type": "file"}]});
    assert_eq!(detect(&msg), Some(Dialect::Copilot));
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Ambiguous / unknown dialect handling (7 tests)
// ═══════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════
// 6. Confidence and evidence (4 tests)
// ═══════════════════════════════════════════════════════════════════════

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

#[test]
fn detect_all_returns_multiple_for_ambiguous_payload() {
    let msg = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}], "temperature": 0.7});
    let results = DialectDetector::new().detect_all(&msg);
    assert!(!results.is_empty());
    assert_eq!(results[0].dialect, Dialect::OpenAi);
}

#[test]
fn detect_confidence_positive_for_strong_signals() {
    let msg = json!({"model": "claude-3-sonnet", "type": "message"});
    let r = DialectDetector::new().detect(&msg).unwrap();
    assert!(r.confidence > 0.3);
}

// ═══════════════════════════════════════════════════════════════════════
// 7. detect_dialect (detect module free function) (4 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_dialect_openai_via_free_fn() {
    let v = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "x"}]});
    let r = detect_dialect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn detect_dialect_none_for_non_object() {
    assert!(detect_dialect(&json!(42)).is_none());
}

#[test]
fn detect_dialect_none_for_empty_object() {
    assert!(detect_dialect(&json!({})).is_none());
}

#[test]
fn detect_dialect_claude_via_model_prefix() {
    let v = json!({"model": "claude-3-opus", "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]});
    let r = detect_dialect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

// ═══════════════════════════════════════════════════════════════════════
// 8. detect_from_headers (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn headers_detect_claude_anthropic_version() {
    let mut h = BTreeMap::new();
    h.insert("anthropic-version".into(), "2023-06-01".into());
    let r = detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn headers_detect_gemini_goog_api_key() {
    let mut h = BTreeMap::new();
    h.insert("x-goog-api-key".into(), "AIza...".into());
    let r = detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn headers_detect_copilot_github_token() {
    let mut h = BTreeMap::new();
    h.insert("x-github-token".into(), "ghu_xxx".into());
    let r = detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn headers_detect_openai_organization() {
    let mut h = BTreeMap::new();
    h.insert("openai-organization".into(), "org-123".into());
    let r = detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn headers_detect_none_for_empty() {
    let h = BTreeMap::new();
    assert!(detect_from_headers(&h).is_none());
}

#[test]
fn headers_detect_none_for_unrelated_headers() {
    let mut h = BTreeMap::new();
    h.insert("content-type".into(), "application/json".into());
    assert!(detect_from_headers(&h).is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 9. detect_from_endpoint (8 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn endpoint_detect_anthropic() {
    let d = DialectDetector::new();
    let r = d
        .detect_from_endpoint("https://api.anthropic.com/v1/messages")
        .unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn endpoint_detect_gemini() {
    let d = DialectDetector::new();
    let r = d
        .detect_from_endpoint(
            "https://generativelanguage.googleapis.com/v1/models/gemini-pro:generateContent",
        )
        .unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn endpoint_detect_moonshot_kimi() {
    let d = DialectDetector::new();
    let r = d
        .detect_from_endpoint("https://api.moonshot.cn/v1/chat/completions")
        .unwrap();
    assert_eq!(r.dialect, Dialect::Kimi);
}

#[test]
fn endpoint_detect_openai_chat() {
    let d = DialectDetector::new();
    let r = d
        .detect_from_endpoint("https://api.openai.com/v1/chat/completions")
        .unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn endpoint_detect_openai_responses_as_codex() {
    let d = DialectDetector::new();
    let r = d
        .detect_from_endpoint("https://api.openai.com/v1/responses")
        .unwrap();
    assert_eq!(r.dialect, Dialect::Codex);
}

#[test]
fn endpoint_detect_github_copilot() {
    let d = DialectDetector::new();
    let r = d
        .detect_from_endpoint("https://api.github.com/copilot/chat/completions")
        .unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn endpoint_detect_none_for_unknown_url() {
    let d = DialectDetector::new();
    assert!(
        d.detect_from_endpoint("https://example.com/api/chat")
            .is_none()
    );
}

#[test]
fn endpoint_confidence_is_reasonable() {
    let d = DialectDetector::new();
    let r = d
        .detect_from_endpoint("https://api.anthropic.com/v1/messages")
        .unwrap();
    assert!(r.confidence >= 0.5 && r.confidence <= 1.0);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. DialectDetector from_headers method (4 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detector_headers_detect_claude() {
    let d = DialectDetector::new();
    let mut h = BTreeMap::new();
    h.insert("anthropic-version".into(), "2023-06-01".into());
    h.insert("x-api-key".into(), "sk-ant-xxx".into());
    let r = d.detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn detector_headers_detect_copilot_integration_id() {
    let d = DialectDetector::new();
    let mut h = BTreeMap::new();
    h.insert("copilot-integration-id".into(), "vscode".into());
    let r = d.detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn detector_headers_empty_returns_none() {
    let d = DialectDetector::new();
    assert!(d.detect_from_headers(&BTreeMap::new()).is_none());
}

#[test]
fn detector_headers_openai_project() {
    let d = DialectDetector::new();
    let mut h = BTreeMap::new();
    h.insert("openai-project".into(), "proj_123".into());
    let r = d.detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

// ═══════════════════════════════════════════════════════════════════════
// 11. DialectValidator (lib.rs) validation for each dialect (14 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_openai_valid_request() {
    let v = DialectValidator::new();
    let msg = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let r = v.validate(&msg, Dialect::OpenAi);
    assert!(r.valid);
    assert!(r.errors.is_empty());
}

#[test]
fn validate_openai_missing_model() {
    let v = DialectValidator::new();
    let msg = json!({"messages": [{"role": "user", "content": "hi"}]});
    let r = v.validate(&msg, Dialect::OpenAi);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.path == "/model"));
}

#[test]
fn validate_openai_missing_role() {
    let v = DialectValidator::new();
    let msg = json!({"model": "gpt-4", "messages": [{"content": "hi"}]});
    let r = v.validate(&msg, Dialect::OpenAi);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.path.contains("role")));
}

#[test]
fn validate_openai_non_object_returns_error() {
    let v = DialectValidator::new();
    let r = v.validate(&json!("oops"), Dialect::OpenAi);
    assert!(!r.valid);
    assert_eq!(r.errors[0].path, "/");
}

#[test]
fn validate_claude_valid_request() {
    let v = DialectValidator::new();
    let msg = json!({"model": "claude-3-opus", "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]});
    let r = v.validate(&msg, Dialect::Claude);
    assert!(r.valid);
}

#[test]
fn validate_claude_response_without_model_is_valid() {
    let v = DialectValidator::new();
    let msg = json!({"type": "message", "content": [{"type": "text", "text": "hi"}], "stop_reason": "end_turn"});
    let r = v.validate(&msg, Dialect::Claude);
    assert!(r.valid);
}

#[test]
fn validate_claude_bad_content_type() {
    let v = DialectValidator::new();
    let msg = json!({"model": "claude-3", "messages": [{"role": "user", "content": 42}]});
    let r = v.validate(&msg, Dialect::Claude);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.path.contains("content")));
}

#[test]
fn validate_gemini_valid() {
    let v = DialectValidator::new();
    let msg = json!({"contents": [{"parts": [{"text": "hi"}]}]});
    let r = v.validate(&msg, Dialect::Gemini);
    assert!(r.valid);
}

#[test]
fn validate_gemini_missing_parts() {
    let v = DialectValidator::new();
    let msg = json!({"contents": [{"role": "user"}]});
    let r = v.validate(&msg, Dialect::Gemini);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.path.contains("parts")));
}

#[test]
fn validate_codex_valid() {
    let v = DialectValidator::new();
    let msg = json!({"items": [{"type": "message", "content": "done"}], "status": "completed"});
    let r = v.validate(&msg, Dialect::Codex);
    assert!(r.valid);
}

#[test]
fn validate_codex_item_missing_type() {
    let v = DialectValidator::new();
    let msg = json!({"items": [{"content": "done"}]});
    let r = v.validate(&msg, Dialect::Codex);
    assert!(!r.valid);
}

#[test]
fn validate_kimi_valid() {
    let v = DialectValidator::new();
    let msg = json!({"model": "kimi", "messages": [{"role": "user", "content": "hi"}]});
    let r = v.validate(&msg, Dialect::Kimi);
    assert!(r.valid);
}

#[test]
fn validate_kimi_missing_role() {
    let v = DialectValidator::new();
    let msg = json!({"messages": [{"content": "hi"}]});
    let r = v.validate(&msg, Dialect::Kimi);
    assert!(!r.valid);
}

#[test]
fn validate_copilot_valid() {
    let v = DialectValidator::new();
    let msg = json!({"model": "copilot", "messages": [{"role": "user", "content": "hi"}]});
    let r = v.validate(&msg, Dialect::Copilot);
    assert!(r.valid);
}

// ═══════════════════════════════════════════════════════════════════════
// 12. RequestValidator (validate module) (10 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn req_validator_openai_valid() {
    let v = RequestValidator::new();
    let r = v.validate(
        Dialect::OpenAi,
        &json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]}),
    );
    assert!(r.is_valid());
    assert_eq!(r.error_count(), 0);
}

#[test]
fn req_validator_openai_missing_model_and_messages() {
    let v = RequestValidator::new();
    let r = v.validate(Dialect::OpenAi, &json!({}));
    assert!(!r.is_valid());
    assert!(r.error_count() >= 2);
}

#[test]
fn req_validator_openai_unknown_role_warning() {
    let v = RequestValidator::new();
    let r = v.validate(
        Dialect::OpenAi,
        &json!({"model": "gpt-4", "messages": [{"role": "narrator", "content": "hi"}]}),
    );
    assert!(r.is_valid()); // warning only
    assert!(r.has_warnings());
}

#[test]
fn req_validator_openai_empty_messages_warning() {
    let v = RequestValidator::new();
    let r = v.validate(Dialect::OpenAi, &json!({"model": "gpt-4", "messages": []}));
    assert!(r.is_valid());
    assert!(r.has_warnings());
}

#[test]
fn req_validator_claude_requires_max_tokens() {
    let v = RequestValidator::new();
    let r = v.validate(
        Dialect::Claude,
        &json!({"model": "claude-3", "messages": [{"role": "user", "content": "hi"}]}),
    );
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field == "max_tokens"));
}

#[test]
fn req_validator_claude_non_alternating_warning() {
    let v = RequestValidator::new();
    let r = v.validate(
        Dialect::Claude,
        &json!({
            "model": "claude-3", "max_tokens": 100,
            "messages": [{"role": "user", "content": "q1"}, {"role": "user", "content": "q2"}]
        }),
    );
    assert!(r.is_valid()); // warning only
    assert!(r.issues.iter().any(|i| i.code == "non_alternating_roles"));
}

#[test]
fn req_validator_gemini_empty_contents_error() {
    let v = RequestValidator::new();
    let r = v.validate(
        Dialect::Gemini,
        &json!({"model": "gemini-pro", "contents": []}),
    );
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.code == "empty_contents"));
}

#[test]
fn req_validator_gemini_empty_parts_warning() {
    let v = RequestValidator::new();
    let r = v.validate(
        Dialect::Gemini,
        &json!({"model": "gemini-pro", "contents": [{"parts": []}]}),
    );
    assert!(r.is_valid());
    assert!(r.issues.iter().any(|i| i.code == "empty_parts"));
}

#[test]
fn req_validator_non_object_invalid() {
    let v = RequestValidator::new();
    let r = v.validate(Dialect::OpenAi, &json!(42));
    assert!(!r.is_valid());
    assert_eq!(r.issues[0].code, "invalid_type");
}

#[test]
fn req_validator_model_name_with_spaces_warning() {
    let v = RequestValidator::new();
    let r = v.validate(
        Dialect::OpenAi,
        &json!({"model": "gpt 4", "messages": [{"role": "user", "content": "hi"}]}),
    );
    assert!(r.is_valid());
    assert!(r.issues.iter().any(|i| i.code == "model_name_has_spaces"));
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Severity and ValidationIssue serde (5 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn severity_display_values() {
    assert_eq!(Severity::Info.to_string(), "info");
    assert_eq!(Severity::Warning.to_string(), "warning");
    assert_eq!(Severity::Error.to_string(), "error");
}

#[test]
fn severity_serde_roundtrip() {
    for sev in [Severity::Info, Severity::Warning, Severity::Error] {
        let json = serde_json::to_string(&sev).unwrap();
        let back: Severity = serde_json::from_str(&json).unwrap();
        assert_eq!(sev, back);
    }
}

#[test]
fn severity_ordering() {
    assert!(Severity::Info < Severity::Warning);
    assert!(Severity::Warning < Severity::Error);
}

#[test]
fn validation_issue_display() {
    let issue = ValidationIssue {
        severity: Severity::Error,
        field: "model".into(),
        message: "missing".into(),
        code: "missing_required_field".into(),
    };
    let s = format!("{issue}");
    assert!(s.contains("error"));
    assert!(s.contains("model"));
}

#[test]
fn validation_issue_serde_roundtrip() {
    let issue = ValidationIssue {
        severity: Severity::Warning,
        field: "messages".into(),
        message: "empty".into(),
        code: "empty_messages".into(),
    };
    let json = serde_json::to_value(&issue).unwrap();
    let back: ValidationIssue = serde_json::from_value(json).unwrap();
    assert_eq!(issue, back);
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Builtin fingerprints (detect module) (3 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn builtin_fingerprints_cover_all_dialects() {
    let fps = builtin_fingerprints();
    for &d in Dialect::all() {
        assert!(fps.contains_key(&d), "missing fingerprint for {d:?}");
    }
}

#[test]
fn builtin_fingerprints_openai_has_gpt_prefix() {
    let fps = builtin_fingerprints();
    let fp = fps.get(&Dialect::OpenAi).unwrap();
    assert!(fp.model_prefix_patterns.iter().any(|p| p == "gpt-"));
}

#[test]
fn builtin_fingerprints_claude_has_anthropic_header() {
    let fps = builtin_fingerprints();
    let fp = fps.get(&Dialect::Claude).unwrap();
    assert!(
        fp.header_markers
            .iter()
            .any(|(k, _)| k == "anthropic-version")
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 15. DialectRegistry — parse/serialize roundtrips (12 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_with_builtins_has_six() {
    let reg = DialectRegistry::with_builtins();
    assert_eq!(reg.len(), 6);
    assert!(!reg.is_empty());
}

#[test]
fn registry_list_dialects() {
    let reg = DialectRegistry::with_builtins();
    let list = reg.list_dialects();
    assert_eq!(list.len(), 6);
}

#[test]
fn registry_supports_pair_openai_claude() {
    let reg = DialectRegistry::with_builtins();
    assert!(reg.supports_pair(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn registry_empty_does_not_support_pair() {
    let reg = DialectRegistry::new();
    assert!(!reg.supports_pair(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn registry_parse_openai_request() {
    let reg = DialectRegistry::with_builtins();
    let v = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hello"}]});
    let ir = reg.parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.model.as_deref(), Some("gpt-4"));
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::User);
}

#[test]
fn registry_parse_claude_request() {
    let reg = DialectRegistry::with_builtins();
    let v = json!({"model": "claude-3", "system": "be nice", "messages": [{"role": "user", "content": "hi"}], "max_tokens": 100});
    let ir = reg.parse(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.system_prompt.as_deref(), Some("be nice"));
}

#[test]
fn registry_parse_gemini_request() {
    let reg = DialectRegistry::with_builtins();
    let v = json!({"model": "gemini-pro", "contents": [{"parts": [{"text": "hello"}]}]});
    let ir = reg.parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.messages.len(), 1);
}

#[test]
fn registry_parse_codex_request() {
    let reg = DialectRegistry::with_builtins();
    let v = json!({"model": "codex-mini", "instructions": "fix the bug", "input": "code here"});
    let ir = reg.parse(Dialect::Codex, &v).unwrap();
    assert_eq!(ir.system_prompt.as_deref(), Some("fix the bug"));
    assert_eq!(ir.messages[0].text_content(), "code here");
}

#[test]
fn registry_parse_kimi_preserves_metadata() {
    let reg = DialectRegistry::with_builtins();
    let v = json!({"model": "kimi", "messages": [{"role": "user", "content": "hi"}], "refs": ["url"], "search_plus": true});
    let ir = reg.parse(Dialect::Kimi, &v).unwrap();
    assert!(ir.metadata.contains_key("kimi_refs"));
    assert!(ir.metadata.contains_key("kimi_search_plus"));
}

#[test]
fn registry_parse_copilot_preserves_metadata() {
    let reg = DialectRegistry::with_builtins();
    let v = json!({"model": "copilot", "messages": [{"role": "user", "content": "hi"}], "references": [{"type": "file"}], "agent_mode": true});
    let ir = reg.parse(Dialect::Copilot, &v).unwrap();
    assert!(ir.metadata.contains_key("copilot_references"));
    assert!(ir.metadata.contains_key("copilot_agent_mode"));
}

#[test]
fn registry_parse_unregistered_dialect_errors() {
    let reg = DialectRegistry::new();
    let err = reg.parse(Dialect::OpenAi, &json!({})).unwrap_err();
    assert_eq!(err.dialect, Dialect::OpenAi);
    assert!(err.message.contains("not registered"));
}

#[test]
fn registry_serialize_unregistered_dialect_errors() {
    let reg = DialectRegistry::new();
    let ir = IrRequest::new(vec![]);
    let err = reg.serialize(Dialect::Claude, &ir).unwrap_err();
    assert_eq!(err.dialect, Dialect::Claude);
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Registry roundtrip: parse then serialize (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_openai_parse_serialize() {
    let reg = DialectRegistry::with_builtins();
    let v = json!({"model": "gpt-4", "messages": [{"role": "system", "content": "sys"}, {"role": "user", "content": "hi"}]});
    let ir = reg.parse(Dialect::OpenAi, &v).unwrap();
    let out = reg.serialize(Dialect::OpenAi, &ir).unwrap();
    assert_eq!(out["model"], "gpt-4");
    assert!(out["messages"].as_array().unwrap().len() >= 2);
}

#[test]
fn roundtrip_claude_parse_serialize() {
    let reg = DialectRegistry::with_builtins();
    let v = json!({"model": "claude-3", "system": "be nice", "messages": [{"role": "user", "content": "hi"}], "max_tokens": 100});
    let ir = reg.parse(Dialect::Claude, &v).unwrap();
    let out = reg.serialize(Dialect::Claude, &ir).unwrap();
    assert_eq!(out["system"], "be nice");
}

#[test]
fn roundtrip_gemini_parse_serialize() {
    let reg = DialectRegistry::with_builtins();
    let v = json!({"model": "gemini-pro", "contents": [{"parts": [{"text": "hello"}]}]});
    let ir = reg.parse(Dialect::Gemini, &v).unwrap();
    let out = reg.serialize(Dialect::Gemini, &ir).unwrap();
    assert!(out["contents"].as_array().is_some());
}

#[test]
fn roundtrip_codex_parse_serialize() {
    let reg = DialectRegistry::with_builtins();
    let v = json!({"model": "codex-mini", "instructions": "do it", "input": "code"});
    let ir = reg.parse(Dialect::Codex, &v).unwrap();
    let out = reg.serialize(Dialect::Codex, &ir).unwrap();
    assert_eq!(out["instructions"], "do it");
    assert_eq!(out["input"], "code");
}

#[test]
fn roundtrip_kimi_parse_serialize_preserves_refs() {
    let reg = DialectRegistry::with_builtins();
    let v =
        json!({"model": "kimi", "messages": [{"role": "user", "content": "hi"}], "refs": ["url1"]});
    let ir = reg.parse(Dialect::Kimi, &v).unwrap();
    let out = reg.serialize(Dialect::Kimi, &ir).unwrap();
    assert_eq!(out["refs"], json!(["url1"]));
}

#[test]
fn roundtrip_copilot_parse_serialize_preserves_references() {
    let reg = DialectRegistry::with_builtins();
    let v = json!({"model": "copilot", "messages": [{"role": "user", "content": "hi"}], "references": [{"type": "file"}]});
    let ir = reg.parse(Dialect::Copilot, &v).unwrap();
    let out = reg.serialize(Dialect::Copilot, &ir).unwrap();
    assert_eq!(out["references"], json!([{"type": "file"}]));
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Cross-dialect conversion via registry (4 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cross_dialect_openai_to_claude() {
    let reg = DialectRegistry::with_builtins();
    let v = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}], "max_tokens": 50});
    let ir = reg.parse(Dialect::OpenAi, &v).unwrap();
    let out = reg.serialize(Dialect::Claude, &ir).unwrap();
    assert!(out["messages"].as_array().is_some());
}

#[test]
fn cross_dialect_claude_to_gemini() {
    let reg = DialectRegistry::with_builtins();
    let v = json!({"model": "claude-3", "system": "sys", "messages": [{"role": "user", "content": "hi"}], "max_tokens": 50});
    let ir = reg.parse(Dialect::Claude, &v).unwrap();
    let out = reg.serialize(Dialect::Gemini, &ir).unwrap();
    assert!(out["contents"].as_array().is_some());
}

#[test]
fn cross_dialect_gemini_to_openai() {
    let reg = DialectRegistry::with_builtins();
    let v = json!({"model": "gemini-pro", "contents": [{"parts": [{"text": "hello"}]}]});
    let ir = reg.parse(Dialect::Gemini, &v).unwrap();
    let out = reg.serialize(Dialect::OpenAi, &ir).unwrap();
    assert!(out["messages"].as_array().is_some());
}

#[test]
fn cross_dialect_openai_to_codex() {
    let reg = DialectRegistry::with_builtins();
    let v = json!({"model": "gpt-4", "messages": [{"role": "system", "content": "sys"}, {"role": "user", "content": "code"}]});
    let ir = reg.parse(Dialect::OpenAi, &v).unwrap();
    let out = reg.serialize(Dialect::Codex, &ir).unwrap();
    assert_eq!(out["instructions"], "sys");
}

// ═══════════════════════════════════════════════════════════════════════
// 18. IR types serde and helpers (12 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ir_role_serde_roundtrip() {
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        let json = serde_json::to_string(&role).unwrap();
        let back: IrRole = serde_json::from_str(&json).unwrap();
        assert_eq!(role, back);
    }
}

#[test]
fn ir_message_text_helper() {
    let msg = IrMessage::text(IrRole::User, "hello");
    assert_eq!(msg.text_content(), "hello");
    assert_eq!(msg.role, IrRole::User);
}

#[test]
fn ir_message_tool_calls_empty_for_text() {
    let msg = IrMessage::text(IrRole::User, "hi");
    assert!(msg.tool_calls().is_empty());
}

#[test]
fn ir_content_block_as_text() {
    let block = IrContentBlock::Text {
        text: "hello".into(),
    };
    assert_eq!(block.as_text(), Some("hello"));
}

#[test]
fn ir_content_block_is_tool_call() {
    let block = IrContentBlock::ToolCall {
        id: "1".into(),
        name: "fn".into(),
        input: json!({}),
    };
    assert!(block.is_tool_call());
    assert!(!block.is_tool_result());
}

#[test]
fn ir_content_block_is_tool_result() {
    let block = IrContentBlock::ToolResult {
        tool_call_id: "1".into(),
        content: vec![],
        is_error: false,
    };
    assert!(block.is_tool_result());
    assert!(!block.is_tool_call());
}

#[test]
fn ir_usage_from_io() {
    let u = IrUsage::from_io(100, 200);
    assert_eq!(u.input_tokens, 100);
    assert_eq!(u.output_tokens, 200);
    assert_eq!(u.total_tokens, 300);
}

#[test]
fn ir_usage_merge() {
    let a = IrUsage::from_io(10, 20);
    let b = IrUsage::from_io(5, 15);
    let c = a.merge(b);
    assert_eq!(c.input_tokens, 15);
    assert_eq!(c.output_tokens, 35);
    assert_eq!(c.total_tokens, 50);
}

#[test]
fn ir_request_builder_chain() {
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "hi")])
        .with_model("gpt-4")
        .with_system_prompt("be nice")
        .with_config(IrGenerationConfig {
            max_tokens: Some(100),
            ..Default::default()
        });
    assert_eq!(ir.model.as_deref(), Some("gpt-4"));
    assert_eq!(ir.system_prompt.as_deref(), Some("be nice"));
    assert_eq!(ir.config.max_tokens, Some(100));
}

#[test]
fn ir_response_text_helper() {
    let r = IrResponse::text("hello world");
    assert_eq!(r.text_content(), "hello world");
    assert!(!r.has_tool_calls());
}

#[test]
fn ir_stop_reason_serde_roundtrip() {
    for sr in [
        IrStopReason::EndTurn,
        IrStopReason::StopSequence,
        IrStopReason::MaxTokens,
        IrStopReason::ToolUse,
        IrStopReason::ContentFilter,
        IrStopReason::Other("custom".into()),
    ] {
        let json = serde_json::to_string(&sr).unwrap();
        let back: IrStopReason = serde_json::from_str(&json).unwrap();
        assert_eq!(sr, back);
    }
}

#[test]
fn ir_request_all_tool_calls() {
    let ir = IrRequest::new(vec![
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolCall {
                id: "1".into(),
                name: "fn1".into(),
                input: json!({}),
            }],
        ),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolCall {
                id: "2".into(),
                name: "fn2".into(),
                input: json!({}),
            }],
        ),
    ]);
    assert_eq!(ir.all_tool_calls().len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 19. DialectError (3 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dialect_error_display() {
    let err = DialectError {
        dialect: Dialect::OpenAi,
        message: "bad input".into(),
    };
    let s = format!("{err}");
    assert!(s.contains("OpenAI"));
    assert!(s.contains("bad input"));
}

#[test]
fn dialect_error_is_std_error() {
    let err = DialectError {
        dialect: Dialect::Claude,
        message: "fail".into(),
    };
    let _: &dyn std::error::Error = &err;
}

#[test]
fn dialect_error_eq() {
    let a = DialectError {
        dialect: Dialect::Gemini,
        message: "x".into(),
    };
    let b = DialectError {
        dialect: Dialect::Gemini,
        message: "x".into(),
    };
    assert_eq!(a, b);
}

// ═══════════════════════════════════════════════════════════════════════
// 20. DialectEntry metadata (4 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dialect_entry_openai_metadata() {
    let reg = DialectRegistry::with_builtins();
    let entry = reg.get(Dialect::OpenAi).unwrap();
    assert_eq!(entry.name, "openai");
    assert_eq!(entry.version, "v1");
    assert_eq!(entry.dialect, Dialect::OpenAi);
}

#[test]
fn dialect_entry_claude_metadata() {
    let reg = DialectRegistry::with_builtins();
    let entry = reg.get(Dialect::Claude).unwrap();
    assert_eq!(entry.name, "claude");
    assert_eq!(entry.version, "v1");
}

#[test]
fn dialect_entry_all_have_v1_version() {
    let reg = DialectRegistry::with_builtins();
    for &d in Dialect::all() {
        let entry = reg.get(d).unwrap();
        assert_eq!(entry.version, "v1", "dialect {:?} expected v1", d);
    }
}

#[test]
fn dialect_entry_names_match_snake_case() {
    let reg = DialectRegistry::with_builtins();
    let expected = [
        (Dialect::OpenAi, "openai"),
        (Dialect::Claude, "claude"),
        (Dialect::Gemini, "gemini"),
        (Dialect::Codex, "codex"),
        (Dialect::Kimi, "kimi"),
        (Dialect::Copilot, "copilot"),
    ];
    for (d, name) in expected {
        assert_eq!(reg.get(d).unwrap().name, name);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 21. ValidationError (lib.rs) (3 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validation_error_display_format() {
    let e = abp_dialect::ValidationError {
        path: "/model".into(),
        message: "missing".into(),
    };
    assert_eq!(format!("{e}"), "/model: missing");
}

#[test]
fn validation_error_is_std_error() {
    let e = abp_dialect::ValidationError {
        path: "/".into(),
        message: "bad".into(),
    };
    let _: &dyn std::error::Error = &e;
}

#[test]
fn validation_result_warnings_field() {
    let v = DialectValidator::new();
    let msg = json!({"messages": [{"role": "user", "content": "hi"}]});
    let r = v.validate(&msg, Dialect::OpenAi);
    // Missing model → should have warnings about no messages or produce error
    // This tests the warnings vec is accessible
    let _ = r.warnings.len();
}
