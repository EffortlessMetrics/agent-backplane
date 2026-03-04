// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep dialect parser tests — model names, headers, endpoints, edge cases.
//!
//! Covers model-name prefix detection for all six dialects, header-based
//! and endpoint-based detection, ambiguous/mixed signals, confidence
//! thresholds, and various edge cases.

use std::collections::BTreeMap;

use abp_dialect::{Dialect, DialectDetector};
use serde_json::{Value, json};

fn detector() -> DialectDetector {
    DialectDetector::new()
}

fn detect(v: &Value) -> (Dialect, f64) {
    let r = detector().detect(v).expect("expected detection result");
    (r.dialect, r.confidence)
}

fn headers(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════
// 1. OpenAI model name detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_model_gpt4() {
    let v = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let (d, c) = detect(&v);
    assert_eq!(d, Dialect::OpenAi);
    assert!(c > 0.5);
}

#[test]
fn openai_model_gpt4o() {
    let v = json!({"model": "gpt-4o", "messages": [{"role": "user", "content": "hi"}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn openai_model_gpt4_turbo() {
    let v =
        json!({"model": "gpt-4-turbo-2024-04-09", "messages": [{"role": "user", "content": "x"}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn openai_model_gpt35_turbo() {
    let v = json!({"model": "gpt-3.5-turbo", "messages": [{"role": "user", "content": "x"}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn openai_model_chatgpt_4o() {
    let v = json!({"model": "chatgpt-4o-latest", "messages": [{"role": "user", "content": "x"}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn openai_model_case_insensitive() {
    let v = json!({"model": "GPT-4-Turbo", "messages": [{"role": "user", "content": "x"}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::OpenAi);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Claude model name detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn claude_model_claude3_opus() {
    let v = json!({"model": "claude-3-opus-20240229", "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn claude_model_claude_sonnet() {
    let v = json!({"model": "claude-sonnet-4-20250514", "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn claude_model_claude_haiku() {
    let v = json!({"model": "claude-3-haiku-20240307", "type": "message"});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn claude_model_case_insensitive() {
    let v = json!({"model": "Claude-3-Opus", "type": "message"});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Claude);
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Gemini model name detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn gemini_model_15_pro() {
    let v = json!({"model": "gemini-1.5-pro", "contents": [{"parts": [{"text": "hi"}]}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn gemini_model_15_flash() {
    let v = json!({"model": "gemini-1.5-flash", "contents": [{"parts": [{"text": "hi"}]}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn gemini_model_models_prefix() {
    let v = json!({"model": "models/gemini-1.5-pro", "contents": [{"parts": [{"text": "hi"}]}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn gemini_model_2_flash() {
    let v = json!({"model": "gemini-2.0-flash", "contents": [{"parts": [{"text": "hi"}]}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn gemini_model_case_insensitive() {
    let v = json!({"model": "Gemini-1.5-Pro", "contents": [{"parts": [{"text": "hi"}]}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Gemini);
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Codex model name detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn codex_model_codex_mini() {
    let v = json!({"model": "codex-mini", "items": [{"type": "message"}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Codex);
}

#[test]
fn codex_model_o1_preview_with_items() {
    let v = json!({"model": "o1-preview", "items": [{"type": "message"}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Codex);
}

#[test]
fn codex_model_o3_mini_with_items() {
    let v = json!({"model": "o3-mini", "items": [{"type": "message"}], "status": "completed"});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Codex);
}

#[test]
fn codex_model_o4_mini_with_items() {
    let v = json!({"model": "o4-mini", "items": [{"type": "function_call"}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Codex);
}

#[test]
fn codex_model_contains_codex() {
    let v = json!({"model": "my-codex-v2", "items": [{"type": "message"}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Codex);
}

#[test]
fn codex_model_case_insensitive() {
    let v = json!({"model": "CODEX-MINI", "items": [{"type": "msg"}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Codex);
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Kimi model name detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn kimi_model_moonshot_v1() {
    let v = json!({"model": "moonshot-v1", "refs": ["https://example.com"]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Kimi);
}

#[test]
fn kimi_model_moonshot_v1_32k() {
    let v = json!({"model": "moonshot-v1-32k", "refs": ["doc"]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Kimi);
}

#[test]
fn kimi_model_kimi_bare() {
    let v =
        json!({"model": "kimi", "refs": ["doc"], "messages": [{"role": "user", "content": "hi"}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Kimi);
}

#[test]
fn kimi_model_kimi_v2() {
    let v = json!({"model": "kimi-v2", "search_plus": true});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Kimi);
}

#[test]
fn kimi_model_case_insensitive() {
    let v = json!({"model": "Moonshot-V1-128k", "refs": ["doc"]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Kimi);
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Copilot model name detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn copilot_model_copilot_gpt4() {
    let v = json!({"model": "copilot-gpt-4", "references": [{"type": "file"}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Copilot);
}

#[test]
fn copilot_model_contains_copilot() {
    let v = json!({"model": "github-copilot-chat", "references": [{"type": "file"}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Copilot);
}

#[test]
fn copilot_model_copilot_chat() {
    let v = json!({"model": "copilot-chat", "agent_mode": true});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Copilot);
}

#[test]
fn copilot_model_case_insensitive() {
    let v = json!({"model": "Copilot-GPT-4", "references": [{"type": "file"}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Copilot);
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Model name edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn model_empty_string() {
    let v = json!({"model": "", "messages": [{"role": "user", "content": "hi"}]});
    let r = detector().detect(&v);
    // Should still detect via structural signals (messages).
    assert!(r.is_some());
}

#[test]
fn model_non_string_ignored() {
    let v = json!({"model": 42, "messages": [{"role": "user", "content": "hi"}]});
    let r = detector().detect(&v);
    // Model field is non-string, ignored; messages still contribute.
    assert!(r.is_some());
}

#[test]
fn model_unknown_prefers_structural() {
    let v = json!({"model": "unknown-model-xyz", "messages": [{"role": "user", "content": "hi"}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn model_partial_gpt_no_dash() {
    // "gpt" without a dash should NOT trigger the OpenAI model prefix.
    let v = json!({"model": "gpt", "messages": [{"role": "user", "content": "hi"}]});
    let r = detector().detect(&v).unwrap();
    // Still detects via structural signals, but the model prefix evidence should be absent.
    assert!(
        !r.evidence.iter().any(|e| e.contains("OpenAI prefix")),
        "should not match OpenAI model prefix without dash"
    );
}

#[test]
fn model_o1_with_messages_prefers_openai() {
    // o1 model with OpenAI-style messages should detect as OpenAI due to structural signals.
    let v = json!({"model": "o1-preview", "messages": [{"role": "user", "content": "hi"}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn model_o3_with_items_prefers_codex() {
    let v = json!({"model": "o3-mini", "items": [{"type": "message"}]});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Codex);
}

#[test]
fn model_o4_without_signals_gives_codex_prefix() {
    // Bare o4 model without structural context: Codex should win on prefix.
    let v = json!({"model": "o4-mini"});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Codex);
    assert!(r.evidence.iter().any(|e| e.contains("Codex prefix")));
}

#[test]
fn model_gemini_models_slash_prefix() {
    // Test the "models/" prefix format used by Gemini REST API.
    let v = json!({"model": "models/gemini-2.0-flash-exp"});
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Header-based detection — Claude
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn header_anthropic_version_detects_claude() {
    let h = headers(&[("anthropic-version", "2023-06-01")]);
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
    assert!(r.confidence > 0.0);
}

#[test]
fn header_x_api_key_detects_claude() {
    let h = headers(&[("x-api-key", "sk-ant-api03-xxx")]);
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn header_both_claude_headers_higher_confidence() {
    let single = headers(&[("anthropic-version", "2023-06-01")]);
    let both = headers(&[
        ("anthropic-version", "2023-06-01"),
        ("x-api-key", "sk-ant-xxx"),
    ]);
    let c1 = detector().detect_from_headers(&single).unwrap().confidence;
    let c2 = detector().detect_from_headers(&both).unwrap().confidence;
    assert!(c2 > c1, "two headers ({c2}) should exceed one ({c1})");
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Header-based detection — Copilot
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn header_copilot_integration_id() {
    let h = headers(&[("copilot-integration-id", "my-ext")]);
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn header_github_token_detects_copilot() {
    let h = headers(&[("x-github-token", "ghu_xxxx")]);
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn header_both_copilot_headers() {
    let h = headers(&[
        ("copilot-integration-id", "my-ext"),
        ("x-github-token", "ghu_xxxx"),
    ]);
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
    assert!(r.confidence > 0.4);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Header-based detection — Gemini
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn header_x_goog_api_key_detects_gemini() {
    let h = headers(&[("x-goog-api-key", "AIza-xxx")]);
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Header-based detection — OpenAI
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn header_openai_organization_detects_openai() {
    let h = headers(&[("openai-organization", "org-abc123")]);
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn header_openai_project_detects_openai() {
    let h = headers(&[("openai-project", "proj-xyz")]);
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Header edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn header_empty_map_returns_none() {
    assert!(detector().detect_from_headers(&BTreeMap::new()).is_none());
}

#[test]
fn header_unknown_headers_returns_none() {
    let h = headers(&[
        ("x-custom-header", "value"),
        ("content-type", "application/json"),
    ]);
    assert!(detector().detect_from_headers(&h).is_none());
}

#[test]
fn header_with_empty_values() {
    let h = headers(&[("anthropic-version", "")]);
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn header_multiple_dialect_headers_highest_wins() {
    // Claude has two possible headers, Gemini has one — if both present,
    // Claude should win with higher cumulative score.
    let h = headers(&[
        ("anthropic-version", "2023-06-01"),
        ("x-api-key", "sk-ant-xxx"),
        ("x-goog-api-key", "AIza-xxx"),
    ]);
    let r = detector().detect_from_headers(&h).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn header_evidence_populated() {
    let h = headers(&[("anthropic-version", "2023-06-01")]);
    let r = detector().detect_from_headers(&h).unwrap();
    assert!(!r.evidence.is_empty());
    assert!(r.evidence.iter().any(|e| e.contains("anthropic-version")));
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Endpoint-based detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn endpoint_openai_api() {
    let r = detector()
        .detect_from_endpoint("https://api.openai.com/v1/chat/completions")
        .unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
    assert!((r.confidence - 0.8).abs() < f64::EPSILON);
}

#[test]
fn endpoint_openai_responses_api_codex() {
    let r = detector()
        .detect_from_endpoint("https://api.openai.com/v1/responses")
        .unwrap();
    assert_eq!(r.dialect, Dialect::Codex);
    assert!((r.confidence - 0.8).abs() < f64::EPSILON);
}

#[test]
fn endpoint_anthropic_api() {
    let r = detector()
        .detect_from_endpoint("https://api.anthropic.com/v1/messages")
        .unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn endpoint_anthropic_complete() {
    let r = detector()
        .detect_from_endpoint("https://api.anthropic.com/v1/complete")
        .unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn endpoint_gemini_api() {
    let r = detector()
        .detect_from_endpoint(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-1.5-pro:generateContent",
        )
        .unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn endpoint_gemini_stream() {
    let r = detector()
        .detect_from_endpoint(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-1.5-pro:streamGenerateContent",
        )
        .unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn endpoint_moonshot_api() {
    let r = detector()
        .detect_from_endpoint("https://api.moonshot.cn/v1/chat/completions")
        .unwrap();
    assert_eq!(r.dialect, Dialect::Kimi);
}

#[test]
fn endpoint_moonshot_files() {
    let r = detector()
        .detect_from_endpoint("https://api.moonshot.cn/v1/files")
        .unwrap();
    assert_eq!(r.dialect, Dialect::Kimi);
}

#[test]
fn endpoint_github_copilot_api() {
    let r = detector()
        .detect_from_endpoint("https://api.github.com/copilot/chat/completions")
        .unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn endpoint_unknown_returns_none() {
    assert!(
        detector()
            .detect_from_endpoint("https://custom-llm.example.com/v1/generate")
            .is_none()
    );
}

#[test]
fn endpoint_empty_returns_none() {
    assert!(detector().detect_from_endpoint("").is_none());
}

#[test]
fn endpoint_case_insensitive() {
    let r = detector()
        .detect_from_endpoint("https://API.OPENAI.COM/V1/chat/completions")
        .unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn endpoint_with_query_params() {
    let r = detector()
        .detect_from_endpoint(
            "https://generativelanguage.googleapis.com/v1/models/gemini-pro:generateContent?key=abc",
        )
        .unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn endpoint_evidence_populated() {
    let r = detector()
        .detect_from_endpoint("https://api.anthropic.com/v1/messages")
        .unwrap();
    assert!(!r.evidence.is_empty());
    assert!(r.evidence.iter().any(|e| e.contains("Anthropic")));
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Ambiguous / mixed signals
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ambiguous_messages_only_low_confidence() {
    let v = json!({"messages": [{"role": "user", "content": "hi"}]});
    let r = detector().detect(&v).unwrap();
    assert!(
        r.confidence < 0.6,
        "bare messages confidence {} too high",
        r.confidence
    );
}

#[test]
fn ambiguous_model_unknown_with_messages() {
    let v = json!({"model": "unknown-model", "messages": [{"role": "user", "content": "hi"}]});
    let (d, _) = detect(&v);
    // OpenAI should win because messages + generic model.
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn ambiguous_openai_and_gemini_signals() {
    let v = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "contents": [{"parts": [{"text": "hi"}]}]
    });
    let results = detector().detect_all(&v);
    assert!(results.len() >= 2);
    let dialects: Vec<_> = results.iter().map(|r| r.dialect).collect();
    assert!(dialects.contains(&Dialect::OpenAi));
    assert!(dialects.contains(&Dialect::Gemini));
}

#[test]
fn ambiguous_claude_and_openai_signals() {
    // Has "type": "message" (Claude) and "choices" (OpenAI).
    let v = json!({
        "type": "message",
        "choices": [{}],
        "model": "some-model"
    });
    let results = detector().detect_all(&v);
    assert!(results.len() >= 2);
    let dialects: Vec<_> = results.iter().map(|r| r.dialect).collect();
    assert!(dialects.contains(&Dialect::Claude));
    assert!(dialects.contains(&Dialect::OpenAi));
}

#[test]
fn ambiguous_kimi_and_openai_signals() {
    let v = json!({
        "model": "some-model",
        "messages": [{"role": "user", "content": "hi"}],
        "refs": ["http://example.com"]
    });
    let results = detector().detect_all(&v);
    let dialects: Vec<_> = results.iter().map(|r| r.dialect).collect();
    assert!(dialects.contains(&Dialect::Kimi));
    assert!(dialects.contains(&Dialect::OpenAi));
}

#[test]
fn ambiguous_codex_model_openai_structure() {
    // o1 model with OpenAI-style structure → OpenAI wins over Codex prefix alone.
    let v = json!({
        "model": "o1-preview",
        "messages": [{"role": "user", "content": "think step by step"}],
        "temperature": 0.0
    });
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn ambiguous_detect_all_sorted_descending() {
    let v = json!({
        "model": "some-model",
        "messages": [{"role": "user", "content": "hi"}],
        "refs": ["http://example.com"],
        "temperature": 0.5
    });
    let results = detector().detect_all(&v);
    for w in results.windows(2) {
        assert!(
            w[0].confidence >= w[1].confidence,
            "results should be sorted descending"
        );
    }
}

#[test]
fn ambiguous_highest_confidence_wins() {
    // Claude model prefix is a strong signal that should dominate.
    let v = json!({
        "model": "claude-3-opus",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
        "temperature": 0.5
    });
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn ambiguous_structural_beats_weak_model() {
    // Codex structural signals (items + status + object=response) should dominate
    // even without a known model prefix.
    let v = json!({
        "model": "unknown-model",
        "object": "response",
        "status": "completed",
        "items": [{"type": "message"}]
    });
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::Codex);
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Confidence thresholds
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn confidence_model_prefix_alone_above_03() {
    let v = json!({"model": "gpt-4"});
    let r = detector().detect(&v).unwrap();
    assert!(r.confidence > 0.3, "prefix alone: {}", r.confidence);
}

#[test]
fn confidence_model_plus_fields_above_06() {
    let v = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "choices": [{}]
    });
    let r = detector().detect(&v).unwrap();
    assert!(r.confidence > 0.6, "model+fields: {}", r.confidence);
}

#[test]
fn confidence_full_request_above_08() {
    let v = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "choices": [{}],
        "temperature": 0.7
    });
    let r = detector().detect(&v).unwrap();
    assert!(r.confidence > 0.8, "full request: {}", r.confidence);
}

#[test]
fn confidence_single_field_below_06() {
    let v = json!({"choices": [{}]});
    let r = detector().detect(&v).unwrap();
    assert!(
        r.confidence < 0.6,
        "single field confidence {} too high",
        r.confidence
    );
}

#[test]
fn confidence_never_exceeds_one_openai() {
    let v = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "choices": [{}],
        "temperature": 0.7,
        "top_p": 0.9,
        "max_tokens": 100
    });
    let r = detector().detect(&v).unwrap();
    assert!(r.confidence <= 1.0);
}

#[test]
fn confidence_never_exceeds_one_claude() {
    let v = json!({
        "model": "claude-3-opus",
        "type": "message",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
        "stop_reason": "end_turn",
        "content": [{"type": "text", "text": "hi"}]
    });
    let r = detector().detect(&v).unwrap();
    assert!(r.confidence <= 1.0);
}

#[test]
fn confidence_never_exceeds_one_gemini() {
    let v = json!({
        "model": "gemini-1.5-pro",
        "contents": [{"parts": [{"text": "hi"}]}],
        "candidates": [{}],
        "generationConfig": {"temperature": 0.5}
    });
    let r = detector().detect(&v).unwrap();
    assert!(r.confidence <= 1.0);
}

#[test]
fn confidence_positive_when_detected() {
    let v = json!({"references": [{"type": "file"}]});
    let r = detector().detect(&v).unwrap();
    assert!(r.confidence > 0.0);
}

#[test]
fn confidence_header_above_03() {
    let h = headers(&[("anthropic-version", "2023-06-01")]);
    let r = detector().detect_from_headers(&h).unwrap();
    assert!(r.confidence > 0.3, "header confidence: {}", r.confidence);
}

#[test]
fn confidence_endpoint_is_08() {
    let r = detector()
        .detect_from_endpoint("https://api.openai.com/v1/chat/completions")
        .unwrap();
    assert!(
        (r.confidence - 0.8).abs() < f64::EPSILON,
        "endpoint confidence: {}",
        r.confidence
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn edge_null_returns_none() {
    assert!(detector().detect(&json!(null)).is_none());
}

#[test]
fn edge_array_returns_none() {
    assert!(detector().detect(&json!([1, 2, 3])).is_none());
}

#[test]
fn edge_number_returns_none() {
    assert!(detector().detect(&json!(42)).is_none());
}

#[test]
fn edge_string_returns_none() {
    assert!(detector().detect(&json!("hello")).is_none());
}

#[test]
fn edge_bool_returns_none() {
    assert!(detector().detect(&json!(true)).is_none());
}

#[test]
fn edge_empty_object_returns_none() {
    assert!(detector().detect(&json!({})).is_none());
}

#[test]
fn edge_deeply_nested_no_signals() {
    let v = json!({
        "data": {
            "inner": {
                "messages": [{"role": "user", "content": "hi"}]
            }
        }
    });
    assert!(detector().detect(&v).is_none());
}

#[test]
fn edge_large_messages_array() {
    let msgs: Vec<Value> = (0..100)
        .map(|i| json!({"role": "user", "content": format!("message {i}")}))
        .collect();
    let v = json!({"model": "gpt-4", "messages": msgs});
    let (d, _) = detect(&v);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn edge_all_evidence_non_empty() {
    let examples: Vec<Value> = vec![
        json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]}),
        json!({"type": "message", "content": [{"type": "text", "text": "hi"}]}),
        json!({"contents": [{"parts": [{"text": "hi"}]}]}),
        json!({"items": [{"type": "message"}], "status": "completed"}),
        json!({"references": [{"type": "file"}]}),
        json!({"refs": ["doc"], "search_plus": true}),
    ];
    for example in &examples {
        let result = detector().detect(example).expect("should detect");
        assert!(
            !result.evidence.is_empty(),
            "evidence should be populated for {:?}",
            result.dialect
        );
    }
}

#[test]
fn edge_detect_all_empty_for_non_object() {
    assert!(detector().detect_all(&json!(null)).is_empty());
    assert!(detector().detect_all(&json!(42)).is_empty());
    assert!(detector().detect_all(&json!("hi")).is_empty());
}

#[test]
fn edge_detect_all_empty_for_empty_object() {
    assert!(detector().detect_all(&json!({})).is_empty());
}
