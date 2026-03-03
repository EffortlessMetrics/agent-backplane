// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the OpenAI shim crate — validates that ABP can act
//! as an OpenAI SDK drop-in replacement.

use abp_core::ir::{IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, UsageNormalized};
use abp_shim_openai::{
    events_to_stream_events, ir_to_messages, ir_usage_to_usage, messages_to_ir, mock_receipt,
    mock_receipt_with_usage, receipt_to_response, request_to_ir, request_to_work_order, tools_to_ir,
    ChatCompletionRequest, ChatCompletionResponse, Choice, Delta, FunctionCall, FunctionDef,
    Message, OpenAiClient, ProcessFn, Role, ShimError, StreamChoice, StreamEvent, Tool, ToolCall,
    Usage,
};
use abp_shim_openai::ResponseFormat;
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

// ═══════════════════════════════════════════════════════════════════════════
// 1. OpenAI types fidelity (~15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t01_chat_completion_request_serde_roundtrip() {
    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("Hello")])
        .temperature(0.7)
        .max_tokens(100)
        .build();

    let json = serde_json::to_string(&req).unwrap();
    let parsed: ChatCompletionRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.model, "gpt-4o");
    assert_eq!(parsed.temperature, Some(0.7));
    assert_eq!(parsed.max_tokens, Some(100));
}

#[test]
fn t02_chat_completion_response_serde_roundtrip() {
    let resp = ChatCompletionResponse {
        id: "chatcmpl-test".into(),
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

    let json = serde_json::to_string(&resp).unwrap();
    let parsed: ChatCompletionResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "chatcmpl-test");
    assert_eq!(parsed.object, "chat.completion");
    assert_eq!(parsed.model, "gpt-4o");
    assert_eq!(parsed.choices.len(), 1);
}

#[test]
fn t03_system_message_serde() {
    let msg = Message::system("You are a helpful assistant.");
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"system"#));
    let parsed: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.role, Role::System);
    assert_eq!(parsed.content.as_deref(), Some("You are a helpful assistant."));
}

#[test]
fn t04_user_message_serde() {
    let msg = Message::user("What is Rust?");
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"user"#));
    let parsed: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.role, Role::User);
}

#[test]
fn t05_assistant_message_serde() {
    let msg = Message::assistant("Rust is a systems programming language.");
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"assistant"#));
    let parsed: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.role, Role::Assistant);
    assert_eq!(
        parsed.content.as_deref(),
        Some("Rust is a systems programming language.")
    );
}

#[test]
fn t06_tool_message_serde() {
    let msg = Message::tool("call_123", "tool result data");
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains(r#""role":"tool"#));
    let parsed: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.role, Role::Tool);
    assert_eq!(parsed.tool_call_id.as_deref(), Some("call_123"));
    assert_eq!(parsed.content.as_deref(), Some("tool result data"));
}

#[test]
fn t07_assistant_message_with_tool_calls_serde() {
    let msg = Message::assistant_with_tool_calls(vec![ToolCall {
        id: "call_abc".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "get_weather".into(),
            arguments: r#"{"location":"NYC"}"#.into(),
        },
    }]);
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.role, Role::Assistant);
    assert!(parsed.content.is_none());
    let tcs = parsed.tool_calls.unwrap();
    assert_eq!(tcs.len(), 1);
    assert_eq!(tcs[0].id, "call_abc");
    assert_eq!(tcs[0].function.name, "get_weather");
}

#[test]
fn t08_tool_definition_serde_roundtrip() {
    let tool = Tool::function(
        "search",
        "Search the web",
        json!({"type": "object", "properties": {"query": {"type": "string"}}}),
    );
    let json = serde_json::to_string(&tool).unwrap();
    assert!(json.contains(r#""type":"function"#));
    let parsed: Tool = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tool);
}

#[test]
fn t09_function_def_fields() {
    let fd = FunctionDef {
        name: "calculate".into(),
        description: "Math calculator".into(),
        parameters: json!({"type": "object"}),
    };
    assert_eq!(fd.name, "calculate");
    assert_eq!(fd.description, "Math calculator");
}

#[test]
fn t10_tool_call_serde_roundtrip() {
    let tc = ToolCall {
        id: "call_xyz".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "read_file".into(),
            arguments: r#"{"path":"main.rs"}"#.into(),
        },
    };
    let json = serde_json::to_string(&tc).unwrap();
    let parsed: ToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, tc);
}

