#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive dialect parser tests: detection, parsing, normalization across all 6 SDK dialects.

use std::collections::BTreeMap;

use abp_dialect::detect::{
    builtin_fingerprints, detect_dialect, detect_from_headers, DialectDetectionResult,
    DialectFingerprint,
};
use abp_dialect::ir::{
    IrContentBlock, IrGenerationConfig, IrMessage, IrRequest, IrResponse, IrRole, IrStopReason,
    IrToolDefinition, IrUsage,
};
use abp_dialect::registry::{DialectEntry, DialectError, DialectRegistry};
use abp_dialect::validate::{RequestValidator, Severity, ValidationIssue, ValidationResult};
use abp_dialect::{Dialect, DialectDetector, DialectValidator, ValidationError};
use serde_json::{json, Value};

// ═══════════════════════════════════════════════════════════════════════
// Helper constructors
// ═══════════════════════════════════════════════════════════════════════

fn detector() -> DialectDetector {
    DialectDetector::new()
}

fn registry() -> DialectRegistry {
    DialectRegistry::with_builtins()
}

fn req_validator() -> RequestValidator {
    RequestValidator::new()
}

fn validator() -> DialectValidator {
    DialectValidator::new()
}

// ═══════════════════════════════════════════════════════════════════════
// § 1 — Dialect enum basics
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dialect_all_returns_exactly_six() {
    assert_eq!(Dialect::all().len(), 6);
}

#[test]
fn dialect_all_contains_every_variant() {
    let all = Dialect::all();
    assert!(all.contains(&Dialect::OpenAi));
    assert!(all.contains(&Dialect::Claude));
    assert!(all.contains(&Dialect::Gemini));
    assert!(all.contains(&Dialect::Codex));
    assert!(all.contains(&Dialect::Kimi));
    assert!(all.contains(&Dialect::Copilot));
}

#[test]
fn dialect_all_has_no_duplicates() {
    let all = Dialect::all();
    let mut seen = std::collections::HashSet::new();
    for d in all {
        assert!(seen.insert(d), "duplicate dialect: {d:?}");
    }
}

#[test]
fn dialect_label_openai() {
    assert_eq!(Dialect::OpenAi.label(), "OpenAI");
}

#[test]
fn dialect_label_claude() {
    assert_eq!(Dialect::Claude.label(), "Claude");
}

#[test]
fn dialect_label_gemini() {
    assert_eq!(Dialect::Gemini.label(), "Gemini");
}

#[test]
fn dialect_label_codex() {
    assert_eq!(Dialect::Codex.label(), "Codex");
}

#[test]
fn dialect_label_kimi() {
    assert_eq!(Dialect::Kimi.label(), "Kimi");
}

#[test]
fn dialect_label_copilot() {
    assert_eq!(Dialect::Copilot.label(), "Copilot");
}

#[test]
fn dialect_display_matches_label() {
    for d in Dialect::all() {
        assert_eq!(format!("{d}"), d.label());
    }
}

#[test]
fn dialect_debug_is_not_empty() {
    for d in Dialect::all() {
        assert!(!format!("{d:?}").is_empty());
    }
}

#[test]
fn dialect_clone_eq() {
    for &d in Dialect::all() {
        let cloned = d;
        assert_eq!(d, cloned);
    }
}

#[test]
fn dialect_ord_is_deterministic() {
    let mut sorted = Dialect::all().to_vec();
    sorted.sort();
    let mut sorted2 = sorted.clone();
    sorted2.sort();
    assert_eq!(sorted, sorted2);
}

#[test]
fn dialect_hash_consistent() {
    use std::collections::HashMap;
    let mut map = HashMap::new();
    for &d in Dialect::all() {
        map.insert(d, d.label());
    }
    assert_eq!(map.len(), 6);
    assert_eq!(map[&Dialect::OpenAi], "OpenAI");
}

// ═══════════════════════════════════════════════════════════════════════
// § 2 — Serde representations
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn serde_openai_snake_case() {
    let s = serde_json::to_string(&Dialect::OpenAi).unwrap();
    assert_eq!(s, "\"open_ai\"");
}

#[test]
fn serde_claude_snake_case() {
    let s = serde_json::to_string(&Dialect::Claude).unwrap();
    assert_eq!(s, "\"claude\"");
}

#[test]
fn serde_gemini_snake_case() {
    let s = serde_json::to_string(&Dialect::Gemini).unwrap();
    assert_eq!(s, "\"gemini\"");
}

#[test]
fn serde_codex_snake_case() {
    let s = serde_json::to_string(&Dialect::Codex).unwrap();
    assert_eq!(s, "\"codex\"");
}

#[test]
fn serde_kimi_snake_case() {
    let s = serde_json::to_string(&Dialect::Kimi).unwrap();
    assert_eq!(s, "\"kimi\"");
}

#[test]
fn serde_copilot_snake_case() {
    let s = serde_json::to_string(&Dialect::Copilot).unwrap();
    assert_eq!(s, "\"copilot\"");
}

#[test]
fn serde_roundtrip_all_dialects() {
    for &d in Dialect::all() {
        let s = serde_json::to_string(&d).unwrap();
        let back: Dialect = serde_json::from_str(&s).unwrap();
        assert_eq!(d, back);
    }
}

#[test]
fn serde_unknown_string_fails() {
    let result = serde_json::from_str::<Dialect>("\"unknown_dialect\"");
    assert!(result.is_err());
}

#[test]
fn serde_number_fails() {
    let result = serde_json::from_str::<Dialect>("42");
    assert!(result.is_err());
}

#[test]
fn serde_null_fails() {
    let result = serde_json::from_str::<Dialect>("null");
    assert!(result.is_err());
}

#[test]
fn serde_dialect_in_json_object() {
    let obj = json!({"dialect": "claude"});
    #[derive(serde::Deserialize)]
    struct Wrapper {
        dialect: Dialect,
    }
    let w: Wrapper = serde_json::from_value(obj).unwrap();
    assert_eq!(w.dialect, Dialect::Claude);
}

