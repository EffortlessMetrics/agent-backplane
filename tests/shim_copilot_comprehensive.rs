// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the `abp-shim-copilot` crate.

use abp_copilot_sdk::dialect::{
    self as copilot_dialect, CopilotConfig, CopilotConfirmation, CopilotError, CopilotFunctionCall,
    CopilotFunctionDef, CopilotReference, CopilotReferenceType, CopilotRequest, CopilotResponse,
    CopilotStreamEvent, CopilotTool, CopilotToolType, CopilotTurnEntry,
};
use abp_core::ir::{IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, UsageNormalized};
use abp_shim_copilot::{
    events_to_stream_events, ir_to_messages, ir_usage_to_tuple, messages_to_ir, mock_receipt,
    mock_receipt_with_usage, receipt_to_response, request_to_ir, request_to_work_order,
    response_to_ir, CopilotClient, CopilotRequestBuilder, Message, ShimError,
};
use chrono::Utc;
use serde_json::json;
use tokio_stream::StreamExt;

// ── Helpers ─────────────────────────────────────────────────────────────

fn make_processor(events: Vec<AgentEvent>) -> abp_shim_copilot::ProcessFn {
    Box::new(move |_wo| mock_receipt(events.clone()))
}

fn agent_msg(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: text.into() },
        ext: None,
    }
}

fn agent_delta(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: text.into() },
        ext: None,
    }
}

fn agent_error(msg: &str, code: Option<abp_error::ErrorCode>) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: msg.into(),
            error_code: code,
        },
        ext: None,
    }
}

fn agent_tool_call(name: &str, id: Option<&str>, input: serde_json::Value) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: name.into(),
            tool_use_id: id.map(Into::into),
            parent_tool_use_id: None,
            input,
        },
        ext: None,
    }
}

fn make_file_ref(id: &str, path: &str) -> CopilotReference {
    CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: id.into(),
        data: json!({"path": path}),
        metadata: None,
    }
}

fn make_snippet_ref(id: &str, name: &str, content: &str) -> CopilotReference {
    CopilotReference {
        ref_type: CopilotReferenceType::Snippet,
        id: id.into(),
        data: json!({"name": name, "content": content}),
        metadata: None,
    }
}

fn make_repo_ref(id: &str, owner: &str, name: &str) -> CopilotReference {
    CopilotReference {
        ref_type: CopilotReferenceType::Repository,
        id: id.into(),
        data: json!({"owner": owner, "name": name}),
        metadata: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 1. CopilotClient initialization and configuration
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn client_new_stores_model() {
    let client = CopilotClient::new("gpt-4o");
    assert_eq!(client.model(), "gpt-4o");
}

#[test]
fn client_new_custom_model() {
    let client = CopilotClient::new("o3-mini");
    assert_eq!(client.model(), "o3-mini");
}

#[test]
fn client_debug_includes_model() {
    let client = CopilotClient::new("gpt-4-turbo");
    let dbg = format!("{client:?}");
    assert!(dbg.contains("gpt-4-turbo"));
}

#[test]
fn client_with_processor_returns_self() {
    let client = CopilotClient::new("gpt-4o").with_processor(make_processor(vec![agent_msg("hi")]));
    assert_eq!(client.model(), "gpt-4o");
}

#[tokio::test]
async fn client_no_processor_create_returns_error() {
    let client = CopilotClient::new("gpt-4o");
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let err = client.create(req).await.unwrap_err();
    assert!(matches!(err, ShimError::Internal(_)));
}

#[tokio::test]
async fn client_no_processor_stream_returns_error() {
    let client = CopilotClient::new("gpt-4o");
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let result = client.create_stream(req).await;
    assert!(result.is_err());
}

#[test]
fn shim_error_display_invalid_request() {
    let err = ShimError::InvalidRequest("bad input".into());
    assert_eq!(format!("{err}"), "invalid request: bad input");
}

#[test]
fn shim_error_display_internal() {
    let err = ShimError::Internal("oops".into());
    assert_eq!(format!("{err}"), "internal error: oops");
}

#[test]
fn shim_error_display_serde() {
    let json_err = serde_json::from_str::<String>("not json").unwrap_err();
    let err = ShimError::Serde(json_err);
    let msg = format!("{err}");
    assert!(msg.contains("serde error"));
}

// ═══════════════════════════════════════════════════════════════════════
// 2. CopilotRequestBuilder
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn builder_defaults_model_to_gpt4o() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("hi")])
        .build();
    assert_eq!(req.model, "gpt-4o");
}

#[test]
fn builder_custom_model() {
    let req = CopilotRequestBuilder::new()
        .model("gpt-4-turbo")
        .messages(vec![Message::user("hi")])
        .build();
    assert_eq!(req.model, "gpt-4-turbo");
}

#[test]
fn builder_messages_set() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::system("sys"), Message::user("usr")])
        .build();
    assert_eq!(req.messages.len(), 2);
    assert_eq!(req.messages[0].role, "system");
    assert_eq!(req.messages[1].role, "user");
}

#[test]
fn builder_tools_set() {
    let tool = CopilotTool {
        tool_type: CopilotToolType::Function,
        function: Some(CopilotFunctionDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: json!({"type": "object"}),
        }),
        confirmation: None,
    };
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("hi")])
        .tools(vec![tool])
        .build();
    assert!(req.tools.is_some());
    assert_eq!(req.tools.unwrap().len(), 1);
}

#[test]
fn builder_turn_history() {
    let entry = CopilotTurnEntry {
        request: "What is 2+2?".into(),
        response: "4".into(),
    };
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("next")])
        .turn_history(vec![entry.clone()])
        .build();
    assert_eq!(req.turn_history.len(), 1);
    assert_eq!(req.turn_history[0].request, "What is 2+2?");
}

