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
//! Comprehensive tests for dialect detection and parsing functionality.

use abp_dialect::{Dialect, DialectDetector, DialectValidator, ValidationError};
use serde_json::{Value, json};
use std::collections::HashSet;

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
// 1. Dialect enum variants and display
// ═══════════════════════════════════════════════════════════════════════

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
    for &d in Dialect::all() {
        assert_eq!(format!("{d}"), d.label());
    }
}

#[test]
fn dialect_all_returns_six_variants() {
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
fn dialect_all_no_duplicates() {
    let all = Dialect::all();
    let set: HashSet<Dialect> = all.iter().copied().collect();
    assert_eq!(set.len(), all.len());
}

#[test]
fn dialect_debug_is_not_empty() {
    for &d in Dialect::all() {
        let dbg = format!("{d:?}");
        assert!(!dbg.is_empty(), "Debug for {d} was empty");
    }
}

#[test]
fn dialect_clone_equals_original() {
    for &d in Dialect::all() {
        #[allow(clippy::clone_on_copy)]
        let cloned = d.clone();
        assert_eq!(d, cloned);
    }
}

#[test]
fn dialect_copy_semantics() {
    let a = Dialect::OpenAi;
    let b = a; // copy
    assert_eq!(a, b);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Dialect serde roundtrip
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
        let json_str = serde_json::to_string(&d).unwrap();
        let back: Dialect = serde_json::from_str(&json_str).unwrap();
        assert_eq!(d, back, "roundtrip failed for {d:?}");
    }
}

#[test]
fn serde_invalid_variant_fails() {
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

// ═══════════════════════════════════════════════════════════════════════
// 3. Dialect comparison and ordering
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dialect_eq_same_variant() {
    assert_eq!(Dialect::OpenAi, Dialect::OpenAi);
    assert_eq!(Dialect::Claude, Dialect::Claude);
    assert_eq!(Dialect::Gemini, Dialect::Gemini);
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
fn dialect_hash_consistent() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    for &d in Dialect::all() {
        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();
        d.hash(&mut h1);
        d.hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish(), "Hash not consistent for {d:?}");
    }
}

#[test]
fn dialect_hash_differs_between_variants() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let hashes: Vec<u64> = Dialect::all()
        .iter()
        .map(|d| {
            let mut h = DefaultHasher::new();
            d.hash(&mut h);
            h.finish()
        })
        .collect();
    let unique: HashSet<u64> = hashes.iter().copied().collect();
    // Not guaranteed but highly likely for 6 variants
    assert!(unique.len() >= 4, "too many hash collisions");
}

#[test]
fn dialect_can_be_used_as_hashset_key() {
    let mut set = HashSet::new();
    for &d in Dialect::all() {
        set.insert(d);
    }
    assert_eq!(set.len(), 6);
}

// ═══════════════════════════════════════════════════════════════════════
// 4. OpenAI detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_openai_minimal_request() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hello"}]
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
    assert!(c > 0.4);
}

#[test]
fn detect_openai_with_temperature() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "temperature": 0.7
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn detect_openai_with_top_p() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
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
        "max_tokens": 256
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn detect_openai_response_with_choices() {
    let msg = json!({
        "id": "chatcmpl-abc",
        "object": "chat.completion",
        "model": "gpt-4",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}]
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
    assert!(c >= 0.5);
}

