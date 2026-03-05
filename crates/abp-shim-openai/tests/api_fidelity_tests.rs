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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! API fidelity tests — verify shim types faithfully mirror the OpenAI REST API
//! wire format and that IR conversions preserve semantics.

use abp_core::ir::{IrContentBlock, IrRole};
use abp_core::{AgentEvent, AgentEventKind};
use abp_shim_openai::{
    events_to_stream_events, ir_to_messages, ir_usage_to_usage, messages_to_ir, mock_receipt,
    receipt_to_response, request_to_work_order, tools_to_ir,
};
use abp_shim_openai::{
    ChatCompletionRequest, ChatCompletionResponse, Choice, FunctionCall, Message, ResponseFormat,
    Role, StreamEvent, Tool, ToolCall, ToolChoice, ToolChoiceFunctionRef, ToolChoiceMode, Usage,
};
use chrono::Utc;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// 1. Type fidelity — wire-format field names match OpenAI API
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_json_has_correct_field_names() {
    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("hi")])
        .temperature(0.5)
        .max_tokens(128)
        .stream(true)
        .build();
    let v = serde_json::to_value(&req).unwrap();
    // OpenAI expects these exact top-level keys
    assert!(v.get("model").is_some());
    assert!(v.get("messages").is_some());
    assert!(v.get("temperature").is_some());
    assert!(v.get("max_tokens").is_some());
    assert!(v.get("stream").is_some());
}

#[test]
fn response_has_required_openai_fields() {
    let resp = ChatCompletionResponse {
        id: "chatcmpl-abc".into(),
        object: "chat.completion".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![Choice {
            index: 0,
            message: Message::assistant("ok"),
            finish_reason: Some("stop".into()),
        }],
        usage: Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
    };
    let v = serde_json::to_value(&resp).unwrap();
    for key in ["id", "object", "created", "model", "choices", "usage"] {
        assert!(v.get(key).is_some(), "missing required field: {key}");
    }
}

#[test]
fn choice_json_has_index_message_finish_reason() {
    let choice = Choice {
        index: 0,
        message: Message::assistant("hi"),
        finish_reason: Some("stop".into()),
    };
    let v = serde_json::to_value(&choice).unwrap();
    assert!(v.get("index").is_some());
    assert!(v.get("message").is_some());
    assert!(v.get("finish_reason").is_some());
}

#[test]
fn message_json_has_role_and_content() {
    let msg = Message::user("hello");
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["role"], "user");
    assert_eq!(v["content"], "hello");
    // tool_calls and tool_call_id should be omitted when None
    assert!(v.get("tool_calls").is_none());
    assert!(v.get("tool_call_id").is_none());
}

#[test]
fn tool_call_type_field_serializes_as_type() {
    let tc = ToolCall {
        id: "call_1".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "f".into(),
            arguments: "{}".into(),
        },
    };
    let v = serde_json::to_value(&tc).unwrap();
    // OpenAI wire format uses "type", not "call_type"
    assert_eq!(v["type"], "function");
    assert!(v.get("call_type").is_none());
}

#[test]
fn usage_json_field_names_match_openai() {
    let usage = Usage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    let v = serde_json::to_value(&usage).unwrap();
    assert_eq!(v["prompt_tokens"], 100);
    assert_eq!(v["completion_tokens"], 50);
    assert_eq!(v["total_tokens"], 150);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Serialization fidelity — deserialize from realistic OpenAI JSON
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deserialize_realistic_openai_response() {
    let json = json!({
        "id": "chatcmpl-9abc123",
        "object": "chat.completion",
        "created": 1700000000,
        "model": "gpt-4o-2024-05-13",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Hello! How can I help you today?"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 12,
            "completion_tokens": 8,
            "total_tokens": 20
        }
    });
    let resp: ChatCompletionResponse = serde_json::from_value(json).unwrap();
    assert_eq!(resp.id, "chatcmpl-9abc123");
    assert_eq!(resp.choices[0].message.role, Role::Assistant);
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Hello! How can I help you today?")
    );
    assert_eq!(resp.usage.unwrap().total_tokens, 20);
}