#[test]
fn t11_usage_serde_roundtrip() {
    let usage = Usage {
        prompt_tokens: 500,
        completion_tokens: 200,
        total_tokens: 700,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let parsed: Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, usage);
}

#[test]
fn t12_role_serde_roundtrip() {
    for (role, expected) in [
        (Role::System, r#""system""#),
        (Role::User, r#""user""#),
        (Role::Assistant, r#""assistant""#),
        (Role::Tool, r#""tool""#),
    ] {
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, expected);
        let parsed: Role = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, role);
    }
}

#[test]
fn t13_model_names_in_request() {
    for model in ["gpt-4o", "gpt-4o-mini", "gpt-4-turbo", "o3-mini", "gpt-4.1"] {
        let req = ChatCompletionRequest::builder()
            .model(model)
            .messages(vec![Message::user("test")])
            .build();
        assert_eq!(req.model, model);
    }
}

#[test]
fn t14_request_optional_fields_omitted_in_json() {
    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("test")])
        .build();
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("temperature").is_none());
    assert!(json.get("max_tokens").is_none());
    assert!(json.get("tools").is_none());
    assert!(json.get("stream").is_none());
    assert!(json.get("stop").is_none());
}

#[test]
fn t15_stream_event_serde_roundtrip() {
    let se = StreamEvent {
        id: "chatcmpl-stream".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: Delta {
                role: Some("assistant".into()),
                content: Some("Hello".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let json = serde_json::to_string(&se).unwrap();
    let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "chatcmpl-stream");
    assert_eq!(parsed.object, "chat.completion.chunk");
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Request translation (~15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t16_request_to_work_order_preserves_model() {
    let req = ChatCompletionRequest::builder()
        .model("gpt-4-turbo")
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
}

#[test]
fn t17_request_to_work_order_preserves_temperature() {
    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
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
fn t18_request_to_work_order_preserves_max_tokens() {
    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("test")])
        .max_tokens(2048)
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("max_tokens"),
        Some(&serde_json::Value::from(2048))
    );
}

#[test]
fn t19_request_to_work_order_preserves_stop_sequences() {
    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("test")])
        .stop(vec!["END".into(), "DONE".into()])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.vendor.get("stop"), Some(&json!(["END", "DONE"])));
}

#[test]
fn t20_request_to_work_order_task_from_last_user_message() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![
            Message::user("first question"),
            Message::assistant("answer"),
            Message::user("follow-up question"),
        ])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "follow-up question");
}

#[test]
fn t21_request_to_work_order_system_message_preserved_in_ir() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![
            Message::system("You are an expert."),
            Message::user("Hi"),
        ])
        .build();
    let ir = request_to_ir(&req);
    assert_eq!(ir.messages[0].role, IrRole::System);
    assert_eq!(ir.messages[0].text_content(), "You are an expert.");
}

#[test]
fn t22_tool_definitions_to_ir() {
    let tools = vec![
        Tool::function(
            "read_file",
            "Read a file from disk",
            json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        ),
        Tool::function(
            "write_file",
            "Write a file",
            json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}),
        ),
    ];
    let ir = tools_to_ir(&tools);
    assert_eq!(ir.len(), 2);
    assert_eq!(ir[0].name, "read_file");
    assert_eq!(ir[0].description, "Read a file from disk");
    assert_eq!(ir[1].name, "write_file");
}

#[test]
fn t23_request_to_ir_user_messages_preserved() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![
            Message::user("Hello"),
            Message::assistant("Hi there"),
            Message::user("How are you?"),
        ])
        .build();
    let ir = request_to_ir(&req);
    assert_eq!(ir.len(), 3);
    assert_eq!(ir.messages[0].role, IrRole::User);
    assert_eq!(ir.messages[1].role, IrRole::Assistant);
    assert_eq!(ir.messages[2].role, IrRole::User);
}

