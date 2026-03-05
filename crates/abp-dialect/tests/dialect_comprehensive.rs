#![allow(clippy::all)]
#![allow(unknown_lints)]

use std::collections::BTreeMap;

use serde_json::json;

use abp_dialect::detect::{
    DialectDetectionResult, DialectFingerprint, builtin_fingerprints, detect_dialect,
    detect_from_headers,
};
use abp_dialect::ir::{
    IrContentBlock, IrGenerationConfig, IrMessage, IrRequest, IrResponse, IrRole, IrStopReason,
    IrToolDefinition, IrUsage,
};
use abp_dialect::registry::{DialectError, DialectRegistry, parse_response};
use abp_dialect::validate::{
    RequestValidator, Severity, ValidationIssue, ValidationResult as VResult,
};
use abp_dialect::{Dialect, DialectDetector, DialectValidator, ValidationError};

// ═══════════════════════════════════════════════════════════════════════
// 1. Dialect enum — variants, Display, serde, equality, ordering
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dialect_all_returns_six_variants() {
    assert_eq!(Dialect::all().len(), 6);
}

#[test]
fn dialect_all_order_is_deterministic() {
    let a = Dialect::all();
    let b = Dialect::all();
    assert_eq!(a, b);
}

#[test]
fn dialect_display_openai() {
    assert_eq!(format!("{}", Dialect::OpenAi), "OpenAI");
}

#[test]
fn dialect_display_claude() {
    assert_eq!(format!("{}", Dialect::Claude), "Claude");
}

#[test]
fn dialect_display_gemini() {
    assert_eq!(format!("{}", Dialect::Gemini), "Gemini");
}

#[test]
fn dialect_display_codex() {
    assert_eq!(format!("{}", Dialect::Codex), "Codex");
}

#[test]
fn dialect_display_kimi() {
    assert_eq!(format!("{}", Dialect::Kimi), "Kimi");
}

#[test]
fn dialect_display_copilot() {
    assert_eq!(format!("{}", Dialect::Copilot), "Copilot");
}

#[test]
fn dialect_label_matches_display() {
    for &d in Dialect::all() {
        assert_eq!(d.to_string(), d.label());
    }
}

#[test]
fn dialect_serde_roundtrip_all_variants() {
    for &d in Dialect::all() {
        let s = serde_json::to_string(&d).unwrap();
        let back: Dialect = serde_json::from_str(&s).unwrap();
        assert_eq!(back, d);
    }
}

#[test]
fn dialect_serde_snake_case_names() {
    assert_eq!(
        serde_json::to_string(&Dialect::OpenAi).unwrap(),
        "\"open_ai\""
    );
    assert_eq!(
        serde_json::to_string(&Dialect::Claude).unwrap(),
        "\"claude\""
    );
    assert_eq!(
        serde_json::to_string(&Dialect::Gemini).unwrap(),
        "\"gemini\""
    );
    assert_eq!(serde_json::to_string(&Dialect::Codex).unwrap(), "\"codex\"");
    assert_eq!(serde_json::to_string(&Dialect::Kimi).unwrap(), "\"kimi\"");
    assert_eq!(
        serde_json::to_string(&Dialect::Copilot).unwrap(),
        "\"copilot\""
    );
}

