// SPDX-License-Identifier: MIT OR Apache-2.0
//! Additional coverage tests for the OpenAI SDK: request serde, JSON field names,
//! edge cases in mapping, and streaming accumulator boundaries.

use abp_core::AgentEventKind;
use abp_openai_sdk::dialect::{
    OpenAIChoice, OpenAIConfig, OpenAIFunctionCall, OpenAIFunctionDef, OpenAIMessage,
    OpenAIRequest, OpenAIResponse, OpenAIToolCall, OpenAIToolDef, OpenAIUsage, ToolChoice,
    ToolChoiceFunctionRef, ToolChoiceMode, map_response, map_work_order,
};
use abp_openai_sdk::response_format::ResponseFormat;
use abp_openai_sdk::streaming::{
    ChatCompletionChunk, ChunkChoice, ChunkDelta, ChunkFunctionCall, ChunkToolCall,
    ToolCallAccumulator, map_chunk,
};

// ---------------------------------------------------------------------------
// OpenAIRequest serde
// ---------------------------------------------------------------------------

#[test]
fn openai_request_full_serde_roundtrip() {
    let req = OpenAIRequest {
        model: "gpt-4o".into(),
        messages: vec![OpenAIMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_calls: None,
            tool_call_id: None,
        }],
        tools: Some(vec![OpenAIToolDef {
            tool_type: "function".into(),
            function: OpenAIFunctionDef {
                name: "bash".into(),
                description: "Run command".into(),
                parameters: serde_json::json!({"type": "object"}),
            },
        }]),
        tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Auto)),
        temperature: Some(0.7),
        max_tokens: Some(4096),
        response_format: Some(ResponseFormat::text()),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: OpenAIRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, "gpt-4o");
    assert_eq!(parsed.messages.len(), 1);
    assert!(parsed.tools.unwrap().len() == 1);
    assert_eq!(parsed.max_tokens, Some(4096));
}

#[test]
fn openai_request_omits_none_fields_in_json() {
    let req = OpenAIRequest {
        model: "gpt-4o".into(),
        messages: vec![],
        tools: None,
        tool_choice: None,
        temperature: None,
        max_tokens: None,
        response_format: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("tools"));
    assert!(!json.contains("tool_choice"));
    assert!(!json.contains("temperature"));
    assert!(!json.contains("max_tokens"));
    assert!(!json.contains("response_format"));
}

// ---------------------------------------------------------------------------
// JSON field name verification
// ---------------------------------------------------------------------------

#[test]
fn openai_tool_call_json_uses_type_not_call_type() {
    let tc = OpenAIToolCall {
        id: "call_1".into(),
        call_type: "function".into(),
        function: OpenAIFunctionCall {
            name: "test".into(),
            arguments: "{}".into(),
        },
    };
    let json = serde_json::to_value(&tc).unwrap();
    assert!(json.get("type").is_some(), "should have 'type' key");
    assert!(
        json.get("call_type").is_none(),
        "should not have 'call_type' key"
    );
    assert_eq!(json["type"], "function");
}

#[test]
fn openai_tool_def_json_uses_type_not_tool_type() {
    let def = OpenAIToolDef {
        tool_type: "function".into(),
        function: OpenAIFunctionDef {
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
// ToolChoice edge cases
// ---------------------------------------------------------------------------

#[test]
fn tool_choice_required_mode_roundtrip() {
    let choice = ToolChoice::Mode(ToolChoiceMode::Required);
    let json = serde_json::to_string(&choice).unwrap();
    assert_eq!(json, "\"required\"");
    let parsed: ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, choice);
}

// ---------------------------------------------------------------------------
// Usage serde
// ---------------------------------------------------------------------------

#[test]
fn openai_usage_serde_roundtrip() {
    let usage = OpenAIUsage {
        prompt_tokens: 100,
        completion_tokens: 42,
        total_tokens: 142,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let parsed: OpenAIUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.prompt_tokens, 100);
    assert_eq!(parsed.completion_tokens, 42);
    assert_eq!(parsed.total_tokens, 142);
}

// ---------------------------------------------------------------------------
// map_response edge cases
// ---------------------------------------------------------------------------

#[test]
fn map_response_empty_choices_produces_no_events() {
    let resp = OpenAIResponse {
        id: "chatcmpl-empty".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
    };
    let events = map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn map_response_null_content_and_no_tool_calls_produces_no_events() {
    let resp = OpenAIResponse {
        id: "chatcmpl-null".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: OpenAIMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
    };
    let events = map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn map_response_with_only_tool_calls_no_content() {
    let resp = OpenAIResponse {
        id: "chatcmpl-tc-only".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: OpenAIMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![OpenAIToolCall {
                    id: "call_only".into(),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: "read_file".into(),
                        arguments: r#"{"path":"a.rs"}"#.into(),
                    },
                }]),
                tool_call_id: None,
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
    };
    let events = map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::ToolCall { .. }));
}

