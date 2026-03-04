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
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Deep surface-area tests for the OpenAI shim — validates that ABP faithfully
//! mirrors the OpenAI Chat Completions API wire format, conversions, streaming,
//! function calling, message roles, model names, parameters, error handling,
//! client configuration, and serialization fidelity.

use abp_core::ir::{IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, UsageNormalized};
use abp_shim_openai::{
    ChatCompletionRequest, ChatCompletionResponse, Choice, Delta, FunctionCall, FunctionDef,
    Message, OpenAiClient, ProcessFn, ResponseFormat, Role, ShimError, StreamChoice, StreamEvent,
    Tool, ToolCall, ToolChoice, ToolChoiceFunctionRef, ToolChoiceMode, Usage,
};
use abp_shim_openai::{
    events_to_stream_events, ir_to_messages, ir_usage_to_usage, messages_to_ir, mock_receipt,
    mock_receipt_with_usage, receipt_to_response, request_to_ir, request_to_work_order,
    tools_to_ir,
};
use chrono::Utc;
use serde_json::json;
use tokio_stream::StreamExt;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn make_processor(events: Vec<AgentEvent>) -> ProcessFn {
    Box::new(move |_wo| mock_receipt(events.clone()))
}

fn make_processor_with_usage(events: Vec<AgentEvent>, usage: UsageNormalized) -> ProcessFn {
    Box::new(move |_wo| mock_receipt_with_usage(events.clone(), usage.clone()))
}

fn assistant_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: text.to_string(),
        },
        ext: None,
    }
}

fn delta_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: text.to_string(),
        },
        ext: None,
    }
}

fn tool_call_event(name: &str, id: &str, input: serde_json::Value) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: name.to_string(),
            tool_use_id: Some(id.to_string()),
            parent_tool_use_id: None,
            input,
        },
        ext: None,
    }
}

fn error_event(msg: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: msg.to_string(),
            error_code: None,
        },
        ext: None,
    }
}

fn sample_usage() -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(150),
        output_tokens: Some(75),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: None,
    }
}

fn weather_tool() -> Tool {
    Tool::function(
        "get_weather",
        "Get current weather",
        json!({
            "type": "object",
            "properties": {
                "location": {"type": "string"},
                "unit": {"type": "string", "enum": ["celsius", "fahrenheit"]}
            },
            "required": ["location"]
        }),
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Chat completions request — exact OpenAI format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t01_request_serializes_all_fields() {
    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![
            Message::system("You are helpful."),
            Message::user("Hello"),
        ])
        .temperature(0.7)
        .max_tokens(1024)
        .stream(true)
        .stop(vec!["END".into()])
        .build();

    let v = serde_json::to_value(&req).unwrap();
    assert_eq!(v["model"], "gpt-4o");
    assert!(v["messages"].is_array());
    assert_eq!(v["temperature"], 0.7);
    assert_eq!(v["max_tokens"], 1024);
    assert_eq!(v["stream"], true);
    assert_eq!(v["stop"][0], "END");
}

#[test]
fn t02_request_omits_none_fields() {
    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("Hi")])
        .build();

    let v = serde_json::to_value(&req).unwrap();
    assert!(v.get("temperature").is_none());
    assert!(v.get("max_tokens").is_none());
    assert!(v.get("stream").is_none());
    assert!(v.get("tools").is_none());
    assert!(v.get("tool_choice").is_none());
    assert!(v.get("stop").is_none());
    assert!(v.get("response_format").is_none());
}

#[test]
fn t03_request_json_roundtrip() {
    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("test")])
        .temperature(0.5)
        .max_tokens(512)
        .build();

    let json_str = serde_json::to_string(&req).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed["model"], "gpt-4o");
    assert_eq!(parsed["temperature"], 0.5);
}

