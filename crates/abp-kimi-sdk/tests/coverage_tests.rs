// SPDX-License-Identifier: MIT OR Apache-2.0
//! Additional coverage tests for the Kimi SDK: request serde, message edge cases,
//! accumulator boundaries, stream chunk mapping, and config validation.

use abp_core::{AgentEventKind, WorkOrderBuilder};
use abp_kimi_sdk::dialect::{
    KimiBuiltinFunction, KimiBuiltinTool, KimiChoice, KimiChunk, KimiChunkDelta, KimiConfig,
    KimiFunctionCall, KimiFunctionDef, KimiMessage, KimiRequest, KimiResponse, KimiResponseMessage,
    KimiRole, KimiTool, KimiToolCall, KimiToolDef, ToolCallAccumulator, from_canonical_model,
    map_response, map_stream_event, map_work_order,
};

// ---------------------------------------------------------------------------
// KimiRequest serde
// ---------------------------------------------------------------------------

#[test]
fn kimi_request_full_serde_roundtrip() {
    let req = KimiRequest {
        model: "moonshot-v1-32k".into(),
        messages: vec![
            KimiMessage {
                role: "system".into(),
                content: Some("Be helpful.".into()),
                tool_call_id: None,
                tool_calls: None,
            },
            KimiMessage {
                role: "user".into(),
                content: Some("Hello".into()),
                tool_call_id: None,
                tool_calls: None,
            },
        ],
        max_tokens: Some(4096),
        temperature: Some(0.7),
        stream: Some(true),
        tools: Some(vec![KimiTool::Function {
            function: KimiFunctionDef {
                name: "search".into(),
                description: "Search the web".into(),
                parameters: serde_json::json!({"type": "object"}),
            },
        }]),
        use_search: Some(true),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: KimiRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, "moonshot-v1-32k");
    assert_eq!(parsed.messages.len(), 2);
    assert_eq!(parsed.max_tokens, Some(4096));
    assert_eq!(parsed.stream, Some(true));
}

// ---------------------------------------------------------------------------
// KimiMessage edge cases
// ---------------------------------------------------------------------------

