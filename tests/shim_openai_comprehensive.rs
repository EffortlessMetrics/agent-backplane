#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
//! Comprehensive integration tests for the `abp-shim-openai` crate.
//!
//! Categories:
//!   1. Shim initialization & configuration
//!   2. Request translation (OpenAI → IR)
//!   3. Response translation (IR → OpenAI)
//!   4. Tool/function calling roundtrip
//!   5. Streaming response handling
//!   6. Model mapping
//!   7. Error translation
//!   8. Chat completions end-to-end
//!   9. Edge cases

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, Outcome, UsageNormalized, WorkOrderBuilder};
use abp_openai_sdk::dialect::{
    self, CanonicalToolDef, OpenAIChoice, OpenAIConfig, OpenAIFunctionCall, OpenAIMessage,
    OpenAIResponse, OpenAIToolCall, ToolChoice, ToolChoiceFunctionRef, ToolChoiceMode,
};
use abp_openai_sdk::lowering;
use abp_openai_sdk::response_format::ResponseFormat;
use abp_openai_sdk::streaming::{
    ChatCompletionChunk, ChunkChoice, ChunkDelta, ChunkFunctionCall, ChunkToolCall,
    ToolCallAccumulator,
};
use abp_openai_sdk::validation::{self, ExtendedRequestFields};
use abp_shim_openai::{
    events_to_stream_events, ir_to_messages, ir_usage_to_usage, messages_to_ir, mock_receipt,
    mock_receipt_with_usage, receipt_to_response, request_to_ir, request_to_work_order,
    tools_to_ir, ChatCompletionRequest, ChatCompletionResponse, Choice, Delta, FunctionCall,
    Message, OpenAiClient, ProcessFn, Role, ShimError, StreamChoice, StreamEvent, Tool, ToolCall,
    Usage,
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

fn assistant_msg_event(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: text.to_string(),
        },
        ext: None,
    }
}

fn assistant_delta_event(text: &str) -> AgentEvent {
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
// 1. Shim initialization & configuration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn client_new_sets_model() {
    let client = OpenAiClient::new("gpt-4o");
    assert_eq!(client.model(), "gpt-4o");
}

#[test]
fn client_new_custom_model() {
    let client = OpenAiClient::new("o3-mini");
    assert_eq!(client.model(), "o3-mini");
}

#[test]
fn client_debug_impl() {
    let client = OpenAiClient::new("gpt-4o");
    let dbg = format!("{client:?}");
    assert!(dbg.contains("gpt-4o"));
}

#[test]
fn client_with_processor_configures_backend() {
    let events = vec![assistant_msg_event("hi")];
    let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
    assert_eq!(client.model(), "gpt-4o");
}

#[test]
fn builder_defaults_model_to_gpt4o() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .build();
    assert_eq!(req.model, "gpt-4o");
}

#[test]
fn builder_overrides_model() {
    let req = ChatCompletionRequest::builder()
        .model("gpt-4-turbo")
        .messages(vec![Message::user("test")])
        .build();
    assert_eq!(req.model, "gpt-4-turbo");
}

#[test]
fn builder_sets_temperature() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .temperature(0.5)
        .build();
    assert_eq!(req.temperature, Some(0.5));
}

#[test]
fn builder_sets_max_tokens() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .max_tokens(2048)
        .build();
    assert_eq!(req.max_tokens, Some(2048));
}

#[test]
fn builder_sets_stop_sequences() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .stop(vec!["END".into()])
        .build();
    assert_eq!(req.stop, Some(vec!["END".to_string()]));
}

#[test]
fn builder_sets_stream_flag() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .stream(true)
        .build();
    assert_eq!(req.stream, Some(true));
}

#[test]
fn builder_sets_response_format() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .response_format(ResponseFormat::json_object())
        .build();
    assert!(req.response_format.is_some());
}

#[test]
fn builder_sets_tools() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .tools(vec![Tool::function("foo", "desc", json!({}))])
        .build();
    assert_eq!(req.tools.as_ref().unwrap().len(), 1);
}

#[test]
fn builder_sets_tool_choice() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .tool_choice(ToolChoice::Mode(ToolChoiceMode::Auto))
        .build();
    assert!(req.tool_choice.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Request translation (OpenAI dialect → IR)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_to_ir_simple_user_message() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("Hello")])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "Hello");
}

#[test]
fn request_to_ir_system_and_user() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::system("Be concise."), Message::user("Hi")])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.len(), 2);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
}

#[test]
fn request_to_ir_multi_turn() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![
            Message::user("Hi"),
            Message::assistant("Hello!"),
            Message::user("Bye"),
        ])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.len(), 3);
    assert_eq!(conv.messages[2].text_content(), "Bye");
}

#[test]
fn request_to_ir_with_tool_call_message() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::assistant_with_tool_calls(vec![ToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "read".into(),
                arguments: r#"{"path":"a.rs"}"#.into(),
            },
        }])])
        .build();
    let conv = request_to_ir(&req);
    let blocks = conv.messages[0].tool_use_blocks();
    assert_eq!(blocks.len(), 1);
}

#[test]
fn request_to_ir_with_tool_result() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::tool("call_1", "contents")])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.messages[0].role, IrRole::Tool);
}

#[test]
fn request_to_ir_empty_messages() {
    let req = ChatCompletionRequest::builder().messages(vec![]).build();
    let conv = request_to_ir(&req);
    assert!(conv.is_empty());
}

#[test]
fn request_to_ir_preserves_system_content() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::system("You are helpful.")])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(
        conv.system_message().unwrap().text_content(),
        "You are helpful."
    );
}

// ── lowering module direct tests ────────────────────────────────────────