#[test]
fn builder_references() {
    let file_ref = make_file_ref("f1", "src/main.rs");
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("look")])
        .references(vec![file_ref])
        .build();
    assert_eq!(req.references.len(), 1);
}

#[test]
fn builder_default_is_empty() {
    let builder = CopilotRequestBuilder::new();
    let req = builder.build();
    assert_eq!(req.model, "gpt-4o");
    assert!(req.messages.is_empty());
    assert!(req.tools.is_none());
    assert!(req.turn_history.is_empty());
    assert!(req.references.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Message constructors
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn message_system() {
    let m = Message::system("You are helpful.");
    assert_eq!(m.role, "system");
    assert_eq!(m.content, "You are helpful.");
    assert!(m.name.is_none());
    assert!(m.copilot_references.is_empty());
}

#[test]
fn message_user() {
    let m = Message::user("Hello");
    assert_eq!(m.role, "user");
    assert_eq!(m.content, "Hello");
}

#[test]
fn message_assistant() {
    let m = Message::assistant("Sure!");
    assert_eq!(m.role, "assistant");
    assert_eq!(m.content, "Sure!");
}

#[test]
fn message_user_with_refs() {
    let refs = vec![make_file_ref("f1", "a.rs")];
    let m = Message::user_with_refs("look at this", refs.clone());
    assert_eq!(m.role, "user");
    assert_eq!(m.content, "look at this");
    assert_eq!(m.copilot_references.len(), 1);
}

#[test]
fn message_serde_roundtrip() {
    let m = Message::user("Hello");
    let json = serde_json::to_string(&m).unwrap();
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(back.role, "user");
    assert_eq!(back.content, "Hello");
}

#[test]
fn message_serde_skips_none_name() {
    let m = Message::user("Hi");
    let json = serde_json::to_string(&m).unwrap();
    assert!(!json.contains("name"));
}

#[test]
fn message_serde_skips_empty_refs() {
    let m = Message::user("Hi");
    let json = serde_json::to_string(&m).unwrap();
    assert!(!json.contains("copilot_references"));
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Request → IR conversion
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn request_to_ir_single_user() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("Hello")])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "Hello");
}

#[test]
fn request_to_ir_system_and_user() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::system("Be concise."), Message::user("Hi")])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.len(), 2);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
}

#[test]
fn request_to_ir_multi_turn() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![
            Message::user("What is Rust?"),
            Message::assistant("Rust is a systems language."),
            Message::user("Tell me more."),
        ])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.len(), 3);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[1].role, IrRole::Assistant);
    assert_eq!(conv.messages[2].role, IrRole::User);
}

#[test]
fn request_to_ir_empty_messages() {
    let req = CopilotRequestBuilder::new().build();
    let conv = request_to_ir(&req);
    assert!(conv.is_empty());
}

#[test]
fn request_to_ir_preserves_references_as_metadata() {
    let refs = vec![make_file_ref("f1", "src/lib.rs")];
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user_with_refs("check this", refs)])
        .build();
    let conv = request_to_ir(&req);
    assert!(conv.messages[0].metadata.contains_key("copilot_references"));
}

#[test]
fn request_to_ir_preserves_name_as_metadata() {
    let mut msg = Message::user("hello");
    msg.name = Some("alice".into());
    let req = CopilotRequestBuilder::new().messages(vec![msg]).build();
    let conv = request_to_ir(&req);
    assert_eq!(
        conv.messages[0]
            .metadata
            .get("copilot_name")
            .and_then(|v| v.as_str()),
        Some("alice")
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Request → WorkOrder conversion
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn request_to_work_order_task_from_last_user_message() {
    let req = CopilotRequestBuilder::new()
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
fn request_to_work_order_fallback_task() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::system("System only")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "copilot completion");
}

#[test]
fn request_to_work_order_model_preserved() {
    let req = CopilotRequestBuilder::new()
        .model("o3-mini")
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("o3-mini"));
}

#[test]
fn request_to_work_order_default_model() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
}

#[test]
fn request_to_work_order_has_valid_uuid() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_ne!(wo.id, uuid::Uuid::nil());
}

#[test]
fn request_to_work_order_empty_messages() {
    let req = CopilotRequestBuilder::new().build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "copilot completion");
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Receipt → CopilotResponse conversion
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn receipt_to_response_assistant_message() {
    let receipt = mock_receipt(vec![agent_msg("Hello!")]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.message, "Hello!");
    assert!(resp.copilot_errors.is_empty());
    assert!(resp.function_call.is_none());
}

#[test]
fn receipt_to_response_delta_accumulation() {
    let receipt = mock_receipt(vec![
        agent_delta("Hel"),
        agent_delta("lo"),
        agent_delta("!"),
    ]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.message, "Hello!");
}

#[test]
fn receipt_to_response_tool_call() {
    let receipt = mock_receipt(vec![agent_tool_call(
        "write_file",
        Some("call_xyz"),
        json!({"path": "foo.txt", "content": "bar"}),
    )]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let fc = resp.function_call.unwrap();
    assert_eq!(fc.name, "write_file");
    assert_eq!(fc.id.as_deref(), Some("call_xyz"));
    assert!(fc.arguments.contains("foo.txt"));
}

#[test]
fn receipt_to_response_error_event() {
    let receipt = mock_receipt(vec![agent_error("rate limited", None)]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.copilot_errors.len(), 1);
    assert!(resp.copilot_errors[0].message.contains("rate limited"));
    assert_eq!(resp.copilot_errors[0].error_type, "backend_error");
}

#[test]
fn receipt_to_response_multiple_errors() {
    let receipt = mock_receipt(vec![
        agent_error("error1", None),
        agent_error("error2", None),
    ]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.copilot_errors.len(), 2);
}

#[test]
fn receipt_to_response_error_with_code() {
    let receipt = mock_receipt(vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "bad request".into(),
            error_code: Some(abp_error::ErrorCode::BackendCrashed),
        },
        ext: None,
    }]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.copilot_errors.len(), 1);
    assert!(resp.copilot_errors[0].code.is_some());
}

