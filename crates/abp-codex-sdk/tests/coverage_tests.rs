// SPDX-License-Identifier: MIT OR Apache-2.0
//! Additional coverage tests for the Codex SDK: request serde, stream event variants,
//! content parts, edge cases in mapping, and sandbox configuration.

use abp_codex_sdk::dialect::{
    CodexConfig, CodexContentPart, CodexInputItem, CodexRequest, CodexResponse, CodexResponseItem,
    CodexStreamDelta, CodexStreamEvent, CodexTextFormat, CodexTool, CodexUsage, FileAccess,
    NetworkAccess, ReasoningSummary, SandboxConfig, from_canonical_model, map_response,
    map_stream_event,
};
use abp_core::AgentEventKind;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// CodexRequest serde
// ---------------------------------------------------------------------------

#[test]
fn codex_request_full_serde_roundtrip() {
    let req = CodexRequest {
        model: "codex-mini-latest".into(),
        input: vec![CodexInputItem::Message {
            role: "user".into(),
            content: "Fix the bug".into(),
        }],
        max_output_tokens: Some(8192),
        temperature: Some(0.3),
        tools: vec![CodexTool::CodeInterpreter {}],
        text: Some(CodexTextFormat::Text {}),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: CodexRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, "codex-mini-latest");
    assert_eq!(parsed.max_output_tokens, Some(8192));
    assert_eq!(parsed.temperature, Some(0.3));
    assert_eq!(parsed.tools.len(), 1);
}

#[test]
fn codex_request_with_json_schema_text_format() {
    let req = CodexRequest {
        model: "codex-mini-latest".into(),
        input: vec![],
        max_output_tokens: None,
        temperature: None,
        tools: vec![],
        text: Some(CodexTextFormat::JsonSchema {
            name: "output".into(),
            schema: serde_json::json!({"type": "object"}),
            strict: true,
        }),
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("json_schema"));
    assert!(json.contains("output"));
}

// ---------------------------------------------------------------------------
// CodexContentPart serde
// ---------------------------------------------------------------------------

#[test]
fn codex_content_part_output_text_serde_roundtrip() {
    let part = CodexContentPart::OutputText {
        text: "result".into(),
    };
    let json = serde_json::to_string(&part).unwrap();
    let parsed: CodexContentPart = serde_json::from_str(&json).unwrap();
    let json2 = serde_json::to_string(&parsed).unwrap();
    assert_eq!(json, json2);
}

#[test]
fn codex_content_part_json_has_type_field() {
    let part = CodexContentPart::OutputText { text: "hi".into() };
    let json = serde_json::to_value(&part).unwrap();
    assert_eq!(json["type"], "output_text");
}

// ---------------------------------------------------------------------------
// CodexInputItem serde
// ---------------------------------------------------------------------------

#[test]
fn codex_input_item_message_serde_roundtrip() {
    let item = CodexInputItem::Message {
        role: "user".into(),
        content: "Hello world".into(),
    };
    let json = serde_json::to_string(&item).unwrap();
    let parsed: CodexInputItem = serde_json::from_str(&json).unwrap();
    let CodexInputItem::Message { role, content } = &parsed;
    assert_eq!(role, "user");
    assert_eq!(content, "Hello world");
}

// ---------------------------------------------------------------------------
// map_response edge cases
// ---------------------------------------------------------------------------

#[test]
fn map_response_empty_output_produces_no_events() {
    let resp = CodexResponse {
        id: "r_empty".into(),
        model: "codex-mini-latest".into(),
        output: vec![],
        usage: None,
        status: None,
    };
    let events = map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn map_response_empty_message_content_produces_no_events() {
    let resp = CodexResponse {
        id: "r_no_content".into(),
        model: "codex-mini-latest".into(),
        output: vec![CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![],
        }],
        usage: None,
        status: None,
    };
    let events = map_response(&resp);
    assert!(events.is_empty());
}

// ---------------------------------------------------------------------------
// Stream event serde for all variants
// ---------------------------------------------------------------------------