#[test]
fn lowering_user_text_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "user".into(),
        content: Some("Hello".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let conv = lowering::to_ir(&msgs);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content.as_deref(), Some("Hello"));
}

#[test]
fn lowering_system_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "system".into(),
        content: Some("Prompt".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let conv = lowering::to_ir(&msgs);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "system");
}

#[test]
fn lowering_assistant_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: Some("Reply".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let conv = lowering::to_ir(&msgs);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].content.as_deref(), Some("Reply"));
}

#[test]
fn lowering_tool_result_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "tool".into(),
        content: Some("result".into()),
        tool_calls: None,
        tool_call_id: Some("c1".into()),
    }];
    let conv = lowering::to_ir(&msgs);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "tool");
    assert_eq!(back[0].tool_call_id.as_deref(), Some("c1"));
}

#[test]
fn lowering_empty_content_produces_no_text_block() {
    let msgs = vec![OpenAIMessage {
        role: "user".into(),
        content: Some(String::new()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let conv = lowering::to_ir(&msgs);
    assert!(conv.messages[0].content.is_empty());
}

#[test]
fn lowering_none_content_produces_no_block() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: None,
        tool_call_id: None,
    }];
    let conv = lowering::to_ir(&msgs);
    assert!(conv.messages[0].content.is_empty());
}

#[test]
fn lowering_unknown_role_defaults_to_user() {
    let msgs = vec![OpenAIMessage {
        role: "developer".into(),
        content: Some("hi".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn lowering_malformed_tool_arguments_kept_as_string() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "call_bad".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "foo".into(),
                arguments: "not-json".into(),
            },
        }]),
        tool_call_id: None,
    }];
    let conv = lowering::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => {
            assert_eq!(input, &serde_json::Value::String("not-json".into()));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn lowering_multiple_tool_calls_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![
            OpenAIToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "a".into(),
                    arguments: "{}".into(),
                },
            },
            OpenAIToolCall {
                id: "c2".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "b".into(),
                    arguments: "{}".into(),
                },
            },
        ]),
        tool_call_id: None,
    }];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].content.len(), 2);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].tool_calls.as_ref().unwrap().len(), 2);
}

#[test]
fn lowering_tool_result_without_content() {
    let msgs = vec![OpenAIMessage {
        role: "tool".into(),
        content: None,
        tool_calls: None,
        tool_call_id: Some("c1".into()),
    }];
    let conv = lowering::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { content, .. } => assert!(content.is_empty()),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn lowering_assistant_text_and_tool_call() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: Some("Let me check.".into()),
        tool_calls: Some(vec![OpenAIToolCall {
            id: "c7".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "ls".into(),
                arguments: "{}".into(),
            },
        }]),
        tool_call_id: None,
    }];
    let conv = lowering::to_ir(&msgs);
    assert_eq!(conv.messages[0].content.len(), 2);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].content.as_deref(), Some("Let me check."));
    assert!(back[0].tool_calls.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Response translation (IR → OpenAI dialect)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_to_messages_simple_text() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "Hello")]);
    let msgs = ir_to_messages(&conv);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].role, Role::User);
    assert_eq!(msgs[0].content.as_deref(), Some("Hello"));
}

#[test]
fn ir_to_messages_system() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::System, "Prompt")]);
    let msgs = ir_to_messages(&conv);
    assert_eq!(msgs[0].role, Role::System);
}

#[test]
fn ir_to_messages_assistant() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "Reply")]);
    let msgs = ir_to_messages(&conv);
    assert_eq!(msgs[0].role, Role::Assistant);
}

#[test]
fn ir_to_messages_tool_use() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "call_1".into(),
            name: "read".into(),
            input: json!({"path": "a.rs"}),
        }],
    )]);
    let msgs = ir_to_messages(&conv);
    let tc = &msgs[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "call_1");
    assert_eq!(tc.function.name, "read");
}

#[test]
fn ir_to_messages_tool_result() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "call_1".into(),
            content: vec![IrContentBlock::Text { text: "ok".into() }],
            is_error: false,
        }],
    )]);
    let msgs = ir_to_messages(&conv);
    assert_eq!(msgs[0].role, Role::Tool);
    assert_eq!(msgs[0].tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(msgs[0].content.as_deref(), Some("ok"));
}

#[test]
fn ir_to_messages_empty_conversation() {
    let conv = IrConversation::new();
    let msgs = ir_to_messages(&conv);
    assert!(msgs.is_empty());
}

#[test]
fn messages_to_ir_and_back_roundtrip() {
    let original = vec![
        Message::system("Be brief."),
        Message::user("Hi"),
        Message::assistant("Hello!"),
    ];
    let conv = messages_to_ir(&original);
    let back = ir_to_messages(&conv);
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, Role::System);
    assert_eq!(back[1].role, Role::User);
    assert_eq!(back[2].role, Role::Assistant);
    assert_eq!(back[2].content.as_deref(), Some("Hello!"));
}

#[test]
fn messages_to_ir_tool_call_roundtrip() {
    let original = vec![Message::assistant_with_tool_calls(vec![ToolCall {
        id: "call_1".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "search".into(),
            arguments: r#"{"q":"rust"}"#.into(),
        },
    }])];
    let conv = messages_to_ir(&original);
    let back = ir_to_messages(&conv);
    let tc = &back[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "call_1");
    assert_eq!(tc.function.name, "search");
}