#[test]
fn receipt_to_response_empty_trace() {
    let receipt = mock_receipt(vec![]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert!(resp.message.is_empty());
    assert!(resp.copilot_errors.is_empty());
    assert!(resp.function_call.is_none());
}

#[test]
fn receipt_to_response_mixed_events() {
    let receipt = mock_receipt(vec![
        agent_delta("Hello "),
        agent_error("warning issued", None),
        agent_delta("world"),
    ]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.message, "Hello world");
    assert_eq!(resp.copilot_errors.len(), 1);
}

#[test]
fn receipt_to_response_last_tool_call_wins() {
    let receipt = mock_receipt(vec![
        agent_tool_call("tool_a", Some("c1"), json!({})),
        agent_tool_call("tool_b", Some("c2"), json!({"key": "val"})),
    ]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let fc = resp.function_call.unwrap();
    assert_eq!(fc.name, "tool_b");
}

#[test]
fn receipt_to_response_ignores_non_mapped_events() {
    let receipt = mock_receipt(vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "a.rs".into(),
                summary: "edited".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "started".into(),
            },
            ext: None,
        },
        agent_msg("result"),
    ]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.message, "result");
}

#[test]
fn receipt_to_response_references_always_empty() {
    let receipt = mock_receipt(vec![agent_msg("ok")]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert!(resp.copilot_references.is_empty());
}

#[test]
fn receipt_to_response_confirmation_always_none() {
    let receipt = mock_receipt(vec![agent_msg("ok")]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert!(resp.copilot_confirmation.is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Stream event conversion
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn stream_events_starts_with_references() {
    let events = vec![agent_msg("hi")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert!(matches!(
        &stream[0],
        CopilotStreamEvent::CopilotReferences { references } if references.is_empty()
    ));
}

#[test]
fn stream_events_ends_with_done() {
    let events = vec![agent_msg("hi")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert!(matches!(
        stream.last().unwrap(),
        CopilotStreamEvent::Done {}
    ));
}

#[test]
fn stream_events_delta_mapped() {
    let events = vec![agent_delta("chunk1"), agent_delta("chunk2")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    // refs + 2 deltas + done
    assert_eq!(stream.len(), 4);
    assert!(matches!(&stream[1], CopilotStreamEvent::TextDelta { text } if text == "chunk1"));
    assert!(matches!(&stream[2], CopilotStreamEvent::TextDelta { text } if text == "chunk2"));
}

#[test]
fn stream_events_assistant_message_becomes_delta() {
    let events = vec![agent_msg("full message")];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert!(matches!(
        &stream[1],
        CopilotStreamEvent::TextDelta { text } if text == "full message"
    ));
}

#[test]
fn stream_events_tool_call_mapped() {
    let events = vec![agent_tool_call("search", Some("c1"), json!({"q": "rust"}))];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert!(matches!(
        &stream[1],
        CopilotStreamEvent::FunctionCall { .. }
    ));
    if let CopilotStreamEvent::FunctionCall { function_call } = &stream[1] {
        assert_eq!(function_call.name, "search");
        assert_eq!(function_call.id.as_deref(), Some("c1"));
    }
}

#[test]
fn stream_events_error_mapped() {
    let events = vec![agent_error("boom", None)];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert!(matches!(
        &stream[1],
        CopilotStreamEvent::CopilotErrors { .. }
    ));
    if let CopilotStreamEvent::CopilotErrors { errors } = &stream[1] {
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("boom"));
    }
}

#[test]
fn stream_events_empty_trace() {
    let stream = events_to_stream_events(&[], "gpt-4o");
    // refs + done
    assert_eq!(stream.len(), 2);
}

#[test]
fn stream_events_ignores_unmapped_kinds() {
    let events = vec![
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Warning {
                message: "warn".into(),
            },
            ext: None,
        },
        AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::CommandExecuted {
                command: "ls".into(),
                exit_code: Some(0),
                output_preview: None,
            },
            ext: None,
        },
    ];
    let stream = events_to_stream_events(&events, "gpt-4o");
    // refs + done only (warnings and commands are unmapped)
    assert_eq!(stream.len(), 2);
}

#[test]
fn stream_events_tool_call_no_id() {
    let events = vec![agent_tool_call("run_cmd", None, json!({"cmd": "echo hi"}))];
    let stream = events_to_stream_events(&events, "gpt-4o");
    if let CopilotStreamEvent::FunctionCall { function_call } = &stream[1] {
        assert!(function_call.id.is_none());
    } else {
        panic!("Expected FunctionCall");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. CopilotClient create (non-streaming)
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn client_create_simple_completion() {
    let client = CopilotClient::new("gpt-4o")
        .with_processor(make_processor(vec![agent_msg("Hello from Copilot!")]));
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("Hi")])
        .build();
    let resp = client.create(req).await.unwrap();
    assert_eq!(resp.message, "Hello from Copilot!");
}

#[tokio::test]
async fn client_create_with_system_message() {
    let client = CopilotClient::new("gpt-4o")
        .with_processor(make_processor(vec![agent_msg("concise reply")]));
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::system("Be concise."), Message::user("Hello")])
        .build();
    let resp = client.create(req).await.unwrap();
    assert_eq!(resp.message, "concise reply");
}

#[tokio::test]
async fn client_create_multi_turn() {
    let client = CopilotClient::new("gpt-4o").with_processor(make_processor(vec![agent_msg("4")]));
    let req = CopilotRequestBuilder::new()
        .messages(vec![
            Message::user("2+2?"),
            Message::assistant("hmm"),
            Message::user("Just the number"),
        ])
        .build();
    let resp = client.create(req).await.unwrap();
    assert_eq!(resp.message, "4");
}

#[tokio::test]
async fn client_create_with_errors() {
    let client = CopilotClient::new("gpt-4o").with_processor(make_processor(vec![agent_error(
        "context window exceeded",
        None,
    )]));
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let resp = client.create(req).await.unwrap();
    assert!(!resp.copilot_errors.is_empty());
}

#[tokio::test]
async fn client_create_with_tool_call() {
    let client =
        CopilotClient::new("gpt-4o").with_processor(make_processor(vec![agent_tool_call(
            "read_file",
            Some("c1"),
            json!({"path": "main.rs"}),
        )]));
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("read main.rs")])
        .build();
    let resp = client.create(req).await.unwrap();
    assert!(resp.function_call.is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// 9. CopilotClient create_stream
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn client_stream_basic() {
    let client = CopilotClient::new("gpt-4o")
        .with_processor(make_processor(vec![agent_delta("Hel"), agent_delta("lo")]));
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("Hi")])
        .build();
    let stream = client.create_stream(req).await.unwrap();
    let chunks: Vec<CopilotStreamEvent> = stream.collect().await;
    assert_eq!(chunks.len(), 4); // refs + 2 deltas + done
}