#[test]
fn deserialize_openai_response_with_tool_calls() {
    let json = json!({
        "id": "chatcmpl-tool1",
        "object": "chat.completion",
        "created": 1700000001,
        "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_abc123",
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "arguments": "{\"location\":\"San Francisco\",\"unit\":\"celsius\"}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {
            "prompt_tokens": 50,
            "completion_tokens": 30,
            "total_tokens": 80
        }
    });
    let resp: ChatCompletionResponse = serde_json::from_value(json).unwrap();
    assert!(resp.choices[0].message.content.is_none());
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs[0].id, "call_abc123");
    assert_eq!(tcs[0].call_type, "function");
    assert_eq!(tcs[0].function.name, "get_weather");
    assert!(tcs[0].function.arguments.contains("San Francisco"));
}

#[test]
fn deserialize_streaming_chunk_with_delta() {
    let json = json!({
        "id": "chatcmpl-stream1",
        "object": "chat.completion.chunk",
        "created": 1700000002,
        "model": "gpt-4o",
        "choices": [{
            "index": 0,
            "delta": {
                "role": "assistant",
                "content": "Hello"
            },
            "finish_reason": null
        }]
    });
    let chunk: StreamEvent = serde_json::from_value(json).unwrap();
    assert_eq!(chunk.object, "chat.completion.chunk");
    assert_eq!(chunk.choices[0].delta.role.as_deref(), Some("assistant"));
    assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hello"));
    assert!(chunk.choices[0].finish_reason.is_none());
}

#[test]
fn tool_choice_none_serializes_as_string() {
    let tc = ToolChoice::Mode(ToolChoiceMode::None);
    let v = serde_json::to_value(&tc).unwrap();
    assert_eq!(v, json!("none"));
}

#[test]
fn tool_choice_auto_serializes_as_string() {
    let tc = ToolChoice::Mode(ToolChoiceMode::Auto);
    let v = serde_json::to_value(&tc).unwrap();
    assert_eq!(v, json!("auto"));
}

#[test]
fn tool_choice_required_serializes_as_string() {
    let tc = ToolChoice::Mode(ToolChoiceMode::Required);
    let v = serde_json::to_value(&tc).unwrap();
    assert_eq!(v, json!("required"));
}

#[test]
fn tool_choice_specific_function_serializes_as_object() {
    let tc = ToolChoice::Function {
        tool_type: "function".into(),
        function: ToolChoiceFunctionRef {
            name: "get_weather".into(),
        },
    };
    let v = serde_json::to_value(&tc).unwrap();
    assert_eq!(v["type"], "function");
    assert_eq!(v["function"]["name"], "get_weather");
}

#[test]
fn response_format_json_object_serializes_correctly() {
    let rf = ResponseFormat::json_object();
    let v = serde_json::to_value(&rf).unwrap();
    assert_eq!(v["type"], "json_object");
}

#[test]
fn response_format_json_schema_has_strict_and_schema() {
    let rf = ResponseFormat::json_schema(
        "person",
        json!({"type": "object", "properties": {"name": {"type": "string"}}}),
    );
    let v = serde_json::to_value(&rf).unwrap();
    assert_eq!(v["type"], "json_schema");
    assert_eq!(v["json_schema"]["name"], "person");
    assert_eq!(v["json_schema"]["strict"], true);
    assert!(v["json_schema"]["schema"].is_object());
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Conversion fidelity — IR roundtrips preserve semantics
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn user_message_ir_roundtrip() {
    let msgs = vec![Message::user("What is 2+2?")];
    let ir = messages_to_ir(&msgs);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[0].text_content(), "What is 2+2?");
    let back = ir_to_messages(&ir);
    assert_eq!(back[0].role, Role::User);
    assert_eq!(back[0].content.as_deref(), Some("What is 2+2?"));
}

#[test]
fn assistant_message_ir_roundtrip() {
    let msgs = vec![Message::assistant("The answer is 4.")];
    let ir = messages_to_ir(&msgs);
    assert_eq!(ir.messages[0].role, IrRole::Assistant);
    let back = ir_to_messages(&ir);
    assert_eq!(back[0].role, Role::Assistant);
    assert_eq!(back[0].content.as_deref(), Some("The answer is 4."));
}

#[test]
fn system_message_ir_roundtrip() {
    let msgs = vec![Message::system("You are a math tutor.")];
    let ir = messages_to_ir(&msgs);
    assert_eq!(ir.messages[0].role, IrRole::System);
    let back = ir_to_messages(&ir);
    assert_eq!(back[0].role, Role::System);
    assert_eq!(back[0].content.as_deref(), Some("You are a math tutor."));
}