#[test]
fn messages_to_ir_tool_result_roundtrip() {
    let original = vec![Message::tool("call_1", "file data")];
    let conv = messages_to_ir(&original);
    let back = ir_to_messages(&conv);
    assert_eq!(back[0].role, Role::Tool);
    assert_eq!(back[0].content.as_deref(), Some("file data"));
    assert_eq!(back[0].tool_call_id.as_deref(), Some("call_1"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Tool/function calling roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_function_constructor() {
    let tool = Tool::function("search", "Search web", json!({"type": "object"}));
    assert_eq!(tool.tool_type, "function");
    assert_eq!(tool.function.name, "search");
    assert_eq!(tool.function.description, "Search web");
}

#[test]
fn tools_to_ir_single() {
    let tools = vec![Tool::function(
        "read",
        "Read file",
        json!({"type": "object"}),
    )];
    let ir = tools_to_ir(&tools);
    assert_eq!(ir.len(), 1);
    assert_eq!(ir[0].name, "read");
    assert_eq!(ir[0].description, "Read file");
}

#[test]
fn tools_to_ir_multiple() {
    let tools = vec![
        Tool::function("a", "A", json!({})),
        Tool::function("b", "B", json!({})),
        Tool::function("c", "C", json!({})),
    ];
    let ir = tools_to_ir(&tools);
    assert_eq!(ir.len(), 3);
    assert_eq!(ir[2].name, "c");
}

#[test]
fn tools_to_ir_empty() {
    let ir = tools_to_ir(&[]);
    assert!(ir.is_empty());
}

#[test]
fn tools_to_ir_preserves_parameters_schema() {
    let schema = json!({
        "type": "object",
        "properties": {
            "path": {"type": "string"}
        },
        "required": ["path"]
    });
    let tools = vec![Tool::function("read_file", "Read", schema.clone())];
    let ir = tools_to_ir(&tools);
    assert_eq!(ir[0].parameters, schema);
}

// ── SDK dialect tool def roundtrip ──────────────────────────────────────

#[test]
fn canonical_tool_def_to_openai_and_back() {
    let canonical = CanonicalToolDef {
        name: "edit_file".into(),
        description: "Edit a file".into(),
        parameters_schema: json!({"type": "object"}),
    };
    let openai = dialect::tool_def_to_openai(&canonical);
    assert_eq!(openai.tool_type, "function");
    assert_eq!(openai.function.name, "edit_file");

    let back = dialect::tool_def_from_openai(&openai);
    assert_eq!(back, canonical);
}

// ── Tool choice serialization ───────────────────────────────────────────

#[test]
fn tool_choice_mode_serialization() {
    let mode = ToolChoice::Mode(ToolChoiceMode::Auto);
    let json = serde_json::to_string(&mode).unwrap();
    assert_eq!(json, r#""auto""#);

    let none = ToolChoice::Mode(ToolChoiceMode::None);
    let json = serde_json::to_string(&none).unwrap();
    assert_eq!(json, r#""none""#);

    let required = ToolChoice::Mode(ToolChoiceMode::Required);
    let json = serde_json::to_string(&required).unwrap();
    assert_eq!(json, r#""required""#);
}

#[test]
fn tool_choice_function_serialization() {
    let choice = ToolChoice::Function {
        tool_type: "function".into(),
        function: ToolChoiceFunctionRef {
            name: "get_weather".into(),
        },
    };
    let json = serde_json::to_value(&choice).unwrap();
    assert_eq!(json["type"], "function");
    assert_eq!(json["function"]["name"], "get_weather");
}

#[test]
fn tool_choice_deserialization_auto() {
    let choice: ToolChoice = serde_json::from_str(r#""auto""#).unwrap();
    assert_eq!(choice, ToolChoice::Mode(ToolChoiceMode::Auto));
}

#[test]
fn tool_choice_deserialization_function() {
    let json = r#"{"type":"function","function":{"name":"search"}}"#;
    let choice: ToolChoice = serde_json::from_str(json).unwrap();
    match choice {
        ToolChoice::Function { function, .. } => assert_eq!(function.name, "search"),
        _ => panic!("expected Function variant"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Streaming response handling
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn events_to_stream_events_text_deltas() {
    let events = vec![assistant_delta_event("Hel"), assistant_delta_event("lo!")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    // 2 deltas + 1 final stop chunk
    assert_eq!(stream.len(), 3);
    assert_eq!(stream[0].choices[0].delta.content.as_deref(), Some("Hel"));
    assert_eq!(stream[1].choices[0].delta.content.as_deref(), Some("lo!"));
    assert_eq!(stream[2].choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn events_to_stream_events_final_chunk_is_stop() {
    let events = vec![assistant_delta_event("hi")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    let last = stream.last().unwrap();
    assert_eq!(last.choices[0].finish_reason.as_deref(), Some("stop"));
    assert!(last.choices[0].delta.content.is_none());
}

#[test]
fn events_to_stream_events_tool_call() {
    let events = vec![tool_call_event("search", "call_s1", json!({"q": "rust"}))];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert_eq!(stream.len(), 2); // tool chunk + stop
    let tc = &stream[0].choices[0].delta.tool_calls.as_ref().unwrap()[0];
    assert_eq!(
        tc.function.as_ref().unwrap().name.as_deref(),
        Some("search")
    );
}

#[test]
fn events_to_stream_events_model_propagated() {
    let events = vec![assistant_delta_event("hi")];
    let stream = events_to_stream_events(&events, "o3-mini");
    for chunk in &stream {
        assert_eq!(chunk.model, "o3-mini");
    }
}

#[test]
fn events_to_stream_events_object_type() {
    let events = vec![assistant_delta_event("hi")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    for chunk in &stream {
        assert_eq!(chunk.object, "chat.completion.chunk");
    }
}

#[test]
fn events_to_stream_events_empty_produces_stop_only() {
    let stream = events_to_stream_events(&[], "gpt-4o");
    assert_eq!(stream.len(), 1);
    assert_eq!(stream[0].choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn events_to_stream_events_assistant_message_as_chunk() {
    let events = vec![assistant_msg_event("Hello!")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert_eq!(stream.len(), 2); // message chunk + stop
    assert_eq!(
        stream[0].choices[0].delta.content.as_deref(),
        Some("Hello!")
    );
    assert_eq!(
        stream[0].choices[0].delta.role.as_deref(),
        Some("assistant")
    );
}

#[tokio::test]
async fn streaming_completion_collects_all_chunks() {
    let events = vec![
        assistant_delta_event("A"),
        assistant_delta_event("B"),
        assistant_delta_event("C"),
    ];
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
    assert_eq!(chunks.len(), 4); // 3 deltas + stop
}

// ── ToolCallAccumulator (from abp-openai-sdk streaming) ─────────────────

#[test]
fn accumulator_single_tool_call() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: Some("call_1".into()),
        call_type: Some("function".into()),
        function: Some(ChunkFunctionCall {
            name: Some("read".into()),
            arguments: Some(r#"{"path":"#.into()),
        }),
    }]);
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: None,
        call_type: None,
        function: Some(ChunkFunctionCall {
            name: None,
            arguments: Some(r#""a.rs"}"#.into()),
        }),
    }]);
    let events = acc.finish();
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => {
            assert_eq!(tool_name, "read");
            assert_eq!(tool_use_id.as_deref(), Some("call_1"));
            assert_eq!(input, &json!({"path": "a.rs"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn accumulator_multiple_tool_calls() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[
        ChunkToolCall {
            index: 0,
            id: Some("c1".into()),
            call_type: Some("function".into()),
            function: Some(ChunkFunctionCall {
                name: Some("a".into()),
                arguments: Some("{}".into()),
            }),
        },
        ChunkToolCall {
            index: 1,
            id: Some("c2".into()),
            call_type: Some("function".into()),
            function: Some(ChunkFunctionCall {
                name: Some("b".into()),
                arguments: Some("{}".into()),
            }),
        },
    ]);
    let events = acc.finish();
    assert_eq!(events.len(), 2);
}

#[test]
fn accumulator_finish_as_openai() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: Some("c1".into()),
        call_type: Some("function".into()),
        function: Some(ChunkFunctionCall {
            name: Some("read".into()),
            arguments: Some("{}".into()),
        }),
    }]);
    let pairs = acc.finish_as_openai();
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].0, "c1");
    assert_eq!(pairs[0].1.name, "read");
}

#[test]
fn accumulator_empty_name_filtered_out() {
    let mut acc = ToolCallAccumulator::new();
    acc.feed(&[ChunkToolCall {
        index: 0,
        id: Some("c1".into()),
        call_type: None,
        function: None,
    }]);
    let events = acc.finish();
    assert!(events.is_empty());
}

// ── map_chunk (from streaming module) ───────────────────────────────────

#[test]
fn map_chunk_text_delta() {
    use abp_openai_sdk::streaming::map_chunk;
    let chunk = ChatCompletionChunk {
        id: "c1".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: None,
                content: Some("hello".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let events = map_chunk(&chunk);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "hello"),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn map_chunk_empty_content_no_event() {
    use abp_openai_sdk::streaming::map_chunk;
    let chunk = ChatCompletionChunk {
        id: "c1".into(),
        object: "chat.completion.chunk".into(),
        created: 0,
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: None,
                content: Some(String::new()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let events = map_chunk(&chunk);
    assert!(events.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Model mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn to_canonical_model_adds_prefix() {
    assert_eq!(dialect::to_canonical_model("gpt-4o"), "openai/gpt-4o");
}

#[test]
fn from_canonical_model_strips_prefix() {
    assert_eq!(dialect::from_canonical_model("openai/gpt-4o"), "gpt-4o");
}

#[test]
fn from_canonical_model_no_prefix_unchanged() {
    assert_eq!(dialect::from_canonical_model("claude-3"), "claude-3");
}

#[test]
fn is_known_model_gpt4o() {
    assert!(dialect::is_known_model("gpt-4o"));
}

#[test]
fn is_known_model_gpt4o_mini() {
    assert!(dialect::is_known_model("gpt-4o-mini"));
}

#[test]
fn is_known_model_gpt4_turbo() {
    assert!(dialect::is_known_model("gpt-4-turbo"));
}

#[test]
fn is_known_model_o1() {
    assert!(dialect::is_known_model("o1"));
}

#[test]
fn is_known_model_o1_mini() {
    assert!(dialect::is_known_model("o1-mini"));
}

#[test]
fn is_known_model_o3_mini() {
    assert!(dialect::is_known_model("o3-mini"));
}

#[test]
fn is_known_model_gpt41() {
    assert!(dialect::is_known_model("gpt-4.1"));
}

#[test]
fn is_known_model_unknown_returns_false() {
    assert!(!dialect::is_known_model("claude-3-sonnet"));
}

#[test]
fn is_known_model_empty_returns_false() {
    assert!(!dialect::is_known_model(""));
}

#[test]
fn request_model_maps_to_work_order() {
    let req = ChatCompletionRequest::builder()
        .model("o3-mini")
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("o3-mini"));
}

#[tokio::test]
async fn model_preserved_in_response() {
    let events = vec![assistant_msg_event("ok")];
    let client = OpenAiClient::new("gpt-4-turbo").with_processor(make_processor(events));
    let req = ChatCompletionRequest::builder()
        .model("gpt-4-turbo")
        .messages(vec![Message::user("test")])
        .build();
    let resp = client.chat().completions().create(req).await.unwrap();
    assert_eq!(resp.model, "gpt-4-turbo");
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Error translation
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn no_processor_returns_internal_error() {
    let client = OpenAiClient::new("gpt-4o");
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .build();
    let err = client.chat().completions().create(req).await.unwrap_err();
    assert!(matches!(err, ShimError::Internal(_)));
}

#[tokio::test]
async fn no_processor_stream_returns_internal_error() {
    let client = OpenAiClient::new("gpt-4o");
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .build();
    let result = client.chat().completions().create_stream(req).await;
    assert!(result.is_err());
    match result {
        Err(ShimError::Internal(_)) => {}
        Err(e) => panic!("expected Internal error, got {e:?}"),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

#[tokio::test]
async fn error_event_in_response() {
    let events = vec![error_event("rate limit exceeded")];
    let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .build();
    let resp = client.chat().completions().create(req).await.unwrap();
    let content = resp.choices[0].message.content.as_deref().unwrap();
    assert!(content.contains("rate limit exceeded"));
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn shim_error_display_invalid_request() {
    let e = ShimError::InvalidRequest("bad".into());
    assert!(e.to_string().contains("bad"));
}

#[test]
fn shim_error_display_internal() {
    let e = ShimError::Internal("oops".into());
    assert!(e.to_string().contains("oops"));
}

#[test]
fn shim_error_from_serde() {
    let bad_json = serde_json::from_str::<serde_json::Value>("not json");
    let e: ShimError = bad_json.unwrap_err().into();
    assert!(matches!(e, ShimError::Serde(_)));
}

// ── Validation errors (from abp-openai-sdk validation module) ───────────

#[test]
fn validation_rejects_logprobs() {
    let fields = ExtendedRequestFields {
        logprobs: Some(true),
        ..Default::default()
    };
    let err = validation::validate_for_mapped_mode(&fields).unwrap_err();
    assert!(err.errors.iter().any(|e| e.param == "logprobs"));
}

#[test]
fn validation_rejects_seed() {
    let fields = ExtendedRequestFields {
        seed: Some(42),
        ..Default::default()
    };
    let err = validation::validate_for_mapped_mode(&fields).unwrap_err();
    assert!(err.errors.iter().any(|e| e.param == "seed"));
}

#[test]
fn validation_rejects_logit_bias() {
    let mut bias = std::collections::BTreeMap::new();
    bias.insert("100".into(), 1.0);
    let fields = ExtendedRequestFields {
        logit_bias: Some(bias),
        ..Default::default()
    };
    let err = validation::validate_for_mapped_mode(&fields).unwrap_err();
    assert!(err.errors.iter().any(|e| e.param == "logit_bias"));
}

#[test]
fn validation_passes_with_no_extended_fields() {
    let fields = ExtendedRequestFields::default();
    assert!(validation::validate_for_mapped_mode(&fields).is_ok());
}

#[test]
fn validation_multiple_errors() {
    let mut bias = std::collections::BTreeMap::new();
    bias.insert("1".into(), 0.5);
    let fields = ExtendedRequestFields {
        logprobs: Some(true),
        seed: Some(99),
        logit_bias: Some(bias),
        ..Default::default()
    };
    let err = validation::validate_for_mapped_mode(&fields).unwrap_err();
    assert!(err.errors.len() >= 3);
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Chat completions end-to-end
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn simple_chat_completion() {
    let events = vec![assistant_msg_event("Hello!")];
    let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("Hi")])
        .build();
    let resp = client.chat().completions().create(req).await.unwrap();
    assert_eq!(resp.object, "chat.completion");
    assert_eq!(resp.choices.len(), 1);
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[tokio::test]
async fn chat_completion_with_system_prompt() {
    let events = vec![assistant_msg_event("I am helpful.")];
    let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
    let req = ChatCompletionRequest::builder()
        .messages(vec![
            Message::system("You are a helpful assistant."),
            Message::user("Hello"),
        ])
        .build();
    let resp = client.chat().completions().create(req).await.unwrap();
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("I am helpful.")
    );
}

#[tokio::test]
async fn chat_completion_multi_turn() {
    let events = vec![assistant_msg_event("4")];
    let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
    let req = ChatCompletionRequest::builder()
        .messages(vec![
            Message::user("What is 2+2?"),
            Message::assistant("Let me calculate..."),
            Message::user("Just the number"),
        ])
        .build();
    let resp = client.chat().completions().create(req).await.unwrap();
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("4"));
}

#[tokio::test]
async fn chat_completion_tool_call() {
    let events = vec![tool_call_event(
        "get_weather",
        "call_abc",
        json!({"location": "SF"}),
    )];
    let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("Weather in SF?")])
        .tools(vec![Tool::function(
            "get_weather",
            "Get weather",
            json!({"type": "object", "properties": {"location": {"type": "string"}}}),
        )])
        .build();
    let resp = client.chat().completions().create(req).await.unwrap();
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs[0].id, "call_abc");
    assert_eq!(tcs[0].function.name, "get_weather");
}

#[tokio::test]
async fn chat_completion_multi_tool_calls() {
    let events = vec![
        tool_call_event("a", "c1", json!({})),
        tool_call_event("b", "c2", json!({})),
    ];
    let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("do both")])
        .build();
    let resp = client.chat().completions().create(req).await.unwrap();
    let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
    assert_eq!(tcs.len(), 2);
}

#[tokio::test]
async fn chat_completion_text_and_tool_call() {
    let events = vec![
        assistant_msg_event("Let me check."),
        tool_call_event("ls", "call_ls", json!({})),
    ];
    let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("list")])
        .build();
    let resp = client.chat().completions().create(req).await.unwrap();
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Let me check.")
    );
    assert!(resp.choices[0].message.tool_calls.is_some());
}

#[tokio::test]
async fn chat_completion_response_id_format() {
    let events = vec![assistant_msg_event("ok")];
    let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .build();
    let resp = client.chat().completions().create(req).await.unwrap();
    assert!(resp.id.starts_with("chatcmpl-"));
}

#[tokio::test]
async fn chat_completion_usage_tracking() {
    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        cache_read_tokens: None,
        cache_write_tokens: None,
        request_units: None,
        estimated_cost_usd: None,
    };
    let events = vec![assistant_msg_event("response")];
    let client =
        OpenAiClient::new("gpt-4o").with_processor(make_processor_with_usage(events, usage));
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .build();
    let resp = client.chat().completions().create(req).await.unwrap();
    let u = resp.usage.unwrap();
    assert_eq!(u.prompt_tokens, 100);
    assert_eq!(u.completion_tokens, 50);
    assert_eq!(u.total_tokens, 150);
}

#[tokio::test]
async fn streaming_tool_call_event() {
    let events = vec![tool_call_event("search", "call_s1", json!({"q": "rust"}))];
    let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("search")])
        .stream(true)
        .build();
    let stream = client
        .chat()
        .completions()
        .create_stream(req)
        .await
        .unwrap();
    let chunks: Vec<StreamEvent> = stream.collect().await;
    assert_eq!(chunks.len(), 2); // tool + stop
}

// ── request_to_work_order mapping ───────────────────────────────────────

#[test]
fn work_order_has_temperature() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .temperature(0.7)
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("temperature"),
        Some(&serde_json::Value::from(0.7))
    );
}

#[test]
fn work_order_has_max_tokens() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .max_tokens(1024)
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("max_tokens"),
        Some(&serde_json::Value::from(1024))
    );
}

#[test]
fn work_order_has_stop_sequences() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .stop(vec!["END".into(), "STOP".into()])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(
        wo.config.vendor.get("stop").unwrap(),
        &json!(["END", "STOP"])
    );
}

#[test]
fn work_order_task_from_user_message() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("Fix the bug")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Fix the bug");
}