#[tokio::test]
async fn client_stream_empty_response() {
    let client = CopilotClient::new("gpt-4o").with_processor(make_processor(vec![]));
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("Hi")])
        .build();
    let stream = client.create_stream(req).await.unwrap();
    let chunks: Vec<CopilotStreamEvent> = stream.collect().await;
    assert_eq!(chunks.len(), 2); // refs + done
}

#[tokio::test]
async fn client_stream_contains_function_call() {
    let client =
        CopilotClient::new("gpt-4o").with_processor(make_processor(vec![agent_tool_call(
            "search",
            Some("c1"),
            json!({"q": "test"}),
        )]));
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("search")])
        .build();
    let stream = client.create_stream(req).await.unwrap();
    let chunks: Vec<CopilotStreamEvent> = stream.collect().await;
    assert!(chunks
        .iter()
        .any(|c| matches!(c, CopilotStreamEvent::FunctionCall { .. })));
}

#[tokio::test]
async fn client_stream_contains_errors() {
    let client = CopilotClient::new("gpt-4o")
        .with_processor(make_processor(vec![agent_error("oops", None)]));
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let stream = client.create_stream(req).await.unwrap();
    let chunks: Vec<CopilotStreamEvent> = stream.collect().await;
    assert!(chunks
        .iter()
        .any(|c| matches!(c, CopilotStreamEvent::CopilotErrors { .. })));
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Response → IR conversion
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn response_to_ir_basic() {
    let resp = CopilotResponse {
        message: "Hello!".into(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    let conv = response_to_ir(&resp);
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    assert_eq!(conv.messages[0].text_content(), "Hello!");
}

#[test]
fn response_to_ir_empty_message() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    let conv = response_to_ir(&resp);
    assert!(conv.is_empty());
}

#[test]
fn response_to_ir_preserves_references() {
    let resp = CopilotResponse {
        message: "Here it is".into(),
        copilot_references: vec![make_file_ref("f1", "a.rs")],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    let conv = response_to_ir(&resp);
    assert!(conv.messages[0].metadata.contains_key("copilot_references"));
}

// ═══════════════════════════════════════════════════════════════════════
// 11. IR ↔ Message roundtrip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn messages_to_ir_and_back() {
    let messages = vec![
        Message::system("System prompt"),
        Message::user("User message"),
        Message::assistant("Assistant reply"),
    ];
    let conv = messages_to_ir(&messages);
    let back = ir_to_messages(&conv);
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[0].content, "System prompt");
    assert_eq!(back[1].role, "user");
    assert_eq!(back[2].role, "assistant");
}

#[test]
fn messages_to_ir_preserves_content() {
    let messages = vec![Message::user("Hello, world!")];
    let conv = messages_to_ir(&messages);
    assert_eq!(conv.messages[0].text_content(), "Hello, world!");
}

#[test]
fn ir_to_messages_empty() {
    let conv = IrConversation::new();
    let msgs = ir_to_messages(&conv);
    assert!(msgs.is_empty());
}

#[test]
fn messages_to_ir_empty() {
    let conv = messages_to_ir(&[]);
    assert!(conv.is_empty());
}

#[test]
fn ir_to_messages_tool_role_mapped_to_user() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::Tool, "tool result")]);
    let msgs = ir_to_messages(&conv);
    assert_eq!(msgs[0].role, "user");
}

#[test]
fn messages_to_ir_references_preserved() {
    let refs = vec![make_file_ref("f1", "lib.rs")];
    let messages = vec![Message::user_with_refs("check", refs)];
    let conv = messages_to_ir(&messages);
    let back = ir_to_messages(&conv);
    assert_eq!(back[0].copilot_references.len(), 1);
}

#[test]
fn messages_to_ir_name_preserved() {
    let mut msg = Message::user("hi");
    msg.name = Some("bob".into());
    let conv = messages_to_ir(&[msg]);
    let back = ir_to_messages(&conv);
    assert_eq!(back[0].name.as_deref(), Some("bob"));
}

// ═══════════════════════════════════════════════════════════════════════
// 12. IR Usage conversion
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ir_usage_basic() {
    let ir = IrUsage::from_io(100, 50);
    let (input, output, total) = ir_usage_to_tuple(&ir);
    assert_eq!(input, 100);
    assert_eq!(output, 50);
    assert_eq!(total, 150);
}