#[test]
fn t04_request_builder_defaults_model_to_gpt4o() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .build();

    assert_eq!(req.model, "gpt-4o");
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Chat completions response — parse OpenAI format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t05_response_has_all_required_fields() {
    let resp = ChatCompletionResponse {
        id: "chatcmpl-test123".into(),
        object: "chat.completion".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![Choice {
            index: 0,
            message: Message::assistant("Hello!"),
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
        assert!(v.get(key).is_some(), "missing key: {key}");
    }
}

#[test]
fn t06_response_object_is_chat_completion() {
    let events = vec![assistant_event("ok")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.object, "chat.completion");
}

#[test]
fn t07_response_id_starts_with_chatcmpl() {
    let events = vec![assistant_event("ok")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert!(resp.id.starts_with("chatcmpl-"));
}

#[test]
fn t08_response_created_is_nonzero() {
    let events = vec![assistant_event("ok")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert!(resp.created > 0);
}

#[test]
fn t09_response_choices_has_one_entry() {
    let events = vec![assistant_event("Hello")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.choices.len(), 1);
    assert_eq!(resp.choices[0].index, 0);
}

#[test]
fn t10_response_finish_reason_stop_for_text() {
    let events = vec![assistant_event("done")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Streaming response — SSE event format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t11_stream_events_object_is_chunk() {
    let events = vec![delta_event("hi")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert_eq!(stream[0].object, "chat.completion.chunk");
}

#[test]
fn t12_stream_events_end_with_stop() {
    let events = vec![delta_event("hello")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    let last = stream.last().unwrap();
    assert_eq!(last.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn t13_stream_multiple_deltas() {
    let events = vec![
        delta_event("Hel"),
        delta_event("lo "),
        delta_event("wor"),
        delta_event("ld!"),
    ];
    let stream = events_to_stream_events(&events, "gpt-4o");
    // 4 deltas + 1 stop
    assert_eq!(stream.len(), 5);
    assert_eq!(stream[0].choices[0].delta.content.as_deref(), Some("Hel"));
    assert_eq!(stream[3].choices[0].delta.content.as_deref(), Some("ld!"));
}

#[test]
fn t14_stream_assistant_message_has_role() {
    let events = vec![assistant_event("Hello")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert_eq!(
        stream[0].choices[0].delta.role.as_deref(),
        Some("assistant")
    );
}

#[test]
fn t15_stream_delta_no_role() {
    let events = vec![delta_event("hi")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert!(stream[0].choices[0].delta.role.is_none());
}

#[test]
fn t16_stream_model_preserved() {
    let events = vec![delta_event("x")];
    let stream = events_to_stream_events(&events, "gpt-4-turbo");
    assert_eq!(stream[0].model, "gpt-4-turbo");
}

#[tokio::test]
async fn t17_streaming_via_client() {
    let events = vec![delta_event("A"), delta_event("B")];
    let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .stream(true)
        .build();

    let stream = client
        .chat()
        .completions()
        .create_stream(req)
        .await
        .unwrap();
    let chunks: Vec<StreamEvent> = stream.collect().await;
    assert_eq!(chunks.len(), 3); // 2 deltas + stop
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Function calling — tool/function definitions and calls
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t18_tool_function_constructor() {
    let tool = weather_tool();
    assert_eq!(tool.tool_type, "function");
    assert_eq!(tool.function.name, "get_weather");
    assert!(!tool.function.description.is_empty());
}

#[test]
fn t19_tool_serializes_with_type_field() {
    let tool = weather_tool();
    let v = serde_json::to_value(&tool).unwrap();
    assert_eq!(v["type"], "function");
    assert_eq!(v["function"]["name"], "get_weather");
    assert!(v["function"]["parameters"]["properties"]["location"].is_object());
}

#[test]
fn t20_tool_call_response_finish_reason() {
    let events = vec![tool_call_event(
        "get_weather",
        "call_w1",
        json!({"location": "NYC"}),
    )];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
}

#[test]
fn t21_tool_call_has_id_type_function() {
    let events = vec![tool_call_event("search", "call_s1", json!({"q": "rust"}))];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let tc = &resp.choices[0].message.tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "call_s1");
    assert_eq!(tc.call_type, "function");
    assert_eq!(tc.function.name, "search");
}

#[test]
fn t22_tool_call_arguments_are_json_string() {
    let events = vec![tool_call_event("calc", "call_c1", json!({"expr": "2+2"}))];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let tc = &resp.choices[0].message.tool_calls.as_ref().unwrap()[0];
    let parsed: serde_json::Value = serde_json::from_str(&tc.function.arguments).unwrap();
    assert_eq!(parsed["expr"], "2+2");
}

#[test]
fn t23_multiple_tool_calls_in_single_response() {
    let events = vec![
        tool_call_event("read_file", "call_1", json!({"path": "a.rs"})),
        tool_call_event("read_file", "call_2", json!({"path": "b.rs"})),
        tool_call_event("read_file", "call_3", json!({"path": "c.rs"})),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs.len(), 3);
    assert_eq!(tcs[0].id, "call_1");
    assert_eq!(tcs[2].id, "call_3");
}

#[test]
fn t24_stream_tool_call_event() {
    let events = vec![tool_call_event(
        "get_weather",
        "call_w1",
        json!({"location": "London"}),
    )];
    let stream = events_to_stream_events(&events, "gpt-4o");
    let tc = &stream[0].choices[0].delta.tool_calls.as_ref().unwrap()[0];
    assert_eq!(
        tc.function.as_ref().unwrap().name.as_deref(),
        Some("get_weather")
    );
    assert_eq!(tc.id.as_deref(), Some("call_w1"));
    assert_eq!(tc.call_type.as_deref(), Some("function"));
}

#[test]
fn t25_tool_choice_mode_auto_serialization() {
    let tc = ToolChoice::Mode(ToolChoiceMode::Auto);
    let v = serde_json::to_value(&tc).unwrap();
    assert_eq!(v, json!("auto"));
}

#[test]
fn t26_tool_choice_mode_none_serialization() {
    let tc = ToolChoice::Mode(ToolChoiceMode::None);
    let v = serde_json::to_value(&tc).unwrap();
    assert_eq!(v, json!("none"));
}

#[test]
fn t27_tool_choice_mode_required_serialization() {
    let tc = ToolChoice::Mode(ToolChoiceMode::Required);
    let v = serde_json::to_value(&tc).unwrap();
    assert_eq!(v, json!("required"));
}

#[test]
fn t28_tool_choice_function_serialization() {
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

// ═══════════════════════════════════════════════════════════════════════════
// 5. System/user/assistant/tool messages — role handling
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t29_system_message_constructor() {
    let msg = Message::system("You are a pirate.");
    assert_eq!(msg.role, Role::System);
    assert_eq!(msg.content.as_deref(), Some("You are a pirate."));
    assert!(msg.tool_calls.is_none());
    assert!(msg.tool_call_id.is_none());
}

#[test]
fn t30_user_message_constructor() {
    let msg = Message::user("Hello there");
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.content.as_deref(), Some("Hello there"));
}

#[test]
fn t31_assistant_message_constructor() {
    let msg = Message::assistant("General Kenobi");
    assert_eq!(msg.role, Role::Assistant);
    assert_eq!(msg.content.as_deref(), Some("General Kenobi"));
    assert!(msg.tool_calls.is_none());
}

#[test]
fn t32_assistant_with_tool_calls_constructor() {
    let tc = ToolCall {
        id: "call_1".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "search".into(),
            arguments: r#"{"q":"test"}"#.into(),
        },
    };
    let msg = Message::assistant_with_tool_calls(vec![tc]);
    assert_eq!(msg.role, Role::Assistant);
    assert!(msg.content.is_none());
    assert_eq!(msg.tool_calls.as_ref().unwrap().len(), 1);
}

#[test]
fn t33_tool_message_constructor() {
    let msg = Message::tool("call_abc", "result value");
    assert_eq!(msg.role, Role::Tool);
    assert_eq!(msg.content.as_deref(), Some("result value"));
    assert_eq!(msg.tool_call_id.as_deref(), Some("call_abc"));
}

#[test]
fn t34_role_serialization() {
    assert_eq!(serde_json::to_value(Role::System).unwrap(), json!("system"));
    assert_eq!(serde_json::to_value(Role::User).unwrap(), json!("user"));
    assert_eq!(
        serde_json::to_value(Role::Assistant).unwrap(),
        json!("assistant")
    );
    assert_eq!(serde_json::to_value(Role::Tool).unwrap(), json!("tool"));
}

#[test]
fn t35_message_role_in_json() {
    let msg = Message::system("test");
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["role"], "system");

    let msg = Message::user("test");
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["role"], "user");
}

#[test]
fn t36_tool_message_serialization_has_tool_call_id() {
    let msg = Message::tool("call_123", "output");
    let v = serde_json::to_value(&msg).unwrap();
    assert_eq!(v["role"], "tool");
    assert_eq!(v["tool_call_id"], "call_123");
    assert_eq!(v["content"], "output");
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Model names — gpt-4, gpt-4o, gpt-3.5-turbo, o1, etc.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t37_model_gpt4o_preserved() {
    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
}

#[test]
fn t38_model_gpt4o_mini() {
    let req = ChatCompletionRequest::builder()
        .model("gpt-4o-mini")
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o-mini"));
}

#[test]
fn t39_model_gpt4_turbo() {
    let req = ChatCompletionRequest::builder()
        .model("gpt-4-turbo")
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
}

#[test]
fn t40_model_o1() {
    let req = ChatCompletionRequest::builder()
        .model("o1")
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("o1"));
}

#[test]
fn t41_model_o1_mini() {
    let req = ChatCompletionRequest::builder()
        .model("o1-mini")
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("o1-mini"));
}

#[test]
fn t42_model_o3_mini() {
    let req = ChatCompletionRequest::builder()
        .model("o3-mini")
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("o3-mini"));
}

#[test]
fn t43_model_gpt35_turbo() {
    let req = ChatCompletionRequest::builder()
        .model("gpt-3.5-turbo")
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-3.5-turbo"));
}

#[test]
fn t44_model_name_in_receipt_response() {
    let events = vec![assistant_event("ok")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4.1");
    assert_eq!(resp.model, "gpt-4.1");
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Parameters — temperature, max_tokens, top_p, etc.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t45_temperature_in_work_order() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .temperature(1.5)
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("temperature"),
        Some(&serde_json::Value::from(1.5))
    );
}

#[test]
fn t46_max_tokens_in_work_order() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .max_tokens(4096)
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("max_tokens"),
        Some(&serde_json::Value::from(4096))
    );
}

#[test]
fn t47_stop_sequences_in_work_order() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .stop(vec!["STOP".into(), "\n\n".into()])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.vendor.get("stop"), Some(&json!(["STOP", "\n\n"])));
}

#[test]
fn t48_temperature_zero() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .temperature(0.0)
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("temperature"),
        Some(&serde_json::Value::from(0.0))
    );
}

#[test]
fn t49_temperature_two() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .temperature(2.0)
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("temperature"),
        Some(&serde_json::Value::from(2.0))
    );
}