#[test]
fn work_order_task_uses_last_user_message() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![
            Message::user("First question"),
            Message::assistant("Answer"),
            Message::user("Follow up"),
        ])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Follow up");
}

#[test]
fn work_order_no_user_message_defaults() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::system("sys")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "chat completion");
}

// ── receipt_to_response ─────────────────────────────────────────────────

#[test]
fn receipt_to_response_basic() {
    let events = vec![assistant_msg_event("Hello!")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.object, "chat.completion");
    assert_eq!(resp.model, "gpt-4o");
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
}

#[test]
fn receipt_to_response_delta_concatenation() {
    let events = vec![
        assistant_delta_event("A"),
        assistant_delta_event("B"),
        assistant_delta_event("C"),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("ABC"));
}

#[test]
fn receipt_to_response_tool_calls() {
    let events = vec![tool_call_event("read", "call_1", json!({"path": "a.rs"}))];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    let tc = &resp.choices[0].message.tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "read");
}

#[test]
fn receipt_to_response_error_event() {
    let events = vec![error_event("quota exceeded")];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert!(resp.choices[0]
        .message
        .content
        .as_deref()
        .unwrap()
        .contains("quota exceeded"));
}

#[test]
fn receipt_to_response_usage() {
    let usage = UsageNormalized {
        input_tokens: Some(200),
        output_tokens: Some(100),
        ..Default::default()
    };
    let events = vec![assistant_msg_event("ok")];
    let receipt = mock_receipt_with_usage(events, usage);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let u = resp.usage.unwrap();
    assert_eq!(u.prompt_tokens, 200);
    assert_eq!(u.completion_tokens, 100);
    assert_eq!(u.total_tokens, 300);
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn message_constructors_system() {
    let msg = Message::system("sys");
    assert_eq!(msg.role, Role::System);
    assert_eq!(msg.content.as_deref(), Some("sys"));
    assert!(msg.tool_calls.is_none());
    assert!(msg.tool_call_id.is_none());
}

#[test]
fn message_constructors_user() {
    let msg = Message::user("usr");
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.content.as_deref(), Some("usr"));
}