#[test]
fn tool_call_converts_to_ir_tool_use() {
    let msgs = vec![Message::assistant_with_tool_calls(vec![ToolCall {
        id: "call_99".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "calculator".into(),
            arguments: r#"{"expr":"2+2"}"#.into(),
        },
    }])];
    let ir = messages_to_ir(&msgs);
    let blocks = ir.messages[0].tool_use_blocks();
    assert_eq!(blocks.len(), 1);
    match &blocks[0] {
        IrContentBlock::ToolUse { id, name, .. } => {
            assert_eq!(id, "call_99");
            assert_eq!(name, "calculator");
        }
        _ => panic!("expected ToolUse block"),
    }
}

#[test]
fn tool_result_converts_to_ir_tool_result() {
    let msgs = vec![Message::tool("call_99", "4")];
    let ir = messages_to_ir(&msgs);
    assert_eq!(ir.messages[0].role, IrRole::Tool);
    let back = ir_to_messages(&ir);
    assert_eq!(back[0].role, Role::Tool);
    assert_eq!(back[0].tool_call_id.as_deref(), Some("call_99"));
    assert_eq!(back[0].content.as_deref(), Some("4"));
}

#[test]
fn multi_turn_conversation_roundtrip() {
    let msgs = vec![
        Message::system("You are helpful."),
        Message::user("What is Rust?"),
        Message::assistant("Rust is a systems programming language."),
        Message::user("What about Go?"),
        Message::assistant("Go is a concurrent programming language."),
    ];
    let ir = messages_to_ir(&msgs);
    assert_eq!(ir.len(), 5);
    let back = ir_to_messages(&ir);
    assert_eq!(back.len(), 5);
    assert_eq!(back[0].role, Role::System);
    assert_eq!(back[1].role, Role::User);
    assert_eq!(back[2].role, Role::Assistant);
    assert_eq!(back[3].role, Role::User);
    assert_eq!(back[4].role, Role::Assistant);
    assert_eq!(
        back[2].content.as_deref(),
        Some("Rust is a systems programming language.")
    );
    assert_eq!(
        back[4].content.as_deref(),
        Some("Go is a concurrent programming language.")
    );
}

#[test]
fn tool_definitions_roundtrip_through_ir() {
    let tools = vec![Tool::function(
        "search",
        "Search the web",
        json!({"type": "object", "properties": {"q": {"type": "string"}}, "required": ["q"]}),
    )];
    let ir_defs = tools_to_ir(&tools);
    assert_eq!(ir_defs.len(), 1);
    assert_eq!(ir_defs[0].name, "search");
    assert_eq!(ir_defs[0].description, "Search the web");
    assert_eq!(ir_defs[0].parameters["required"], json!(["q"]));
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_messages_array_produces_empty_ir() {
    let ir = messages_to_ir(&[]);
    assert!(ir.is_empty());
}

#[test]
fn message_with_null_content_deserializes() {
    let json = json!({"role": "assistant", "content": null});
    let msg: Message = serde_json::from_value(json).unwrap();
    assert_eq!(msg.role, Role::Assistant);
    assert!(msg.content.is_none());
}

#[test]
fn multiple_tool_calls_in_single_message() {
    let msg = Message::assistant_with_tool_calls(vec![
        ToolCall {
            id: "call_a".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"a.rs"}"#.into(),
            },
        },
        ToolCall {
            id: "call_b".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"b.rs"}"#.into(),
            },
        },
        ToolCall {
            id: "call_c".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "write_file".into(),
                arguments: r#"{"path":"c.rs","content":"fn main(){}"}"#.into(),
            },
        },
    ]);
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["tool_calls"].as_array().unwrap().len(), 3);
    // Verify IR conversion preserves all three
    let ir = messages_to_ir(&[msg]);
    assert_eq!(ir.messages[0].tool_use_blocks().len(), 3);
}