// ---------------------------------------------------------------------------
// map_work_order with config parameters
// ---------------------------------------------------------------------------

#[test]
fn map_work_order_applies_temperature_from_config() {
    let wo = abp_core::WorkOrderBuilder::new("test").build();
    let cfg = OpenAIConfig {
        temperature: Some(0.5),
        ..OpenAIConfig::default()
    };
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.temperature, Some(0.5));
}

#[test]
fn map_work_order_applies_max_tokens_from_config() {
    let wo = abp_core::WorkOrderBuilder::new("test").build();
    let cfg = OpenAIConfig {
        max_tokens: Some(2048),
        ..OpenAIConfig::default()
    };
    let req = map_work_order(&wo, &cfg);
    assert_eq!(req.max_tokens, Some(2048));
}

// ---------------------------------------------------------------------------
// Streaming edge cases
// ---------------------------------------------------------------------------

#[test]
fn chunk_delta_default_has_all_none() {
    let delta = ChunkDelta::default();
    assert!(delta.role.is_none());
    assert!(delta.content.is_none());
    assert!(delta.tool_calls.is_none());
}

#[test]
fn map_chunk_empty_choices_produces_no_events() {
    let chunk = ChatCompletionChunk {
        id: "chatcmpl-noc".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
    };
    let events = map_chunk(&chunk);
    assert!(events.is_empty());
}

#[test]
fn tool_call_accumulator_finish_empty_returns_empty() {
    let acc = ToolCallAccumulator::new();
    let events = acc.finish();
    assert!(events.is_empty());
}

#[test]
fn map_chunk_with_tool_call_fragments_not_emitted_directly() {
    let chunk = ChatCompletionChunk {
        id: "chatcmpl-tc-frag".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: None,
                content: None,
                tool_calls: Some(vec![ChunkToolCall {
                    index: 0,
                    id: Some("call_1".into()),
                    call_type: Some("function".into()),
                    function: Some(ChunkFunctionCall {
                        name: Some("bash".into()),
                        arguments: Some(r#"{"cmd":"ls"}"#.into()),
                    }),
                }]),
            },
            finish_reason: None,
        }],
        usage: None,
    };
    // map_chunk doesn't emit tool calls directly — they go through accumulator
    let events = map_chunk(&chunk);
    assert!(events.is_empty());
}

// ---------------------------------------------------------------------------
// OpenAIFunctionDef standalone serde
// ---------------------------------------------------------------------------

#[test]
fn openai_function_def_serde_roundtrip() {
    let def = OpenAIFunctionDef {
        name: "get_weather".into(),
        description: "Get current weather for a location".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "location": {"type": "string"},
                "unit": {"type": "string", "enum": ["celsius", "fahrenheit"]}
            },
            "required": ["location"]
        }),
    };
    let json = serde_json::to_string(&def).unwrap();
    let parsed: OpenAIFunctionDef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, def);
}

// ---------------------------------------------------------------------------
// ToolChoiceFunctionRef standalone serde
// ---------------------------------------------------------------------------

#[test]
fn tool_choice_function_ref_serde_roundtrip() {
    let func_ref = ToolChoiceFunctionRef {
        name: "bash".into(),
    };
    let json = serde_json::to_string(&func_ref).unwrap();
    let parsed: ToolChoiceFunctionRef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, func_ref);
}