#[test]
fn message_constructors_assistant() {
    let msg = Message::assistant("asst");
    assert_eq!(msg.role, Role::Assistant);
    assert_eq!(msg.content.as_deref(), Some("asst"));
}

#[test]
fn message_constructors_tool() {
    let msg = Message::tool("id1", "result");
    assert_eq!(msg.role, Role::Tool);
    assert_eq!(msg.tool_call_id.as_deref(), Some("id1"));
}

#[test]
fn message_constructors_assistant_with_tool_calls() {
    let msg = Message::assistant_with_tool_calls(vec![]);
    assert_eq!(msg.role, Role::Assistant);
    assert!(msg.content.is_none());
    assert!(msg.tool_calls.unwrap().is_empty());
}

#[test]
fn role_serde_roundtrip() {
    for (role, expected) in [
        (Role::System, "\"system\""),
        (Role::User, "\"user\""),
        (Role::Assistant, "\"assistant\""),
        (Role::Tool, "\"tool\""),
    ] {
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, expected);
        let back: Role = serde_json::from_str(&json).unwrap();
        assert_eq!(back, role);
    }
}

#[test]
fn request_serialization_skips_none_fields() {
    let req = ChatCompletionRequest::builder()
        .messages(vec![Message::user("test")])
        .build();
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("tools").is_none());
    assert!(json.get("temperature").is_none());
    assert!(json.get("max_tokens").is_none());
    assert!(json.get("stop").is_none());
    assert!(json.get("stream").is_none());
    assert!(json.get("response_format").is_none());
}

