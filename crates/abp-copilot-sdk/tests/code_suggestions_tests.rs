// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for Copilot-specific code suggestion/completion types and JsonSchema derives.

use abp_copilot_sdk::types::*;
use schemars::schema_for;

// ── CompletionType serde ────────────────────────────────────────────────

#[test]
fn completion_type_inline_roundtrip() {
    let json = serde_json::to_string(&CompletionType::Inline).unwrap();
    assert_eq!(json, r#""inline""#);
    let ct: CompletionType = serde_json::from_str(&json).unwrap();
    assert_eq!(ct, CompletionType::Inline);
}

#[test]
fn completion_type_block_roundtrip() {
    let json = serde_json::to_string(&CompletionType::Block).unwrap();
    assert_eq!(json, r#""block""#);
    let ct: CompletionType = serde_json::from_str(&json).unwrap();
    assert_eq!(ct, CompletionType::Block);
}

#[test]
fn completion_type_fill_in_middle_roundtrip() {
    let json = serde_json::to_string(&CompletionType::FillInMiddle).unwrap();
    assert_eq!(json, r#""fill_in_middle""#);
    let ct: CompletionType = serde_json::from_str(&json).unwrap();
    assert_eq!(ct, CompletionType::FillInMiddle);
}

// ── CopilotCompletionRequest serde ──────────────────────────────────────

#[test]
fn completion_request_minimal_roundtrip() {
    let req = CopilotCompletionRequest {
        model: "copilot-codex".into(),
        prompt: "fn main() {".into(),
        suffix: None,
        max_tokens: None,
        temperature: None,
        top_p: None,
        n: None,
        stop: None,
        completion_type: None,
        language: None,
        file_path: None,
        cursor_line: None,
        cursor_column: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let req2: CopilotCompletionRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, req2);
}

#[test]
fn completion_request_with_suffix_and_metadata() {
    let req = CopilotCompletionRequest {
        model: "copilot-codex".into(),
        prompt: "fn main() {\n    let x = ".into(),
        suffix: Some(";\n}".into()),
        max_tokens: Some(128),
        temperature: Some(0.2),
        top_p: Some(0.95),
        n: Some(3),
        stop: Some(vec!["\n\n".into()]),
        completion_type: Some(CompletionType::FillInMiddle),
        language: Some("rust".into()),
        file_path: Some("src/main.rs".into()),
        cursor_line: Some(1),
        cursor_column: Some(12),
    };
    let json = serde_json::to_string(&req).unwrap();
    let req2: CopilotCompletionRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req, req2);
    assert_eq!(req2.suffix.as_deref(), Some(";\n}"));
    assert_eq!(req2.language.as_deref(), Some("rust"));
}

#[test]
fn completion_request_optional_fields_omitted_when_none() {
    let req = CopilotCompletionRequest {
        model: "copilot-codex".into(),
        prompt: "print(".into(),
        suffix: None,
        max_tokens: None,
        temperature: None,
        top_p: None,
        n: None,
        stop: None,
        completion_type: None,
        language: None,
        file_path: None,
        cursor_line: None,
        cursor_column: None,
    };
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("suffix").is_none());
    assert!(json.get("max_tokens").is_none());
    assert!(json.get("temperature").is_none());
    assert!(json.get("language").is_none());
    assert!(json.get("file_path").is_none());
    assert!(json.get("cursor_line").is_none());
}

// ── CopilotCompletionResponse serde ─────────────────────────────────────

#[test]
fn completion_response_roundtrip() {
    let resp = CopilotCompletionResponse {
        id: "cmpl-abc123".into(),
        object: "text_completion".into(),
        created: 1700000000,
        model: "copilot-codex".into(),
        choices: vec![
            CopilotCompletionChoice {
                index: 0,
                text: "println!(\"Hello, world!\");".into(),
                finish_reason: Some("stop".into()),
            },
            CopilotCompletionChoice {
                index: 1,
                text: "println!(\"Hi!\");".into(),
                finish_reason: Some("stop".into()),
            },
        ],
        usage: Some(CopilotUsage {
            prompt_tokens: 20,
            completion_tokens: 10,
            total_tokens: 30,
            copilot_tokens: None,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let resp2: CopilotCompletionResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(resp, resp2);
    assert_eq!(resp2.choices.len(), 2);
    assert_eq!(resp2.object, "text_completion");
}

#[test]
fn completion_choice_minimal() {
    let choice = CopilotCompletionChoice {
        index: 0,
        text: "42".into(),
        finish_reason: None,
    };
    let json = serde_json::to_value(&choice).unwrap();
    assert_eq!(json["text"], "42");
    assert!(json.get("finish_reason").is_none());
}

// ── CopilotCodeSuggestion serde ─────────────────────────────────────────

#[test]
fn code_suggestion_roundtrip() {
    let suggestion = CopilotCodeSuggestion {
        id: "sug-1".into(),
        text: "fn hello() -> String { \"world\".to_string() }".into(),
        score: Some(0.95),
        range_start_line: Some(10),
        range_end_line: Some(12),
        language: Some("rust".into()),
        completion_type: Some(CompletionType::Block),
    };
    let json = serde_json::to_string(&suggestion).unwrap();
    let sug2: CopilotCodeSuggestion = serde_json::from_str(&json).unwrap();
    assert_eq!(suggestion, sug2);
}

#[test]
fn code_suggestion_minimal() {
    let suggestion = CopilotCodeSuggestion {
        id: "sug-2".into(),
        text: "x + 1".into(),
        score: None,
        range_start_line: None,
        range_end_line: None,
        language: None,
        completion_type: None,
    };
    let json = serde_json::to_value(&suggestion).unwrap();
    assert_eq!(json["id"], "sug-2");
    assert_eq!(json["text"], "x + 1");
    assert!(json.get("score").is_none());
    assert!(json.get("language").is_none());
}

// ── JsonSchema generation ───────────────────────────────────────────────

#[test]
fn reference_type_has_json_schema() {
    let schema = schema_for!(ReferenceType);
    let json = serde_json::to_value(&schema).unwrap();
    assert!(json.get("oneOf").is_some() || json.get("enum").is_some());
}

#[test]
fn copilot_chat_request_has_json_schema() {
    let schema = schema_for!(CopilotChatRequest);
    let json = serde_json::to_value(&schema).unwrap();
    let props = &json["properties"];
    assert!(props.get("model").is_some());
    assert!(props.get("messages").is_some());
}

#[test]
fn copilot_chat_response_has_json_schema() {
    let schema = schema_for!(CopilotChatResponse);
    let json = serde_json::to_value(&schema).unwrap();
    let props = &json["properties"];
    assert!(props.get("id").is_some());
    assert!(props.get("choices").is_some());
}

#[test]
fn copilot_completion_request_has_json_schema() {
    let schema = schema_for!(CopilotCompletionRequest);
    let json = serde_json::to_value(&schema).unwrap();
    let props = &json["properties"];
    assert!(props.get("model").is_some());
    assert!(props.get("prompt").is_some());
    assert!(props.get("suffix").is_some());
}

#[test]
fn copilot_completion_response_has_json_schema() {
    let schema = schema_for!(CopilotCompletionResponse);
    let json = serde_json::to_value(&schema).unwrap();
    let props = &json["properties"];
    assert!(props.get("choices").is_some());
    assert!(props.get("model").is_some());
}

#[test]
fn copilot_code_suggestion_has_json_schema() {
    let schema = schema_for!(CopilotCodeSuggestion);
    let json = serde_json::to_value(&schema).unwrap();
    let props = &json["properties"];
    assert!(props.get("text").is_some());
    assert!(props.get("score").is_some());
}

#[test]
fn copilot_stream_chunk_has_json_schema() {
    let schema = schema_for!(CopilotStreamChunk);
    let json = serde_json::to_value(&schema).unwrap();
    let props = &json["properties"];
    assert!(props.get("choices").is_some());
}

#[test]
fn copilot_usage_has_json_schema() {
    let schema = schema_for!(CopilotUsage);
    let json = serde_json::to_value(&schema).unwrap();
    let props = &json["properties"];
    assert!(props.get("prompt_tokens").is_some());
    assert!(props.get("completion_tokens").is_some());
    assert!(props.get("copilot_tokens").is_some());
}

#[test]
fn completion_type_has_json_schema() {
    let schema = schema_for!(CompletionType);
    let json = serde_json::to_value(&schema).unwrap();
    assert!(json.get("oneOf").is_some() || json.get("enum").is_some());
}

// ── Dialect types JsonSchema ────────────────────────────────────────────

#[test]
fn dialect_copilot_reference_has_json_schema() {
    use abp_copilot_sdk::dialect::CopilotReference;
    let schema = schema_for!(CopilotReference);
    let json = serde_json::to_value(&schema).unwrap();
    assert!(json.get("properties").is_some());
}

#[test]
fn dialect_copilot_config_has_json_schema() {
    use abp_copilot_sdk::dialect::CopilotConfig;
    let schema = schema_for!(CopilotConfig);
    let json = serde_json::to_value(&schema).unwrap();
    let props = &json["properties"];
    assert!(props.get("token").is_some());
    assert!(props.get("base_url").is_some());
}

#[test]
fn dialect_copilot_stream_event_has_json_schema() {
    use abp_copilot_sdk::dialect::CopilotStreamEvent;
    let schema = schema_for!(CopilotStreamEvent);
    let json = serde_json::to_value(&schema).unwrap();
    // Tagged enum produces oneOf
    assert!(json.get("oneOf").is_some());
}