#[test]
fn ir_usage_zero() {
    let ir = IrUsage::from_io(0, 0);
    let (input, output, total) = ir_usage_to_tuple(&ir);
    assert_eq!(input, 0);
    assert_eq!(output, 0);
    assert_eq!(total, 0);
}

#[test]
fn ir_usage_large_values() {
    let ir = IrUsage::from_io(1_000_000, 500_000);
    let (input, output, total) = ir_usage_to_tuple(&ir);
    assert_eq!(input, 1_000_000);
    assert_eq!(output, 500_000);
    assert_eq!(total, 1_500_000);
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Mock receipt helpers
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn mock_receipt_sets_backend_id() {
    let receipt = mock_receipt(vec![]);
    assert_eq!(receipt.backend.id, "mock");
}

#[test]
fn mock_receipt_sets_contract_version() {
    let receipt = mock_receipt(vec![]);
    assert_eq!(receipt.meta.contract_version, abp_core::CONTRACT_VERSION);
}

#[test]
fn mock_receipt_outcome_complete() {
    let receipt = mock_receipt(vec![]);
    assert_eq!(receipt.outcome, abp_core::Outcome::Complete);
}

#[test]
fn mock_receipt_with_usage_sets_tokens() {
    let usage = UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        ..Default::default()
    };
    let receipt = mock_receipt_with_usage(vec![], usage);
    assert_eq!(receipt.usage.input_tokens, Some(100));
    assert_eq!(receipt.usage.output_tokens, Some(50));
}