#[test]
fn request_serialization_includes_set_fields() {
    let req = ChatCompletionRequest::builder()
        .model("gpt-4o")
        .messages(vec![Message::user("test")])
        .temperature(0.5)
        .max_tokens(100)
        .stream(true)
        .build();
    let json = serde_json::to_value(&req).unwrap();
    assert!(json.get("temperature").is_some());
    assert!(json.get("max_tokens").is_some());
    assert!(json.get("stream").is_some());
}

#[test]
fn response_serialization_roundtrip() {
    let resp = ChatCompletionResponse {
        id: "chatcmpl-123".into(),
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
    let back: ChatCompletionResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "chatcmpl-123");
    assert_eq!(back.choices[0].message.content.as_deref(), Some("Hello!"));
    assert_eq!(back.usage.unwrap().total_tokens, 15);
}

#[test]
fn ir_usage_to_usage_conversion() {
    let ir = IrUsage::from_io(200, 100);
    let usage = ir_usage_to_usage(&ir);
    assert_eq!(usage.prompt_tokens, 200);
    assert_eq!(usage.completion_tokens, 100);
    assert_eq!(usage.total_tokens, 300);
}

#[test]
fn ir_usage_to_usage_zero() {
    let ir = IrUsage::from_io(0, 0);
    let usage = ir_usage_to_usage(&ir);
    assert_eq!(usage.total_tokens, 0);
}