#[test]
fn t50_no_params_means_no_vendor_config_pollution() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert!(wo.config.vendor.get("temperature").is_none());
    assert!(wo.config.vendor.get("max_tokens").is_none());
    assert!(wo.config.vendor.get("stop").is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Error responses — OpenAI error format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t51_error_event_in_receipt_response() {
    let events = vec![error_event("rate limit exceeded")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let content = resp.choices[0].message.content.as_deref().unwrap();
    assert!(content.contains("rate limit exceeded"));
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn t52_error_event_in_stream() {
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "server error".into(),
            error_code: None,
        },
        ext: None,
    }];
    let stream = events_to_stream_events(&events, "gpt-4o");
    // error event is not mapped to a delta in events_to_stream_events
    // but the stop chunk is always appended
    assert!(!stream.is_empty());
}

#[tokio::test]
async fn t53_no_processor_returns_internal_error() {
    let client = OpenAiClient::new("gpt-4o");
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .build();
    let err = client.chat().completions().create(req).await.unwrap_err();
    assert!(matches!(err, ShimError::Internal(_)));
}

#[tokio::test]
async fn t54_no_processor_stream_returns_internal_error() {
    let client = OpenAiClient::new("gpt-4o");
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .stream(true)
        .build();
    let result = client.chat().completions().create_stream(req).await;
    assert!(result.is_err());
}

