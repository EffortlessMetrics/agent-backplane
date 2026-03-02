// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for SDK dialect detection via `DialectDetector`.

use abp_dialect::{DetectionResult, Dialect, DialectDetector};
use serde_json::{Value, json};

fn detector() -> DialectDetector {
    DialectDetector::new()
}

fn detect(v: &Value) -> (Dialect, f64) {
    let r = detector().detect(v).expect("expected detection result");
    (r.dialect, r.confidence)
}

fn detect_opt(v: &Value) -> Option<DetectionResult> {
    detector().detect(v)
}

// ═══════════════════════════════════════════════════════════════════════
// 1. OpenAI dialect detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_gpt4_model_with_messages() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hello"}]
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
    assert!(c > 0.4);
}

#[test]
fn openai_gpt35_turbo_model() {
    let msg = json!({
        "model": "gpt-3.5-turbo",
        "messages": [{"role": "system", "content": "be brief"}, {"role": "user", "content": "hi"}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn openai_gpt4o_model() {
    let msg = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "summarize"}],
        "temperature": 0.5
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn openai_choices_key_only() {
    let msg = json!({"choices": [{"message": {"role": "assistant", "content": "ok"}}]});
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
    assert!(c >= 0.4);
}

#[test]
fn openai_tool_choice_field() {
    let msg = json!({
        "model": "gpt-4-turbo",
        "messages": [{"role": "user", "content": "look up price"}],
        "tool_choice": "auto",
        "tools": [{"type": "function", "function": {"name": "search"}}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn openai_function_call_field() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "weather?"}],
        "function_call": "auto",
        "functions": [{"name": "get_weather"}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn openai_temperature_top_p_max_tokens() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "test"}],
        "temperature": 0.8,
        "top_p": 0.95,
        "max_tokens": 512
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
    assert!(c > 0.5);
}

#[test]
fn openai_response_with_usage() {
    let msg = json!({
        "id": "chatcmpl-abc",
        "object": "chat.completion",
        "model": "gpt-4",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "done"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn openai_streaming_chunk() {
    let msg = json!({
        "id": "chatcmpl-xyz",
        "object": "chat.completion.chunk",
        "model": "gpt-4",
        "choices": [{"index": 0, "delta": {"content": "Hi"}, "finish_reason": null}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn openai_response_with_tool_calls() {
    let msg = json!({
        "model": "gpt-4-turbo",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "f", "arguments": "{}"}}]
            },
            "finish_reason": "tool_calls"
        }]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn openai_empty_choices() {
    let msg = json!({"choices": []});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn openai_messages_with_system_role_string_content() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "You are helpful."},
            {"role": "user", "content": "Hi"},
            {"role": "assistant", "content": "Hello!"}
        ]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn openai_frequency_presence_penalty() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "test"}],
        "frequency_penalty": 0.5,
        "presence_penalty": 0.3
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Claude dialect detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn claude_messages_with_content_blocks() {
    let msg = json!({
        "model": "claude-3-opus-20240229",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hello"}]}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn claude_response_type_message() {
    let msg = json!({
        "type": "message",
        "model": "claude-sonnet-4-20250514",
        "content": [{"type": "text", "text": "hi"}],
        "stop_reason": "end_turn"
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
    assert!(c > 0.5);
}

#[test]
fn claude_system_as_separate_string_field() {
    let msg = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "system": "You are a poet.",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "write a haiku"}]}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn claude_thinking_blocks() {
    let msg = json!({
        "type": "message",
        "model": "claude-sonnet-4-20250514",
        "content": [
            {"type": "thinking", "thinking": "let me reason..."},
            {"type": "text", "text": "Answer."}
        ],
        "stop_reason": "end_turn"
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn claude_tool_use_block() {
    let msg = json!({
        "type": "message",
        "model": "claude-sonnet-4-20250514",
        "content": [{"type": "tool_use", "id": "toolu_1", "name": "search", "input": {}}],
        "stop_reason": "tool_use"
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn claude_tool_result_message() {
    let msg = json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": "ok"}]}
        ]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn claude_image_content_block() {
    let msg = json!({
        "model": "claude-3-opus-20240229",
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "describe this"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "abc"}}
            ]
        }]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn claude_stop_reason_field() {
    let msg = json!({
        "type": "message",
        "content": [{"type": "text", "text": "ok"}],
        "stop_reason": "max_tokens"
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn claude_content_array_without_type_message() {
    let msg = json!({
        "content": [{"type": "text", "text": "standalone"}],
        "stop_reason": "end_turn"
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn claude_multiple_text_blocks() {
    let msg = json!({
        "type": "message",
        "model": "claude-3-haiku-20240307",
        "content": [
            {"type": "text", "text": "Part 1"},
            {"type": "text", "text": "Part 2"}
        ],
        "stop_reason": "end_turn"
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn claude_usage_input_output_tokens() {
    let msg = json!({
        "type": "message",
        "model": "claude-sonnet-4-20250514",
        "content": [{"type": "text", "text": "hi"}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 10, "output_tokens": 5}
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn claude_request_with_max_tokens() {
    let msg = json!({
        "model": "claude-3-opus-20240229",
        "max_tokens": 4096,
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Gemini dialect detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn gemini_contents_with_parts() {
    let msg = json!({
        "contents": [{"role": "user", "parts": [{"text": "explain gravity"}]}]
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
    assert!(c >= 0.5);
}

#[test]
fn gemini_candidates_response() {
    let msg = json!({
        "candidates": [{
            "content": {"parts": [{"text": "answer"}], "role": "model"},
            "finishReason": "STOP"
        }]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn gemini_generation_config_camel_case() {
    let msg = json!({
        "contents": [{"parts": [{"text": "hi"}]}],
        "generationConfig": {"temperature": 0.7, "topP": 0.9, "topK": 40}
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn gemini_generation_config_snake_case() {
    let msg = json!({
        "contents": [{"parts": [{"text": "hi"}]}],
        "generation_config": {"temperature": 0.5, "max_output_tokens": 512}
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn gemini_safety_settings() {
    let msg = json!({
        "contents": [{"parts": [{"text": "tell a story"}]}],
        "safetySettings": [{"category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_MEDIUM_AND_ABOVE"}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn gemini_system_instruction() {
    let msg = json!({
        "systemInstruction": {"parts": [{"text": "You are a tutor."}]},
        "contents": [{"role": "user", "parts": [{"text": "teach me"}]}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn gemini_function_declarations() {
    let msg = json!({
        "contents": [{"parts": [{"text": "weather?"}]}],
        "tools": [{"functionDeclarations": [{"name": "get_weather", "parameters": {}}]}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn gemini_function_call_response() {
    let msg = json!({
        "contents": [
            {"role": "user", "parts": [{"text": "weather?"}]},
            {"role": "model", "parts": [{"functionCall": {"name": "get_weather", "args": {"city": "NYC"}}}]}
        ]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn gemini_function_response_part() {
    let msg = json!({
        "contents": [
            {"role": "function", "parts": [{"functionResponse": {"name": "f", "response": {"result": "ok"}}}]}
        ]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn gemini_usage_metadata() {
    let msg = json!({
        "candidates": [{"content": {"parts": [{"text": "hi"}], "role": "model"}, "finishReason": "STOP"}],
        "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 2, "totalTokenCount": 7}
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn gemini_multimodal_parts() {
    let msg = json!({
        "contents": [{
            "parts": [
                {"text": "what is this?"},
                {"inlineData": {"mimeType": "image/jpeg", "data": "base64..."}}
            ]
        }]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn gemini_candidates_only() {
    let msg = json!({"candidates": []});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Codex dialect detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn codex_items_with_type() {
    let msg = json!({
        "items": [{"type": "message", "content": "hello"}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Codex);
}

#[test]
fn codex_object_response() {
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
fn codex_status_field() {
    let msg = json!({
        "status": "completed",
        "items": [{"type": "function_call", "name": "run_tests"}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Codex);
}

#[test]
fn codex_instructions_field() {
    // instructions is a Codex request field; items+type gives Codex signal
    let msg = json!({
        "items": [{"type": "message"}],
        "status": "in_progress"
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Codex);
}

#[test]
fn codex_multiple_items() {
    let msg = json!({
        "object": "response",
        "status": "completed",
        "items": [
            {"type": "message", "content": "Analyzing..."},
            {"type": "function_call", "name": "read_file", "arguments": "{}"},
            {"type": "message", "content": "Done."}
        ]
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::Codex);
    assert!(c > 0.5);
}

#[test]
fn codex_status_in_progress() {
    let msg = json!({
        "status": "in_progress",
        "items": [{"type": "message"}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Codex);
}

#[test]
fn codex_object_response_without_items() {
    let msg = json!({
        "object": "response",
        "status": "completed"
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Codex);
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Kimi dialect detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn kimi_refs_field() {
    let msg = json!({
        "model": "moonshot-v1-8k",
        "messages": [{"role": "user", "content": "search this"}],
        "refs": ["https://example.com"]
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::Kimi);
    assert!(c > 0.5);
}

#[test]
fn kimi_search_plus_field() {
    let msg = json!({
        "model": "moonshot-v1-32k",
        "messages": [{"role": "user", "content": "find info"}],
        "search_plus": true
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Kimi);
}

#[test]
fn kimi_use_search_via_search_plus() {
    let msg = json!({
        "model": "moonshot-v1",
        "messages": [{"role": "user", "content": "news"}],
        "search_plus": false
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Kimi);
}

#[test]
fn kimi_refs_with_multiple_urls() {
    let msg = json!({
        "model": "moonshot-v1-128k",
        "messages": [{"role": "user", "content": "summarize these"}],
        "refs": ["https://a.com", "https://b.com", "https://c.com"]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Kimi);
}

#[test]
fn kimi_refs_only_no_messages() {
    let msg = json!({"refs": ["https://example.com"]});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Kimi);
}

#[test]
fn kimi_search_plus_without_model() {
    let msg = json!({
        "messages": [{"role": "user", "content": "search"}],
        "search_plus": true
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Kimi);
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Copilot dialect detection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn copilot_references_field() {
    let msg = json!({
        "messages": [{"role": "user", "content": "fix bug"}],
        "references": [{"type": "file", "path": "src/main.rs"}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Copilot);
}

#[test]
fn copilot_agent_mode() {
    let msg = json!({
        "messages": [{"role": "user", "content": "refactor"}],
        "agent_mode": true
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::Copilot);
    assert!(c >= 0.45);
}

#[test]
fn copilot_confirmations_field() {
    let msg = json!({
        "confirmations": [{"id": "c1", "accepted": true}],
        "agent_mode": true
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Copilot);
}

#[test]
fn copilot_agent_mode_with_confirmations() {
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
fn copilot_references_multiple_files() {
    let msg = json!({
        "messages": [{"role": "user", "content": "review"}],
        "references": [
            {"type": "file", "path": "a.rs"},
            {"type": "file", "path": "b.rs"},
            {"type": "snippet", "content": "fn main() {}"}
        ]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Copilot);
}

#[test]
fn copilot_references_without_messages() {
    let msg = json!({"references": [{"type": "file"}]});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Copilot);
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Ambiguous requests matching multiple dialects
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ambiguous_messages_plus_contents() {
    let msg = json!({
        "model": "some-model",
        "messages": [{"role": "user", "content": "hello"}],
        "contents": [{"parts": [{"text": "hello"}]}]
    });
    let results = detector().detect_all(&msg);
    assert!(results.len() >= 2);
    let dialects: Vec<_> = results.iter().map(|r| r.dialect).collect();
    assert!(dialects.contains(&Dialect::OpenAi));
    assert!(dialects.contains(&Dialect::Gemini));
}

#[test]
fn ambiguous_messages_plus_refs() {
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
fn ambiguous_messages_plus_references() {
    let msg = json!({
        "messages": [{"role": "user", "content": "fix"}],
        "references": [{"type": "file"}],
        "model": "gpt-4"
    });
    let results = detector().detect_all(&msg);
    let dialects: Vec<_> = results.iter().map(|r| r.dialect).collect();
    assert!(dialects.contains(&Dialect::OpenAi));
    assert!(dialects.contains(&Dialect::Copilot));
}

#[test]
fn ambiguous_openai_vs_kimi_messages_with_string_content() {
    // Both OpenAI and Kimi use messages with role, but without refs/search_plus
    // OpenAI should win because string content is a strong OpenAI signal.
    let msg = json!({
        "model": "some-model",
        "messages": [{"role": "user", "content": "plain text"}],
        "temperature": 0.5
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn ambiguous_model_key_shared() {
    // "model" alone with no other signals is weakly OpenAI
    let msg = json!({"model": "unknown-model"});
    let r = detect_opt(&msg);
    // model alone without contents/items gives OpenAI a small score
    assert!(r.is_some());
    assert_eq!(r.unwrap().dialect, Dialect::OpenAi);
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Confidence scoring
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn confidence_capped_at_one_openai() {
    let msg = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "choices": [{}],
        "temperature": 1.0,
        "top_p": 1.0,
        "max_tokens": 100
    });
    let (_, c) = detect(&msg);
    assert!(c <= 1.0);
}

#[test]
fn confidence_capped_at_one_gemini() {
    let msg = json!({
        "contents": [{"parts": [{"text": "x"}]}],
        "candidates": [{}],
        "generationConfig": {"temperature": 1.0}
    });
    let (_, c) = detect(&msg);
    assert!(c <= 1.0);
}

#[test]
fn confidence_capped_at_one_claude() {
    let msg = json!({
        "type": "message",
        "model": "claude-3",
        "content": [{"type": "text", "text": "x"}],
        "stop_reason": "end_turn",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "y"}]}]
    });
    let (_, c) = detect(&msg);
    assert!(c <= 1.0);
}

#[test]
fn confidence_capped_at_one_codex() {
    let msg = json!({
        "object": "response",
        "status": "completed",
        "items": [{"type": "message"}]
    });
    let (_, c) = detect(&msg);
    assert!(c <= 1.0);
}

#[test]
fn confidence_capped_at_one_kimi() {
    let msg = json!({
        "refs": ["a"],
        "search_plus": true,
        "messages": [{"role": "user", "content": "hi"}]
    });
    let (_, c) = detect(&msg);
    assert!(c <= 1.0);
}

#[test]
fn confidence_capped_at_one_copilot() {
    let msg = json!({
        "references": [{"type": "file"}],
        "confirmations": [{}],
        "agent_mode": true
    });
    let (_, c) = detect(&msg);
    assert!(c <= 1.0);
}

#[test]
fn confidence_increases_with_more_openai_signals() {
    let minimal = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let richer = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "choices": [{}],
        "temperature": 0.7
    });
    let (_, c_min) = detect(&minimal);
    let (_, c_rich) = detect(&richer);
    assert!(c_rich > c_min);
}

#[test]
fn confidence_increases_with_more_gemini_signals() {
    let minimal = json!({"contents": [{"parts": [{"text": "hi"}]}]});
    let richer = json!({
        "contents": [{"parts": [{"text": "hi"}]}],
        "generationConfig": {"temperature": 0.5}
    });
    let (_, c_min) = detect(&minimal);
    let (_, c_rich) = detect(&richer);
    assert!(c_rich > c_min);
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Unknown/unrecognized dialects
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn unknown_no_known_keys() {
    let msg = json!({"foo": "bar", "baz": 42});
    assert!(detect_opt(&msg).is_none());
}

#[test]
fn unknown_deeply_nested_signals() {
    let msg = json!({
        "data": {
            "inner": {
                "messages": [{"role": "user", "content": "hi"}]
            }
        }
    });
    assert!(detect_opt(&msg).is_none());
}

#[test]
fn unknown_only_unrelated_keys() {
    let msg = json!({"timestamp": 123456, "version": "1.0", "source": "test"});
    assert!(detect_opt(&msg).is_none());
}

#[test]
fn unknown_random_nested_object() {
    let msg = json!({"config": {"nested": {"deep": true}}, "id": "abc"});
    assert!(detect_opt(&msg).is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Edge cases: empty, minimal, maximal
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn edge_empty_object() {
    assert!(detect_opt(&json!({})).is_none());
}

#[test]
fn edge_null_value() {
    assert!(detect_opt(&json!(null)).is_none());
}

#[test]
fn edge_boolean_value() {
    assert!(detect_opt(&json!(true)).is_none());
}

#[test]
fn edge_number_value() {
    assert!(detect_opt(&json!(42)).is_none());
}

#[test]
fn edge_string_value() {
    assert!(detect_opt(&json!("hello")).is_none());
}

#[test]
fn edge_array_value() {
    assert!(detect_opt(&json!([1, 2, 3])).is_none());
}

#[test]
fn edge_minimal_openai() {
    let msg = json!({"choices": []});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn edge_minimal_claude() {
    let msg = json!({"type": "message"});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn edge_minimal_gemini() {
    let msg = json!({"candidates": []});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

#[test]
fn edge_minimal_codex() {
    let msg = json!({"items": [{"type": "x"}]});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Codex);
}

#[test]
fn edge_minimal_kimi() {
    let msg = json!({"refs": []});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Kimi);
}

#[test]
fn edge_minimal_copilot() {
    let msg = json!({"references": []});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Copilot);
}

#[test]
fn edge_maximal_openai() {
    let msg = json!({
        "model": "gpt-4-turbo-2024-04-09",
        "messages": [
            {"role": "system", "content": "system prompt"},
            {"role": "user", "content": "user prompt"},
            {"role": "assistant", "content": "response"}
        ],
        "temperature": 0.7,
        "top_p": 0.95,
        "max_tokens": 4096,
        "frequency_penalty": 0.0,
        "presence_penalty": 0.0,
        "n": 1,
        "stream": true,
        "stop": ["\n"],
        "tools": [{"type": "function", "function": {"name": "f", "parameters": {}}}],
        "tool_choice": "auto"
    });
    let (d, c) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
    assert!(c > 0.5);
}

#[test]
fn edge_maximal_gemini() {
    let msg = json!({
        "contents": [
            {"role": "user", "parts": [{"text": "query"}]},
            {"role": "model", "parts": [{"text": "answer"}]}
        ],
        "generationConfig": {
            "temperature": 0.8,
            "topP": 0.95,
            "topK": 40,
            "maxOutputTokens": 2048,
            "stopSequences": ["END"]
        },
        "safetySettings": [
            {"category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_MEDIUM_AND_ABOVE"}
        ],
        "systemInstruction": {"parts": [{"text": "system"}]},
        "tools": [{"functionDeclarations": [{"name": "tool1"}]}]
    });
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Gemini);
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Detection with partial information
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn partial_openai_model_only() {
    // model alone (without contents/items) maps to OpenAI
    let msg = json!({"model": "gpt-4"});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn partial_openai_temperature_only() {
    let msg = json!({"temperature": 0.5, "model": "gpt-4"});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::OpenAi);
}

#[test]
fn partial_claude_type_message_only() {
    let msg = json!({"type": "message"});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Claude);
}

#[test]
fn partial_gemini_contents_without_parts() {
    // contents array where items lack "parts" — no Gemini match
    let msg = json!({"contents": [{"role": "user"}]});
    // contents without parts doesn't trigger Gemini
    let r = detect_opt(&msg);
    // Should be None or not Gemini
    if let Some(res) = r {
        assert_ne!(res.dialect, Dialect::Gemini);
    }
}

#[test]
fn partial_codex_status_without_candidates() {
    let msg = json!({"status": "completed"});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Codex);
}

#[test]
fn partial_kimi_refs_empty_array() {
    let msg = json!({"refs": []});
    let (d, _) = detect(&msg);
    assert_eq!(d, Dialect::Kimi);
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Detection stability (same input = same result)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn stability_openai_repeated() {
    let msg = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let r1 = detect(&msg);
    let r2 = detect(&msg);
    assert_eq!(r1.0, r2.0);
    assert!((r1.1 - r2.1).abs() < f64::EPSILON);
}

#[test]
fn stability_claude_repeated() {
    let msg = json!({"type": "message", "content": [{"type": "text", "text": "hi"}], "stop_reason": "end_turn"});
    let r1 = detect(&msg);
    let r2 = detect(&msg);
    assert_eq!(r1.0, r2.0);
    assert!((r1.1 - r2.1).abs() < f64::EPSILON);
}

#[test]
fn stability_gemini_repeated() {
    let msg = json!({"contents": [{"parts": [{"text": "hi"}]}]});
    let r1 = detect(&msg);
    let r2 = detect(&msg);
    assert_eq!(r1.0, r2.0);
    assert!((r1.1 - r2.1).abs() < f64::EPSILON);
}

#[test]
fn stability_codex_repeated() {
    let msg = json!({"items": [{"type": "message"}], "status": "completed"});
    let r1 = detect(&msg);
    let r2 = detect(&msg);
    assert_eq!(r1.0, r2.0);
    assert!((r1.1 - r2.1).abs() < f64::EPSILON);
}

#[test]
fn stability_kimi_repeated() {
    let msg = json!({"refs": ["a"], "messages": [{"role": "user", "content": "x"}]});
    let r1 = detect(&msg);
    let r2 = detect(&msg);
    assert_eq!(r1.0, r2.0);
    assert!((r1.1 - r2.1).abs() < f64::EPSILON);
}

#[test]
fn stability_copilot_repeated() {
    let msg = json!({"references": [{"type": "file"}], "agent_mode": true});
    let r1 = detect(&msg);
    let r2 = detect(&msg);
    assert_eq!(r1.0, r2.0);
    assert!((r1.1 - r2.1).abs() < f64::EPSILON);
}

#[test]
fn stability_detect_all_repeated() {
    let msg = json!({
        "model": "x",
        "messages": [{"role": "user", "content": "hi"}],
        "refs": ["a"]
    });
    let r1 = detector().detect_all(&msg);
    let r2 = detector().detect_all(&msg);
    assert_eq!(r1.len(), r2.len());
    for (a, b) in r1.iter().zip(r2.iter()) {
        assert_eq!(a.dialect, b.dialect);
        assert!((a.confidence - b.confidence).abs() < f64::EPSILON);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 13. All Dialect enum variants covered
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn all_variants_label() {
    for d in Dialect::all() {
        assert!(!d.label().is_empty());
    }
}

#[test]
fn all_variants_display() {
    for d in Dialect::all() {
        let s = format!("{d}");
        assert!(!s.is_empty());
    }
}

#[test]
fn all_variants_detectable() {
    let examples: Vec<(Dialect, Value)> = vec![
        (
            Dialect::OpenAi,
            json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]}),
        ),
        (
            Dialect::Claude,
            json!({"type": "message", "content": [{"type": "text", "text": "hi"}], "stop_reason": "end_turn"}),
        ),
        (
            Dialect::Gemini,
            json!({"contents": [{"parts": [{"text": "hi"}]}]}),
        ),
        (
            Dialect::Codex,
            json!({"items": [{"type": "message"}], "status": "completed"}),
        ),
        (
            Dialect::Kimi,
            json!({"refs": ["a"], "messages": [{"role": "user", "content": "hi"}]}),
        ),
        (
            Dialect::Copilot,
            json!({"references": [{"type": "file"}], "agent_mode": true}),
        ),
    ];
    for (expected, msg) in &examples {
        let r = detector()
            .detect(msg)
            .unwrap_or_else(|| panic!("no detection for {expected}"));
        assert_eq!(r.dialect, *expected, "mismatch for {expected}");
    }
}

#[test]
fn all_variants_count() {
    assert_eq!(Dialect::all().len(), 6, "expected 6 dialect variants");
}

#[test]
fn all_variants_in_detect_all_for_ambiguous() {
    // Build a message that overlaps multiple dialects
    let msg = json!({
        "model": "x",
        "messages": [{"role": "user", "content": "hi"}],
        "contents": [{"parts": [{"text": "hi"}]}],
        "refs": ["a"],
        "references": [{"type": "file"}],
        "agent_mode": true,
        "items": [{"type": "message"}],
        "status": "ok",
        "search_plus": true,
        "confirmations": [],
        "choices": [{}],
        "type": "message",
        "content": [{"type": "text", "text": "hi"}],
        "stop_reason": "end_turn",
        "candidates": [],
        "temperature": 0.5
    });
    let results = detector().detect_all(&msg);
    // Should detect all 6 dialects
    let dialects: Vec<_> = results.iter().map(|r| r.dialect).collect();
    for d in Dialect::all() {
        assert!(dialects.contains(d), "missing dialect {d} in detect_all");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 14. detect_all ordering and evidence
// ═══════════════════════════════════════════════════════════════════════

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
            "not sorted: {} ({}) >= {} ({})",
            w[0].dialect,
            w[0].confidence,
            w[1].dialect,
            w[1].confidence
        );
    }
}

#[test]
fn detect_all_empty_for_scalar() {
    assert!(detector().detect_all(&json!(42)).is_empty());
    assert!(detector().detect_all(&json!("s")).is_empty());
    assert!(detector().detect_all(&json!(null)).is_empty());
    assert!(detector().detect_all(&json!(true)).is_empty());
}

#[test]
fn evidence_always_populated() {
    let examples: Vec<Value> = vec![
        json!({"choices": [{}]}),
        json!({"type": "message"}),
        json!({"contents": [{"parts": [{"text": "hi"}]}]}),
        json!({"items": [{"type": "message"}]}),
        json!({"refs": ["a"]}),
        json!({"references": [{}]}),
    ];
    for example in &examples {
        let r = detector().detect(example).expect("should detect");
        assert!(!r.evidence.is_empty(), "no evidence for {:?}", r.dialect);
    }
}

#[test]
fn detect_all_empty_for_empty_object() {
    assert!(detector().detect_all(&json!({})).is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Serde roundtrip for Dialect
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dialect_serde_roundtrip() {
    for d in Dialect::all() {
        let json = serde_json::to_string(d).unwrap();
        let back: Dialect = serde_json::from_str(&json).unwrap();
        assert_eq!(*d, back);
    }
}

#[test]
fn dialect_serde_snake_case() {
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