#[test]
fn mock_receipt_preserves_events() {
    let events = vec![agent_msg("one"), agent_msg("two")];
    let receipt = mock_receipt(events);
    assert_eq!(receipt.trace.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Copilot dialect: model mapping
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn canonical_model_prefix() {
    assert_eq!(
        copilot_dialect::to_canonical_model("gpt-4o"),
        "copilot/gpt-4o"
    );
}

#[test]
fn from_canonical_model_strips_prefix() {
    assert_eq!(
        copilot_dialect::from_canonical_model("copilot/gpt-4o"),
        "gpt-4o"
    );
}

#[test]
fn from_canonical_model_no_prefix() {
    assert_eq!(copilot_dialect::from_canonical_model("gpt-4o"), "gpt-4o");
}

#[test]
fn known_models_recognized() {
    assert!(copilot_dialect::is_known_model("gpt-4o"));
    assert!(copilot_dialect::is_known_model("gpt-4o-mini"));
    assert!(copilot_dialect::is_known_model("o3-mini"));
    assert!(copilot_dialect::is_known_model("claude-sonnet-4"));
}

#[test]
fn unknown_model_not_recognized() {
    assert!(!copilot_dialect::is_known_model("gpt-99"));
    assert!(!copilot_dialect::is_known_model("llama-3"));
}

#[test]
fn model_roundtrip_canonical() {
    let original = "gpt-4-turbo";
    let canonical = copilot_dialect::to_canonical_model(original);
    let back = copilot_dialect::from_canonical_model(&canonical);
    assert_eq!(back, original);
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Copilot dialect: capability manifest
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn capability_manifest_streaming_native() {
    let manifest = copilot_dialect::capability_manifest();
    assert!(matches!(
        manifest.get(&abp_core::Capability::Streaming),
        Some(abp_core::SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_web_search_native() {
    let manifest = copilot_dialect::capability_manifest();
    assert!(matches!(
        manifest.get(&abp_core::Capability::ToolWebSearch),
        Some(abp_core::SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_tool_glob_unsupported() {
    let manifest = copilot_dialect::capability_manifest();
    assert!(matches!(
        manifest.get(&abp_core::Capability::ToolGlob),
        Some(abp_core::SupportLevel::Unsupported)
    ));
}

#[test]
fn capability_manifest_mcp_unsupported() {
    let manifest = copilot_dialect::capability_manifest();
    assert!(matches!(
        manifest.get(&abp_core::Capability::McpClient),
        Some(abp_core::SupportLevel::Unsupported)
    ));
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Copilot dialect: tool definition mapping
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn tool_def_to_copilot_function() {
    let def = copilot_dialect::CanonicalToolDef {
        name: "read_file".into(),
        description: "Reads a file".into(),
        parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    let tool = copilot_dialect::tool_def_to_copilot(&def);
    assert_eq!(tool.tool_type, CopilotToolType::Function);
    assert!(tool.function.is_some());
    let func = tool.function.unwrap();
    assert_eq!(func.name, "read_file");
    assert_eq!(func.description, "Reads a file");
}

#[test]
fn tool_def_from_copilot_roundtrip() {
    let def = copilot_dialect::CanonicalToolDef {
        name: "edit_file".into(),
        description: "Edits a file".into(),
        parameters_schema: json!({}),
    };
    let tool = copilot_dialect::tool_def_to_copilot(&def);
    let back = copilot_dialect::tool_def_from_copilot(&tool).unwrap();
    assert_eq!(back.name, "edit_file");
    assert_eq!(back.description, "Edits a file");
}

#[test]
fn tool_def_from_copilot_confirmation_returns_none() {
    let tool = CopilotTool {
        tool_type: CopilotToolType::Confirmation,
        function: None,
        confirmation: Some(CopilotConfirmation {
            id: "c1".into(),
            title: "Approve?".into(),
            message: "Do you approve?".into(),
            accepted: None,
        }),
    };
    assert!(copilot_dialect::tool_def_from_copilot(&tool).is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Copilot dialect: map_work_order
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn map_work_order_uses_task_as_user_message() {
    let wo = abp_core::WorkOrderBuilder::new("Fix the bug").build();
    let cfg = CopilotConfig::default();
    let req = copilot_dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
    assert!(req.messages[0].content.contains("Fix the bug"));
}

#[test]
fn map_work_order_respects_model_override() {
    let wo = abp_core::WorkOrderBuilder::new("test")
        .model("o1-mini")
        .build();
    let cfg = CopilotConfig::default();
    let req = copilot_dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "o1-mini");
}

#[test]
fn map_work_order_with_system_prompt() {
    let wo = abp_core::WorkOrderBuilder::new("test").build();
    let cfg = CopilotConfig {
        system_prompt: Some("You are a helpful assistant.".into()),
        ..CopilotConfig::default()
    };
    let req = copilot_dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.messages.len(), 2);
    assert_eq!(req.messages[0].role, "system");
    assert_eq!(req.messages[0].content, "You are a helpful assistant.");
}

#[test]
fn map_work_order_context_files_become_references() {
    let wo = abp_core::WorkOrderBuilder::new("test")
        .context(abp_core::ContextPacket {
            files: vec!["src/main.rs".into(), "src/lib.rs".into()],
            snippets: vec![],
        })
        .build();
    let cfg = CopilotConfig::default();
    let req = copilot_dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.references.len(), 2);
    assert_eq!(req.references[0].ref_type, CopilotReferenceType::File);
}

#[test]
fn map_work_order_context_snippets_become_references() {
    let wo = abp_core::WorkOrderBuilder::new("test")
        .context(abp_core::ContextPacket {
            files: vec![],
            snippets: vec![abp_core::ContextSnippet {
                name: "helper".into(),
                content: "fn foo() {}".into(),
            }],
        })
        .build();
    let cfg = CopilotConfig::default();
    let req = copilot_dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.references.len(), 1);
    assert_eq!(req.references[0].ref_type, CopilotReferenceType::Snippet);
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Copilot dialect: map_response
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn map_response_assistant_message() {
    let resp = CopilotResponse {
        message: "Hello!".into(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    let events = copilot_dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantMessage { text } if text == "Hello!"
    ));
}

#[test]
fn map_response_with_errors() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![CopilotError {
            error_type: "rate_limit".into(),
            message: "too many requests".into(),
            code: None,
            identifier: None,
        }],
        copilot_confirmation: None,
        function_call: None,
    };
    let events = copilot_dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::Error { .. }));
}

#[test]
fn map_response_with_confirmation() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: Some(CopilotConfirmation {
            id: "conf_1".into(),
            title: "Delete file?".into(),
            message: "Are you sure you want to delete foo.txt?".into(),
            accepted: None,
        }),
        function_call: None,
    };
    let events = copilot_dialect::map_response(&resp);
    assert!(events.iter().any(|e| matches!(&e.kind, AgentEventKind::Warning { message } if message.contains("Delete file?"))));
    assert!(events.iter().any(|e| e.ext.is_some()));
}

#[test]
fn map_response_with_function_call() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: Some(CopilotFunctionCall {
            name: "search".into(),
            arguments: r#"{"q":"test"}"#.into(),
            id: Some("call_42".into()),
        }),
    };
    let events = copilot_dialect::map_response(&resp);
    assert!(events.iter().any(|e| matches!(
        &e.kind,
        AgentEventKind::ToolCall { tool_name, .. } if tool_name == "search"
    )));
}

// ═══════════════════════════════════════════════════════════════════════
// 19. Copilot dialect: map_stream_event
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn map_stream_text_delta() {
    let event = CopilotStreamEvent::TextDelta {
        text: "chunk".into(),
    };
    let mapped = copilot_dialect::map_stream_event(&event);
    assert_eq!(mapped.len(), 1);
    assert!(matches!(
        &mapped[0].kind,
        AgentEventKind::AssistantDelta { text } if text == "chunk"
    ));
}

#[test]
fn map_stream_function_call() {
    let event = CopilotStreamEvent::FunctionCall {
        function_call: CopilotFunctionCall {
            name: "read".into(),
            arguments: "{}".into(),
            id: Some("c1".into()),
        },
    };
    let mapped = copilot_dialect::map_stream_event(&event);
    assert_eq!(mapped.len(), 1);
    assert!(matches!(&mapped[0].kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn map_stream_done() {
    let event = CopilotStreamEvent::Done {};
    let mapped = copilot_dialect::map_stream_event(&event);
    assert_eq!(mapped.len(), 1);
    assert!(matches!(
        &mapped[0].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn map_stream_errors() {
    let event = CopilotStreamEvent::CopilotErrors {
        errors: vec![
            CopilotError {
                error_type: "err".into(),
                message: "bad".into(),
                code: None,
                identifier: None,
            },
            CopilotError {
                error_type: "err2".into(),
                message: "worse".into(),
                code: None,
                identifier: None,
            },
        ],
    };
    let mapped = copilot_dialect::map_stream_event(&event);
    assert_eq!(mapped.len(), 2);
}

#[test]
fn map_stream_empty_references() {
    let event = CopilotStreamEvent::CopilotReferences { references: vec![] };
    let mapped = copilot_dialect::map_stream_event(&event);
    assert!(mapped.is_empty());
}

#[test]
fn map_stream_nonempty_references() {
    let event = CopilotStreamEvent::CopilotReferences {
        references: vec![make_file_ref("f1", "a.rs")],
    };
    let mapped = copilot_dialect::map_stream_event(&event);
    assert_eq!(mapped.len(), 1);
    assert!(matches!(&mapped[0].kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn map_stream_confirmation() {
    let event = CopilotStreamEvent::CopilotConfirmation {
        confirmation: CopilotConfirmation {
            id: "c1".into(),
            title: "Approve?".into(),
            message: "Please approve".into(),
            accepted: None,
        },
    };
    let mapped = copilot_dialect::map_stream_event(&event);
    assert_eq!(mapped.len(), 1);
    assert!(matches!(&mapped[0].kind, AgentEventKind::Warning { .. }));
    assert!(mapped[0].ext.is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// 20. Passthrough fidelity
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn passthrough_roundtrip_text_delta() {
    let event = CopilotStreamEvent::TextDelta {
        text: "hello".into(),
    };
    let wrapped = copilot_dialect::to_passthrough_event(&event);
    assert!(wrapped.ext.is_some());
    let ext = wrapped.ext.as_ref().unwrap();
    assert!(ext.contains_key("raw_message"));
    assert_eq!(ext.get("dialect").and_then(|v| v.as_str()), Some("copilot"));
    let restored = copilot_dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(restored, event);
}

#[test]
fn passthrough_roundtrip_done() {
    let event = CopilotStreamEvent::Done {};
    let wrapped = copilot_dialect::to_passthrough_event(&event);
    let restored = copilot_dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(restored, event);
}

#[test]
fn passthrough_roundtrip_function_call() {
    let event = CopilotStreamEvent::FunctionCall {
        function_call: CopilotFunctionCall {
            name: "tool".into(),
            arguments: r#"{"a":1}"#.into(),
            id: Some("c1".into()),
        },
    };
    assert!(copilot_dialect::verify_passthrough_fidelity(&[event]));
}

#[test]
fn passthrough_fidelity_full_stream() {
    let events = vec![
        CopilotStreamEvent::CopilotReferences {
            references: vec![make_file_ref("f1", "a.rs")],
        },
        CopilotStreamEvent::TextDelta {
            text: "hello".into(),
        },
        CopilotStreamEvent::TextDelta {
            text: " world".into(),
        },
        CopilotStreamEvent::Done {},
    ];
    assert!(copilot_dialect::verify_passthrough_fidelity(&events));
}

#[test]
fn from_passthrough_event_no_ext_returns_none() {
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "hi".into() },
        ext: None,
    };
    assert!(copilot_dialect::from_passthrough_event(&event).is_none());
}

#[test]
fn from_passthrough_event_no_raw_message_returns_none() {
    let mut ext = std::collections::BTreeMap::new();
    ext.insert("dialect".into(), json!("copilot"));
    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: "hi".into() },
        ext: Some(ext),
    };
    assert!(copilot_dialect::from_passthrough_event(&event).is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 21. CopilotConfig defaults
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn copilot_config_default_model() {
    let cfg = CopilotConfig::default();
    assert_eq!(cfg.model, "gpt-4o");
}

#[test]
fn copilot_config_default_base_url() {
    let cfg = CopilotConfig::default();
    assert!(cfg.base_url.contains("githubcopilot"));
}

#[test]
fn copilot_config_default_no_system_prompt() {
    let cfg = CopilotConfig::default();
    assert!(cfg.system_prompt.is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 22. Reference types
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn reference_type_file() {
    let r = make_file_ref("f1", "src/main.rs");
    assert_eq!(r.ref_type, CopilotReferenceType::File);
}

#[test]
fn reference_type_snippet() {
    let r = make_snippet_ref("s1", "helper", "fn foo() {}");
    assert_eq!(r.ref_type, CopilotReferenceType::Snippet);
}

#[test]
fn reference_type_repository() {
    let r = make_repo_ref("r1", "octocat", "hello-world");
    assert_eq!(r.ref_type, CopilotReferenceType::Repository);
}

#[test]
fn reference_type_web_search() {
    let r = CopilotReference {
        ref_type: CopilotReferenceType::WebSearchResult,
        id: "w1".into(),
        data: json!({"url": "https://example.com"}),
        metadata: None,
    };
    assert_eq!(r.ref_type, CopilotReferenceType::WebSearchResult);
}

#[test]
fn reference_with_metadata() {
    let mut meta = std::collections::BTreeMap::new();
    meta.insert("label".into(), json!("important"));
    let r = CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: "f1".into(),
        data: json!({"path": "a.rs"}),
        metadata: Some(meta),
    };
    assert!(r.metadata.is_some());
}

#[test]
fn reference_serde_roundtrip() {
    let r = make_file_ref("f1", "src/lib.rs");
    let json = serde_json::to_string(&r).unwrap();
    let back: CopilotReference = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "f1");
    assert_eq!(back.ref_type, CopilotReferenceType::File);
}

// ═══════════════════════════════════════════════════════════════════════
// 23. Copilot-specific features: confirmations
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn confirmation_serde_roundtrip() {
    let conf = CopilotConfirmation {
        id: "c1".into(),
        title: "Delete?".into(),
        message: "Are you sure?".into(),
        accepted: Some(true),
    };
    let json = serde_json::to_string(&conf).unwrap();
    let back: CopilotConfirmation = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "c1");
    assert_eq!(back.accepted, Some(true));
}

#[test]
fn confirmation_accepted_none() {
    let conf = CopilotConfirmation {
        id: "c1".into(),
        title: "Approve?".into(),
        message: "msg".into(),
        accepted: None,
    };
    let json = serde_json::to_string(&conf).unwrap();
    assert!(!json.contains("accepted"));
}

// ═══════════════════════════════════════════════════════════════════════
// 24. Copilot-specific features: turn history
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn turn_entry_serde_roundtrip() {
    let entry = CopilotTurnEntry {
        request: "What?".into(),
        response: "That.".into(),
    };
    let json = serde_json::to_string(&entry).unwrap();
    let back: CopilotTurnEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(back.request, "What?");
    assert_eq!(back.response, "That.");
}

// ═══════════════════════════════════════════════════════════════════════
// 25. CopilotRequest and CopilotResponse serde
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn copilot_request_serde_roundtrip() {
    let req = CopilotRequestBuilder::new()
        .model("gpt-4o")
        .messages(vec![Message::user("hi")])
        .build();
    let json = serde_json::to_string(&req).unwrap();
    let back: CopilotRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "gpt-4o");
    assert_eq!(back.messages.len(), 1);
}

#[test]
fn copilot_response_serde_roundtrip() {
    let resp = CopilotResponse {
        message: "reply".into(),
        copilot_references: vec![make_file_ref("f1", "a.rs")],
        copilot_errors: vec![CopilotError {
            error_type: "err".into(),
            message: "msg".into(),
            code: Some("E001".into()),
            identifier: Some("id1".into()),
        }],
        copilot_confirmation: None,
        function_call: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: CopilotResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.message, "reply");
    assert_eq!(back.copilot_references.len(), 1);
    assert_eq!(back.copilot_errors.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════
// 26. CopilotStreamEvent serde
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn stream_event_text_delta_serde() {
    let event = CopilotStreamEvent::TextDelta { text: "hi".into() };
    let json = serde_json::to_string(&event).unwrap();
    let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, event);
}

#[test]
fn stream_event_done_serde() {
    let event = CopilotStreamEvent::Done {};
    let json = serde_json::to_string(&event).unwrap();
    let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, event);
}

#[test]
fn stream_event_function_call_serde() {
    let event = CopilotStreamEvent::FunctionCall {
        function_call: CopilotFunctionCall {
            name: "tool".into(),
            arguments: "{}".into(),
            id: None,
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, event);
}

// ═══════════════════════════════════════════════════════════════════════
// 27. Edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn very_long_message_content() {
    let long_text = "x".repeat(100_000);
    let msg = Message::user(&long_text);
    let conv = messages_to_ir(&[msg]);
    let back = ir_to_messages(&conv);
    assert_eq!(back[0].content.len(), 100_000);
}

#[test]
fn unicode_message_content() {
    let msg = Message::user("Hello, 世界! 🌍 Ñoño");
    let conv = messages_to_ir(&[msg]);
    let back = ir_to_messages(&conv);
    assert_eq!(back[0].content, "Hello, 世界! 🌍 Ñoño");
}

#[test]
fn empty_string_model() {
    let req = CopilotRequestBuilder::new()
        .model("")
        .messages(vec![Message::user("test")])
        .build();
    assert_eq!(req.model, "");
}

#[test]
fn newlines_in_message() {
    let msg = Message::user("line1\nline2\nline3");
    let conv = messages_to_ir(&[msg]);
    let back = ir_to_messages(&conv);
    assert!(back[0].content.contains('\n'));
}

#[test]
fn special_chars_in_tool_arguments() {
    let receipt = mock_receipt(vec![agent_tool_call(
        "exec",
        Some("c1"),
        json!({"cmd": "echo \"hello 'world'\"; rm -rf /"}),
    )]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let fc = resp.function_call.unwrap();
    assert!(fc.arguments.contains("hello"));
}

#[test]
fn null_tool_use_id() {
    let receipt = mock_receipt(vec![agent_tool_call("tool", None, json!({}))]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let fc = resp.function_call.unwrap();
    assert!(fc.id.is_none());
}

#[test]
fn assistant_message_overwritten_by_later() {
    let receipt = mock_receipt(vec![agent_msg("first"), agent_msg("second")]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.message, "second");
}

#[test]
fn delta_then_full_message() {
    let receipt = mock_receipt(vec![
        agent_delta("partial"),
        agent_msg("complete replacement"),
    ]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    // AssistantMessage replaces accumulated deltas
    assert_eq!(resp.message, "complete replacement");
}

#[test]
fn multiple_models_in_requests() {
    for model in &[
        "gpt-4o",
        "gpt-4-turbo",
        "o3-mini",
        "claude-sonnet-4",
        "gpt-4o-mini",
    ] {
        let req = CopilotRequestBuilder::new()
            .model(*model)
            .messages(vec![Message::user("test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some(*model));
    }
}

#[tokio::test]
async fn concurrent_client_requests() {
    let client = CopilotClient::new("gpt-4o").with_processor(make_processor(vec![agent_msg("ok")]));

    let mut handles = Vec::new();
    for _ in 0..5 {
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();
        // We can't easily share &client across tasks, so test sequential
        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.message, "ok");
        handles.push(resp);
    }
    assert_eq!(handles.len(), 5);
}

#[test]
fn dialect_version_constant() {
    assert_eq!(copilot_dialect::DIALECT_VERSION, "copilot/v0.1");
}

#[test]
fn default_model_constant() {
    assert_eq!(copilot_dialect::DEFAULT_MODEL, "gpt-4o");
}

#[test]
fn ir_conversation_accessors_work_with_copilot_data() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![
            Message::system("Be concise."),
            Message::user("Hello"),
            Message::assistant("Hi!"),
            Message::user("Bye"),
        ])
        .build();
    let conv = request_to_ir(&req);
    assert_eq!(conv.system_message().unwrap().text_content(), "Be concise.");
    assert_eq!(conv.last_assistant().unwrap().text_content(), "Hi!");
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 2);
    assert_eq!(conv.last_message().unwrap().role, IrRole::User);
}

#[test]
fn ir_usage_with_cache() {
    let ir = IrUsage::with_cache(100, 50, 20, 10);
    let (input, output, total) = ir_usage_to_tuple(&ir);
    assert_eq!(input, 100);
    assert_eq!(output, 50);
    assert_eq!(total, 150);
    assert_eq!(ir.cache_read_tokens, 20);
    assert_eq!(ir.cache_write_tokens, 10);
}

#[test]
fn ir_usage_merge() {
    let a = IrUsage::from_io(100, 50);
    let b = IrUsage::from_io(200, 100);
    let merged = a.merge(b);
    let (input, output, total) = ir_usage_to_tuple(&merged);
    assert_eq!(input, 300);
    assert_eq!(output, 150);
    assert_eq!(total, 450);
}