#[test]
fn t55_shim_error_display_invalid_request() {
    let e = ShimError::InvalidRequest("bad model".into());
    let msg = e.to_string();
    assert!(msg.contains("invalid request"));
    assert!(msg.contains("bad model"));
}

#[test]
fn t56_shim_error_display_internal() {
    let e = ShimError::Internal("something broke".into());
    let msg = e.to_string();
    assert!(msg.contains("internal error"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Client configuration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t57_client_model_accessor() {
    let client = OpenAiClient::new("gpt-4o");
    assert_eq!(client.model(), "gpt-4o");
}

#[test]
fn t58_client_debug_impl() {
    let client = OpenAiClient::new("gpt-4o");
    let debug = format!("{:?}", client);
    assert!(debug.contains("OpenAiClient"));
    assert!(debug.contains("gpt-4o"));
}

#[test]
fn t59_client_chat_api_chain() {
    let events = vec![assistant_event("ok")];
    let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
    // Verify the API chain compiles and works
    let _chat = client.chat();
    let _completions = client.chat().completions();
}

#[test]
fn t60_http_client_default_base_url() {
    let client = abp_shim_openai::client::Client::new("sk-test-key").unwrap();
    assert_eq!(client.base_url(), "https://api.openai.com/v1");
}

#[test]
fn t61_http_client_custom_base_url() {
    let client = abp_shim_openai::client::Client::builder("sk-test")
        .base_url("https://custom.api.example.com/v1")
        .build()
        .unwrap();
    assert_eq!(client.base_url(), "https://custom.api.example.com/v1");
}

#[test]
fn t62_http_client_custom_timeout() {
    let client = abp_shim_openai::client::Client::builder("sk-test")
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .unwrap();
    assert_eq!(client.base_url(), "https://api.openai.com/v1");
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Serialization fidelity — JSON matches OpenAI API format
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t63_response_json_fidelity() {
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
    // Verify OpenAI-expected structure
    assert_eq!(v["choices"][0]["index"], 0);
    assert_eq!(v["choices"][0]["finish_reason"], "stop");
    assert_eq!(v["choices"][0]["message"]["role"], "assistant");
    assert_eq!(v["choices"][0]["message"]["content"], "ok");
    assert_eq!(v["usage"]["prompt_tokens"], 10);
    assert_eq!(v["usage"]["completion_tokens"], 5);
    assert_eq!(v["usage"]["total_tokens"], 15);
}

#[test]
fn t64_tool_call_json_fidelity() {
    let tc = ToolCall {
        id: "call_abc123".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "get_weather".into(),
            arguments: r#"{"location":"SF"}"#.into(),
        },
    };
    let v = serde_json::to_value(&tc).unwrap();
    assert_eq!(v["id"], "call_abc123");
    assert_eq!(v["type"], "function");
    assert_eq!(v["function"]["name"], "get_weather");
    assert_eq!(v["function"]["arguments"], r#"{"location":"SF"}"#);
}

#[test]
fn t65_stream_chunk_json_fidelity() {
    let event = StreamEvent {
        id: "chatcmpl-chunk1".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: Delta {
                role: Some("assistant".into()),
                content: Some("Hi".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let v = serde_json::to_value(&event).unwrap();
    assert_eq!(v["object"], "chat.completion.chunk");
    assert_eq!(v["choices"][0]["delta"]["role"], "assistant");
    assert_eq!(v["choices"][0]["delta"]["content"], "Hi");
    assert!(v["choices"][0]["finish_reason"].is_null());
}

#[test]
fn t66_usage_total_equals_sum() {
    let u = Usage {
        prompt_tokens: 200,
        completion_tokens: 100,
        total_tokens: 300,
    };
    assert_eq!(u.total_tokens, u.prompt_tokens + u.completion_tokens);
}

#[test]
fn t67_response_format_text_serialization() {
    let rf = ResponseFormat::text();
    let v = serde_json::to_value(&rf).unwrap();
    assert_eq!(v["type"], "text");
}

#[test]
fn t68_response_format_json_object_serialization() {
    let rf = ResponseFormat::json_object();
    let v = serde_json::to_value(&rf).unwrap();
    assert_eq!(v["type"], "json_object");
}

#[test]
fn t69_response_format_json_schema_serialization() {
    let rf = ResponseFormat::json_schema(
        "my_schema",
        json!({"type": "object", "properties": {"x": {"type": "number"}}}),
    );
    let v = serde_json::to_value(&rf).unwrap();
    assert_eq!(v["type"], "json_schema");
    assert_eq!(v["json_schema"]["name"], "my_schema");
    assert_eq!(v["json_schema"]["strict"], true);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Request → WorkOrder conversion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t70_request_to_work_order_extracts_task_from_last_user_msg() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![
            Message::user("First question"),
            Message::assistant("Answer"),
            Message::user("Second question"),
        ])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Second question");
}

#[test]
fn t71_request_to_work_order_fallback_task() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::system("System only")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "chat completion");
}

#[test]
fn t72_request_to_ir_system_and_user() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::system("Be concise."), Message::user("Hello")])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.len(), 2);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
}