#[test]
fn t24_request_to_ir_tool_call_messages() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![
            Message::user("Read main.rs"),
            Message::assistant_with_tool_calls(vec![ToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: FunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"main.rs"}"#.into(),
                },
            }]),
            Message::tool("call_1", "fn main() {}"),
        ])
        .build();
    let ir = request_to_ir(&req);
    assert_eq!(ir.len(), 3);
    assert_eq!(ir.messages[1].role, IrRole::Assistant);
    assert!(!ir.messages[1].tool_use_blocks().is_empty());
    assert_eq!(ir.messages[2].role, IrRole::Tool);
}

#[test]
fn t25_request_to_work_order_no_user_message_defaults_task() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::system("You are a bot.")])
        .build();
    let wo = request_to_work_order(&req);
    // Should default to "chat completion" or similar when no user message
    assert!(!wo.task.is_empty());
}

#[test]
fn t26_builder_defaults_model_to_gpt4o() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .build();
    assert_eq!(req.model, "gpt-4o");
}

#[test]
fn t27_request_stream_flag_preserved() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .stream(true)
        .build();
    assert_eq!(req.stream, Some(true));
}

#[test]
fn t28_request_response_format_preserved() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .response_format(ResponseFormat::json_object())
        .build();
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("response_format").is_some());
}

#[test]
fn t29_request_tool_choice_preserved() {
    use abp_shim_openai::ToolChoice;
    use abp_shim_openai::ToolChoiceMode;

    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .tool_choice(ToolChoice::Mode(ToolChoiceMode::Auto))
        .build();
    assert!(req.tool_choice.is_some());
}