// ═══════════════════════════════════════════════════════════════════════
// § 3 — Dialect detection from request JSON (DialectDetector)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_openai_from_gpt_model_and_messages() {
    let v = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn detect_openai_from_chatgpt_model() {
    let v = json!({"model": "chatgpt-4o-latest", "messages": [{"role": "user", "content": "hi"}]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn detect_openai_from_choices_key() {
    let v =
        json!({"choices": [{"message": {"role": "assistant", "content": "hi"}}], "model": "gpt-4"});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn detect_claude_from_model_prefix() {
    let v = json!({"model": "claude-3-opus-20240229", "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn detect_claude_from_type_message() {
    let v = json!({"type": "message", "model": "claude-3-opus", "content": [{"type": "text", "text": "hi"}], "stop_reason": "end_turn"});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn detect_gemini_from_contents_with_parts() {
    let v = json!({"contents": [{"parts": [{"text": "hello"}]}]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_gemini_from_candidates_key() {
    let v = json!({"candidates": [{"content": {"parts": [{"text": "hi"}]}}]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_gemini_from_model_prefix() {
    let v = json!({"model": "gemini-1.5-pro", "contents": [{"parts": [{"text": "hi"}]}]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_codex_from_items_with_type() {
    let v = json!({"items": [{"type": "message", "content": "done"}], "status": "completed"});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Codex);
}

#[test]
fn detect_codex_from_object_response() {
    let v = json!({"object": "response", "status": "completed", "items": [{"type": "message"}]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Codex);
}

#[test]
fn detect_kimi_from_refs_field() {
    let v = json!({"model": "kimi", "messages": [{"role": "user", "content": "search this"}], "refs": ["https://example.com"]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Kimi);
}

#[test]
fn detect_kimi_from_search_plus() {
    let v = json!({"model": "kimi", "messages": [{"role": "user", "content": "hello"}], "search_plus": true});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Kimi);
}

#[test]
fn detect_kimi_from_moonshot_model() {
    let v = json!({"model": "moonshot-v1-32k", "refs": ["doc"]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Kimi);
}

#[test]
fn detect_copilot_from_references() {
    let v = json!({"messages": [{"role": "user", "content": "fix"}], "references": [{"type": "file", "path": "a.rs"}]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn detect_copilot_from_agent_mode() {
    let v = json!({"messages": [{"role": "user", "content": "do"}], "agent_mode": true, "confirmations": []});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn detect_copilot_from_copilot_model() {
    let v = json!({"model": "copilot-gpt-4", "messages": [{"role": "user", "content": "hi"}], "references": []});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
}

// ═══════════════════════════════════════════════════════════════════════
// § 4 — Dialect detection from response JSON
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_openai_response_with_choices() {
    let v = json!({"choices": [{"message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}], "model": "gpt-4", "usage": {"prompt_tokens": 10, "completion_tokens": 5}});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn detect_claude_response_with_stop_reason() {
    let v = json!({"type": "message", "model": "claude-3-opus", "content": [{"type": "text", "text": "reply"}], "stop_reason": "end_turn"});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn detect_gemini_response_with_candidates() {
    let v = json!({"candidates": [{"content": {"parts": [{"text": "answer"}]}}], "usageMetadata": {"promptTokenCount": 5}});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_codex_response_object() {
    let v = json!({"object": "response", "status": "completed", "items": [{"type": "message", "role": "assistant", "content": "done"}]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Codex);
}

// ═══════════════════════════════════════════════════════════════════════
// § 5 — Dialect detection from model strings
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_model_gpt4() {
    let v = json!({"model": "gpt-4-turbo", "messages": [{"role": "user", "content": "test"}]});
    assert_eq!(detector().detect(&v).unwrap().dialect, Dialect::OpenAi);
}

#[test]
fn detect_model_gpt35() {
    let v = json!({"model": "gpt-3.5-turbo", "messages": [{"role": "user", "content": "test"}]});
    assert_eq!(detector().detect(&v).unwrap().dialect, Dialect::OpenAi);
}

#[test]
fn detect_model_o1() {
    // o1- models could match both OpenAI and Codex scorer
    let v = json!({"model": "o1-preview", "messages": [{"role": "user", "content": "test"}]});
    let r = detector().detect(&v).unwrap();
    // Either OpenAi or Codex is acceptable for o1- since both claim it
    assert!(r.dialect == Dialect::OpenAi || r.dialect == Dialect::Codex);
}

#[test]
fn detect_model_claude_3_5() {
    let v = json!({"model": "claude-3-5-sonnet-20241022", "messages": [{"role": "user", "content": "hi"}]});
    assert_eq!(detector().detect(&v).unwrap().dialect, Dialect::Claude);
}

#[test]
fn detect_model_gemini_flash() {
    let v = json!({"model": "gemini-2.0-flash", "contents": [{"parts": [{"text": "hi"}]}]});
    assert_eq!(detector().detect(&v).unwrap().dialect, Dialect::Gemini);
}

#[test]
fn detect_model_gemini_with_prefix() {
    let v = json!({"model": "models/gemini-1.5-pro", "contents": [{"parts": [{"text": "hi"}]}]});
    assert_eq!(detector().detect(&v).unwrap().dialect, Dialect::Gemini);
}

#[test]
fn detect_model_codex_mini() {
    let v = json!({"model": "codex-mini-latest", "instructions": "fix the bug", "items": [{"type": "message"}]});
    assert_eq!(detector().detect(&v).unwrap().dialect, Dialect::Codex);
}

#[test]
fn detect_model_moonshot() {
    let v = json!({"model": "moonshot-v1-128k", "messages": [{"role": "user", "content": "q"}], "refs": ["doc"]});
    assert_eq!(detector().detect(&v).unwrap().dialect, Dialect::Kimi);
}

#[test]
fn detect_model_copilot_prefix() {
    let v = json!({"model": "copilot-chat", "messages": [{"role": "user", "content": "hi"}], "references": []});
    assert_eq!(detector().detect(&v).unwrap().dialect, Dialect::Copilot);
}

// ═══════════════════════════════════════════════════════════════════════
// § 6 — Unknown/ambiguous dialect handling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_returns_none_for_null() {
    assert!(detector().detect(&json!(null)).is_none());
}

#[test]
fn detect_returns_none_for_number() {
    assert!(detector().detect(&json!(42)).is_none());
}

#[test]
fn detect_returns_none_for_string() {
    assert!(detector().detect(&json!("hello")).is_none());
}

#[test]
fn detect_returns_none_for_array() {
    assert!(detector().detect(&json!([1, 2, 3])).is_none());
}

#[test]
fn detect_returns_none_for_bool() {
    assert!(detector().detect(&json!(true)).is_none());
}

#[test]
fn detect_returns_none_for_empty_object() {
    assert!(detector().detect(&json!({})).is_none());
}

#[test]
fn detect_all_returns_empty_for_non_object() {
    assert!(detector().detect_all(&json!(null)).is_empty());
    assert!(detector().detect_all(&json!(42)).is_empty());
    assert!(detector().detect_all(&json!("x")).is_empty());
}

#[test]
fn detect_all_sorted_descending_confidence() {
    let v = json!({"model": "x", "messages": [{"role": "user", "content": "hi"}], "refs": ["a"]});
    let results = detector().detect_all(&v);
    for w in results.windows(2) {
        assert!(w[0].confidence >= w[1].confidence);
    }
}

#[test]
fn detect_all_returns_multiple_for_ambiguous_input() {
    let v = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}], "temperature": 0.7});
    let results = detector().detect_all(&v);
    assert!(!results.is_empty());
    assert_eq!(results[0].dialect, Dialect::OpenAi);
}

#[test]
fn confidence_capped_at_one() {
    let v = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}], "choices": [{}], "temperature": 0.7, "top_p": 0.9, "max_tokens": 100});
    let r = detector().detect(&v).unwrap();
    assert!(r.confidence <= 1.0);
}

#[test]
fn evidence_is_populated_for_detection() {
    let v = json!({"choices": [{}]});
    let r = detector().detect(&v).unwrap();
    assert!(!r.evidence.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// § 7 — Header-based detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_claude_from_anthropic_version_header() {
    let mut h = BTreeMap::new();
    h.insert("anthropic-version".into(), "2023-06-01".into());
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn detect_claude_from_x_api_key_header() {
    let mut h = BTreeMap::new();
    h.insert("x-api-key".into(), "sk-ant-xxx".into());
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn detect_copilot_from_github_token_header() {
    let mut h = BTreeMap::new();
    h.insert("x-github-token".into(), "ghu_xxx".into());
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn detect_copilot_from_integration_id_header() {
    let mut h = BTreeMap::new();
    h.insert("copilot-integration-id".into(), "vscode".into());
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn detect_gemini_from_goog_api_key_header() {
    let mut h = BTreeMap::new();
    h.insert("x-goog-api-key".into(), "AIza...".into());
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_openai_from_openai_org_header() {
    let mut h = BTreeMap::new();
    h.insert("openai-organization".into(), "org-xxx".into());
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn detect_headers_returns_none_for_empty() {
    assert!(detector().detect_from_headers(&BTreeMap::new()).is_none());
}

#[test]
fn detect_headers_returns_none_for_unrelated() {
    let mut h = BTreeMap::new();
    h.insert("content-type".into(), "application/json".into());
    assert!(detector().detect_from_headers(&h).is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// § 8 — Endpoint-based detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_endpoint_anthropic() {
    let r = detector()
        .detect_from_endpoint("https://api.anthropic.com/v1/messages")
        .unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn detect_endpoint_gemini() {
    let r = detector()
        .detect_from_endpoint(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-pro:generateContent",
        )
        .unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_endpoint_moonshot() {
    let r = detector()
        .detect_from_endpoint("https://api.moonshot.cn/v1/chat/completions")
        .unwrap();
    assert_eq!(r.dialect, Dialect::Kimi);
}

#[test]
fn detect_endpoint_openai_chat() {
    let r = detector()
        .detect_from_endpoint("https://api.openai.com/v1/chat/completions")
        .unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn detect_endpoint_openai_responses_is_codex() {
    let r = detector()
        .detect_from_endpoint("https://api.openai.com/v1/responses")
        .unwrap();
    assert_eq!(r.dialect, Dialect::Codex);
}

#[test]
fn detect_endpoint_github_copilot() {
    let r = detector()
        .detect_from_endpoint("https://api.github.com/copilot/chat")
        .unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn detect_endpoint_unknown_returns_none() {
    assert!(detector()
        .detect_from_endpoint("https://example.com/api")
        .is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// § 9 — Fingerprint-based detection (detect module)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fingerprint_builtin_covers_all_dialects() {
    let fps = builtin_fingerprints();
    for d in Dialect::all() {
        assert!(fps.contains_key(d), "missing fingerprint for {d:?}");
    }
}

#[test]
fn fingerprint_detect_openai_with_messages_and_roles() {
    let v = json!({"messages": [{"role": "user", "content": "hi"}]});
    let r = detect_dialect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn fingerprint_detect_returns_none_for_non_object() {
    assert!(detect_dialect(&json!(42)).is_none());
    assert!(detect_dialect(&json!("hi")).is_none());
    assert!(detect_dialect(&json!(null)).is_none());
}

#[test]
fn fingerprint_detect_returns_none_for_empty() {
    assert!(detect_dialect(&json!({})).is_none());
}

#[test]
fn fingerprint_detect_claude_structural_boost() {
    let v = json!({"model": "claude-3-opus", "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]});
    let r = detect_dialect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
    assert!(r.evidence.iter().any(|e| e.contains("content blocks")));
}

#[test]
fn fingerprint_detect_gemini_structural_boost() {
    let v = json!({"model": "gemini-1.5-pro", "contents": [{"parts": [{"text": "hi"}]}]});
    let r = detect_dialect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
    assert!(r.evidence.iter().any(|e| e.contains("parts")));
}

#[test]
fn fingerprint_detect_codex_structural_boost() {
    let v =
        json!({"model": "codex-mini", "instructions": "fix bug", "items": [{"type": "message"}]});
    let r = detect_dialect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Codex);
}

#[test]
fn fingerprint_detect_copilot_typed_references() {
    let v = json!({"references": [{"type": "file", "path": "src/main.rs"}]});
    let r = detect_dialect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn fingerprint_headers_detect_claude() {
    let mut h = BTreeMap::new();
    h.insert("anthropic-version".into(), "2023-06-01".into());
    let r = detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn fingerprint_headers_detect_gemini() {
    let mut h = BTreeMap::new();
    h.insert("x-goog-api-key".into(), "key".into());
    let r = detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn fingerprint_headers_returns_none_for_empty() {
    assert!(detect_from_headers(&BTreeMap::new()).is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// § 10 — Registry: version parsing & entry metadata
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_has_all_six_dialects() {
    let reg = registry();
    assert_eq!(reg.len(), 6);
    for &d in Dialect::all() {
        assert!(reg.get(d).is_some(), "missing entry for {d:?}");
    }
}

#[test]
fn registry_openai_entry_metadata() {
    let e = registry().get(Dialect::OpenAi).unwrap().clone();
    assert_eq!(e.dialect, Dialect::OpenAi);
    assert_eq!(e.name, "openai");
    assert_eq!(e.version, "v1");
}

#[test]
fn registry_claude_entry_metadata() {
    let e = registry().get(Dialect::Claude).unwrap().clone();
    assert_eq!(e.dialect, Dialect::Claude);
    assert_eq!(e.name, "claude");
    assert_eq!(e.version, "v1");
}

#[test]
fn registry_gemini_entry_metadata() {
    let e = registry().get(Dialect::Gemini).unwrap().clone();
    assert_eq!(e.dialect, Dialect::Gemini);
    assert_eq!(e.name, "gemini");
    assert_eq!(e.version, "v1");
}

#[test]
fn registry_codex_entry_metadata() {
    let e = registry().get(Dialect::Codex).unwrap().clone();
    assert_eq!(e.dialect, Dialect::Codex);
    assert_eq!(e.name, "codex");
    assert_eq!(e.version, "v1");
}

#[test]
fn registry_kimi_entry_metadata() {
    let e = registry().get(Dialect::Kimi).unwrap().clone();
    assert_eq!(e.dialect, Dialect::Kimi);
    assert_eq!(e.name, "kimi");
    assert_eq!(e.version, "v1");
}

#[test]
fn registry_copilot_entry_metadata() {
    let e = registry().get(Dialect::Copilot).unwrap().clone();
    assert_eq!(e.dialect, Dialect::Copilot);
    assert_eq!(e.name, "copilot");
    assert_eq!(e.version, "v1");
}

#[test]
fn registry_list_dialects_returns_six() {
    let dialects = registry().list_dialects();
    assert_eq!(dialects.len(), 6);
}

#[test]
fn registry_supports_pair_all_combinations() {
    let reg = registry();
    for &a in Dialect::all() {
        for &b in Dialect::all() {
            assert!(reg.supports_pair(a, b));
        }
    }
}

#[test]
fn registry_empty_has_no_entries() {
    let reg = DialectRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
}

#[test]
fn registry_parse_error_for_unregistered_dialect() {
    let reg = DialectRegistry::new();
    let err = reg.parse(Dialect::OpenAi, &json!({})).unwrap_err();
    assert_eq!(err.dialect, Dialect::OpenAi);
    assert!(err.message.contains("not registered"));
}

#[test]
fn registry_serialize_error_for_unregistered_dialect() {
    let reg = DialectRegistry::new();
    let ir = IrRequest::new(vec![]);
    let err = reg.serialize(Dialect::OpenAi, &ir).unwrap_err();
    assert_eq!(err.dialect, Dialect::OpenAi);
}

#[test]
fn dialect_error_display() {
    let e = DialectError {
        dialect: Dialect::Claude,
        message: "test error".into(),
    };
    let s = format!("{e}");
    assert!(s.contains("Claude"));
    assert!(s.contains("test error"));
}

#[test]
fn dialect_error_is_std_error() {
    let e = DialectError {
        dialect: Dialect::OpenAi,
        message: "oops".into(),
    };
    let _: &dyn std::error::Error = &e;
}

// ═══════════════════════════════════════════════════════════════════════
// § 11 — Registry parsing: OpenAI
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_openai_basic_request() {
    let v = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hello"}]});
    let ir = registry().parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.model.as_deref(), Some("gpt-4"));
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[0].text_content(), "hello");
}

#[test]
fn parse_openai_extracts_system_prompt() {
    let v = json!({"model": "gpt-4", "messages": [{"role": "system", "content": "be brief"}, {"role": "user", "content": "hi"}]});
    let ir = registry().parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.system_prompt.as_deref(), Some("be brief"));
}

#[test]
fn parse_openai_tool_calls() {
    let v = json!({"model": "gpt-4", "messages": [{"role": "assistant", "content": null, "tool_calls": [{"id": "tc1", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\":\"NYC\"}"}}]}]});
    let ir = registry().parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.messages[0].tool_calls().len(), 1);
    if let IrContentBlock::ToolCall { id, name, .. } = &ir.messages[0].content[0] {
        assert_eq!(id, "tc1");
        assert_eq!(name, "get_weather");
    } else {
        panic!("expected tool call");
    }
}

#[test]
fn parse_openai_tool_result() {
    let v = json!({"model": "gpt-4", "messages": [{"role": "tool", "content": "72F", "tool_call_id": "tc1"}]});
    let ir = registry().parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.messages[0].role, IrRole::Tool);
    assert!(ir.messages[0].content[0].is_tool_result());
}

#[test]
fn parse_openai_generation_config() {
    let v = json!({"model": "gpt-4", "messages": [], "temperature": 0.5, "top_p": 0.9, "max_tokens": 200, "stop": ["END"]});
    let ir = registry().parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.config.temperature, Some(0.5));
    assert_eq!(ir.config.top_p, Some(0.9));
    assert_eq!(ir.config.max_tokens, Some(200));
    assert_eq!(ir.config.stop_sequences, vec!["END"]);
}

#[test]
fn parse_openai_non_object_fails() {
    assert!(registry().parse(Dialect::OpenAi, &json!(42)).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// § 12 — Registry parsing: Claude
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_claude_basic_request() {
    let v = json!({"model": "claude-3-opus", "system": "be helpful", "messages": [{"role": "user", "content": "hello"}], "max_tokens": 1024});
    let ir = registry().parse(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.model.as_deref(), Some("claude-3-opus"));
    assert_eq!(ir.system_prompt.as_deref(), Some("be helpful"));
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].text_content(), "hello");
}

#[test]
fn parse_claude_content_blocks() {
    let v = json!({"model": "claude-3", "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}, {"type": "text", "text": " there"}]}], "max_tokens": 100});
    let ir = registry().parse(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.messages[0].content.len(), 2);
    assert_eq!(ir.messages[0].text_content(), "hi there");
}

#[test]
fn parse_claude_tool_use_block() {
    let v = json!({"model": "claude-3", "messages": [{"role": "assistant", "content": [{"type": "tool_use", "id": "tu1", "name": "calc", "input": {"expr": "2+2"}}]}], "max_tokens": 100});
    let ir = registry().parse(Dialect::Claude, &v).unwrap();
    assert!(ir.messages[0].content[0].is_tool_call());
}

#[test]
fn parse_claude_tool_result_block() {
    let v = json!({"model": "claude-3", "messages": [{"role": "user", "content": [{"type": "tool_result", "tool_use_id": "tu1", "content": "4"}]}], "max_tokens": 100});
    let ir = registry().parse(Dialect::Claude, &v).unwrap();
    assert!(ir.messages[0].content[0].is_tool_result());
}

#[test]
fn parse_claude_thinking_block() {
    let v = json!({"model": "claude-3", "messages": [{"role": "assistant", "content": [{"type": "thinking", "thinking": "let me think"}]}], "max_tokens": 100});
    let ir = registry().parse(Dialect::Claude, &v).unwrap();
    if let IrContentBlock::Thinking { text } = &ir.messages[0].content[0] {
        assert_eq!(text, "let me think");
    } else {
        panic!("expected thinking block");
    }
}

#[test]
fn parse_claude_image_block() {
    let v = json!({"model": "claude-3", "messages": [{"role": "user", "content": [{"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "abc123"}}]}], "max_tokens": 100});
    let ir = registry().parse(Dialect::Claude, &v).unwrap();
    if let IrContentBlock::Image { media_type, data } = &ir.messages[0].content[0] {
        assert_eq!(media_type, "image/png");
        assert_eq!(data, "abc123");
    } else {
        panic!("expected image block");
    }
}

#[test]
fn parse_claude_tools() {
    let v = json!({"model": "claude-3", "messages": [], "max_tokens": 100, "tools": [{"name": "calc", "description": "calculate", "input_schema": {"type": "object"}}]});
    let ir = registry().parse(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.tools[0].name, "calc");
}

#[test]
fn parse_claude_non_object_fails() {
    assert!(registry().parse(Dialect::Claude, &json!("hi")).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// § 13 — Registry parsing: Gemini
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_gemini_basic_request() {
    let v = json!({"model": "gemini-1.5-pro", "contents": [{"role": "user", "parts": [{"text": "hello"}]}]});
    let ir = registry().parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.model.as_deref(), Some("gemini-1.5-pro"));
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].text_content(), "hello");
}

#[test]
fn parse_gemini_system_instruction() {
    let v = json!({"model": "gemini-pro", "system_instruction": {"parts": [{"text": "be concise"}]}, "contents": [{"parts": [{"text": "hi"}]}]});
    let ir = registry().parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.system_prompt.as_deref(), Some("be concise"));
}

#[test]
fn parse_gemini_model_role_maps_to_assistant() {
    let v = json!({"contents": [{"role": "model", "parts": [{"text": "reply"}]}]});
    let ir = registry().parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.messages[0].role, IrRole::Assistant);
}

#[test]
fn parse_gemini_function_call() {
    let v = json!({"contents": [{"role": "model", "parts": [{"functionCall": {"name": "search", "args": {"q": "rust"}}}]}]});
    let ir = registry().parse(Dialect::Gemini, &v).unwrap();
    assert!(ir.messages[0].content[0].is_tool_call());
}

#[test]
fn parse_gemini_function_response() {
    let v = json!({"contents": [{"role": "user", "parts": [{"functionResponse": {"name": "search", "response": {"results": []}}}]}]});
    let ir = registry().parse(Dialect::Gemini, &v).unwrap();
    assert!(ir.messages[0].content[0].is_tool_result());
}

#[test]
fn parse_gemini_inline_data() {
    let v = json!({"contents": [{"parts": [{"inlineData": {"mimeType": "image/jpeg", "data": "base64data"}}]}]});
    let ir = registry().parse(Dialect::Gemini, &v).unwrap();
    if let IrContentBlock::Image { media_type, data } = &ir.messages[0].content[0] {
        assert_eq!(media_type, "image/jpeg");
        assert_eq!(data, "base64data");
    } else {
        panic!("expected image block");
    }
}

#[test]
fn parse_gemini_generation_config() {
    let v = json!({"contents": [{"parts": [{"text": "hi"}]}], "generationConfig": {"maxOutputTokens": 512, "temperature": 0.8, "topP": 0.95, "topK": 40}});
    let ir = registry().parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.config.max_tokens, Some(512));
    assert_eq!(ir.config.temperature, Some(0.8));
    assert_eq!(ir.config.top_p, Some(0.95));
    assert_eq!(ir.config.top_k, Some(40));
}

#[test]
fn parse_gemini_tools() {
    let v = json!({"contents": [], "tools": [{"functionDeclarations": [{"name": "calc", "description": "calculate", "parameters": {}}]}]});
    let ir = registry().parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.tools[0].name, "calc");
}

#[test]
fn parse_gemini_non_object_fails() {
    assert!(registry().parse(Dialect::Gemini, &json!([])).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// § 14 — Registry parsing: Codex
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_codex_instructions_as_system_prompt() {
    let v = json!({"model": "codex-mini", "instructions": "be helpful", "input": "hello"});
    let ir = registry().parse(Dialect::Codex, &v).unwrap();
    assert_eq!(ir.system_prompt.as_deref(), Some("be helpful"));
}

#[test]
fn parse_codex_input_as_user_message() {
    let v = json!({"model": "codex-mini", "input": "fix the bug"});
    let ir = registry().parse(Dialect::Codex, &v).unwrap();
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[0].text_content(), "fix the bug");
}

#[test]
fn parse_codex_items_message() {
    let v = json!({"model": "codex-mini", "items": [{"type": "message", "role": "assistant", "content": "done"}]});
    let ir = registry().parse(Dialect::Codex, &v).unwrap();
    assert_eq!(ir.messages[0].role, IrRole::Assistant);
    assert_eq!(ir.messages[0].text_content(), "done");
}

#[test]
fn parse_codex_items_function_call() {
    let v = json!({"model": "codex-mini", "items": [{"type": "function_call", "call_id": "fc1", "name": "run_tests", "arguments": "{}"}]});
    let ir = registry().parse(Dialect::Codex, &v).unwrap();
    assert!(ir.messages[0].content[0].is_tool_call());
}

#[test]
fn parse_codex_non_object_fails() {
    assert!(registry().parse(Dialect::Codex, &json!(true)).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// § 15 — Registry parsing: Kimi
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_kimi_basic_request() {
    let v = json!({"model": "moonshot-v1-32k", "messages": [{"role": "user", "content": "hello"}], "refs": ["doc1"]});
    let ir = registry().parse(Dialect::Kimi, &v).unwrap();
    assert_eq!(ir.model.as_deref(), Some("moonshot-v1-32k"));
    assert_eq!(ir.messages[0].text_content(), "hello");
}

#[test]
fn parse_kimi_preserves_refs_in_metadata() {
    let v = json!({"model": "kimi", "messages": [{"role": "user", "content": "q"}], "refs": ["a", "b"]});
    let ir = registry().parse(Dialect::Kimi, &v).unwrap();
    assert!(ir.metadata.contains_key("kimi_refs"));
}

#[test]
fn parse_kimi_preserves_search_plus_in_metadata() {
    let v = json!({"model": "kimi", "messages": [{"role": "user", "content": "q"}], "search_plus": true});
    let ir = registry().parse(Dialect::Kimi, &v).unwrap();
    assert!(ir.metadata.contains_key("kimi_search_plus"));
}

#[test]
fn parse_kimi_non_object_fails() {
    assert!(registry().parse(Dialect::Kimi, &json!(null)).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// § 16 — Registry parsing: Copilot
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_copilot_basic_request() {
    let v = json!({"model": "copilot-gpt-4", "messages": [{"role": "user", "content": "fix bug"}], "references": [{"type": "file"}]});
    let ir = registry().parse(Dialect::Copilot, &v).unwrap();
    assert_eq!(ir.model.as_deref(), Some("copilot-gpt-4"));
}

#[test]
fn parse_copilot_preserves_references_in_metadata() {
    let v = json!({"model": "copilot-gpt-4", "messages": [], "references": [{"type": "file", "path": "a.rs"}]});
    let ir = registry().parse(Dialect::Copilot, &v).unwrap();
    assert!(ir.metadata.contains_key("copilot_references"));
}

#[test]
fn parse_copilot_preserves_confirmations_in_metadata() {
    let v = json!({"model": "copilot-gpt-4", "messages": [], "confirmations": [{"id": "c1"}]});
    let ir = registry().parse(Dialect::Copilot, &v).unwrap();
    assert!(ir.metadata.contains_key("copilot_confirmations"));
}

#[test]
fn parse_copilot_preserves_agent_mode_in_metadata() {
    let v = json!({"model": "copilot-gpt-4", "messages": [], "agent_mode": true});
    let ir = registry().parse(Dialect::Copilot, &v).unwrap();
    assert_eq!(
        ir.metadata.get("copilot_agent_mode"),
        Some(&Value::Bool(true))
    );
}

#[test]
fn parse_copilot_non_object_fails() {
    assert!(registry().parse(Dialect::Copilot, &json!("x")).is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// § 17 — Registry serialization round-trips
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_openai_text_message() {
    let reg = registry();
    let v = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hello"}]});
    let ir = reg.parse(Dialect::OpenAi, &v).unwrap();
    let out = reg.serialize(Dialect::OpenAi, &ir).unwrap();
    assert_eq!(out["model"], "gpt-4");
    let msgs = out["messages"].as_array().unwrap();
    assert!(msgs.iter().any(|m| m["content"] == "hello"));
}

#[test]
fn roundtrip_claude_text_message() {
    let reg = registry();
    let v = json!({"model": "claude-3", "system": "be brief", "messages": [{"role": "user", "content": "hi"}], "max_tokens": 100});
    let ir = reg.parse(Dialect::Claude, &v).unwrap();
    let out = reg.serialize(Dialect::Claude, &ir).unwrap();
    assert_eq!(out["model"], "claude-3");
    assert_eq!(out["system"], "be brief");
}

#[test]
fn roundtrip_gemini_text_message() {
    let reg = registry();
    let v = json!({"model": "gemini-pro", "contents": [{"role": "user", "parts": [{"text": "hello"}]}]});
    let ir = reg.parse(Dialect::Gemini, &v).unwrap();
    let out = reg.serialize(Dialect::Gemini, &ir).unwrap();
    assert_eq!(out["model"], "gemini-pro");
    let contents = out["contents"].as_array().unwrap();
    assert!(!contents.is_empty());
}

#[test]
fn roundtrip_codex_instructions() {
    let reg = registry();
    let v =
        json!({"model": "codex-mini", "instructions": "fix the bug", "input": "main.rs has error"});
    let ir = reg.parse(Dialect::Codex, &v).unwrap();
    let out = reg.serialize(Dialect::Codex, &ir).unwrap();
    assert_eq!(out["instructions"], "fix the bug");
    assert_eq!(out["input"], "main.rs has error");
}

#[test]
fn roundtrip_kimi_preserves_refs() {
    let reg = registry();
    let v = json!({"model": "kimi", "messages": [{"role": "user", "content": "q"}], "refs": ["a"]});
    let ir = reg.parse(Dialect::Kimi, &v).unwrap();
    let out = reg.serialize(Dialect::Kimi, &ir).unwrap();
    assert_eq!(out["refs"], json!(["a"]));
}

#[test]
fn roundtrip_copilot_preserves_agent_mode() {
    let reg = registry();
    let v = json!({"model": "copilot-gpt-4", "messages": [{"role": "user", "content": "go"}], "agent_mode": true});
    let ir = reg.parse(Dialect::Copilot, &v).unwrap();
    let out = reg.serialize(Dialect::Copilot, &ir).unwrap();
    assert_eq!(out["agent_mode"], true);
}

// ═══════════════════════════════════════════════════════════════════════
// § 18 — Cross-dialect serialization (parse A → serialize B)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cross_dialect_openai_to_claude() {
    let reg = registry();
    let v = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}], "max_tokens": 100});
    let ir = reg.parse(Dialect::OpenAi, &v).unwrap();
    let out = reg.serialize(Dialect::Claude, &ir).unwrap();
    assert_eq!(out["model"], "gpt-4");
    // Claude format uses messages array
    assert!(out["messages"].is_array());
}

#[test]
fn cross_dialect_claude_to_gemini() {
    let reg = registry();
    let v = json!({"model": "claude-3", "system": "be brief", "messages": [{"role": "user", "content": "hi"}], "max_tokens": 100});
    let ir = reg.parse(Dialect::Claude, &v).unwrap();
    let out = reg.serialize(Dialect::Gemini, &ir).unwrap();
    // Gemini uses "contents" and "system_instruction"
    assert!(out["contents"].is_array());
    assert!(out.get("system_instruction").is_some());
}

#[test]
fn cross_dialect_gemini_to_openai() {
    let reg = registry();
    let v = json!({"model": "gemini-pro", "contents": [{"role": "user", "parts": [{"text": "hello"}]}]});
    let ir = reg.parse(Dialect::Gemini, &v).unwrap();
    let out = reg.serialize(Dialect::OpenAi, &ir).unwrap();
    assert!(out["messages"].is_array());
}

// ═══════════════════════════════════════════════════════════════════════
// § 19 — Validation (DialectValidator in lib.rs)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_openai_valid_request() {
    let v = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let r = validator().validate(&v, Dialect::OpenAi);
    assert!(r.valid);
    assert!(r.errors.is_empty());
}

#[test]
fn validate_openai_missing_model_is_error() {
    let v = json!({"messages": [{"role": "user", "content": "hi"}]});
    let r = validator().validate(&v, Dialect::OpenAi);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.path.contains("model")));
}

#[test]
fn validate_openai_missing_role_in_messages() {
    let v = json!({"model": "gpt-4", "messages": [{"content": "hi"}]});
    let r = validator().validate(&v, Dialect::OpenAi);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.path.contains("role")));
}

#[test]
fn validate_openai_non_array_messages() {
    let v = json!({"model": "gpt-4", "messages": "not an array"});
    let r = validator().validate(&v, Dialect::OpenAi);
    assert!(!r.valid);
}

#[test]
fn validate_openai_no_messages_warns() {
    let v = json!({"model": "gpt-4"});
    let r = validator().validate(&v, Dialect::OpenAi);
    assert!(r.valid); // just a warning
    assert!(!r.warnings.is_empty());
}

#[test]
fn validate_claude_valid_request() {
    let v = json!({"model": "claude-3", "messages": [{"role": "user", "content": "hi"}]});
    let r = validator().validate(&v, Dialect::Claude);
    assert!(r.valid);
}

#[test]
fn validate_claude_response_without_model_is_ok() {
    let v = json!({"type": "message", "content": [{"type": "text", "text": "hi"}], "stop_reason": "end_turn"});
    let r = validator().validate(&v, Dialect::Claude);
    assert!(r.valid);
}

#[test]
fn validate_claude_bad_content_type() {
    let v = json!({"model": "claude-3", "messages": [{"role": "user", "content": 42}]});
    let r = validator().validate(&v, Dialect::Claude);
    assert!(!r.valid);
}

#[test]
fn validate_gemini_valid_request() {
    let v = json!({"contents": [{"parts": [{"text": "hi"}]}]});
    let r = validator().validate(&v, Dialect::Gemini);
    assert!(r.valid);
}

#[test]
fn validate_gemini_missing_parts() {
    let v = json!({"contents": [{"role": "user"}]});
    let r = validator().validate(&v, Dialect::Gemini);
    assert!(!r.valid);
}

#[test]
fn validate_gemini_no_contents_and_no_candidates_warns() {
    let v = json!({"model": "gemini-pro"});
    let r = validator().validate(&v, Dialect::Gemini);
    assert!(r.valid); // warning only
    assert!(!r.warnings.is_empty());
}

#[test]
fn validate_codex_valid_response() {
    let v = json!({"items": [{"type": "message", "content": "done"}]});
    let r = validator().validate(&v, Dialect::Codex);
    assert!(r.valid);
}

#[test]
fn validate_codex_item_missing_type() {
    let v = json!({"items": [{"content": "done"}]});
    let r = validator().validate(&v, Dialect::Codex);
    assert!(!r.valid);
}

#[test]
fn validate_kimi_valid_request() {
    let v = json!({"model": "kimi", "messages": [{"role": "user", "content": "hi"}]});
    let r = validator().validate(&v, Dialect::Kimi);
    assert!(r.valid);
}

#[test]
fn validate_kimi_missing_role() {
    let v = json!({"messages": [{"content": "hi"}]});
    let r = validator().validate(&v, Dialect::Kimi);
    assert!(!r.valid);
}

#[test]
fn validate_copilot_valid_request() {
    let v = json!({"model": "copilot", "messages": [{"role": "user", "content": "hi"}]});
    let r = validator().validate(&v, Dialect::Copilot);
    assert!(r.valid);
}

#[test]
fn validate_copilot_missing_role() {
    let v = json!({"messages": [{"content": "hi"}]});
    let r = validator().validate(&v, Dialect::Copilot);
    assert!(!r.valid);
}

#[test]
fn validate_non_object_returns_error() {
    for &d in Dialect::all() {
        let r = validator().validate(&json!(42), d);
        assert!(!r.valid);
        assert_eq!(r.errors[0].path, "/");
    }
}

#[test]
fn validation_error_display() {
    let e = ValidationError {
        path: "/model".into(),
        message: "missing".into(),
    };
    assert_eq!(format!("{e}"), "/model: missing");
}

#[test]
fn validation_error_is_std_error() {
    let e = ValidationError {
        path: "/".into(),
        message: "bad".into(),
    };
    let _: &dyn std::error::Error = &e;
}

// ═══════════════════════════════════════════════════════════════════════
// § 20 — RequestValidator (validate module)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn req_validator_openai_valid() {
    let v = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let r = req_validator().validate(Dialect::OpenAi, &v);
    assert!(r.is_valid());
}

#[test]
fn req_validator_openai_empty_messages_warning() {
    let v = json!({"model": "gpt-4", "messages": []});
    let r = req_validator().validate(Dialect::OpenAi, &v);
    assert!(r.is_valid()); // warning only
    assert!(r.has_warnings());
}

#[test]
fn req_validator_openai_unknown_role_warning() {
    let v = json!({"model": "gpt-4", "messages": [{"role": "narrator", "content": "hi"}]});
    let r = req_validator().validate(Dialect::OpenAi, &v);
    assert!(r.is_valid());
    assert!(r.issues.iter().any(|i| i.code == "unknown_role"));
}

#[test]
fn req_validator_claude_missing_max_tokens() {
    let v = json!({"model": "claude-3", "messages": [{"role": "user", "content": "hi"}]});
    let r = req_validator().validate(Dialect::Claude, &v);
    assert!(!r.is_valid());
    assert!(r
        .issues
        .iter()
        .any(|i| i.code == "missing_required_field" && i.field == "max_tokens"));
}

#[test]
fn req_validator_claude_non_alternating_roles_warning() {
    let v = json!({"model": "claude-3", "messages": [{"role": "user", "content": "q1"}, {"role": "user", "content": "q2"}], "max_tokens": 100});
    let r = req_validator().validate(Dialect::Claude, &v);
    assert!(r.is_valid()); // warning
    assert!(r.issues.iter().any(|i| i.code == "non_alternating_roles"));
}

#[test]
fn req_validator_gemini_empty_contents_error() {
    let v = json!({"model": "gemini-pro", "contents": []});
    let r = req_validator().validate(Dialect::Gemini, &v);
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.code == "empty_contents"));
}

#[test]
fn req_validator_gemini_empty_parts_warning() {
    let v = json!({"model": "gemini-pro", "contents": [{"parts": []}]});
    let r = req_validator().validate(Dialect::Gemini, &v);
    assert!(r.is_valid()); // warning only
    assert!(r.issues.iter().any(|i| i.code == "empty_parts"));
}

#[test]
fn req_validator_model_with_spaces_warning() {
    let v = json!({"model": "gpt 4", "messages": [{"role": "user", "content": "hi"}]});
    let r = req_validator().validate(Dialect::OpenAi, &v);
    assert!(r.issues.iter().any(|i| i.code == "model_name_has_spaces"));
}

#[test]
fn req_validator_empty_model_error() {
    let v = json!({"model": "", "messages": [{"role": "user", "content": "hi"}]});
    let r = req_validator().validate(Dialect::OpenAi, &v);
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.code == "empty_model_name"));
}

#[test]
fn req_validator_non_string_model_error() {
    let v = json!({"model": 42, "messages": [{"role": "user", "content": "hi"}]});
    let r = req_validator().validate(Dialect::OpenAi, &v);
    assert!(!r.is_valid());
    assert!(r
        .issues
        .iter()
        .any(|i| i.code == "invalid_field_type" && i.field == "model"));
}

#[test]
fn req_validator_non_object_error() {
    let r = req_validator().validate(Dialect::OpenAi, &json!(42));
    assert!(!r.is_valid());
    assert_eq!(r.error_count(), 1);
    assert_eq!(r.issues[0].code, "invalid_type");
}

#[test]
fn severity_display_values() {
    assert_eq!(Severity::Info.to_string(), "info");
    assert_eq!(Severity::Warning.to_string(), "warning");
    assert_eq!(Severity::Error.to_string(), "error");
}

#[test]
fn severity_serde_roundtrip() {
    for sev in [Severity::Info, Severity::Warning, Severity::Error] {
        let s = serde_json::to_string(&sev).unwrap();
        let back: Severity = serde_json::from_str(&s).unwrap();
        assert_eq!(sev, back);
    }
}

#[test]
fn validation_result_counts() {
    let r = ValidationResult {
        issues: vec![
            ValidationIssue {
                severity: Severity::Error,
                field: "a".into(),
                message: "e".into(),
                code: "c".into(),
            },
            ValidationIssue {
                severity: Severity::Warning,
                field: "b".into(),
                message: "w".into(),
                code: "c".into(),
            },
            ValidationIssue {
                severity: Severity::Info,
                field: "c".into(),
                message: "i".into(),
                code: "c".into(),
            },
        ],
    };
    assert!(!r.is_valid());
    assert!(r.has_warnings());
    assert_eq!(r.error_count(), 1);
    assert_eq!(r.warning_count(), 1);
    assert_eq!(r.info_count(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// § 21 — IR types
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ir_message_text_helper() {
    let msg = IrMessage::text(IrRole::User, "hello");
    assert_eq!(msg.text_content(), "hello");
    assert_eq!(msg.role, IrRole::User);
}

#[test]
fn ir_message_tool_calls_empty() {
    let msg = IrMessage::text(IrRole::User, "hi");
    assert!(msg.tool_calls().is_empty());
}

#[test]
fn ir_content_block_as_text() {
    let b = IrContentBlock::Text { text: "hi".into() };
    assert_eq!(b.as_text(), Some("hi"));
}

#[test]
fn ir_content_block_is_tool_call() {
    let b = IrContentBlock::ToolCall {
        id: "1".into(),
        name: "f".into(),
        input: Value::Null,
    };
    assert!(b.is_tool_call());
    assert!(!b.is_tool_result());
}

#[test]
fn ir_content_block_is_tool_result() {
    let b = IrContentBlock::ToolResult {
        tool_call_id: "1".into(),
        content: vec![],
        is_error: false,
    };
    assert!(b.is_tool_result());
    assert!(!b.is_tool_call());
}

#[test]
fn ir_usage_from_io() {
    let u = IrUsage::from_io(100, 50);
    assert_eq!(u.input_tokens, 100);
    assert_eq!(u.output_tokens, 50);
    assert_eq!(u.total_tokens, 150);
}

#[test]
fn ir_usage_merge() {
    let a = IrUsage::from_io(10, 5);
    let b = IrUsage::from_io(20, 10);
    let c = a.merge(b);
    assert_eq!(c.input_tokens, 30);
    assert_eq!(c.output_tokens, 15);
    assert_eq!(c.total_tokens, 45);
}

#[test]
fn ir_request_builder_chain() {
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "hi")])
        .with_model("gpt-4")
        .with_system_prompt("be brief")
        .with_config(IrGenerationConfig {
            max_tokens: Some(100),
            ..Default::default()
        });
    assert_eq!(ir.model.as_deref(), Some("gpt-4"));
    assert_eq!(ir.system_prompt.as_deref(), Some("be brief"));
    assert_eq!(ir.config.max_tokens, Some(100));
}

#[test]
fn ir_response_text_helper() {
    let r = IrResponse::text("hello");
    assert_eq!(r.text_content(), "hello");
}

#[test]
fn ir_response_has_tool_calls() {
    let r = IrResponse::new(vec![IrContentBlock::ToolCall {
        id: "1".into(),
        name: "f".into(),
        input: Value::Null,
    }]);
    assert!(r.has_tool_calls());
}

#[test]
fn ir_stop_reason_serde() {
    let r = IrStopReason::EndTurn;
    let s = serde_json::to_string(&r).unwrap();
    assert_eq!(s, "\"end_turn\"");
    let back: IrStopReason = serde_json::from_str(&s).unwrap();
    assert_eq!(back, IrStopReason::EndTurn);
}

#[test]
fn ir_role_serde() {
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        let s = serde_json::to_string(&role).unwrap();
        let back: IrRole = serde_json::from_str(&s).unwrap();
        assert_eq!(role, back);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// § 22 — Response parsing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_openai_response_with_usage() {
    use abp_dialect::registry::parse_response;
    let v = json!({
        "id": "chatcmpl-xxx",
        "model": "gpt-4",
        "choices": [{"message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
    });
    let r = parse_response(Dialect::OpenAi, &v).unwrap();
    assert_eq!(r.text_content(), "hi");
    assert_eq!(r.stop_reason, Some(IrStopReason::EndTurn));
    let u = r.usage.unwrap();
    assert_eq!(u.input_tokens, 10);
    assert_eq!(u.output_tokens, 5);
}

#[test]
fn parse_claude_response_with_usage() {
    use abp_dialect::registry::parse_response;
    let v = json!({
        "id": "msg_xxx",
        "type": "message",
        "model": "claude-3-opus",
        "content": [{"type": "text", "text": "hello"}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 20, "output_tokens": 10}
    });
    let r = parse_response(Dialect::Claude, &v).unwrap();
    assert_eq!(r.text_content(), "hello");
    assert_eq!(r.stop_reason, Some(IrStopReason::EndTurn));
    let u = r.usage.unwrap();
    assert_eq!(u.input_tokens, 20);
    assert_eq!(u.output_tokens, 10);
}

#[test]
fn parse_gemini_response_with_candidates() {
    use abp_dialect::registry::parse_response;
    let v = json!({
        "candidates": [{"content": {"parts": [{"text": "answer"}]}}],
        "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 3, "totalTokenCount": 8}
    });
    let r = parse_response(Dialect::Gemini, &v).unwrap();
    assert_eq!(r.text_content(), "answer");
    let u = r.usage.unwrap();
    assert_eq!(u.input_tokens, 5);
    assert_eq!(u.output_tokens, 3);
    assert_eq!(u.total_tokens, 8);
}

#[test]
fn parse_openai_response_tool_use_stop_reason() {
    use abp_dialect::registry::parse_response;
    let v = json!({
        "choices": [{"message": {"role": "assistant", "content": null, "tool_calls": [{"id": "tc1", "type": "function", "function": {"name": "f", "arguments": "{}"}}]}, "finish_reason": "tool_calls"}],
        "model": "gpt-4"
    });
    let r = parse_response(Dialect::OpenAi, &v).unwrap();
    assert_eq!(r.stop_reason, Some(IrStopReason::ToolUse));
    assert!(r.has_tool_calls());
}

#[test]
fn parse_openai_response_max_tokens_stop_reason() {
    use abp_dialect::registry::parse_response;
    let v = json!({
        "choices": [{"message": {"role": "assistant", "content": "partial"}, "finish_reason": "length"}],
        "model": "gpt-4"
    });
    let r = parse_response(Dialect::OpenAi, &v).unwrap();
    assert_eq!(r.stop_reason, Some(IrStopReason::MaxTokens));
}

#[test]
fn parse_claude_response_stop_sequence() {
    use abp_dialect::registry::parse_response;
    let v = json!({
        "type": "message",
        "model": "claude-3",
        "content": [{"type": "text", "text": "stopped"}],
        "stop_reason": "stop_sequence"
    });
    let r = parse_response(Dialect::Claude, &v).unwrap();
    assert_eq!(r.stop_reason, Some(IrStopReason::StopSequence));
}

#[test]
fn parse_claude_response_tool_use() {
    use abp_dialect::registry::parse_response;
    let v = json!({
        "type": "message",
        "model": "claude-3",
        "content": [{"type": "tool_use", "id": "tu1", "name": "f", "input": {}}],
        "stop_reason": "tool_use"
    });
    let r = parse_response(Dialect::Claude, &v).unwrap();
    assert_eq!(r.stop_reason, Some(IrStopReason::ToolUse));
    assert!(r.has_tool_calls());
}

// ═══════════════════════════════════════════════════════════════════════
// § 23 — Dialect-specific field mapping normalization
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_system_role_becomes_system_prompt() {
    let reg = registry();
    let v = json!({"model": "gpt-4", "messages": [
        {"role": "system", "content": "You are helpful"},
        {"role": "user", "content": "hi"}
    ]});
    let ir = reg.parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.system_prompt.as_deref(), Some("You are helpful"));
}

#[test]
fn claude_system_field_becomes_system_prompt() {
    let reg = registry();
    let v = json!({"model": "claude-3", "system": "You are helpful", "messages": [{"role": "user", "content": "hi"}], "max_tokens": 100});
    let ir = reg.parse(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.system_prompt.as_deref(), Some("You are helpful"));
}

#[test]
fn gemini_system_instruction_becomes_system_prompt() {
    let reg = registry();
    let v = json!({"model": "gemini-pro", "system_instruction": {"parts": [{"text": "You are helpful"}]}, "contents": [{"parts": [{"text": "hi"}]}]});
    let ir = reg.parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.system_prompt.as_deref(), Some("You are helpful"));
}

#[test]
fn codex_instructions_becomes_system_prompt() {
    let reg = registry();
    let v = json!({"model": "codex-mini", "instructions": "You are helpful"});
    let ir = reg.parse(Dialect::Codex, &v).unwrap();
    assert_eq!(ir.system_prompt.as_deref(), Some("You are helpful"));
}

#[test]
fn gemini_role_model_maps_to_assistant() {
    let reg = registry();
    let v = json!({"contents": [{"role": "model", "parts": [{"text": "I can help"}]}]});
    let ir = reg.parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.messages[0].role, IrRole::Assistant);
}

#[test]
fn openai_tool_role_maps_to_ir_tool() {
    let reg = registry();
    let v = json!({"model": "gpt-4", "messages": [{"role": "tool", "content": "result", "tool_call_id": "tc1"}]});
    let ir = reg.parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.messages[0].role, IrRole::Tool);
}

#[test]
fn openai_stop_as_string_parsed() {
    let reg = registry();
    let v = json!({"model": "gpt-4", "messages": [], "stop": "END"});
    let ir = reg.parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.config.stop_sequences, vec!["END"]);
}

#[test]
fn openai_stop_as_array_parsed() {
    let reg = registry();
    let v = json!({"model": "gpt-4", "messages": [], "stop": ["END", "STOP"]});
    let ir = reg.parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.config.stop_sequences, vec!["END", "STOP"]);
}

#[test]
fn openai_max_completion_tokens_fallback() {
    let reg = registry();
    let v = json!({"model": "gpt-4", "messages": [], "max_completion_tokens": 500});
    let ir = reg.parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.config.max_tokens, Some(500));
}

#[test]
fn gemini_generation_config_snake_case_variant() {
    let reg = registry();
    let v = json!({"contents": [{"parts": [{"text": "hi"}]}], "generation_config": {"max_output_tokens": 256, "top_p": 0.9, "top_k": 30}});
    let ir = reg.parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.config.max_tokens, Some(256));
    assert_eq!(ir.config.top_p, Some(0.9));
    assert_eq!(ir.config.top_k, Some(30));
}

// ═══════════════════════════════════════════════════════════════════════
// § 24 — Claude serialization uses system field not message
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn serialize_claude_system_as_top_level_field() {
    let reg = registry();
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "hi")])
        .with_model("claude-3")
        .with_system_prompt("be brief");
    let out = reg.serialize(Dialect::Claude, &ir).unwrap();
    assert_eq!(out["system"], "be brief");
    // System messages should not appear in messages array
    let msgs = out["messages"].as_array().unwrap();
    assert!(msgs.iter().all(|m| m["role"] != "system"));
}

#[test]
fn serialize_gemini_system_as_instruction() {
    let reg = registry();
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "hi")])
        .with_model("gemini-pro")
        .with_system_prompt("be concise");
    let out = reg.serialize(Dialect::Gemini, &ir).unwrap();
    assert!(out["system_instruction"]["parts"][0]["text"] == "be concise");
}

#[test]
fn serialize_codex_system_as_instructions() {
    let reg = registry();
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "hi")])
        .with_model("codex-mini")
        .with_system_prompt("fix bugs");
    let out = reg.serialize(Dialect::Codex, &ir).unwrap();
    assert_eq!(out["instructions"], "fix bugs");
}

// ═══════════════════════════════════════════════════════════════════════
// § 25 — Tool definition normalization across dialects
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_tool_definitions_parsed() {
    let reg = registry();
    let v = json!({"model": "gpt-4", "messages": [], "tools": [{"type": "function", "function": {"name": "calc", "description": "calculate", "parameters": {"type": "object"}}}]});
    let ir = reg.parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.tools[0].name, "calc");
    assert_eq!(ir.tools[0].description, "calculate");
}

#[test]
fn claude_tool_definitions_use_input_schema() {
    let reg = registry();
    let v = json!({"model": "claude-3", "messages": [], "max_tokens": 100, "tools": [{"name": "calc", "description": "calculate", "input_schema": {"type": "object"}}]});
    let ir = reg.parse(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.tools[0].name, "calc");
    assert_eq!(ir.tools[0].parameters, json!({"type": "object"}));
}

#[test]
fn gemini_tool_definitions_use_function_declarations() {
    let reg = registry();
    let v = json!({"contents": [], "tools": [{"functionDeclarations": [{"name": "calc", "description": "calculate", "parameters": {"type": "object"}}]}]});
    let ir = reg.parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.tools[0].name, "calc");
}

#[test]
fn serialize_openai_tools_use_function_wrapper() {
    let reg = registry();
    let ir = IrRequest::new(vec![])
        .with_model("gpt-4")
        .with_tool(IrToolDefinition {
            name: "calc".into(),
            description: "calculate".into(),
            parameters: json!({"type": "object"}),
        });
    let out = reg.serialize(Dialect::OpenAi, &ir).unwrap();
    let tool = &out["tools"][0];
    assert_eq!(tool["type"], "function");
    assert_eq!(tool["function"]["name"], "calc");
}

#[test]
fn serialize_claude_tools_use_input_schema() {
    let reg = registry();
    let ir = IrRequest::new(vec![])
        .with_model("claude-3")
        .with_tool(IrToolDefinition {
            name: "calc".into(),
            description: "calculate".into(),
            parameters: json!({"type": "object"}),
        });
    let out = reg.serialize(Dialect::Claude, &ir).unwrap();
    assert_eq!(out["tools"][0]["input_schema"], json!({"type": "object"}));
}

#[test]
fn serialize_gemini_tools_use_function_declarations() {
    let reg = registry();
    let ir = IrRequest::new(vec![])
        .with_model("gemini-pro")
        .with_tool(IrToolDefinition {
            name: "calc".into(),
            description: "calculate".into(),
            parameters: json!({"type": "object"}),
        });
    let out = reg.serialize(Dialect::Gemini, &ir).unwrap();
    let decls = &out["tools"][0]["functionDeclarations"];
    assert!(decls.is_array());
    assert_eq!(decls[0]["name"], "calc");
}