#[test]
fn stream_event_response_in_progress_serde_roundtrip() {
    let event = CodexStreamEvent::ResponseInProgress {
        response: CodexResponse {
            id: "r1".into(),
            model: "codex-mini-latest".into(),
            output: vec![],
            usage: None,
            status: Some("in_progress".into()),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: CodexStreamEvent = serde_json::from_str(&json).unwrap();
    if let CodexStreamEvent::ResponseInProgress { response } = parsed {
        assert_eq!(response.status.as_deref(), Some("in_progress"));
    } else {
        panic!("expected ResponseInProgress");
    }
}

#[test]
fn stream_event_output_item_added_serde_roundtrip() {
    let event = CodexStreamEvent::OutputItemAdded {
        output_index: 0,
        item: CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "hello".into(),
            }],
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: CodexStreamEvent = serde_json::from_str(&json).unwrap();
    if let CodexStreamEvent::OutputItemAdded { output_index, .. } = parsed {
        assert_eq!(output_index, 0);
    } else {
        panic!("expected OutputItemAdded");
    }
}

#[test]
fn stream_event_error_serde_roundtrip() {
    let event = CodexStreamEvent::Error {
        message: "rate limit".into(),
        code: Some("429".into()),
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: CodexStreamEvent = serde_json::from_str(&json).unwrap();
    if let CodexStreamEvent::Error { message, code } = parsed {
        assert_eq!(message, "rate limit");
        assert_eq!(code.as_deref(), Some("429"));
    } else {
        panic!("expected Error");
    }
}

// ---------------------------------------------------------------------------
// Stream delta all variants
// ---------------------------------------------------------------------------

#[test]
fn stream_delta_function_call_arguments_serde_roundtrip() {
    let delta = CodexStreamDelta::FunctionCallArgumentsDelta {
        delta: r#"{"path":"#.into(),
    };
    let json = serde_json::to_string(&delta).unwrap();
    let parsed: CodexStreamDelta = serde_json::from_str(&json).unwrap();
    let json2 = serde_json::to_string(&parsed).unwrap();
    assert_eq!(json, json2);
}

#[test]
fn stream_delta_reasoning_summary_serde_roundtrip() {
    let delta = CodexStreamDelta::ReasoningSummaryDelta {
        text: "thinking step 1".into(),
    };
    let json = serde_json::to_string(&delta).unwrap();
    let parsed: CodexStreamDelta = serde_json::from_str(&json).unwrap();
    let json2 = serde_json::to_string(&parsed).unwrap();
    assert_eq!(json, json2);
}

// ---------------------------------------------------------------------------
// map_stream_event edge cases
// ---------------------------------------------------------------------------

#[test]
fn map_stream_event_output_item_done_message_produces_assistant_message() {
    let event = CodexStreamEvent::OutputItemDone {
        output_index: 0,
        item: CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "Done.".into(),
            }],
        },
    };
    let events = map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[test]
fn map_stream_event_output_item_done_function_call_output_produces_tool_result() {
    let event = CodexStreamEvent::OutputItemDone {
        output_index: 1,
        item: CodexResponseItem::FunctionCallOutput {
            call_id: "call_done".into(),
            output: "ok".into(),
        },
    };
    let events = map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolResult { .. }));
}

// ---------------------------------------------------------------------------
// CodexUsage serde
// ---------------------------------------------------------------------------

#[test]
fn codex_usage_serde_roundtrip() {
    let usage = CodexUsage {
        input_tokens: 50,
        output_tokens: 25,
        total_tokens: 75,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let parsed: CodexUsage = serde_json::from_str(&json).unwrap();
    let json2 = serde_json::to_string(&parsed).unwrap();
    assert_eq!(json, json2);
}

// ---------------------------------------------------------------------------
// CodexResponse with status
// ---------------------------------------------------------------------------

#[test]
fn codex_response_with_status_serde_roundtrip() {
    let resp = CodexResponse {
        id: "r_status".into(),
        model: "codex-mini-latest".into(),
        output: vec![],
        usage: Some(CodexUsage {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
        }),
        status: Some("completed".into()),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: CodexResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.status.as_deref(), Some("completed"));
    assert_eq!(parsed.usage.unwrap().total_tokens, 15);
}

// ---------------------------------------------------------------------------
// SandboxConfig custom env
// ---------------------------------------------------------------------------

#[test]
fn sandbox_config_custom_env_serde_roundtrip() {
    let mut env = BTreeMap::new();
    env.insert("PATH".into(), "/usr/bin".into());
    env.insert("LANG".into(), "en_US.UTF-8".into());
    let cfg = SandboxConfig {
        env,
        ..SandboxConfig::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: SandboxConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.env.len(), 2);
    assert_eq!(parsed.env["PATH"], "/usr/bin");
}

// ---------------------------------------------------------------------------
// from_canonical_model
// ---------------------------------------------------------------------------

#[test]
fn from_canonical_model_strips_openai_prefix() {
    assert_eq!(
        from_canonical_model("openai/codex-mini-latest"),
        "codex-mini-latest"
    );
}

#[test]
fn from_canonical_model_other_prefix_passes_through() {
    assert_eq!(
        from_canonical_model("anthropic/claude-3"),
        "anthropic/claude-3"
    );
}

// ---------------------------------------------------------------------------
// CodexConfig with custom sandbox
// ---------------------------------------------------------------------------

#[test]
fn codex_config_with_custom_sandbox_serde_roundtrip() {
    let cfg = CodexConfig {
        sandbox: SandboxConfig {
            networking: NetworkAccess::Full,
            file_access: FileAccess::Full,
            timeout_seconds: Some(600),
            memory_mb: Some(2048),
            container_image: Some("node:20".into()),
            env: BTreeMap::new(),
        },
        ..CodexConfig::default()
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: CodexConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.sandbox.timeout_seconds, Some(600));
    assert_eq!(parsed.sandbox.memory_mb, Some(2048));
}

// ---------------------------------------------------------------------------
// ReasoningSummary serde
// ---------------------------------------------------------------------------

#[test]
fn reasoning_summary_serde_roundtrip() {
    let summary = ReasoningSummary {
        text: "Step 1: analyze the problem".into(),
    };
    let json = serde_json::to_string(&summary).unwrap();
    let parsed: ReasoningSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.text, "Step 1: analyze the problem");
}