#[test]
fn detect_openai_streaming_chunk() {
    let msg = json!({
        "id": "chatcmpl-abc",
        "object": "chat.completion.chunk",
        "model": "gpt-4",
        "choices": [{"index": 0, "delta": {"content": "Hello"}, "finish_reason": null}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn detect_openai_function_call_request() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "weather?"}],
        "functions": [{"name": "get_weather", "parameters": {"type": "object"}}],
        "function_call": "auto"
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn detect_openai_tool_choice() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "function", "function": {"name": "f"}}],
        "tool_choice": "auto"
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn detect_openai_multiple_messages() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "You are helpful."},
            {"role": "user", "content": "Question 1"},
            {"role": "assistant", "content": "Answer 1"},
            {"role": "user", "content": "Question 2"}
        ]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn detect_openai_gpt35_model() {
    let msg = json!({
        "model": "gpt-3.5-turbo",
        "messages": [{"role": "user", "content": "hello"}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn detect_openai_choices_only() {
    let msg = json!({"choices": [{"message": {"role": "assistant", "content": "hi"}}]});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn detect_openai_empty_choices() {
    let msg = json!({"choices": []});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn detect_openai_with_usage() {
    let msg = json!({
        "model": "gpt-4",
        "choices": [{"message": {"role": "assistant", "content": "hi"}}],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Claude detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_claude_request_with_content_blocks() {
    let msg = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hello"}]}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn detect_claude_response_with_type_message() {
    let msg = json!({
        "id": "msg_001",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-20250514",
        "content": [{"type": "text", "text": "hi"}],
        "stop_reason": "end_turn"
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
    assert!(c > 0.5);
}

#[test]
fn detect_claude_bare_type_message() {
    let msg = json!({"type": "message"});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn detect_claude_with_stop_reason() {
    let msg = json!({
        "type": "message",
        "content": [{"type": "text", "text": "done"}],
        "stop_reason": "end_turn"
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn detect_claude_thinking_block() {
    let msg = json!({
        "type": "message",
        "model": "claude-sonnet-4-20250514",
        "content": [
            {"type": "thinking", "thinking": "Let me think..."},
            {"type": "text", "text": "Answer."}
        ],
        "stop_reason": "end_turn"
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn detect_claude_tool_use() {
    let msg = json!({
        "type": "message",
        "model": "claude-sonnet-4-20250514",
        "content": [{"type": "tool_use", "id": "toolu_01", "name": "f", "input": {}}],
        "stop_reason": "tool_use"
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn detect_claude_tool_result_conversation() {
    let msg = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": "weather?"}]},
            {"role": "assistant", "content": [{"type": "tool_use", "id": "t1", "name": "w", "input": {}}]},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "sunny"}]}
        ]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn detect_claude_system_as_separate_field() {
    let msg = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 2048,
        "system": "Be brief.",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hello"}]}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn detect_claude_array_content_is_key_signal() {
    // Messages with array content (not string) is the Claude signal
    let msg = json!({
        "model": "some-model",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn detect_claude_with_image_content() {
    let msg = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": [
            {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "abc"}}
        ]}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Gemini detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_gemini_contents_with_parts() {
    let msg = json!({
        "contents": [{"role": "user", "parts": [{"text": "hello"}]}]
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
    assert!(c >= 0.5);
}

#[test]
fn detect_gemini_response_with_candidates() {
    let msg = json!({
        "candidates": [{"content": {"parts": [{"text": "hi"}]}, "finishReason": "STOP"}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn detect_gemini_with_generation_config_camel() {
    let msg = json!({
        "contents": [{"parts": [{"text": "hi"}]}],
        "generationConfig": {"temperature": 0.7}
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn detect_gemini_with_generation_config_snake() {
    let msg = json!({
        "contents": [{"parts": [{"text": "hi"}]}],
        "generation_config": {"temperature": 0.7}
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn detect_gemini_function_declarations() {
    let msg = json!({
        "contents": [{"role": "user", "parts": [{"text": "weather?"}]}],
        "tools": [{"functionDeclarations": [{"name": "get_weather"}]}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn detect_gemini_function_response() {
    let msg = json!({
        "contents": [
            {"role": "user", "parts": [{"text": "weather?"}]},
            {"role": "model", "parts": [{"functionCall": {"name": "w", "args": {}}}]},
            {"role": "function", "parts": [{"functionResponse": {"name": "w", "response": {}}}]}
        ]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn detect_gemini_safety_settings() {
    let msg = json!({
        "contents": [{"parts": [{"text": "story"}]}],
        "safetySettings": [{"category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_MEDIUM_AND_ABOVE"}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn detect_gemini_system_instruction() {
    let msg = json!({
        "systemInstruction": {"parts": [{"text": "Be a tutor."}]},
        "contents": [{"parts": [{"text": "hi"}]}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn detect_gemini_candidates_only() {
    let msg = json!({"candidates": [{"content": {"parts": [{"text": "ok"}]}}]});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn detect_gemini_with_usage_metadata() {
    let msg = json!({
        "candidates": [{"content": {"parts": [{"text": "hi"}]}}],
        "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 3}
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Codex detection
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
fn detect_codex_with_status() {
    let msg = json!({
        "items": [{"type": "message"}],
        "status": "completed"
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::Codex);
    assert!(c > 0.5);
}

#[test]
fn detect_codex_object_response() {
    let msg = json!({
        "object": "response",
        "status": "completed",
        "items": [{"type": "message", "content": "hello"}]
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::Codex);
    assert!(c > 0.5);
}

#[test]
fn detect_codex_function_call_item() {
    let msg = json!({
        "items": [{"type": "function_call", "name": "run_tests"}],
        "status": "in_progress"
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Codex);
}

#[test]
fn detect_codex_object_response_alone_with_status() {
    let msg = json!({
        "object": "response",
        "status": "completed"
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Codex);
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Kimi detection
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
fn detect_kimi_search_plus() {
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
    let msg = json!({"refs": ["https://a.com", "https://b.com"]});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Kimi);
}

#[test]
fn detect_kimi_search_plus_alone() {
    let msg = json!({"search_plus": true});
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::Kimi);
    assert!(c > 0.5);
}

#[test]
fn detect_kimi_both_refs_and_search_plus() {
    let msg = json!({
        "model": "moonshot",
        "messages": [{"role": "user", "content": "find info"}],
        "refs": ["https://example.com"],
        "search_plus": true
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::Kimi);
    assert!(c > 0.8);
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Copilot detection
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
        "agent_mode": true
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Copilot);
}

#[test]
fn detect_copilot_confirmations() {
    let msg = json!({
        "messages": [{"role": "user", "content": "yes"}],
        "confirmations": [{"id": "c1", "accepted": true}],
        "agent_mode": true
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Copilot);
}

#[test]
fn detect_copilot_agent_mode_and_confirmations() {
    let msg = json!({
        "messages": [{"role": "user", "content": "go"}],
        "agent_mode": true,
        "confirmations": []
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::Copilot);
    assert!(c > 0.5);
}

#[test]
fn detect_copilot_references_only() {
    let msg = json!({"references": [{"type": "file"}]});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Copilot);
}

#[test]
fn detect_copilot_all_three_signals() {
    let msg = json!({
        "messages": [{"role": "user", "content": "do"}],
        "references": [{"type": "file"}],
        "agent_mode": true,
        "confirmations": [{"id": "1"}]
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::Copilot);
    assert!(c > 0.8);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Edge cases: empty, scalar, non-object
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn edge_empty_object_returns_none() {
    assert!(detector().detect(&json!({})).is_none());
}

#[test]
fn edge_null_returns_none() {
    assert!(detector().detect(&json!(null)).is_none());
}

#[test]
fn edge_number_returns_none() {
    assert!(detector().detect(&json!(42)).is_none());
}

#[test]
fn edge_float_returns_none() {
    assert!(detector().detect(&json!(9.81)).is_none());
}

#[test]
fn edge_bool_returns_none() {
    assert!(detector().detect(&json!(true)).is_none());
    assert!(detector().detect(&json!(false)).is_none());
}

#[test]
fn edge_string_returns_none() {
    assert!(detector().detect(&json!("hello")).is_none());
}

#[test]
fn edge_array_returns_none() {
    assert!(detector().detect(&json!([1, 2, 3])).is_none());
}

#[test]
fn edge_array_of_messages_returns_none() {
    let msg = json!([
        {"role": "user", "content": "hi"},
        {"role": "assistant", "content": "hello"}
    ]);
    assert!(detector().detect(&msg).is_none());
}

#[test]
fn edge_unknown_keys_returns_none() {
    let msg = json!({"foo": "bar", "baz": 42, "qux": [1, 2]});
    assert!(detector().detect(&msg).is_none());
}

#[test]
fn edge_deeply_nested_signals_ignored() {
    let msg = json!({
        "wrapper": {
            "messages": [{"role": "user", "content": "hi"}]
        }
    });
    assert!(detector().detect(&msg).is_none());
}

#[test]
fn edge_deeply_nested_contents_ignored() {
    let msg = json!({
        "data": {
            "contents": [{"parts": [{"text": "hi"}]}]
        }
    });
    assert!(detector().detect(&msg).is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Edge cases: ambiguous & mixed signals
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn edge_openai_and_gemini_mixed() {
    let msg = json!({
        "model": "mixed",
        "messages": [{"role": "user", "content": "hi"}],
        "contents": [{"parts": [{"text": "hi"}]}]
    });
    let results = detector().detect_all(&msg);
    let dialects: Vec<_> = results.iter().map(|r| r.dialect).collect();
    assert!(dialects.contains(&Dialect::OpenAi));
    assert!(dialects.contains(&Dialect::Gemini));
}

#[test]
fn edge_openai_and_kimi_mixed() {
    let msg = json!({
        "model": "x",
        "messages": [{"role": "user", "content": "hi"}],
        "refs": ["https://example.com"]
    });
    let results = detector().detect_all(&msg);
    let dialects: Vec<_> = results.iter().map(|r| r.dialect).collect();
    assert!(dialects.contains(&Dialect::OpenAi));
    assert!(dialects.contains(&Dialect::Kimi));
}

#[test]
fn edge_claude_and_copilot_mixed() {
    let msg = json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
        "references": [{"type": "file"}]
    });
    let results = detector().detect_all(&msg);
    let dialects: Vec<_> = results.iter().map(|r| r.dialect).collect();
    assert!(dialects.contains(&Dialect::Claude));
    assert!(dialects.contains(&Dialect::Copilot));
}

#[test]
fn edge_status_without_candidates_favors_codex() {
    // "status" + not "candidates" gives Codex signal
    let msg = json!({"status": "completed"});
    let r = detector().detect(&msg);
    assert!(r.is_some());
    assert_eq!(r.unwrap().dialect, Dialect::Codex);
}

#[test]
fn edge_status_with_candidates_not_codex() {
    // "status" + "candidates" — candidates should push toward Gemini, not Codex
    let msg = json!({
        "status": "completed",
        "candidates": [{"content": {"parts": [{"text": "hi"}]}}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

// ═══════════════════════════════════════════════════════════════════════
// 12. detect_all behavior
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_all_empty_for_non_object() {
    assert!(detector().detect_all(&json!(null)).is_empty());
    assert!(detector().detect_all(&json!(42)).is_empty());
    assert!(detector().detect_all(&json!("str")).is_empty());
    assert!(detector().detect_all(&json!([])).is_empty());
}

#[test]
fn detect_all_empty_for_empty_object() {
    assert!(detector().detect_all(&json!({})).is_empty());
}

#[test]
fn detect_all_sorted_descending() {
    let msg = json!({
        "model": "x",
        "messages": [{"role": "user", "content": "hi"}],
        "refs": ["a"],
        "temperature": 0.5
    });
    let results = detector().detect_all(&msg);
    for w in results.windows(2) {
        assert!(
            w[0].confidence >= w[1].confidence,
            "not sorted: {} >= {} failed",
            w[0].confidence,
            w[1].confidence
        );
    }
}

#[test]
fn detect_all_first_matches_detect() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "temperature": 0.7
    });
    let single = detector().detect(&msg).unwrap();
    let all = detector().detect_all(&msg);
    assert!(!all.is_empty());
    assert_eq!(all[0].dialect, single.dialect);
}

#[test]
fn detect_all_returns_at_least_one_for_known() {
    let examples = vec![
        json!({"choices": []}),
        json!({"type": "message"}),
        json!({"contents": [{"parts": [{"text": "hi"}]}]}),
        json!({"items": [{"type": "msg"}]}),
        json!({"refs": ["a"]}),
        json!({"references": [{"type": "file"}]}),
    ];
    for ex in &examples {
        let results = detector().detect_all(ex);
        assert!(!results.is_empty(), "expected match for {ex}");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Confidence scoring properties
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn confidence_is_in_zero_one_range() {
    let payloads = vec![
        json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}], "choices": [], "temperature": 1}),
        json!({"type": "message", "model": "c", "content": [{"type": "text", "text": "hi"}], "stop_reason": "end_turn"}),
        json!({"contents": [{"parts": [{"text": "hi"}]}], "candidates": [{}], "generationConfig": {}}),
        json!({"items": [{"type": "m"}], "status": "ok", "object": "response"}),
        json!({"refs": ["a"], "search_plus": true, "messages": [{"role": "user", "content": "hi"}]}),
        json!({"references": [{}], "agent_mode": true, "confirmations": []}),
    ];
    for p in &payloads {
        let results = detector().detect_all(p);
        for r in &results {
            assert!(
                r.confidence > 0.0,
                "confidence must be >0, got {}",
                r.confidence
            );
            assert!(
                r.confidence <= 1.0,
                "confidence must be <=1, got {}",
                r.confidence
            );
        }
    }
}

#[test]
fn confidence_increases_with_more_signals_openai() {
    let minimal = json!({"choices": []});
    let full = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "choices": [{}],
        "temperature": 0.5
    });
    let (_, c_min) = detect(&minimal);
    let (_, c_full) = detect(&full);
    assert!(c_full > c_min);
}

#[test]
fn confidence_increases_with_more_signals_claude() {
    let minimal = json!({"type": "message"});
    let full = json!({
        "type": "message",
        "model": "claude-sonnet-4-20250514",
        "content": [{"type": "text", "text": "hi"}],
        "stop_reason": "end_turn"
    });
    let (_, c_min) = detect(&minimal);
    let (_, c_full) = detect(&full);
    assert!(c_full > c_min);
}

#[test]
fn confidence_increases_with_more_signals_gemini() {
    let minimal = json!({"candidates": [{}]});
    let full = json!({
        "contents": [{"parts": [{"text": "hi"}]}],
        "candidates": [{}],
        "generationConfig": {"temperature": 0.5}
    });
    let (_, c_min) = detect(&minimal);
    let (_, c_full) = detect(&full);
    assert!(c_full > c_min);
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Evidence is always populated
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn evidence_not_empty_openai() {
    let r = detector().detect(&json!({"choices": []})).unwrap();
    assert!(!r.evidence.is_empty());
}

#[test]
fn evidence_not_empty_claude() {
    let r = detector().detect(&json!({"type": "message"})).unwrap();
    assert!(!r.evidence.is_empty());
}

#[test]
fn evidence_not_empty_gemini() {
    let r = detector()
        .detect(&json!({"contents": [{"parts": [{"text": "hi"}]}]}))
        .unwrap();
    assert!(!r.evidence.is_empty());
}

#[test]
fn evidence_not_empty_codex() {
    let r = detector()
        .detect(&json!({"items": [{"type": "m"}]}))
        .unwrap();
    assert!(!r.evidence.is_empty());
}

#[test]
fn evidence_not_empty_kimi() {
    let r = detector().detect(&json!({"refs": ["a"]})).unwrap();
    assert!(!r.evidence.is_empty());
}

#[test]
fn evidence_not_empty_copilot() {
    let r = detector()
        .detect(&json!({"references": [{"type": "file"}]}))
        .unwrap();
    assert!(!r.evidence.is_empty());
}

#[test]
fn evidence_strings_are_human_readable() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "choices": [{}],
        "temperature": 0.7
    });
    let r = detector().detect(&msg).unwrap();
    for ev in &r.evidence {
        assert!(!ev.is_empty());
        assert!(ev.len() > 3, "evidence too short: {ev}");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Validation: OpenAI
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_openai_valid_request() {
    let msg = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
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
    let msg = json!({"model": "gpt-4", "messages": "not an array"});
    let r = validator().validate(&msg, Dialect::OpenAi);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.path == "/messages"));
}

#[test]
fn validate_openai_no_messages_gives_warning() {
    let msg = json!({"model": "gpt-4"});
    let r = validator().validate(&msg, Dialect::OpenAi);
    assert!(r.valid); // warnings only
    assert!(!r.warnings.is_empty());
}

#[test]
fn validate_openai_multiple_messages_one_missing_role() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "user", "content": "hi"},
            {"content": "no role"}
        ]
    });
    let r = validator().validate(&msg, Dialect::OpenAi);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.path == "/messages/1/role"));
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Validation: Claude
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_claude_valid_request() {
    let msg = json!({
        "model": "claude-sonnet-4-20250514",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
    });
    let r = validator().validate(&msg, Dialect::Claude);
    assert!(r.valid);
}

#[test]
fn validate_claude_response_type_message_no_model_ok() {
    let msg = json!({"type": "message", "content": [{"type": "text", "text": "hi"}]});
    let r = validator().validate(&msg, Dialect::Claude);
    assert!(r.valid);
}

#[test]
fn validate_claude_missing_model_no_type_message() {
    let msg = json!({"messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]});
    let r = validator().validate(&msg, Dialect::Claude);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.path == "/model"));
}

#[test]
fn validate_claude_content_must_be_string_or_array() {
    let msg = json!({
        "model": "claude-3",
        "messages": [{"role": "user", "content": 42}]
    });
    let r = validator().validate(&msg, Dialect::Claude);
    assert!(!r.valid);
    assert!(r.errors.iter().any(|e| e.path.contains("content")));
}

#[test]
fn validate_claude_string_content_is_ok() {
    let msg = json!({
        "model": "claude-3",
        "messages": [{"role": "user", "content": "hello"}]
    });
    let r = validator().validate(&msg, Dialect::Claude);
    assert!(r.valid);
}

#[test]
fn validate_claude_messages_not_array() {
    let msg = json!({"model": "claude-3", "messages": "oops"});
    let r = validator().validate(&msg, Dialect::Claude);
    assert!(!r.valid);
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Validation: Gemini
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_gemini_valid_request() {
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
    let msg = json!({"contents": "not array"});
    let r = validator().validate(&msg, Dialect::Gemini);
    assert!(!r.valid);
}

#[test]
fn validate_gemini_no_contents_no_candidates_warning() {
    let msg = json!({"generationConfig": {}});
    let r = validator().validate(&msg, Dialect::Gemini);
    assert!(r.valid);
    assert!(!r.warnings.is_empty());
}

#[test]
fn validate_gemini_candidates_present_no_warning() {
    let msg = json!({"candidates": [{"content": {"parts": [{"text": "hi"}]}}]});
    let r = validator().validate(&msg, Dialect::Gemini);
    assert!(r.valid);
    // no warning about missing contents when candidates present
    assert!(
        !r.warnings.iter().any(|w| w.contains("contents")),
        "should not warn about missing contents when candidates present"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Validation: Codex
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_codex_valid() {
    let msg = json!({"items": [{"type": "message"}], "status": "completed"});
    let r = validator().validate(&msg, Dialect::Codex);
    assert!(r.valid);
}

#[test]
fn validate_codex_item_missing_type() {
    let msg = json!({"items": [{"content": "done"}]});
    let r = validator().validate(&msg, Dialect::Codex);
    assert!(!r.valid);
}

#[test]
fn validate_codex_no_items_ok() {
    let msg = json!({"status": "completed"});
    let r = validator().validate(&msg, Dialect::Codex);
    assert!(r.valid);
}

// ═══════════════════════════════════════════════════════════════════════
// 19. Validation: Kimi
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_kimi_valid() {
    let msg = json!({
        "messages": [{"role": "user", "content": "hi"}],
        "refs": ["https://example.com"]
    });
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

#[test]
fn validate_kimi_no_messages_ok() {
    let msg = json!({"refs": ["a"]});
    let r = validator().validate(&msg, Dialect::Kimi);
    assert!(r.valid);
}

// ═══════════════════════════════════════════════════════════════════════
// 20. Validation: Copilot
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_copilot_valid() {
    let msg = json!({
        "messages": [{"role": "user", "content": "hi"}],
        "references": [{"type": "file"}]
    });
    let r = validator().validate(&msg, Dialect::Copilot);
    assert!(r.valid);
}

#[test]
fn validate_copilot_missing_role() {
    let msg = json!({"messages": [{"content": "hi"}]});
    let r = validator().validate(&msg, Dialect::Copilot);
    assert!(!r.valid);
}

#[test]
fn validate_copilot_no_messages_ok() {
    let msg = json!({"agent_mode": true});
    let r = validator().validate(&msg, Dialect::Copilot);
    assert!(r.valid);
}

// ═══════════════════════════════════════════════════════════════════════
// 21. Validation: non-object
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_non_object_string() {
    let r = validator().validate(&json!("oops"), Dialect::OpenAi);
    assert!(!r.valid);
    assert_eq!(r.errors[0].path, "/");
}

#[test]
fn validate_non_object_number() {
    let r = validator().validate(&json!(42), Dialect::Claude);
    assert!(!r.valid);
}

#[test]
fn validate_non_object_array() {
    let r = validator().validate(&json!([1, 2, 3]), Dialect::Gemini);
    assert!(!r.valid);
}

#[test]
fn validate_non_object_null() {
    let r = validator().validate(&json!(null), Dialect::Codex);
    assert!(!r.valid);
}

#[test]
fn validate_non_object_bool() {
    let r = validator().validate(&json!(true), Dialect::Kimi);
    assert!(!r.valid);
}

// ═══════════════════════════════════════════════════════════════════════
// 22. ValidationError display and traits
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validation_error_display() {
    let e = ValidationError {
        path: "/model".into(),
        message: "missing".into(),
    };
    assert_eq!(format!("{e}"), "/model: missing");
}

#[test]
fn validation_error_debug() {
    let e = ValidationError {
        path: "/foo".into(),
        message: "bar".into(),
    };
    let dbg = format!("{e:?}");
    assert!(dbg.contains("foo"));
    assert!(dbg.contains("bar"));
}

#[test]
fn validation_error_eq() {
    let e1 = ValidationError {
        path: "/a".into(),
        message: "b".into(),
    };
    let e2 = ValidationError {
        path: "/a".into(),
        message: "b".into(),
    };
    assert_eq!(e1, e2);
}

#[test]
fn validation_error_ne() {
    let e1 = ValidationError {
        path: "/a".into(),
        message: "b".into(),
    };
    let e2 = ValidationError {
        path: "/c".into(),
        message: "d".into(),
    };
    assert_ne!(e1, e2);
}

#[test]
fn validation_error_clone() {
    let e = ValidationError {
        path: "/x".into(),
        message: "y".into(),
    };
    let cloned = e.clone();
    assert_eq!(e, cloned);
}

#[test]
fn validation_error_is_error_trait() {
    let e = ValidationError {
        path: "/test".into(),
        message: "test error".into(),
    };
    let _: &dyn std::error::Error = &e;
}

// ═══════════════════════════════════════════════════════════════════════
// 23. DialectDetector and DialectValidator constructors
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detector_default_works() {
    let d = DialectDetector::default();
    assert!(d.detect(&json!({})).is_none());
}

#[test]
fn validator_default_works() {
    let v = DialectValidator::default();
    let r = v.validate(
        &json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]}),
        Dialect::OpenAi,
    );
    assert!(r.valid);
}

#[test]
fn detector_debug_not_empty() {
    let d = DialectDetector::new();
    assert!(!format!("{d:?}").is_empty());
}

#[test]
fn validator_debug_not_empty() {
    let v = DialectValidator::new();
    assert!(!format!("{v:?}").is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 24. DetectionResult properties
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detection_result_debug() {
    let msg = json!({"choices": []});
    let r = detector().detect(&msg).unwrap();
    let dbg = format!("{r:?}");
    assert!(dbg.contains("DetectionResult"));
}

#[test]
fn detection_result_clone() {
    let msg = json!({"choices": []});
    let r = detector().detect(&msg).unwrap();
    let cloned = r.clone();
    assert_eq!(cloned.dialect, r.dialect);
    assert_eq!(cloned.confidence, r.confidence);
    assert_eq!(cloned.evidence, r.evidence);
}

// ═══════════════════════════════════════════════════════════════════════
// 25. ValidationResult properties
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validation_result_debug() {
    let r = validator().validate(&json!({"model": "gpt-4"}), Dialect::OpenAi);
    assert!(!format!("{r:?}").is_empty());
}

#[test]
fn validation_result_clone() {
    let r = validator().validate(&json!({"model": "gpt-4"}), Dialect::OpenAi);
    let cloned = r.clone();
    assert_eq!(cloned.valid, r.valid);
    assert_eq!(cloned.errors, r.errors);
    assert_eq!(cloned.warnings, r.warnings);
}

// ═══════════════════════════════════════════════════════════════════════
// 26. Cross-dialect detection accuracy
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cross_openai_vs_claude_same_prompt() {
    let openai = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "2+2?"}]});
    let claude = json!({"model": "c3", "messages": [{"role": "user", "content": [{"type": "text", "text": "2+2?"}]}]});
    assert_eq!(detect(&openai).0, Dialect::OpenAi);
    assert_eq!(detect(&claude).0, Dialect::Claude);
}

#[test]
fn cross_openai_vs_gemini_same_prompt() {
    let openai = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let gemini = json!({"contents": [{"parts": [{"text": "hi"}]}]});
    assert_eq!(detect(&openai).0, Dialect::OpenAi);
    assert_eq!(detect(&gemini).0, Dialect::Gemini);
}

#[test]
fn cross_claude_vs_gemini_responses() {
    let claude_resp = json!({
        "type": "message", "content": [{"type": "text", "text": "A"}], "stop_reason": "end_turn"
    });
    let gemini_resp = json!({
        "candidates": [{"content": {"parts": [{"text": "A"}]}, "finishReason": "STOP"}]
    });
    assert_eq!(detect(&claude_resp).0, Dialect::Claude);
    assert_eq!(detect(&gemini_resp).0, Dialect::Gemini);
}

#[test]
fn cross_codex_vs_openai() {
    let codex =
        json!({"object": "response", "status": "completed", "items": [{"type": "message"}]});
    let openai =
        json!({"model": "gpt-4", "choices": [{"message": {"role": "assistant", "content": "hi"}}]});
    assert_eq!(detect(&codex).0, Dialect::Codex);
    assert_eq!(detect(&openai).0, Dialect::OpenAi);
}

#[test]
fn cross_kimi_vs_openai() {
    // Kimi has refs; plain messages+model goes OpenAI
    let kimi = json!({"model": "kimi", "messages": [{"role": "user", "content": "search"}], "refs": ["a"]});
    let openai = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "search"}]});
    assert_eq!(detect(&kimi).0, Dialect::Kimi);
    assert_eq!(detect(&openai).0, Dialect::OpenAi);
}

#[test]
fn cross_copilot_vs_openai() {
    let copilot =
        json!({"messages": [{"role": "user", "content": "fix"}], "references": [{"type": "file"}]});
    let openai = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "fix"}]});
    assert_eq!(detect(&copilot).0, Dialect::Copilot);
    assert_eq!(detect(&openai).0, Dialect::OpenAi);
}