#[test]
fn t73_request_to_ir_multi_turn() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![
            Message::user("What is 2+2?"),
            Message::assistant("4"),
            Message::user("And 3+3?"),
        ])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.len(), 3);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[1].role, IrRole::Assistant);
    assert_eq!(conv.messages[2].role, IrRole::User);
}

#[test]
fn t74_tools_to_ir_conversion() {
    let tools = vec![
        Tool::function("search", "Search web", json!({"type": "object"})),
        Tool::function("calc", "Calculate", json!({"type": "object"})),
    ];
    let ir = tools_to_ir(&tools);
    assert_eq!(ir.len(), 2);
    assert_eq!(ir[0].name, "search");
    assert_eq!(ir[1].name, "calc");
    assert_eq!(ir[0].description, "Search web");
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Receipt → Response conversion
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t75_receipt_with_assistant_message() {
    let events = vec![assistant_event("Hello there!")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Hello there!")
    );
    assert!(resp.choices[0].message.tool_calls.is_none());
}

#[test]
fn t76_receipt_with_deltas_concatenated() {
    let events = vec![delta_event("Hello"), delta_event(" "), delta_event("World")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Hello World")
    );
}

#[test]
fn t77_receipt_with_tool_calls() {
    let events = vec![
        tool_call_event("fn_a", "call_a", json!({"x": 1})),
        tool_call_event("fn_b", "call_b", json!({"y": 2})),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs.len(), 2);
}

#[test]
fn t78_receipt_usage_mapping() {
    let usage = sample_usage();
    let events = vec![assistant_event("ok")];
    let receipt = mock_receipt_with_usage(events, usage);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let u = resp.usage.unwrap();
    assert_eq!(u.prompt_tokens, 150);
    assert_eq!(u.completion_tokens, 75);
    assert_eq!(u.total_tokens, 225);
}

#[test]
fn t79_receipt_zero_usage() {
    let events = vec![assistant_event("ok")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let u = resp.usage.unwrap();
    assert_eq!(u.prompt_tokens, 0);
    assert_eq!(u.completion_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

#[test]
fn t80_receipt_error_event_produces_content() {
    let events = vec![error_event("timeout")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert!(
        resp.choices[0]
            .message
            .content
            .as_deref()
            .unwrap()
            .contains("timeout")
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional coverage: IR roundtrips, edge cases, end-to-end
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t81_messages_to_ir_and_back_roundtrip() {
    let messages = vec![
        Message::system("Be helpful"),
        Message::user("Hi"),
        Message::assistant("Hello!"),
    ];
    let conv = messages_to_ir(&messages);
    let back = ir_to_messages(&conv);
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, Role::System);
    assert_eq!(back[0].content.as_deref(), Some("Be helpful"));
    assert_eq!(back[1].role, Role::User);
    assert_eq!(back[2].role, Role::Assistant);
}

#[test]
fn t82_tool_call_ir_roundtrip() {
    let messages = vec![Message::assistant_with_tool_calls(vec![ToolCall {
        id: "call_99".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "list_files".into(),
            arguments: r#"{"dir":"src"}"#.into(),
        },
    }])];
    let conv = messages_to_ir(&messages);
    let back = ir_to_messages(&conv);
    let tc = &back[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "call_99");
    assert_eq!(tc.function.name, "list_files");
}

#[test]
fn t83_tool_result_ir_roundtrip() {
    let messages = vec![Message::tool("call_99", "file1.rs\nfile2.rs")];
    let conv = messages_to_ir(&messages);
    let back = ir_to_messages(&conv);
    assert_eq!(back[0].role, Role::Tool);
    assert_eq!(back[0].content.as_deref(), Some("file1.rs\nfile2.rs"));
    assert_eq!(back[0].tool_call_id.as_deref(), Some("call_99"));
}

#[test]
fn t84_empty_messages_produce_empty_ir() {
    let conv = messages_to_ir(&[]);
    assert!(conv.is_empty());
}

#[test]
fn t85_ir_usage_conversion() {
    let ir = IrUsage::from_io(500, 250);
    let usage = ir_usage_to_usage(&ir);
    assert_eq!(usage.prompt_tokens, 500);
    assert_eq!(usage.completion_tokens, 250);
    assert_eq!(usage.total_tokens, 750);
}

#[test]
fn t86_ir_usage_zero() {
    let ir = IrUsage::from_io(0, 0);
    let usage = ir_usage_to_usage(&ir);
    assert_eq!(usage.total_tokens, 0);
}

#[tokio::test]
async fn t87_end_to_end_chat_roundtrip() {
    let events = vec![assistant_event("The answer is 42")];
    let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![
            Message::system("You answer questions"),
            Message::user("What is the meaning of life?"),
        ])
        .temperature(0.7)
        .max_tokens(1024)
        .build();

    let resp = client.chat().completions().create(req).await.unwrap();
    assert_eq!(resp.model, "gpt-4o");
    assert_eq!(resp.object, "chat.completion");
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("The answer is 42")
    );
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[tokio::test]
async fn t88_end_to_end_streaming_roundtrip() {
    let events = vec![
        delta_event("The "),
        delta_event("answer "),
        delta_event("is 42"),
    ];
    let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("Question")])
        .stream(true)
        .build();

    let stream = client
        .chat()
        .completions()
        .create_stream(req)
        .await
        .unwrap();
    let chunks: Vec<StreamEvent> = stream.collect().await;
    assert_eq!(chunks.len(), 4); // 3 deltas + stop
    let text: String = chunks
        .iter()
        .filter_map(|c| c.choices[0].delta.content.as_deref())
        .collect();
    assert_eq!(text, "The answer is 42");
}

#[tokio::test]
async fn t89_end_to_end_tool_call_roundtrip() {
    let events = vec![tool_call_event(
        "get_weather",
        "call_w1",
        json!({"location": "Tokyo", "unit": "celsius"}),
    )];
    let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("Weather in Tokyo?")])
        .tools(vec![weather_tool()])
        .build();

    let resp = client.chat().completions().create(req).await.unwrap();
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    let tc = &resp.choices[0].message.tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "get_weather");
    let args: serde_json::Value = serde_json::from_str(&tc.function.arguments).unwrap();
    assert_eq!(args["location"], "Tokyo");
}

#[tokio::test]
async fn t90_end_to_end_usage_tracking() {
    let usage = UsageNormalized {
        input_tokens: Some(42),
        output_tokens: Some(17),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: None,
    };
    let events = vec![assistant_event("done")];
    let client =
        OpenAiClient::new("gpt-4o").with_processor(make_processor_with_usage(events, usage));
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .build();

    let resp = client.chat().completions().create(req).await.unwrap();
    let u = resp.usage.unwrap();
    assert_eq!(u.prompt_tokens, 42);
    assert_eq!(u.completion_tokens, 17);
    assert_eq!(u.total_tokens, 59);
}

#[test]
fn t91_stream_empty_events_still_has_stop() {
    let stream = events_to_stream_events(&[], "gpt-4o");
    assert_eq!(stream.len(), 1);
    assert_eq!(stream[0].choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn t92_function_def_fields() {
    let fd = FunctionDef {
        name: "search".into(),
        description: "Search the web".into(),
        parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
    };
    let v = serde_json::to_value(&fd).unwrap();
    assert_eq!(v["name"], "search");
    assert_eq!(v["description"], "Search the web");
    assert!(v["parameters"]["properties"]["q"].is_object());
}