#[test]
fn ir_usage_with_cache() {
    let ir = IrUsage::with_cache(100, 50, 20, 10);
    let usage = ir_usage_to_usage(&ir);
    assert_eq!(usage.prompt_tokens, 100);
    assert_eq!(usage.completion_tokens, 50);
}

#[test]
fn mock_receipt_creates_valid_receipt() {
    let events = vec![assistant_msg_event("test")];
    let receipt = mock_receipt(events);
    assert_eq!(receipt.outcome, Outcome::Complete);
    assert_eq!(receipt.backend.id, "mock");
    assert_eq!(receipt.trace.len(), 1);
}

#[test]
fn mock_receipt_with_usage_sets_usage() {
    let usage = UsageNormalized {
        input_tokens: Some(50),
        output_tokens: Some(25),
        ..Default::default()
    };
    let receipt = mock_receipt_with_usage(vec![], usage);
    assert_eq!(receipt.usage.input_tokens, Some(50));
    assert_eq!(receipt.usage.output_tokens, Some(25));
}

#[test]
fn response_format_text() {
    let rf = ResponseFormat::text();
    let json = serde_json::to_value(&rf).unwrap();
    assert_eq!(json["type"], "text");
}

#[test]
fn response_format_json_object() {
    let rf = ResponseFormat::json_object();
    let json = serde_json::to_value(&rf).unwrap();
    assert_eq!(json["type"], "json_object");
}

#[test]
fn response_format_json_schema() {
    let rf = ResponseFormat::json_schema(
        "my_schema",
        json!({"type": "object", "properties": {"name": {"type": "string"}}}),
    );
    let json = serde_json::to_value(&rf).unwrap();
    assert_eq!(json["type"], "json_schema");
    assert_eq!(json["json_schema"]["name"], "my_schema");
}

#[test]
fn openai_config_default() {
    let cfg = OpenAIConfig::default();
    assert!(cfg.base_url.contains("openai.com"));
    assert_eq!(cfg.model, "gpt-4o");
    assert!(cfg.max_tokens.unwrap() > 0);
    assert!(cfg.api_key.is_empty());
}

#[test]
fn dialect_version_defined() {
    assert_eq!(dialect::DIALECT_VERSION, "openai/v0.1");
}

#[test]
fn default_model_defined() {
    assert_eq!(dialect::DEFAULT_MODEL, "gpt-4o");
}

// ── SDK dialect map_work_order / map_response ───────────────────────────

#[test]
fn sdk_dialect_map_work_order_basic() {
    let wo = WorkOrderBuilder::new("Refactor auth").build();
    let cfg = OpenAIConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
    assert!(req.messages[0]
        .content
        .as_deref()
        .unwrap()
        .contains("Refactor auth"));
}

#[test]
fn sdk_dialect_map_work_order_model_override() {
    let wo = WorkOrderBuilder::new("task").model("gpt-4-turbo").build();
    let cfg = OpenAIConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "gpt-4-turbo");
}

#[test]
fn sdk_dialect_map_response_assistant_message() {
    let resp = OpenAIResponse {
        id: "chatcmpl-123".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: OpenAIMessage {
                role: "assistant".into(),
                content: Some("Hello!".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Hello!"),
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

#[test]
fn sdk_dialect_map_response_tool_call() {
    let resp = OpenAIResponse {
        id: "chatcmpl-456".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: OpenAIMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![OpenAIToolCall {
                    id: "call_abc".into(),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: "read_file".into(),
                        arguments: r#"{"path":"main.rs"}"#.into(),
                    },
                }]),
                tool_call_id: None,
            },
            finish_reason: Some("tool_calls".into()),
        }],
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall { tool_name, .. } => assert_eq!(tool_name, "read_file"),
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn sdk_dialect_map_response_empty_choices() {
    let resp = OpenAIResponse {
        id: "chatcmpl-empty".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![],
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn sdk_dialect_map_response_empty_content() {
    let resp = OpenAIResponse {
        id: "chatcmpl-empty2".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: OpenAIMessage {
                role: "assistant".into(),
                content: Some(String::new()),
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
    };
    let events = dialect::map_response(&resp);
    assert!(events.is_empty());
}

// ── Capability manifest ─────────────────────────────────────────────────

#[test]
fn capability_manifest_has_streaming() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::Streaming),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_has_structured_output() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::StructuredOutputJsonSchema),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_mcp_unsupported() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::McpClient),
        Some(SupportLevel::Unsupported)
    ));
}