#[test]
fn streaming_vs_non_streaming_request_flag() {
    let non_stream = ChatCompletionRequest::builder()
        .messages(vec![Message::user("hi")])
        .build();
    assert!(non_stream.stream.is_none());
    let v = serde_json::to_value(&non_stream).unwrap();
    assert!(v.get("stream").is_none());

    let stream = ChatCompletionRequest::builder()
        .messages(vec![Message::user("hi")])
        .stream(true)
        .build();
    assert_eq!(stream.stream, Some(true));
    let v = serde_json::to_value(&stream).unwrap();
    assert_eq!(v["stream"], true);
}

#[test]
fn temperature_top_p_max_tokens_preserved_in_work_order() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .temperature(0.3)
        .max_tokens(512)
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("temperature"),
        Some(&serde_json::Value::from(0.3))
    );
    assert_eq!(
        wo.config.vendor.get("max_tokens"),
        Some(&serde_json::Value::from(512))
    );
}

#[test]
fn stream_event_tool_call_delta_has_correct_shape() {
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "exec".into(),
            tool_use_id: Some("call_x".into()),
            parent_tool_use_id: None,
            input: json!({"cmd": "ls"}),
        },
        ext: None,
    }];
    let chunks = events_to_stream_events(&events, "gpt-4o");
    // Tool chunk + stop chunk
    assert_eq!(chunks.len(), 2);
    let delta = &chunks[0].choices[0].delta;
    assert!(delta.content.is_none());
    let stc = &delta.tool_calls.as_ref().unwrap()[0];
    assert_eq!(stc.id.as_deref(), Some("call_x"));
    assert_eq!(stc.call_type.as_deref(), Some("function"));
    assert_eq!(stc.function.as_ref().unwrap().name.as_deref(), Some("exec"));
}

#[test]
fn receipt_with_mixed_events_produces_correct_finish_reason() {
    // Text only → stop
    let r1 = mock_receipt(vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: "hi".into() },
        ext: None,
    }]);
    assert_eq!(
        receipt_to_response(&r1, "gpt-4o").choices[0]
            .finish_reason
            .as_deref(),
        Some("stop")
    );

    // Tool call → tool_calls
    let r2 = mock_receipt(vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "f".into(),
            tool_use_id: Some("c".into()),
            parent_tool_use_id: None,
            input: json!({}),
        },
        ext: None,
    }]);
    assert_eq!(
        receipt_to_response(&r2, "gpt-4o").choices[0]
            .finish_reason
            .as_deref(),
        Some("tool_calls")
    );
}

#[test]
fn ir_usage_maps_correctly() {
    let ir = abp_core::ir::IrUsage::from_io(1000, 500);
    let usage = ir_usage_to_usage(&ir);
    assert_eq!(usage.prompt_tokens, 1000);
    assert_eq!(usage.completion_tokens, 500);
    assert_eq!(usage.total_tokens, 1500);
}

#[test]
fn tool_type_field_always_function_on_wire() {
    let tool = Tool::function("t", "desc", json!({}));
    let v = serde_json::to_value(&tool).unwrap();
    // OpenAI uses "type" not "tool_type"
    assert_eq!(v["type"], "function");
    assert!(v.get("tool_type").is_none());
}

#[test]
fn stream_stop_chunk_has_empty_delta() {
    let chunks = events_to_stream_events(&[], "gpt-4o");
    assert_eq!(chunks.len(), 1);
    let stop = &chunks[0];
    assert_eq!(stop.choices[0].finish_reason.as_deref(), Some("stop"));
    assert!(stop.choices[0].delta.content.is_none());
    assert!(stop.choices[0].delta.role.is_none());
    assert!(stop.choices[0].delta.tool_calls.is_none());
}

#[test]
fn request_deserialized_from_external_json() {
    // Simulate what a real OpenAI SDK client would send
    let json = json!({
        "model": "gpt-4o",
        "messages": [
            {"role": "system", "content": "You are a poet."},
            {"role": "user", "content": "Write a haiku about Rust."}
        ],
        "temperature": 0.9,
        "max_tokens": 64,
        "stream": false
    });
    let req: ChatCompletionRequest = serde_json::from_value(json).unwrap();
    assert_eq!(req.model, "gpt-4o");
    assert_eq!(req.messages.len(), 2);
    assert_eq!(req.messages[0].role, Role::System);
    assert_eq!(req.temperature, Some(0.9));
    assert_eq!(req.max_tokens, Some(64));
    assert_eq!(req.stream, Some(false));
}