#[test]
fn dialect_deserialize_from_string() {
    let d: Dialect = serde_json::from_str("\"claude\"").unwrap();
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn dialect_deserialize_unknown_fails() {
    let r = serde_json::from_str::<Dialect>("\"unknown_vendor\"");
    assert!(r.is_err());
}

#[test]
fn dialect_equality() {
    assert_eq!(Dialect::OpenAi, Dialect::OpenAi);
    assert_ne!(Dialect::OpenAi, Dialect::Claude);
}

#[test]
fn dialect_clone_and_copy() {
    let d = Dialect::Gemini;
    let c = d;
    assert_eq!(d, c);
}

#[test]
fn dialect_debug_format() {
    let s = format!("{:?}", Dialect::OpenAi);
    assert!(s.contains("OpenAi"));
}

#[test]
fn dialect_ord_is_consistent() {
    let mut v: Vec<Dialect> = Dialect::all().to_vec();
    v.sort();
    let mut v2 = v.clone();
    v2.sort();
    assert_eq!(v, v2);
}

#[test]
fn dialect_hash_works() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    for &d in Dialect::all() {
        set.insert(d);
    }
    assert_eq!(set.len(), 6);
    // Inserting duplicates doesn't grow the set
    set.insert(Dialect::Claude);
    assert_eq!(set.len(), 6);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Dialect detection — DialectDetector (lib.rs)
// ═══════════════════════════════════════════════════════════════════════

fn detector() -> DialectDetector {
    DialectDetector::new()
}

#[test]
fn detect_openai_from_model_prefix() {
    let v = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn detect_openai_chatgpt_prefix() {
    let v = json!({"model": "chatgpt-4o", "messages": [{"role": "user", "content": "hi"}]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn detect_openai_from_choices() {
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
    let v = json!({"type": "message", "model": "claude-3", "content": [{"type": "text", "text": "hi"}], "stop_reason": "end_turn"});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn detect_gemini_from_contents_parts() {
    let v = json!({"contents": [{"parts": [{"text": "hello"}]}]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_gemini_from_candidates() {
    let v = json!({"candidates": [{"content": {"parts": [{"text": "hi"}]}}]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_gemini_model_prefix() {
    let v = json!({"model": "gemini-1.5-pro", "contents": [{"parts": [{"text": "hi"}]}]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_gemini_models_slash_prefix() {
    let v = json!({"model": "models/gemini-pro", "contents": [{"parts": [{"text": "hi"}]}]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_codex_from_items_and_status() {
    let v = json!({"object": "response", "status": "completed", "items": [{"type": "message", "content": "done"}]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Codex);
}

#[test]
fn detect_codex_from_model_prefix() {
    let v = json!({"model": "codex-mini", "items": [{"type": "message"}]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Codex);
}

#[test]
fn detect_kimi_from_refs() {
    let v =
        json!({"model": "kimi", "messages": [{"role": "user", "content": "hi"}], "refs": ["url"]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Kimi);
}

#[test]
fn detect_kimi_from_search_plus() {
    let v = json!({"model": "kimi", "messages": [{"role": "user", "content": "hi"}], "search_plus": true});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Kimi);
}

#[test]
fn detect_kimi_moonshot_prefix() {
    let v = json!({"model": "moonshot-v1-32k", "messages": [{"role": "user", "content": "hi"}], "refs": ["a"]});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Kimi);
}

#[test]
fn detect_copilot_from_references() {
    let v = json!({"messages": [{"role": "user", "content": "fix"}], "references": [{"type": "file", "path": "main.rs"}]});
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
fn detect_returns_none_for_non_object() {
    assert!(detector().detect(&json!(42)).is_none());
    assert!(detector().detect(&json!("text")).is_none());
    assert!(detector().detect(&json!(null)).is_none());
    assert!(detector().detect(&json!([])).is_none());
    assert!(detector().detect(&json!(true)).is_none());
}

#[test]
fn detect_returns_none_for_empty_object() {
    assert!(detector().detect(&json!({})).is_none());
}

#[test]
fn detect_all_returns_empty_for_non_object() {
    assert!(detector().detect_all(&json!(null)).is_empty());
}

#[test]
fn detect_all_sorted_descending_confidence() {
    let v =
        json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}], "refs": ["a"]});
    let results = detector().detect_all(&v);
    for w in results.windows(2) {
        assert!(w[0].confidence >= w[1].confidence);
    }
}

#[test]
fn detect_all_returns_multiple_candidates() {
    let v = json!({"model": "x", "messages": [{"role": "user", "content": "hi"}], "refs": ["a"]});
    let results = detector().detect_all(&v);
    assert!(
        results.len() >= 2,
        "ambiguous input should match multiple dialects"
    );
}

#[test]
fn confidence_capped_at_one() {
    let v = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}], "choices": [{}], "temperature": 0.7, "max_tokens": 100});
    let r = detector().detect(&v).unwrap();
    assert!(r.confidence <= 1.0);
}

#[test]
fn evidence_is_non_empty_on_match() {
    let v = json!({"choices": [{}]});
    let r = detector().detect(&v).unwrap();
    assert!(!r.evidence.is_empty());
}

// ── Header detection ────────────────────────────────────────────────

#[test]
fn detect_from_headers_anthropic() {
    let mut h = BTreeMap::new();
    h.insert("anthropic-version".into(), "2023-06-01".into());
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn detect_from_headers_github_copilot() {
    let mut h = BTreeMap::new();
    h.insert("copilot-integration-id".into(), "vscode".into());
    h.insert("x-github-token".into(), "tok".into());
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn detect_from_headers_gemini() {
    let mut h = BTreeMap::new();
    h.insert("x-goog-api-key".into(), "AIza...".into());
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_from_headers_openai() {
    let mut h = BTreeMap::new();
    h.insert("openai-organization".into(), "org-abc".into());
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn detect_from_headers_empty_returns_none() {
    assert!(detector().detect_from_headers(&BTreeMap::new()).is_none());
}

#[test]
fn detect_from_headers_unknown_returns_none() {
    let mut h = BTreeMap::new();
    h.insert("x-custom-header".into(), "val".into());
    assert!(detector().detect_from_headers(&h).is_none());
}

// ── Endpoint detection ──────────────────────────────────────────────

#[test]
fn detect_from_endpoint_anthropic() {
    let r = detector()
        .detect_from_endpoint("https://api.anthropic.com/v1/messages")
        .unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn detect_from_endpoint_gemini() {
    let r = detector()
        .detect_from_endpoint(
            "https://generativelanguage.googleapis.com/v1/models/gemini-pro:generateContent",
        )
        .unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_from_endpoint_kimi() {
    let r = detector()
        .detect_from_endpoint("https://api.moonshot.cn/v1/chat/completions")
        .unwrap();
    assert_eq!(r.dialect, Dialect::Kimi);
}

#[test]
fn detect_from_endpoint_openai() {
    let r = detector()
        .detect_from_endpoint("https://api.openai.com/v1/chat/completions")
        .unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn detect_from_endpoint_codex_responses() {
    let r = detector()
        .detect_from_endpoint("https://api.openai.com/v1/responses")
        .unwrap();
    assert_eq!(r.dialect, Dialect::Codex);
}

#[test]
fn detect_from_endpoint_copilot() {
    let r = detector()
        .detect_from_endpoint("https://api.github.com/copilot/chat")
        .unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn detect_from_endpoint_unknown() {
    assert!(
        detector()
            .detect_from_endpoint("https://example.com/api")
            .is_none()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 3. detect module — fingerprint-based detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn builtin_fingerprints_cover_all_dialects() {
    let fps = builtin_fingerprints();
    for &d in Dialect::all() {
        assert!(fps.contains_key(&d), "missing fingerprint for {d:?}");
    }
}

#[test]
fn fingerprint_detect_openai_model_and_fields() {
    let v = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}], "frequency_penalty": 0.5});
    let r = detect_dialect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
    assert!(r.confidence > 0.4);
}

#[test]
fn fingerprint_detect_claude_model() {
    let v = json!({"model": "claude-3-sonnet", "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]});
    let r = detect_dialect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn fingerprint_detect_gemini() {
    let v = json!({"model": "gemini-1.5-pro", "contents": [{"parts": [{"text": "hi"}]}]});
    let r = detect_dialect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn fingerprint_detect_codex() {
    let v = json!({"model": "codex-mini", "instructions": "fix", "items": [{"type": "msg"}]});
    let r = detect_dialect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Codex);
}

#[test]
fn fingerprint_detect_kimi() {
    let v = json!({"model": "moonshot-v1", "refs": ["a"], "search_plus": true});
    let r = detect_dialect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Kimi);
}

#[test]
fn fingerprint_detect_copilot() {
    let v = json!({"model": "copilot-gpt-4", "references": [{"type": "file"}], "confirmations": [], "agent_mode": true});
    let r = detect_dialect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn fingerprint_detect_none_for_non_object() {
    assert!(detect_dialect(&json!(42)).is_none());
    assert!(detect_dialect(&json!("hello")).is_none());
    assert!(detect_dialect(&json!(null)).is_none());
}

#[test]
fn fingerprint_detect_none_for_empty_object() {
    assert!(detect_dialect(&json!({})).is_none());
}

#[test]
fn fingerprint_headers_detect_anthropic() {
    let mut h = BTreeMap::new();
    h.insert("anthropic-version".into(), "2023-06-01".into());
    let r = detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn fingerprint_headers_detect_copilot() {
    let mut h = BTreeMap::new();
    h.insert("x-github-token".into(), "tok".into());
    h.insert("copilot-integration-id".into(), "vscode".into());
    let r = detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn fingerprint_headers_detect_gemini_goog() {
    let mut h = BTreeMap::new();
    h.insert("x-goog-api-key".into(), "key".into());
    let r = detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn fingerprint_headers_none_for_empty() {
    assert!(detect_from_headers(&BTreeMap::new()).is_none());
}

#[test]
fn fingerprint_headers_bearer_prefix_openai() {
    let mut h = BTreeMap::new();
    h.insert("authorization".into(), "Bearer sk-xxx".into());
    let r = detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Dialect registry — registration, lookup, iteration
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn registry_new_is_empty() {
    let r = DialectRegistry::new();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
}

#[test]
fn registry_with_builtins_has_all_dialects() {
    let r = DialectRegistry::with_builtins();
    assert_eq!(r.len(), 6);
    for &d in Dialect::all() {
        assert!(r.get(d).is_some(), "missing builtin entry for {d:?}");
    }
}

#[test]
fn registry_list_dialects_deterministic() {
    let r = DialectRegistry::with_builtins();
    let a = r.list_dialects();
    let b = r.list_dialects();
    assert_eq!(a, b);
}

#[test]
fn registry_supports_pair_both_registered() {
    let r = DialectRegistry::with_builtins();
    assert!(r.supports_pair(Dialect::OpenAi, Dialect::Claude));
    assert!(r.supports_pair(Dialect::Gemini, Dialect::Copilot));
}

#[test]
fn registry_supports_pair_unregistered() {
    let r = DialectRegistry::new();
    assert!(!r.supports_pair(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn registry_get_returns_none_for_unregistered() {
    let r = DialectRegistry::new();
    assert!(r.get(Dialect::OpenAi).is_none());
}

#[test]
fn registry_entry_has_correct_metadata() {
    let r = DialectRegistry::with_builtins();
    let e = r.get(Dialect::OpenAi).unwrap();
    assert_eq!(e.dialect, Dialect::OpenAi);
    assert_eq!(e.name, "openai");
    assert_eq!(e.version, "v1");
}

#[test]
fn registry_entry_metadata_claude() {
    let r = DialectRegistry::with_builtins();
    let e = r.get(Dialect::Claude).unwrap();
    assert_eq!(e.name, "claude");
}

#[test]
fn registry_entry_metadata_gemini() {
    let r = DialectRegistry::with_builtins();
    let e = r.get(Dialect::Gemini).unwrap();
    assert_eq!(e.name, "gemini");
}

#[test]
fn registry_entry_debug() {
    let r = DialectRegistry::with_builtins();
    let e = r.get(Dialect::OpenAi).unwrap();
    let s = format!("{:?}", e);
    assert!(s.contains("openai"));
}

#[test]
fn registry_parse_error_for_unregistered() {
    let r = DialectRegistry::new();
    let err = r.parse(Dialect::OpenAi, &json!({})).unwrap_err();
    assert_eq!(err.dialect, Dialect::OpenAi);
    assert!(err.message.contains("not registered"));
}

#[test]
fn registry_serialize_error_for_unregistered() {
    let r = DialectRegistry::new();
    let ir = IrRequest::new(vec![]);
    let err = r.serialize(Dialect::Claude, &ir).unwrap_err();
    assert_eq!(err.dialect, Dialect::Claude);
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Dialect parsing — vendor-specific request format parsing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_openai_basic_request() {
    let r = DialectRegistry::with_builtins();
    let v = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hello"}]});
    let ir = r.parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.model.as_deref(), Some("gpt-4"));
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[0].text_content(), "hello");
}

#[test]
fn parse_openai_system_prompt_extracted() {
    let r = DialectRegistry::with_builtins();
    let v = json!({"model": "gpt-4", "messages": [{"role": "system", "content": "be helpful"}, {"role": "user", "content": "hi"}]});
    let ir = r.parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.system_prompt.as_deref(), Some("be helpful"));
}

#[test]
fn parse_openai_with_tools() {
    let r = DialectRegistry::with_builtins();
    let v = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "function", "function": {"name": "get_weather", "description": "get weather", "parameters": {"type": "object"}}}]
    });
    let ir = r.parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.tools.len(), 1);
    assert_eq!(ir.tools[0].name, "get_weather");
}

#[test]
fn parse_openai_config_fields() {
    let r = DialectRegistry::with_builtins();
    let v = json!({"model": "gpt-4", "messages": [], "max_tokens": 100, "temperature": 0.7, "top_p": 0.9, "stop": ["END"]});
    let ir = r.parse(Dialect::OpenAi, &v).unwrap();
    assert_eq!(ir.config.max_tokens, Some(100));
    assert_eq!(ir.config.temperature, Some(0.7));
    assert_eq!(ir.config.top_p, Some(0.9));
    assert_eq!(ir.config.stop_sequences, vec!["END"]);
}

#[test]
fn parse_openai_tool_calls_in_message() {
    let r = DialectRegistry::with_builtins();
    let v = json!({
        "model": "gpt-4",
        "messages": [{
            "role": "assistant",
            "content": null,
            "tool_calls": [{"id": "tc1", "type": "function", "function": {"name": "foo", "arguments": "{\"x\":1}"}}]
        }]
    });
    let ir = r.parse(Dialect::OpenAi, &v).unwrap();
    assert!(!ir.messages[0].tool_calls().is_empty());
}

#[test]
fn parse_openai_non_object_error() {
    let r = DialectRegistry::with_builtins();
    assert!(r.parse(Dialect::OpenAi, &json!("string")).is_err());
}

#[test]
fn parse_claude_basic_request() {
    let r = DialectRegistry::with_builtins();
    let v = json!({
        "model": "claude-3",
        "system": "be helpful",
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 1024
    });
    let ir = r.parse(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.model.as_deref(), Some("claude-3"));
    assert_eq!(ir.system_prompt.as_deref(), Some("be helpful"));
    assert_eq!(ir.config.max_tokens, Some(1024));
}

#[test]
fn parse_claude_content_blocks() {
    let r = DialectRegistry::with_builtins();
    let v = json!({
        "model": "claude-3",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hello"}]}],
        "max_tokens": 100
    });
    let ir = r.parse(Dialect::Claude, &v).unwrap();
    assert_eq!(ir.messages[0].text_content(), "hello");
}

#[test]
fn parse_claude_tool_use_block() {
    let r = DialectRegistry::with_builtins();
    let v = json!({
        "model": "claude-3",
        "messages": [{"role": "assistant", "content": [{"type": "tool_use", "id": "t1", "name": "calc", "input": {"x": 1}}]}],
        "max_tokens": 100
    });
    let ir = r.parse(Dialect::Claude, &v).unwrap();
    assert!(!ir.messages[0].tool_calls().is_empty());
}

#[test]
fn parse_gemini_basic_request() {
    let r = DialectRegistry::with_builtins();
    let v =
        json!({"model": "gemini-pro", "contents": [{"role": "user", "parts": [{"text": "hi"}]}]});
    let ir = r.parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.model.as_deref(), Some("gemini-pro"));
    assert_eq!(ir.messages.len(), 1);
    assert_eq!(ir.messages[0].text_content(), "hi");
}

#[test]
fn parse_gemini_system_instruction() {
    let r = DialectRegistry::with_builtins();
    let v = json!({
        "model": "gemini-pro",
        "system_instruction": {"parts": [{"text": "be nice"}]},
        "contents": [{"parts": [{"text": "hi"}]}]
    });
    let ir = r.parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.system_prompt.as_deref(), Some("be nice"));
}

#[test]
fn parse_gemini_generation_config() {
    let r = DialectRegistry::with_builtins();
    let v = json!({
        "model": "gemini-pro",
        "contents": [{"parts": [{"text": "hi"}]}],
        "generationConfig": {"maxOutputTokens": 200, "temperature": 0.5, "topP": 0.8, "topK": 40}
    });
    let ir = r.parse(Dialect::Gemini, &v).unwrap();
    assert_eq!(ir.config.max_tokens, Some(200));
    assert_eq!(ir.config.temperature, Some(0.5));
    assert_eq!(ir.config.top_p, Some(0.8));
    assert_eq!(ir.config.top_k, Some(40));
}

#[test]
fn parse_codex_request() {
    let r = DialectRegistry::with_builtins();
    let v = json!({"model": "codex-mini", "instructions": "fix bugs", "input": "code here"});
    let ir = r.parse(Dialect::Codex, &v).unwrap();
    assert_eq!(ir.model.as_deref(), Some("codex-mini"));
    assert_eq!(ir.system_prompt.as_deref(), Some("fix bugs"));
    assert!(
        ir.messages
            .iter()
            .any(|m| m.text_content().contains("code here"))
    );
}

#[test]
fn parse_kimi_preserves_metadata() {
    let r = DialectRegistry::with_builtins();
    let v = json!({"model": "kimi", "messages": [{"role": "user", "content": "hi"}], "refs": ["url1"], "search_plus": true});
    let ir = r.parse(Dialect::Kimi, &v).unwrap();
    assert!(ir.metadata.contains_key("kimi_refs"));
    assert!(ir.metadata.contains_key("kimi_search_plus"));
}

#[test]
fn parse_copilot_preserves_metadata() {
    let r = DialectRegistry::with_builtins();
    let v = json!({
        "model": "copilot-gpt-4",
        "messages": [{"role": "user", "content": "fix"}],
        "references": [{"type": "file"}],
        "confirmations": [],
        "agent_mode": true
    });
    let ir = r.parse(Dialect::Copilot, &v).unwrap();
    assert!(ir.metadata.contains_key("copilot_references"));
    assert!(ir.metadata.contains_key("copilot_confirmations"));
    assert!(ir.metadata.contains_key("copilot_agent_mode"));
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Serialization roundtrips
// ═══════════════════════════════════════════════════════════════════════

fn make_simple_ir() -> IrRequest {
    IrRequest::new(vec![IrMessage::text(IrRole::User, "hello")])
        .with_model("test-model")
        .with_system_prompt("be helpful")
}

#[test]
fn serialize_openai_roundtrip() {
    let r = DialectRegistry::with_builtins();
    let ir = make_simple_ir();
    let json = r.serialize(Dialect::OpenAi, &ir).unwrap();
    let obj = json.as_object().unwrap();
    assert_eq!(obj["model"], "test-model");
    assert!(obj["messages"].is_array());
}

#[test]
fn serialize_claude_roundtrip() {
    let r = DialectRegistry::with_builtins();
    let ir = make_simple_ir();
    let json = r.serialize(Dialect::Claude, &ir).unwrap();
    let obj = json.as_object().unwrap();
    assert_eq!(obj["model"], "test-model");
    assert_eq!(obj["system"], "be helpful");
}

#[test]
fn serialize_gemini_roundtrip() {
    let r = DialectRegistry::with_builtins();
    let ir = make_simple_ir();
    let json = r.serialize(Dialect::Gemini, &ir).unwrap();
    let obj = json.as_object().unwrap();
    assert_eq!(obj["model"], "test-model");
    assert!(obj.contains_key("system_instruction"));
    assert!(obj["contents"].is_array());
}

#[test]
fn serialize_codex_roundtrip() {
    let r = DialectRegistry::with_builtins();
    let ir = make_simple_ir();
    let json = r.serialize(Dialect::Codex, &ir).unwrap();
    let obj = json.as_object().unwrap();
    assert_eq!(obj["model"], "test-model");
    assert_eq!(obj["instructions"], "be helpful");
    assert_eq!(obj["input"], "hello");
}

#[test]
fn serialize_kimi_preserves_metadata() {
    let r = DialectRegistry::with_builtins();
    let mut ir = make_simple_ir();
    ir.metadata.insert("kimi_refs".into(), json!(["url1"]));
    ir.metadata.insert("kimi_search_plus".into(), json!(true));
    let json = r.serialize(Dialect::Kimi, &ir).unwrap();
    let obj = json.as_object().unwrap();
    assert!(obj.contains_key("refs"));
    assert!(obj.contains_key("search_plus"));
}

#[test]
fn serialize_copilot_preserves_metadata() {
    let r = DialectRegistry::with_builtins();
    let mut ir = make_simple_ir();
    ir.metadata
        .insert("copilot_references".into(), json!([{"type": "file"}]));
    ir.metadata.insert("copilot_agent_mode".into(), json!(true));
    let json = r.serialize(Dialect::Copilot, &ir).unwrap();
    let obj = json.as_object().unwrap();
    assert!(obj.contains_key("references"));
    assert!(obj.contains_key("agent_mode"));
}

#[test]
fn serialize_openai_with_tools() {
    let r = DialectRegistry::with_builtins();
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "hi")])
        .with_model("gpt-4")
        .with_tool(IrToolDefinition {
            name: "calc".into(),
            description: "calculator".into(),
            parameters: json!({"type": "object"}),
        });
    let json = r.serialize(Dialect::OpenAi, &ir).unwrap();
    let tools = json["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["function"]["name"], "calc");
}

#[test]
fn parse_serialize_openai_round_trip_preserves_model() {
    let r = DialectRegistry::with_builtins();
    let orig = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}], "max_tokens": 50});
    let ir = r.parse(Dialect::OpenAi, &orig).unwrap();
    let out = r.serialize(Dialect::OpenAi, &ir).unwrap();
    assert_eq!(out["model"], "gpt-4");
    assert_eq!(out["max_tokens"], 50);
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Response parsing
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parse_openai_response_basic() {
    let v = json!({
        "id": "chatcmpl-123",
        "model": "gpt-4",
        "choices": [{"message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
    });
    let resp = parse_response(Dialect::OpenAi, &v).unwrap();
    assert_eq!(resp.id.as_deref(), Some("chatcmpl-123"));
    assert_eq!(resp.text_content(), "hi");
    assert_eq!(resp.stop_reason, Some(IrStopReason::EndTurn));
    assert_eq!(resp.usage.unwrap().input_tokens, 10);
}

#[test]
fn parse_claude_response_basic() {
    let v = json!({
        "id": "msg_123",
        "model": "claude-3",
        "content": [{"type": "text", "text": "hello"}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 10, "output_tokens": 20}
    });
    let resp = parse_response(Dialect::Claude, &v).unwrap();
    assert_eq!(resp.text_content(), "hello");
    assert_eq!(resp.stop_reason, Some(IrStopReason::EndTurn));
}

#[test]
fn parse_gemini_response_basic() {
    let v = json!({
        "candidates": [{"content": {"parts": [{"text": "world"}]}}],
        "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 3, "totalTokenCount": 8}
    });
    let resp = parse_response(Dialect::Gemini, &v).unwrap();
    assert_eq!(resp.text_content(), "world");
    assert_eq!(resp.usage.unwrap().total_tokens, 8);
}

#[test]
fn parse_response_fallback_for_kimi() {
    // Kimi falls back to OpenAI-compatible parsing
    let v = json!({
        "choices": [{"message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}]
    });
    let resp = parse_response(Dialect::Kimi, &v).unwrap();
    assert_eq!(resp.text_content(), "ok");
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Validation — DialectValidator (lib.rs)
// ═══════════════════════════════════════════════════════════════════════

fn validator() -> DialectValidator {
    DialectValidator::new()
}

#[test]
fn validate_openai_valid_request() {
    let v = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let r = validator().validate(&v, Dialect::OpenAi);
    assert!(r.valid);
    assert!(r.errors.is_empty());
}

#[test]
fn validate_openai_missing_model() {
    let v = json!({"messages": [{"role": "user", "content": "hi"}]});
    let r = validator().validate(&v, Dialect::OpenAi);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.path.contains("model")));
}

#[test]
fn validate_openai_missing_role_in_message() {
    let v = json!({"model": "gpt-4", "messages": [{"content": "hi"}]});
    let r = validator().validate(&v, Dialect::OpenAi);
    assert!(!r.valid);
}

#[test]
fn validate_openai_messages_not_array() {
    let v = json!({"model": "gpt-4", "messages": "not_array"});
    let r = validator().validate(&v, Dialect::OpenAi);
    assert!(!r.valid);
}

#[test]
fn validate_claude_valid_request() {
    let v = json!({"model": "claude-3", "messages": [{"role": "user", "content": "hi"}]});
    let r = validator().validate(&v, Dialect::Claude);
    assert!(r.valid);
}

#[test]
fn validate_claude_response_no_model_allowed() {
    let v = json!({"type": "message", "content": [{"type": "text", "text": "ok"}], "stop_reason": "end_turn"});
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
fn validate_gemini_valid() {
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
fn validate_gemini_no_contents_or_candidates_warns() {
    let v = json!({"model": "gemini-pro"});
    let r = validator().validate(&v, Dialect::Gemini);
    assert!(r.valid); // just a warning
    assert!(!r.warnings.is_empty());
}

#[test]
fn validate_codex_item_missing_type() {
    let v = json!({"items": [{"content": "done"}]});
    let r = validator().validate(&v, Dialect::Codex);
    assert!(!r.valid);
}

#[test]
fn validate_kimi_missing_role() {
    let v = json!({"messages": [{"content": "hi"}]});
    let r = validator().validate(&v, Dialect::Kimi);
    assert!(!r.valid);
}

#[test]
fn validate_copilot_missing_role() {
    let v = json!({"messages": [{"content": "hi"}]});
    let r = validator().validate(&v, Dialect::Copilot);
    assert!(!r.valid);
}

#[test]
fn validate_non_object_returns_error() {
    let r = validator().validate(&json!("oops"), Dialect::OpenAi);
    assert!(!r.valid);
    assert!(r.errors[0].path == "/");
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Validation — RequestValidator (validate.rs)
// ═══════════════════════════════════════════════════════════════════════

fn rv() -> RequestValidator {
    RequestValidator::new()
}

#[test]
fn rv_non_object_is_error() {
    let r = rv().validate(Dialect::OpenAi, &json!(42));
    assert!(!r.is_valid());
    assert_eq!(r.error_count(), 1);
}

#[test]
fn rv_openai_valid() {
    let r = rv().validate(
        Dialect::OpenAi,
        &json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]}),
    );
    assert!(r.is_valid());
}

#[test]
fn rv_claude_missing_max_tokens() {
    let r = rv().validate(
        Dialect::Claude,
        &json!({"model": "claude-3", "messages": [{"role": "user", "content": "hi"}]}),
    );
    assert!(!r.is_valid());
    assert!(r.issues.iter().any(|i| i.field == "max_tokens"));
}

#[test]
fn rv_gemini_empty_contents_error() {
    let r = rv().validate(
        Dialect::Gemini,
        &json!({"model": "gemini-pro", "contents": []}),
    );
    assert!(!r.is_valid());
}

#[test]
fn rv_model_with_spaces_warns() {
    let r = rv().validate(
        Dialect::OpenAi,
        &json!({"model": "gpt 4", "messages": [{"role": "user", "content": "hi"}]}),
    );
    assert!(r.is_valid()); // warning, not error
    assert!(r.has_warnings());
}

#[test]
fn rv_empty_model_name_error() {
    let r = rv().validate(
        Dialect::OpenAi,
        &json!({"model": "", "messages": [{"role": "user", "content": "hi"}]}),
    );
    assert!(!r.is_valid());
}

#[test]
fn rv_validation_result_counts() {
    let r = rv().validate(
        Dialect::Claude,
        &json!({"model": "claude-3", "messages": [{"role": "user", "content": "hi"}]}),
    );
    assert!(r.error_count() > 0); // missing max_tokens
    assert_eq!(r.info_count(), 0);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Display and Error formatting
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validation_error_display() {
    let e = ValidationError {
        path: "/model".into(),
        message: "missing".into(),
    };
    let s = e.to_string();
    assert_eq!(s, "/model: missing");
}

#[test]
fn validation_error_is_std_error() {
    let e = ValidationError {
        path: "/x".into(),
        message: "bad".into(),
    };
    let _: &dyn std::error::Error = &e;
}

#[test]
fn dialect_error_display() {
    let e = DialectError {
        dialect: Dialect::OpenAi,
        message: "parse failed".into(),
    };
    let s = e.to_string();
    assert!(s.contains("OpenAI"));
    assert!(s.contains("parse failed"));
}

#[test]
fn dialect_error_is_std_error() {
    let e = DialectError {
        dialect: Dialect::Claude,
        message: "oops".into(),
    };
    let _: &dyn std::error::Error = &e;
}

#[test]
fn severity_display() {
    assert_eq!(Severity::Info.to_string(), "info");
    assert_eq!(Severity::Warning.to_string(), "warning");
    assert_eq!(Severity::Error.to_string(), "error");
}

#[test]
fn severity_serde_roundtrip() {
    for s in [Severity::Info, Severity::Warning, Severity::Error] {
        let json = serde_json::to_string(&s).unwrap();
        let back: Severity = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
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
    let s = issue.to_string();
    assert!(s.contains("error"));
    assert!(s.contains("model"));
    assert!(s.contains("missing"));
    assert!(s.contains("missing_required_field"));
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
    assert_eq!(back, issue);
}

// ═══════════════════════════════════════════════════════════════════════
// 11. IR types — serde roundtrips
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
        assert_eq!(back, role);
    }
}

#[test]
fn ir_content_block_text_roundtrip() {
    let b = IrContentBlock::Text {
        text: "hello".into(),
    };
    let json = serde_json::to_value(&b).unwrap();
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, b);
}

#[test]
fn ir_content_block_tool_call_roundtrip() {
    let b = IrContentBlock::ToolCall {
        id: "tc1".into(),
        name: "calc".into(),
        input: json!({"x": 1}),
    };
    let json = serde_json::to_value(&b).unwrap();
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, b);
}

#[test]
fn ir_content_block_tool_result_roundtrip() {
    let b = IrContentBlock::ToolResult {
        tool_call_id: "tc1".into(),
        content: vec![IrContentBlock::Text {
            text: "result".into(),
        }],
        is_error: false,
    };
    let json = serde_json::to_value(&b).unwrap();
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, b);
}

#[test]
fn ir_content_block_image_roundtrip() {
    let b = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "abc".into(),
    };
    let json = serde_json::to_value(&b).unwrap();
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, b);
}

#[test]
fn ir_content_block_thinking_roundtrip() {
    let b = IrContentBlock::Thinking { text: "hmm".into() };
    let json = serde_json::to_value(&b).unwrap();
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, b);
}

#[test]
fn ir_content_block_audio_roundtrip() {
    let b = IrContentBlock::Audio {
        media_type: "audio/wav".into(),
        data: "wav_data".into(),
    };
    let json = serde_json::to_value(&b).unwrap();
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, b);
}

#[test]
fn ir_content_block_custom_roundtrip() {
    let b = IrContentBlock::Custom {
        custom_type: "mytype".into(),
        data: json!({"k": "v"}),
    };
    let json = serde_json::to_value(&b).unwrap();
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, b);
}

#[test]
fn ir_content_block_as_text() {
    let text = IrContentBlock::Text {
        text: "hello".into(),
    };
    assert_eq!(text.as_text(), Some("hello"));
    let tool = IrContentBlock::ToolCall {
        id: "x".into(),
        name: "y".into(),
        input: json!(null),
    };
    assert_eq!(tool.as_text(), None);
}

#[test]
fn ir_content_block_is_tool_call() {
    let tc = IrContentBlock::ToolCall {
        id: "x".into(),
        name: "y".into(),
        input: json!(null),
    };
    assert!(tc.is_tool_call());
    assert!(!tc.is_tool_result());
}

#[test]
fn ir_content_block_is_tool_result() {
    let tr = IrContentBlock::ToolResult {
        tool_call_id: "x".into(),
        content: vec![],
        is_error: false,
    };
    assert!(tr.is_tool_result());
    assert!(!tr.is_tool_call());
}

#[test]
fn ir_message_text_content() {
    let msg = IrMessage::text(IrRole::User, "hello world");
    assert_eq!(msg.text_content(), "hello world");
    assert_eq!(msg.role, IrRole::User);
}

#[test]
fn ir_message_tool_calls() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "let me call".into(),
            },
            IrContentBlock::ToolCall {
                id: "t1".into(),
                name: "foo".into(),
                input: json!(null),
            },
        ],
    );
    assert_eq!(msg.tool_calls().len(), 1);
}

#[test]
fn ir_message_serde_roundtrip() {
    let msg = IrMessage::text(IrRole::User, "hello");
    let json = serde_json::to_value(&msg).unwrap();
    let back: IrMessage = serde_json::from_value(json).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn ir_usage_from_io() {
    let u = IrUsage::from_io(10, 20);
    assert_eq!(u.input_tokens, 10);
    assert_eq!(u.output_tokens, 20);
    assert_eq!(u.total_tokens, 30);
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
fn ir_usage_default() {
    let u = IrUsage::default();
    assert_eq!(u.input_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

#[test]
fn ir_usage_serde_roundtrip() {
    let u = IrUsage::from_io(100, 200);
    let json = serde_json::to_value(&u).unwrap();
    let back: IrUsage = serde_json::from_value(json).unwrap();
    assert_eq!(back, u);
}

#[test]
fn ir_generation_config_default() {
    let c = IrGenerationConfig::default();
    assert!(c.max_tokens.is_none());
    assert!(c.temperature.is_none());
    assert!(c.stop_sequences.is_empty());
}

#[test]
fn ir_generation_config_serde_roundtrip() {
    let c = IrGenerationConfig {
        max_tokens: Some(100),
        temperature: Some(0.7),
        top_p: Some(0.9),
        top_k: Some(40),
        stop_sequences: vec!["END".into()],
        extra: BTreeMap::new(),
    };
    let json = serde_json::to_value(&c).unwrap();
    let back: IrGenerationConfig = serde_json::from_value(json).unwrap();
    assert_eq!(back, c);
}

#[test]
fn ir_request_builder_chain() {
    let ir = IrRequest::new(vec![IrMessage::text(IrRole::User, "hi")])
        .with_model("test")
        .with_system_prompt("prompt")
        .with_config(IrGenerationConfig {
            max_tokens: Some(50),
            ..Default::default()
        })
        .with_tool(IrToolDefinition {
            name: "t".into(),
            description: "d".into(),
            parameters: json!({}),
        });
    assert_eq!(ir.model.as_deref(), Some("test"));
    assert_eq!(ir.system_prompt.as_deref(), Some("prompt"));
    assert_eq!(ir.config.max_tokens, Some(50));
    assert_eq!(ir.tools.len(), 1);
}

#[test]
fn ir_request_system_message() {
    let ir = IrRequest::new(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    assert!(ir.system_message().is_some());
    assert_eq!(ir.system_message().unwrap().text_content(), "sys");
}

#[test]
fn ir_request_all_tool_calls() {
    let ir = IrRequest::new(vec![
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolCall {
                id: "t1".into(),
                name: "a".into(),
                input: json!(null),
            }],
        ),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolCall {
                id: "t2".into(),
                name: "b".into(),
                input: json!(null),
            }],
        ),
    ]);
    assert_eq!(ir.all_tool_calls().len(), 2);
}

#[test]
fn ir_request_serde_roundtrip() {
    let ir = make_simple_ir();
    let json = serde_json::to_value(&ir).unwrap();
    let back: IrRequest = serde_json::from_value(json).unwrap();
    assert_eq!(back, ir);
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
        let json = serde_json::to_value(&sr).unwrap();
        let back: IrStopReason = serde_json::from_value(json).unwrap();
        assert_eq!(back, sr);
    }
}

#[test]
fn ir_response_builder_chain() {
    let resp = IrResponse::text("hello")
        .with_id("r1")
        .with_model("gpt-4")
        .with_stop_reason(IrStopReason::EndTurn)
        .with_usage(IrUsage::from_io(10, 20));
    assert_eq!(resp.id.as_deref(), Some("r1"));
    assert_eq!(resp.model.as_deref(), Some("gpt-4"));
    assert_eq!(resp.text_content(), "hello");
    assert!(!resp.has_tool_calls());
}

#[test]
fn ir_response_has_tool_calls() {
    let resp = IrResponse::new(vec![IrContentBlock::ToolCall {
        id: "t1".into(),
        name: "a".into(),
        input: json!(null),
    }]);
    assert!(resp.has_tool_calls());
    assert_eq!(resp.tool_calls().len(), 1);
}

#[test]
fn ir_response_serde_roundtrip() {
    let resp = IrResponse::text("hello").with_id("r1");
    let json = serde_json::to_value(&resp).unwrap();
    let back: IrResponse = serde_json::from_value(json).unwrap();
    assert_eq!(back, resp);
}

#[test]
fn ir_tool_definition_serde_roundtrip() {
    let td = IrToolDefinition {
        name: "calc".into(),
        description: "a calculator".into(),
        parameters: json!({"type": "object", "properties": {"x": {"type": "number"}}}),
    };
    let json = serde_json::to_value(&td).unwrap();
    let back: IrToolDefinition = serde_json::from_value(json).unwrap();
    assert_eq!(back, td);
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Cross-dialect compatibility / pair checks
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn all_dialect_pairs_supported_in_builtins() {
    let r = DialectRegistry::with_builtins();
    for &a in Dialect::all() {
        for &b in Dialect::all() {
            assert!(
                r.supports_pair(a, b),
                "pair ({a:?}, {b:?}) should be supported"
            );
        }
    }
}

#[test]
fn cross_dialect_openai_to_claude() {
    let r = DialectRegistry::with_builtins();
    let openai = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let ir = r.parse(Dialect::OpenAi, &openai).unwrap();
    let claude_json = r.serialize(Dialect::Claude, &ir).unwrap();
    assert!(claude_json["messages"].is_array());
}

#[test]
fn cross_dialect_claude_to_gemini() {
    let r = DialectRegistry::with_builtins();
    let claude = json!({"model": "claude-3", "system": "be nice", "messages": [{"role": "user", "content": "hi"}], "max_tokens": 100});
    let ir = r.parse(Dialect::Claude, &claude).unwrap();
    let gemini_json = r.serialize(Dialect::Gemini, &ir).unwrap();
    assert!(gemini_json["contents"].is_array());
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn edge_empty_messages_array() {
    let r = DialectRegistry::with_builtins();
    let v = json!({"model": "gpt-4", "messages": []});
    let ir = r.parse(Dialect::OpenAi, &v).unwrap();
    assert!(ir.messages.is_empty());
}

#[test]
fn edge_unknown_model_no_crash() {
    let v = json!({"model": "completely-unknown-model-xyz", "messages": [{"role": "user", "content": "hi"}]});
    // Detection may return something (OpenAI fallback) or None
    let _ = detector().detect(&v);
}

#[test]
fn edge_ambiguous_model_returns_something() {
    let v = json!({"model": "hybrid-model", "messages": [{"role": "user", "content": "hi"}]});
    // Should not panic
    let _ = detector().detect(&v);
}

#[test]
fn edge_very_long_model_name() {
    let long_name = "a".repeat(500);
    let v = json!({"model": long_name, "messages": [{"role": "user", "content": "hi"}]});
    // Validation should warn about long model name
    let r = rv().validate(Dialect::OpenAi, &v);
    assert!(r.issues.iter().any(|i| i.code == "long_model_name"));
}

#[test]
fn edge_model_not_string_error() {
    let r = rv().validate(
        Dialect::OpenAi,
        &json!({"model": 42, "messages": [{"role": "user", "content": "hi"}]}),
    );
    assert!(!r.is_valid());
}

#[test]
fn edge_fingerprint_serde_roundtrip() {
    let fp = DialectFingerprint {
        model_prefix_patterns: vec!["gpt-".into()],
        field_markers: vec!["messages".into()],
        header_markers: vec![("authorization".into(), "Bearer ".into())],
    };
    let json = serde_json::to_value(&fp).unwrap();
    let back: DialectFingerprint = serde_json::from_value(json).unwrap();
    assert_eq!(back.model_prefix_patterns, fp.model_prefix_patterns);
    assert_eq!(back.field_markers, fp.field_markers);
    assert_eq!(back.header_markers, fp.header_markers);
}

#[test]
fn edge_detection_result_serde_roundtrip() {
    let dr = DialectDetectionResult {
        dialect: Dialect::Claude,
        confidence: 0.85,
        evidence: vec!["model match".into()],
    };
    let json = serde_json::to_value(&dr).unwrap();
    let back: DialectDetectionResult = serde_json::from_value(json).unwrap();
    assert_eq!(back.dialect, Dialect::Claude);
}

#[test]
fn edge_dialect_error_equality() {
    let a = DialectError {
        dialect: Dialect::OpenAi,
        message: "err".into(),
    };
    let b = DialectError {
        dialect: Dialect::OpenAi,
        message: "err".into(),
    };
    assert_eq!(a, b);
}

#[test]
fn edge_validation_result_rv_serde_roundtrip() {
    let vr = VResult {
        issues: vec![ValidationIssue {
            severity: Severity::Error,
            field: "model".into(),
            message: "missing".into(),
            code: "missing_required_field".into(),
        }],
    };
    let json = serde_json::to_value(&vr).unwrap();
    let back: VResult = serde_json::from_value(json).unwrap();
    assert_eq!(back.issues.len(), 1);
    assert_eq!(back.issues[0].code, "missing_required_field");
}