#[test]
fn t30_request_to_work_order_without_optional_fields() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert!(!wo.config.vendor.contains_key("temperature"));
    assert!(!wo.config.vendor.contains_key("max_tokens"));
    assert!(!wo.config.vendor.contains_key("stop"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Response translation (~15 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t31_receipt_to_response_basic() {
    let receipt = mock_receipt(vec![assistant_event("Hello!")]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.object, "chat.completion");
    assert_eq!(resp.model, "gpt-4o");
    assert_eq!(resp.choices.len(), 1);
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn t32_receipt_to_response_id_format() {
    let receipt = mock_receipt(vec![assistant_event("test")]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert!(resp.id.starts_with("chatcmpl-"));
}

#[test]
fn t33_receipt_to_response_usage_mapping() {
    let usage = UsageNormalized {
        input_tokens: Some(250),
        output_tokens: Some(100),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: None,
    };
    let receipt = mock_receipt_with_usage(vec![assistant_event("ok")], usage);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let u = resp.usage.unwrap();
    assert_eq!(u.prompt_tokens, 250);
    assert_eq!(u.completion_tokens, 100);
    assert_eq!(u.total_tokens, 350);
}

#[test]
fn t34_receipt_to_response_zero_usage() {
    let usage = UsageNormalized::default();
    let receipt = mock_receipt_with_usage(vec![assistant_event("ok")], usage);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let u = resp.usage.unwrap();
    assert_eq!(u.prompt_tokens, 0);
    assert_eq!(u.completion_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

#[test]
fn t35_receipt_to_response_tool_calls() {
    let receipt = mock_receipt(vec![tool_call_event(
        "get_weather",
        "call_w1",
        json!({"location": "London"}),
    )]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs[0].id, "call_w1");
    assert_eq!(tcs[0].call_type, "function");
    assert_eq!(tcs[0].function.name, "get_weather");
    assert!(tcs[0].function.arguments.contains("London"));
}

#[test]
fn t36_receipt_to_response_multiple_tool_calls() {
    let receipt = mock_receipt(vec![
        tool_call_event("read_file", "call_1", json!({"path": "a.rs"})),
        tool_call_event("read_file", "call_2", json!({"path": "b.rs"})),
    ]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs.len(), 2);
    assert_eq!(tcs[0].id, "call_1");
    assert_eq!(tcs[1].id, "call_2");
}

#[test]
fn t37_receipt_to_response_error_event() {
    let receipt = mock_receipt(vec![error_event("rate limit exceeded")]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let content = resp.choices[0].message.content.as_deref().unwrap();
    assert!(content.contains("rate limit exceeded"));
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn t38_receipt_to_response_delta_accumulation() {
    let receipt = mock_receipt(vec![
        delta_event("Hello"),
        delta_event(", "),
        delta_event("world!"),
    ]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Hello, world!")
    );
}

#[test]
fn t39_receipt_to_response_text_and_tool_calls_combined() {
    let receipt = mock_receipt(vec![
        assistant_event("Let me check."),
        tool_call_event("ls", "call_ls", json!({})),
    ]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Let me check.")
    );
    assert!(resp.choices[0].message.tool_calls.is_some());
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
}

#[test]
fn t40_receipt_to_response_no_events() {
    let receipt = mock_receipt(vec![]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert!(resp.choices[0].message.content.is_none());
    assert!(resp.choices[0].message.tool_calls.is_none());
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn t41_receipt_to_response_model_preserved() {
    let receipt = mock_receipt(vec![assistant_event("ok")]);
    let resp = receipt_to_response(&receipt, "o3-mini");
    assert_eq!(resp.model, "o3-mini");
}

#[test]
fn t42_receipt_to_response_object_field() {
    let receipt = mock_receipt(vec![assistant_event("test")]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.object, "chat.completion");
}

#[test]
fn t43_receipt_to_response_choice_index_zero() {
    let receipt = mock_receipt(vec![assistant_event("test")]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.choices[0].index, 0);
}

#[test]
fn t44_receipt_to_response_created_timestamp() {
    let receipt = mock_receipt(vec![assistant_event("test")]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert!(resp.created > 0);
}

#[test]
fn t45_receipt_to_response_tool_call_no_id_generates_one() {
    let receipt = mock_receipt(vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "search".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({"q": "rust"}),
        },
        ext: None,
    }]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert!(!tcs[0].id.is_empty());
    assert!(tcs[0].id.starts_with("call_"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Streaming (~10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t46_events_to_stream_basic_delta() {
    let events = vec![delta_event("Hello")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    // 1 delta + 1 stop
    assert_eq!(stream.len(), 2);
    assert_eq!(stream[0].choices[0].delta.content.as_deref(), Some("Hello"));
    assert!(stream[0].choices[0].finish_reason.is_none());
}

#[test]
fn t47_events_to_stream_stop_chunk() {
    let events = vec![delta_event("hi")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    let last = stream.last().unwrap();
    assert_eq!(last.choices[0].finish_reason.as_deref(), Some("stop"));
    assert!(last.choices[0].delta.content.is_none());
}

#[test]
fn t48_events_to_stream_multiple_deltas() {
    let events = vec![delta_event("Hel"), delta_event("lo"), delta_event("!")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    // 3 deltas + 1 stop
    assert_eq!(stream.len(), 4);
    assert_eq!(stream[0].choices[0].delta.content.as_deref(), Some("Hel"));
    assert_eq!(stream[1].choices[0].delta.content.as_deref(), Some("lo"));
    assert_eq!(stream[2].choices[0].delta.content.as_deref(), Some("!"));
}

#[test]
fn t49_events_to_stream_object_type() {
    let events = vec![delta_event("test")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    for chunk in &stream {
        assert_eq!(chunk.object, "chat.completion.chunk");
    }
}

#[test]
fn t50_events_to_stream_model_preserved() {
    let events = vec![delta_event("test")];
    let stream = events_to_stream_events(&events, "gpt-4-turbo");
    for chunk in &stream {
        assert_eq!(chunk.model, "gpt-4-turbo");
    }
}

#[test]
fn t51_events_to_stream_tool_call() {
    let events = vec![tool_call_event("search", "call_s1", json!({"q": "rust"}))];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert_eq!(stream.len(), 2); // tool chunk + stop chunk
    let tc = &stream[0].choices[0].delta.tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id.as_deref(), Some("call_s1"));
    assert_eq!(tc.call_type.as_deref(), Some("function"));
    assert_eq!(
        tc.function.as_ref().unwrap().name.as_deref(),
        Some("search")
    );
}

#[test]
fn t52_events_to_stream_assistant_message_has_role() {
    let events = vec![assistant_event("Full message")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert_eq!(
        stream[0].choices[0].delta.role.as_deref(),
        Some("assistant")
    );
    assert_eq!(
        stream[0].choices[0].delta.content.as_deref(),
        Some("Full message")
    );
}

#[test]
fn t53_events_to_stream_empty_events() {
    let events: Vec<AgentEvent> = vec![];
    let stream = events_to_stream_events(&events, "gpt-4o");
    // Only the final stop chunk
    assert_eq!(stream.len(), 1);
    assert_eq!(stream[0].choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn t54_events_to_stream_consistent_id() {
    let events = vec![delta_event("a"), delta_event("b")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    let id = &stream[0].id;
    for chunk in &stream {
        assert_eq!(&chunk.id, id);
    }
}

#[tokio::test]
async fn t55_client_create_stream_produces_events() {
    let events = vec![delta_event("Hel"), delta_event("lo!")];
    let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("Hi")])
        .stream(true)
        .build();

    let stream = client
        .chat()
        .completions()
        .create_stream(req)
        .await
        .unwrap();
    let chunks: Vec<StreamEvent> = stream.collect().await;
    assert_eq!(chunks.len(), 3); // 2 deltas + 1 stop
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Edge cases (~10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t56_empty_messages_to_ir() {
    let conv = messages_to_ir(&[]);
    assert!(conv.is_empty());
}

#[test]
fn t57_empty_message_content() {
    let msg = Message::user("");
    assert_eq!(msg.content.as_deref(), Some(""));
    let ir = messages_to_ir(&[msg]);
    assert_eq!(ir.len(), 1);
}

#[test]
fn t58_very_large_message() {
    let large_text = "x".repeat(100_000);
    let msg = Message::user(&large_text);
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.content.unwrap().len(), 100_000);
}

#[test]
fn t59_unknown_model_name_accepted() {
    let req = ChatCompletionRequest::builder()
        .model("totally-unknown-model-v99")
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("totally-unknown-model-v99"));
}

#[test]
fn t60_temperature_zero() {
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
fn t61_temperature_max_value() {
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
fn t62_max_tokens_one() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .max_tokens(1)
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("max_tokens"),
        Some(&serde_json::Value::from(1))
    );
}

#[test]
fn t63_many_stop_sequences() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .stop(vec![
            "STOP".into(),
            "END".into(),
            "HALT".into(),
            "DONE".into(),
        ])
        .build();
    let wo = request_to_work_order(&req);
    let stop = wo.config.vendor.get("stop").unwrap();
    assert_eq!(stop, &json!(["STOP", "END", "HALT", "DONE"]));
}

#[test]
fn t64_unicode_content_roundtrip() {
    let msg = Message::user("Hello 🌍 — こんにちは — مرحبا");
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.content.as_deref(), Some("Hello 🌍 — こんにちは — مرحبا"));
}

#[tokio::test]
async fn t65_no_processor_error() {
    let client = OpenAiClient::new("gpt-4o");
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .build();
    let err = client.chat().completions().create(req).await.unwrap_err();
    assert!(matches!(err, ShimError::Internal(_)));
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. API surface completeness (~10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn t66_message_constructors_all_variants() {
    let sys = Message::system("sys");
    assert_eq!(sys.role, Role::System);
    assert_eq!(sys.content.as_deref(), Some("sys"));
    assert!(sys.tool_calls.is_none());
    assert!(sys.tool_call_id.is_none());

    let user = Message::user("usr");
    assert_eq!(user.role, Role::User);

    let asst = Message::assistant("asst");
    assert_eq!(asst.role, Role::Assistant);
    assert!(asst.tool_calls.is_none());

    let tool = Message::tool("id1", "result");
    assert_eq!(tool.role, Role::Tool);
    assert_eq!(tool.tool_call_id.as_deref(), Some("id1"));
    assert_eq!(tool.content.as_deref(), Some("result"));
}

#[test]
fn t67_tool_function_constructor() {
    let tool = Tool::function(
        "my_tool",
        "My tool desc",
        json!({"type": "object"}),
    );
    assert_eq!(tool.tool_type, "function");
    assert_eq!(tool.function.name, "my_tool");
    assert_eq!(tool.function.description, "My tool desc");
}

#[test]
fn t68_response_format_text() {
    let rf = ResponseFormat::text();
    let json = serde_json::to_value(&rf).unwrap();
    assert_eq!(json["type"], "text");
}

#[test]
fn t69_response_format_json_object() {
    let rf = ResponseFormat::json_object();
    let json = serde_json::to_value(&rf).unwrap();
    assert_eq!(json["type"], "json_object");
}

#[test]
fn t70_response_format_json_schema() {
    let rf = ResponseFormat::json_schema(
        "my_schema",
        json!({"type": "object", "properties": {"name": {"type": "string"}}}),
    );
    let json = serde_json::to_value(&rf).unwrap();
    assert_eq!(json["type"], "json_schema");
    assert!(json["json_schema"]["name"].as_str() == Some("my_schema"));
}

#[test]
fn t71_ir_usage_to_usage_conversion() {
    let ir = IrUsage::from_io(300, 150);
    let usage = ir_usage_to_usage(&ir);
    assert_eq!(usage.prompt_tokens, 300);
    assert_eq!(usage.completion_tokens, 150);
    assert_eq!(usage.total_tokens, 450);
}

#[test]
fn t72_ir_to_messages_roundtrip() {
    let messages = vec![
        Message::system("System prompt"),
        Message::user("User message"),
        Message::assistant("Reply"),
    ];
    let conv = messages_to_ir(&messages);
    let back = ir_to_messages(&conv);

    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, Role::System);
    assert_eq!(back[0].content.as_deref(), Some("System prompt"));
    assert_eq!(back[1].role, Role::User);
    assert_eq!(back[1].content.as_deref(), Some("User message"));
    assert_eq!(back[2].role, Role::Assistant);
    assert_eq!(back[2].content.as_deref(), Some("Reply"));
}

#[test]
fn t73_ir_to_messages_tool_call_roundtrip() {
    let messages = vec![Message::assistant_with_tool_calls(vec![ToolCall {
        id: "call_rt".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "read_file".into(),
            arguments: r#"{"path":"lib.rs"}"#.into(),
        },
    }])];
    let conv = messages_to_ir(&messages);
    let back = ir_to_messages(&conv);
    assert_eq!(back[0].role, Role::Assistant);
    let tc = &back[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "call_rt");
    assert_eq!(tc.function.name, "read_file");
}

#[test]
fn t74_ir_to_messages_tool_result_roundtrip() {
    let messages = vec![Message::tool("call_rt", "contents of lib.rs")];
    let conv = messages_to_ir(&messages);
    let back = ir_to_messages(&conv);
    assert_eq!(back[0].role, Role::Tool);
    assert_eq!(back[0].content.as_deref(), Some("contents of lib.rs"));
    assert_eq!(back[0].tool_call_id.as_deref(), Some("call_rt"));
}

#[tokio::test]
async fn t75_full_roundtrip_request_response() {
    let events = vec![
        assistant_event("Here's the answer:"),
        tool_call_event("search", "call_s", json!({"query": "rust lang"})),
    ];
    let usage = UsageNormalized {
        input_tokens: Some(50),
        output_tokens: Some(25),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: None,
    };
    let client =
        OpenAiClient::new("gpt-4o").with_processor(make_processor_with_usage(events, usage));
    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![
            Message::system("You are helpful."),
            Message::user("Search for Rust"),
        ])
        .tools(vec![Tool::function(
            "search",
            "Web search",
            json!({"type": "object", "properties": {"query": {"type": "string"}}}),
        )])
        .temperature(0.5)
        .max_tokens(1024)
        .build();

    let resp = client.chat().completions().create(req).await.unwrap();

    // Validate full response structure
    assert_eq!(resp.object, "chat.completion");
    assert_eq!(resp.model, "gpt-4o");
    assert!(resp.id.starts_with("chatcmpl-"));
    assert_eq!(resp.choices.len(), 1);
    assert_eq!(resp.choices[0].index, 0);
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Here's the answer:")
    );
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));

    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs[0].function.name, "search");

    let u = resp.usage.unwrap();
    assert_eq!(u.prompt_tokens, 50);
    assert_eq!(u.completion_tokens, 25);
    assert_eq!(u.total_tokens, 75);
}