#[test]
fn kimi_message_omits_none_fields_in_json() {
    let msg = KimiMessage {
        role: "user".into(),
        content: Some("hi".into()),
        tool_call_id: None,
        tool_calls: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(!json.contains("tool_call_id"));
    assert!(!json.contains("tool_calls"));
}

#[test]
fn kimi_message_with_tool_calls_serde_roundtrip() {
    let msg = KimiMessage {
        role: "assistant".into(),
        content: None,
        tool_call_id: None,
        tool_calls: Some(vec![KimiToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: KimiFunctionCall {
                name: "calc".into(),
                arguments: r#"{"x":1}"#.into(),
            },
        }]),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: KimiMessage = serde_json::from_str(&json).unwrap();
    assert!(parsed.tool_calls.is_some());
    assert_eq!(parsed.tool_calls.unwrap().len(), 1);
}

// ---------------------------------------------------------------------------
// map_response edge cases
// ---------------------------------------------------------------------------

#[test]
fn map_response_empty_choices_produces_no_events() {
    let resp = KimiResponse {
        id: "cmpl_empty".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![],
        usage: None,
        refs: None,
    };
    let events = map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn map_response_tool_calls_only_no_content() {
    let resp = KimiResponse {
        id: "cmpl_tc".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![KimiToolCall {
                    id: "call_1".into(),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: "web_search".into(),
                        arguments: r#"{"q":"test"}"#.into(),
                    },
                }]),
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
        refs: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn map_response_null_content_no_tool_calls_produces_no_events() {
    let resp = KimiResponse {
        id: "cmpl_null".into(),
        model: "moonshot-v1-8k".into(),
        choices: vec![KimiChoice {
            index: 0,
            message: KimiResponseMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: None,
    };
    let events = map_response(&resp);
    assert!(events.is_empty());
}

// ---------------------------------------------------------------------------
// KimiChunkDelta default
// ---------------------------------------------------------------------------

#[test]
fn kimi_chunk_delta_default_all_none() {
    let delta = KimiChunkDelta::default();
    assert!(delta.role.is_none());
    assert!(delta.content.is_none());
    assert!(delta.tool_calls.is_none());
}

// ---------------------------------------------------------------------------
// KimiToolCall JSON uses "type" rename
// ---------------------------------------------------------------------------

#[test]
fn kimi_tool_call_json_uses_type_not_call_type() {
    let tc = KimiToolCall {
        id: "call_1".into(),
        call_type: "function".into(),
        function: KimiFunctionCall {
            name: "test".into(),
            arguments: "{}".into(),
        },
    };
    let json = serde_json::to_value(&tc).unwrap();
    assert!(json.get("type").is_some());
    assert!(json.get("call_type").is_none());
}

#[test]
fn kimi_tool_def_json_uses_type_not_tool_type() {
    let def = KimiToolDef {
        tool_type: "function".into(),
        function: KimiFunctionDef {
            name: "test".into(),
            description: "desc".into(),
            parameters: serde_json::json!({}),
        },
    };
    let json = serde_json::to_value(&def).unwrap();
    assert!(json.get("type").is_some());
    assert!(json.get("tool_type").is_none());
}

// ---------------------------------------------------------------------------
// ToolCallAccumulator edge cases
// ---------------------------------------------------------------------------

#[test]
fn tool_call_accumulator_empty_finish_returns_empty() {
    let acc = ToolCallAccumulator::new();
    let events = acc.finish();
    assert!(events.is_empty());
}

// ---------------------------------------------------------------------------
// map_work_order with system prompt
// ---------------------------------------------------------------------------

#[test]
fn map_work_order_with_k1_reasoning_config() {
    let wo = WorkOrderBuilder::new("Solve problem").build();
    let cfg = KimiConfig {
        use_k1_reasoning: Some(true),
        ..KimiConfig::default()
    };
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
}

// ---------------------------------------------------------------------------
// KimiBuiltinTool / KimiBuiltinFunction serde
// ---------------------------------------------------------------------------

#[test]
fn kimi_builtin_function_serde_roundtrip() {
    let func = KimiBuiltinFunction {
        name: "$web_search".into(),
    };
    let json = serde_json::to_string(&func).unwrap();
    let parsed: KimiBuiltinFunction = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, func);
}

#[test]
fn kimi_builtin_tool_json_has_builtin_function_type() {
    let tool = KimiBuiltinTool {
        tool_type: "builtin_function".into(),
        function: KimiBuiltinFunction {
            name: "$browser".into(),
        },
    };
    let json = serde_json::to_value(&tool).unwrap();
    assert_eq!(json["type"], "builtin_function");
    assert_eq!(json["function"]["name"], "$browser");
}

// ---------------------------------------------------------------------------
// KimiResponseMessage serde
// ---------------------------------------------------------------------------

#[test]
fn kimi_response_message_serde_roundtrip() {
    let msg = KimiResponseMessage {
        role: "assistant".into(),
        content: Some("Answer: 42".into()),
        tool_calls: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: KimiResponseMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.content.as_deref(), Some("Answer: 42"));
}

// ---------------------------------------------------------------------------
// map_stream_event no choices
// ---------------------------------------------------------------------------

#[test]
fn map_stream_event_empty_choices_produces_no_events() {
    let chunk = KimiChunk {
        id: "ch_noc".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "moonshot-v1-8k".into(),
        choices: vec![],
        usage: None,
        refs: None,
    };
    let events = map_stream_event(&chunk);
    assert!(events.is_empty());
}

// ---------------------------------------------------------------------------
// KimiConfig default values
// ---------------------------------------------------------------------------

#[test]
fn kimi_config_default_temperature_is_none() {
    let cfg = KimiConfig::default();
    assert!(cfg.temperature.is_none());
}

#[test]
fn kimi_config_default_use_k1_reasoning_is_none() {
    let cfg = KimiConfig::default();
    assert!(cfg.use_k1_reasoning.is_none());
}

// ---------------------------------------------------------------------------
// from_canonical_model
// ---------------------------------------------------------------------------

#[test]
fn from_canonical_model_strips_moonshot_prefix() {
    assert_eq!(
        from_canonical_model("moonshot/moonshot-v1-8k"),
        "moonshot-v1-8k"
    );
}

#[test]
fn from_canonical_model_other_prefix_passes_through() {
    assert_eq!(from_canonical_model("openai/gpt-4o"), "openai/gpt-4o");
}

// ---------------------------------------------------------------------------
// KimiRole Display
// ---------------------------------------------------------------------------

#[test]
fn kimi_role_display_matches_serde() {
    let roles = [
        (KimiRole::System, "system"),
        (KimiRole::User, "user"),
        (KimiRole::Assistant, "assistant"),
        (KimiRole::Tool, "tool"),
    ];
    for (role, expected) in &roles {
        // Display should match the serde string value
        assert_eq!(role.to_string(), *expected);
        let json = serde_json::to_string(role).unwrap();
        assert_eq!(json, format!("\"{}\"", expected));
    }
}