// ── Large / edge-case conversation tests ────────────────────────────────

#[test]
fn large_conversation_roundtrip() {
    let mut messages = Vec::new();
    for i in 0..50 {
        messages.push(Message::user(format!("msg {i}")));
        messages.push(Message::assistant(format!("reply {i}")));
    }
    let conv = messages_to_ir(&messages);
    assert_eq!(conv.len(), 100);
    let back = ir_to_messages(&conv);
    assert_eq!(back.len(), 100);
}

#[test]
fn unicode_content_roundtrip() {
    let msg = Message::user("こんにちは世界 🌍 café résumé");
    let conv = messages_to_ir(&[msg]);
    let back = ir_to_messages(&conv);
    assert_eq!(
        back[0].content.as_deref(),
        Some("こんにちは世界 🌍 café résumé")
    );
}

#[test]
fn very_long_content_roundtrip() {
    let long = "x".repeat(100_000);
    let msg = Message::user(&long);
    let conv = messages_to_ir(&[msg]);
    let back = ir_to_messages(&conv);
    assert_eq!(back[0].content.as_deref().unwrap().len(), 100_000);
}

#[test]
fn tool_call_with_complex_json_arguments() {
    let complex_input = json!({
        "nested": {
            "array": [1, 2, {"key": "value"}],
            "null_field": null,
            "bool": true
        }
    });
    let args = serde_json::to_string(&complex_input).unwrap();
    let msg = Message::assistant_with_tool_calls(vec![ToolCall {
        id: "call_complex".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "complex_tool".into(),
            arguments: args,
        },
    }]);
    let conv = messages_to_ir(&[msg]);
    let back = ir_to_messages(&conv);
    let tc = &back[0].tool_calls.as_ref().unwrap()[0];
    let parsed: serde_json::Value = serde_json::from_str(&tc.function.arguments).unwrap();
    assert_eq!(parsed["nested"]["array"][2]["key"], "value");
}

#[test]
fn empty_tool_arguments() {
    let msg = Message::assistant_with_tool_calls(vec![ToolCall {
        id: "call_empty".into(),
        call_type: "function".into(),
        function: FunctionCall {
            name: "no_args".into(),
            arguments: "{}".into(),
        },
    }]);
    let conv = messages_to_ir(&[msg]);
    let back = ir_to_messages(&conv);
    let tc = &back[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.arguments, "{}");
}

#[test]
fn stream_event_serde_roundtrip() {
    let event = StreamEvent {
        id: "chatcmpl-test".into(),
        object: "chat.completion.chunk".into(),
        created: 1700000000,
        model: "gpt-4o".into(),
        choices: vec![StreamChoice {
            index: 0,
            delta: Delta {
                role: Some("assistant".into()),
                content: Some("hi".into()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "chatcmpl-test");
    assert_eq!(back.choices[0].delta.content.as_deref(), Some("hi"));
}

#[test]
fn usage_serde_roundtrip() {
    let usage = Usage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    let json = serde_json::to_string(&usage).unwrap();
    let back: Usage = serde_json::from_str(&json).unwrap();
    assert_eq!(back, usage);
}

#[test]
fn receipt_to_response_no_events() {
    let receipt = mock_receipt(vec![]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert!(resp.choices[0].message.content.is_none());
    assert!(resp.choices[0].message.tool_calls.is_none());
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

#[test]
fn receipt_to_response_default_usage_zeros() {
    let receipt = mock_receipt(vec![]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let u = resp.usage.unwrap();
    assert_eq!(u.prompt_tokens, 0);
    assert_eq!(u.completion_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

#[test]
fn tool_call_without_tool_use_id_gets_generated() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: "test".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        },
        ext: None,
    };
    let receipt = mock_receipt(vec![event]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let tc = &resp.choices[0].message.tool_calls.as_ref().unwrap()[0];
    assert!(tc.id.starts_with("call_"));
}

#[test]
fn multi_turn_with_tool_use_flow() {
    let messages = vec![
        Message::user("Read main.rs"),
        Message::assistant_with_tool_calls(vec![ToolCall {
            id: "c1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"main.rs"}"#.into(),
            },
        }]),
        Message::tool("c1", "fn main() {}"),
        Message::assistant("Done reading."),
    ];
    let conv = messages_to_ir(&messages);
    assert_eq!(conv.len(), 4);
    let back = ir_to_messages(&conv);
    assert_eq!(back.len(), 4);
    assert_eq!(back[2].tool_call_id.as_deref(), Some("c1"));
    assert_eq!(back[3].content.as_deref(), Some("Done reading."));
}

#[test]
fn conversation_system_message_accessor() {
    let messages = vec![Message::system("instructions"), Message::user("hi")];
    let conv = messages_to_ir(&messages);
    let sys = conv.system_message().unwrap();
    assert_eq!(sys.text_content(), "instructions");
}

#[test]
fn conversation_last_assistant_accessor() {
    let messages = vec![
        Message::user("hi"),
        Message::assistant("first"),
        Message::user("again"),
        Message::assistant("second"),
    ];
    let conv = messages_to_ir(&messages);
    let last = conv.last_assistant().unwrap();
    assert_eq!(last.text_content(), "second");
}

#[test]
fn conversation_tool_calls_accessor() {
    let messages = vec![Message::assistant_with_tool_calls(vec![
        ToolCall {
            id: "c1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "a".into(),
                arguments: "{}".into(),
            },
        },
        ToolCall {
            id: "c2".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "b".into(),
                arguments: "{}".into(),
            },
        },
    ])];
    let conv = messages_to_ir(&messages);
    assert_eq!(conv.tool_calls().len(), 2);
}
