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
//! Comprehensive integration tests for the `abp-shim-copilot` crate.

use std::collections::BTreeMap;

use abp_copilot_sdk::dialect::{
    self, CanonicalToolDef, CopilotConfig, CopilotConfirmation, CopilotError, CopilotFunctionCall,
    CopilotMessage, CopilotReference, CopilotReferenceType, CopilotRequest, CopilotResponse,
    CopilotStreamEvent, CopilotTool, CopilotToolType,
};
use abp_copilot_sdk::lowering;
use abp_core::ir::{IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, WorkOrderBuilder};
use abp_shim_copilot::{
    events_to_stream_events, ir_to_messages, ir_usage_to_tuple, messages_to_ir, mock_receipt,
    mock_receipt_with_usage, receipt_to_response, request_to_ir, request_to_work_order,
    response_to_ir, CopilotClient, CopilotFunctionDef, CopilotRequestBuilder, Message, ShimError,
};
use chrono::Utc;
use serde_json::json;
use tokio_stream::StreamExt;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn make_processor(
    events: Vec<AgentEvent>,
) -> Box<dyn Fn(&abp_core::WorkOrder) -> abp_core::Receipt + Send + Sync> {
    Box::new(move |_wo| mock_receipt(events.clone()))
}

fn agent_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn sample_reference(ref_type: CopilotReferenceType, id: &str) -> CopilotReference {
    CopilotReference {
        ref_type,
        id: id.into(),
        data: json!({"path": "test.rs"}),
        metadata: None,
    }
}

fn sample_tool_function(name: &str) -> CopilotTool {
    CopilotTool {
        tool_type: CopilotToolType::Function,
        function: Some(CopilotFunctionDef {
            name: name.into(),
            description: format!("Description for {name}"),
            parameters: json!({"type": "object", "properties": {"arg": {"type": "string"}}}),
        }),
        confirmation: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Copilot SDK types fidelity (serde round-trips)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn serde_copilot_message_roundtrip() {
    let msg = CopilotMessage {
        role: "user".into(),
        content: "Hello".into(),
        name: Some("alice".into()),
        copilot_references: vec![],
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: CopilotMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.role, "user");
    assert_eq!(back.content, "Hello");
    assert_eq!(back.name.as_deref(), Some("alice"));
}

#[test]
fn serde_copilot_reference_file_roundtrip() {
    let r = sample_reference(CopilotReferenceType::File, "file-0");
    let json = serde_json::to_string(&r).unwrap();
    let back: CopilotReference = serde_json::from_str(&json).unwrap();
    assert_eq!(back.ref_type, CopilotReferenceType::File);
    assert_eq!(back.id, "file-0");
}

#[test]
fn serde_copilot_reference_snippet_roundtrip() {
    let r = CopilotReference {
        ref_type: CopilotReferenceType::Snippet,
        id: "snippet-0".into(),
        data: json!({"name": "helper.rs", "content": "fn foo() {}"}),
        metadata: Some({
            let mut m = BTreeMap::new();
            m.insert("label".into(), json!("helper"));
            m
        }),
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: CopilotReference = serde_json::from_str(&json).unwrap();
    assert_eq!(back.ref_type, CopilotReferenceType::Snippet);
    assert!(back.metadata.is_some());
}

#[test]
fn serde_copilot_reference_repository_roundtrip() {
    let r = CopilotReference {
        ref_type: CopilotReferenceType::Repository,
        id: "repo-1".into(),
        data: json!({"owner": "octocat", "name": "hello-world"}),
        metadata: None,
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: CopilotReference = serde_json::from_str(&json).unwrap();
    assert_eq!(back.ref_type, CopilotReferenceType::Repository);
}

#[test]
fn serde_copilot_reference_web_search_roundtrip() {
    let r = CopilotReference {
        ref_type: CopilotReferenceType::WebSearchResult,
        id: "web-0".into(),
        data: json!({"url": "https://example.com", "title": "Example"}),
        metadata: None,
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: CopilotReference = serde_json::from_str(&json).unwrap();
    assert_eq!(back.ref_type, CopilotReferenceType::WebSearchResult);
}

#[test]
fn serde_copilot_tool_function_roundtrip() {
    let tool = sample_tool_function("read_file");
    let json = serde_json::to_string(&tool).unwrap();
    let back: CopilotTool = serde_json::from_str(&json).unwrap();
    assert_eq!(back.tool_type, CopilotToolType::Function);
    assert_eq!(back.function.unwrap().name, "read_file");
}

#[test]
fn serde_copilot_tool_confirmation_roundtrip() {
    let tool = CopilotTool {
        tool_type: CopilotToolType::Confirmation,
        function: None,
        confirmation: Some(CopilotConfirmation {
            id: "conf-1".into(),
            title: "Delete file?".into(),
            message: "This will delete src/main.rs".into(),
            accepted: None,
        }),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: CopilotTool = serde_json::from_str(&json).unwrap();
    assert_eq!(back.tool_type, CopilotToolType::Confirmation);
    assert!(back.confirmation.is_some());
    assert!(back.function.is_none());
}

#[test]
fn serde_copilot_function_call_roundtrip() {
    let fc = CopilotFunctionCall {
        name: "search".into(),
        arguments: r#"{"query":"rust"}"#.into(),
        id: Some("call_abc".into()),
    };
    let json = serde_json::to_string(&fc).unwrap();
    let back: CopilotFunctionCall = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "search");
    assert_eq!(back.id.as_deref(), Some("call_abc"));
}

#[test]
fn serde_copilot_error_roundtrip() {
    let err = CopilotError {
        error_type: "rate_limit".into(),
        message: "Too many requests".into(),
        code: Some("429".into()),
        identifier: Some("err-123".into()),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: CopilotError = serde_json::from_str(&json).unwrap();
    assert_eq!(back.error_type, "rate_limit");
    assert_eq!(back.code.as_deref(), Some("429"));
    assert_eq!(back.identifier.as_deref(), Some("err-123"));
}

#[test]
fn serde_copilot_request_roundtrip() {
    let req = CopilotRequest {
        model: "gpt-4o".into(),
        messages: vec![CopilotMessage {
            role: "user".into(),
            content: "Hello".into(),
            name: None,
            copilot_references: vec![],
        }],
        tools: Some(vec![sample_tool_function("write_file")]),
        turn_history: vec![],
        references: vec![],
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: CopilotRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "gpt-4o");
    assert_eq!(back.messages.len(), 1);
    assert!(back.tools.is_some());
}

#[test]
fn serde_copilot_response_roundtrip() {
    let resp = CopilotResponse {
        message: "Hi there".into(),
        copilot_references: vec![sample_reference(CopilotReferenceType::File, "f-0")],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: CopilotResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.message, "Hi there");
    assert_eq!(back.copilot_references.len(), 1);
}

#[test]
fn serde_copilot_config_roundtrip() {
    let cfg = CopilotConfig {
        token: "ghp_test".into(),
        base_url: "https://api.githubcopilot.com".into(),
        model: "gpt-4o".into(),
        system_prompt: Some("Be helpful".into()),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: CopilotConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, "gpt-4o");
    assert_eq!(back.system_prompt.as_deref(), Some("Be helpful"));
}

#[test]
fn serde_stream_event_text_delta_roundtrip() {
    let e = CopilotStreamEvent::TextDelta {
        text: "hello".into(),
    };
    let json = serde_json::to_string(&e).unwrap();
    let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, e);
}

#[test]
fn serde_stream_event_done_roundtrip() {
    let e = CopilotStreamEvent::Done {};
    let json = serde_json::to_string(&e).unwrap();
    let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, e);
}

#[test]
fn serde_stream_event_function_call_roundtrip() {
    let e = CopilotStreamEvent::FunctionCall {
        function_call: CopilotFunctionCall {
            name: "run".into(),
            arguments: "{}".into(),
            id: Some("c1".into()),
        },
    };
    let json = serde_json::to_string(&e).unwrap();
    let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, e);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Request translation: Copilot → ABP WorkOrder
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_translation_basic_user_message() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("Fix the bug")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Fix the bug");
}

#[test]
fn request_translation_model_propagated() {
    let req = CopilotRequestBuilder::new()
        .model("gpt-4-turbo")
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
}

#[test]
fn request_translation_default_model_is_gpt4o() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
}

#[test]
fn request_translation_system_and_user_messages() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![
            Message::system("You are a coding assistant"),
            Message::user("Write a test"),
        ])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Write a test");
}

#[test]
fn request_translation_multi_user_uses_last() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![
            Message::user("First question"),
            Message::assistant("First answer"),
            Message::user("Second question"),
        ])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.task, "Second question");
}

#[test]
fn request_translation_to_ir_preserves_roles() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![
            Message::system("sys"),
            Message::user("usr"),
            Message::assistant("ast"),
        ])
        .build();
    let ir = request_to_ir(&req);
    assert_eq!(ir.messages[0].role, IrRole::System);
    assert_eq!(ir.messages[1].role, IrRole::User);
    assert_eq!(ir.messages[2].role, IrRole::Assistant);
}

#[test]
fn request_translation_to_ir_preserves_content() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("Hello world")])
        .build();
    let ir = request_to_ir(&req);
    assert_eq!(ir.messages[0].text_content(), "Hello world");
}

#[test]
fn request_translation_with_references() {
    let refs = vec![sample_reference(CopilotReferenceType::File, "file-0")];
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user_with_refs("Check file", refs)])
        .build();
    let ir = request_to_ir(&req);
    assert!(ir.messages[0].metadata.contains_key("copilot_references"));
}

#[test]
fn request_translation_ir_roundtrip_messages() {
    let messages = vec![
        Message::system("System prompt"),
        Message::user("User message"),
        Message::assistant("Reply"),
    ];
    let ir = messages_to_ir(&messages);
    let back = ir_to_messages(&ir);
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[0].content, "System prompt");
    assert_eq!(back[1].role, "user");
    assert_eq!(back[2].role, "assistant");
}

#[test]
fn request_translation_work_order_has_contract_fields() {
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("task")])
        .build();
    let wo = request_to_work_order(&req);
    assert!(!wo.id.is_nil());
    assert!(!wo.task.is_empty());
}

#[test]
fn request_translation_builder_with_tools() {
    let tools = vec![sample_tool_function("grep")];
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("search")])
        .tools(tools)
        .build();
    assert!(req.tools.is_some());
    assert_eq!(req.tools.unwrap().len(), 1);
}

#[test]
fn request_translation_builder_with_turn_history() {
    use abp_copilot_sdk::dialect::CopilotTurnEntry;
    let history = vec![CopilotTurnEntry {
        request: "What is rust?".into(),
        response: "A programming language".into(),
    }];
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("More details")])
        .turn_history(history)
        .build();
    assert_eq!(req.turn_history.len(), 1);
}

#[test]
fn request_translation_builder_with_references() {
    let refs = vec![sample_reference(CopilotReferenceType::Repository, "repo-0")];
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("Check repo")])
        .references(refs)
        .build();
    assert_eq!(req.references.len(), 1);
}

#[test]
fn request_translation_work_order_model_o3_mini() {
    let req = CopilotRequestBuilder::new()
        .model("o3-mini")
        .messages(vec![Message::user("solve")])
        .build();
    let wo = request_to_work_order(&req);
    assert_eq!(wo.config.model.as_deref(), Some("o3-mini"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Response translation: ABP Receipt → Copilot response
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn response_translation_assistant_message() {
    let events = vec![agent_event(AgentEventKind::AssistantMessage {
        text: "Hello!".into(),
    })];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.message, "Hello!");
    assert!(resp.copilot_errors.is_empty());
    assert!(resp.function_call.is_none());
}

#[test]
fn response_translation_delta_concatenation() {
    let events = vec![
        agent_event(AgentEventKind::AssistantDelta { text: "Hel".into() }),
        agent_event(AgentEventKind::AssistantDelta { text: "lo!".into() }),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.message, "Hello!");
}

#[test]
fn response_translation_tool_call() {
    let events = vec![agent_event(AgentEventKind::ToolCall {
        tool_name: "edit_file".into(),
        tool_use_id: Some("call_1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "src/lib.rs", "content": "fn main() {}"}),
    })];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let fc = resp.function_call.unwrap();
    assert_eq!(fc.name, "edit_file");
    assert_eq!(fc.id.as_deref(), Some("call_1"));
    assert!(fc.arguments.contains("lib.rs"));
}

#[test]
fn response_translation_error_event() {
    let events = vec![agent_event(AgentEventKind::Error {
        message: "Token limit exceeded".into(),
        error_code: None,
    })];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.copilot_errors.len(), 1);
    assert!(resp.copilot_errors[0].message.contains("Token limit"));
}

#[test]
fn response_translation_multiple_errors() {
    let events = vec![
        agent_event(AgentEventKind::Error {
            message: "Error one".into(),
            error_code: None,
        }),
        agent_event(AgentEventKind::Error {
            message: "Error two".into(),
            error_code: None,
        }),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.copilot_errors.len(), 2);
}

#[test]
fn response_translation_mixed_events() {
    let events = vec![
        agent_event(AgentEventKind::AssistantMessage {
            text: "Working on it...".into(),
        }),
        agent_event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("c1".into()),
            parent_tool_use_id: None,
            input: json!({"path": "a.rs"}),
        }),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.message, "Working on it...");
    assert!(resp.function_call.is_some());
}

#[test]
fn response_translation_empty_trace() {
    let receipt = mock_receipt(vec![]);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert!(resp.message.is_empty());
    assert!(resp.copilot_errors.is_empty());
    assert!(resp.function_call.is_none());
}

#[test]
fn response_translation_ignores_non_mapped_events() {
    let events = vec![
        agent_event(AgentEventKind::RunStarted {
            message: "Starting".into(),
        }),
        agent_event(AgentEventKind::AssistantMessage {
            text: "Done".into(),
        }),
        agent_event(AgentEventKind::RunCompleted {
            message: "Finished".into(),
        }),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.message, "Done");
}

#[test]
fn response_translation_response_to_ir_assistant() {
    let resp = CopilotResponse {
        message: "Test reply".into(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    let ir = response_to_ir(&resp);
    assert_eq!(ir.len(), 1);
    assert_eq!(ir.messages[0].role, IrRole::Assistant);
    assert_eq!(ir.messages[0].text_content(), "Test reply");
}

#[test]
fn response_translation_response_to_ir_empty() {
    let resp = CopilotResponse {
        message: String::new(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    let ir = response_to_ir(&resp);
    assert!(ir.is_empty());
}

#[test]
fn response_translation_last_tool_call_wins() {
    let events = vec![
        agent_event(AgentEventKind::ToolCall {
            tool_name: "first".into(),
            tool_use_id: Some("c1".into()),
            parent_tool_use_id: None,
            input: json!({}),
        }),
        agent_event(AgentEventKind::ToolCall {
            tool_name: "second".into(),
            tool_use_id: Some("c2".into()),
            parent_tool_use_id: None,
            input: json!({}),
        }),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let fc = resp.function_call.unwrap();
    assert_eq!(fc.name, "second");
}

#[test]
fn response_translation_message_overwritten_by_later() {
    let events = vec![
        agent_event(AgentEventKind::AssistantMessage {
            text: "First".into(),
        }),
        agent_event(AgentEventKind::AssistantMessage {
            text: "Second".into(),
        }),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.message, "Second");
}

#[test]
fn response_translation_delta_after_full_message() {
    let events = vec![
        agent_event(AgentEventKind::AssistantMessage {
            text: "Hello".into(),
        }),
        agent_event(AgentEventKind::AssistantDelta {
            text: " world".into(),
        }),
    ];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    // AssistantMessage sets, then delta appends
    assert_eq!(resp.message, "Hello world");
}

#[test]
fn response_translation_error_with_code() {
    // ErrorCode is from abp_error; verify shim handles Some(code) -> Some(string)
    let events = vec![AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: "Rate limited".into(),
            error_code: None,
        },
        ext: None,
    }];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    assert_eq!(resp.copilot_errors.len(), 1);
    assert!(resp.copilot_errors[0].message.contains("Rate limited"));
    // When error_code is None, code should be None
    assert!(resp.copilot_errors[0].code.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Streaming: Event stream mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn streaming_basic_deltas() {
    let events = vec![
        agent_event(AgentEventKind::AssistantDelta {
            text: "Part1".into(),
        }),
        agent_event(AgentEventKind::AssistantDelta {
            text: "Part2".into(),
        }),
    ];
    let stream = events_to_stream_events(&events, "gpt-4o");
    // refs + 2 deltas + done = 4
    assert_eq!(stream.len(), 4);
    assert!(matches!(
        &stream[0],
        CopilotStreamEvent::CopilotReferences { .. }
    ));
    assert!(matches!(&stream[1], CopilotStreamEvent::TextDelta { .. }));
    assert!(matches!(&stream[2], CopilotStreamEvent::TextDelta { .. }));
    assert!(matches!(&stream[3], CopilotStreamEvent::Done {}));
}

#[test]
fn streaming_ends_with_done() {
    let events = vec![agent_event(AgentEventKind::AssistantDelta {
        text: "x".into(),
    })];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert!(matches!(
        stream.last().unwrap(),
        CopilotStreamEvent::Done {}
    ));
}

#[test]
fn streaming_starts_with_references() {
    let events = vec![agent_event(AgentEventKind::AssistantDelta {
        text: "x".into(),
    })];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert!(matches!(
        &stream[0],
        CopilotStreamEvent::CopilotReferences { .. }
    ));
}

#[test]
fn streaming_empty_events_still_has_bookends() {
    let stream = events_to_stream_events(&[], "gpt-4o");
    // references + done
    assert_eq!(stream.len(), 2);
    assert!(matches!(
        &stream[0],
        CopilotStreamEvent::CopilotReferences { .. }
    ));
    assert!(matches!(&stream[1], CopilotStreamEvent::Done {}));
}

#[test]
fn streaming_error_events_mapped() {
    let events = vec![agent_event(AgentEventKind::Error {
        message: "fail".into(),
        error_code: None,
    })];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert_eq!(stream.len(), 3);
    assert!(matches!(
        &stream[1],
        CopilotStreamEvent::CopilotErrors { .. }
    ));
}

#[test]
fn streaming_function_call_events_mapped() {
    let events = vec![agent_event(AgentEventKind::ToolCall {
        tool_name: "search".into(),
        tool_use_id: Some("c1".into()),
        parent_tool_use_id: None,
        input: json!({"q": "rust"}),
    })];
    let stream = events_to_stream_events(&events, "gpt-4o");
    assert!(matches!(
        &stream[1],
        CopilotStreamEvent::FunctionCall { .. }
    ));
}

#[test]
fn streaming_assistant_message_becomes_text_delta() {
    let events = vec![agent_event(AgentEventKind::AssistantMessage {
        text: "Full message".into(),
    })];
    let stream = events_to_stream_events(&events, "gpt-4o");
    match &stream[1] {
        CopilotStreamEvent::TextDelta { text } => assert_eq!(text, "Full message"),
        other => panic!("expected TextDelta, got {other:?}"),
    }
}

#[test]
fn streaming_mixed_events_order_preserved() {
    let events = vec![
        agent_event(AgentEventKind::AssistantDelta { text: "A".into() }),
        agent_event(AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        }),
        agent_event(AgentEventKind::AssistantDelta { text: "B".into() }),
    ];
    let stream = events_to_stream_events(&events, "gpt-4o");
    // refs + delta + function_call + delta + done = 5
    assert_eq!(stream.len(), 5);
    assert!(matches!(&stream[1], CopilotStreamEvent::TextDelta { .. }));
    assert!(matches!(
        &stream[2],
        CopilotStreamEvent::FunctionCall { .. }
    ));
    assert!(matches!(&stream[3], CopilotStreamEvent::TextDelta { .. }));
}

#[tokio::test]
async fn streaming_client_produces_stream() {
    let events = vec![agent_event(AgentEventKind::AssistantDelta {
        text: "Hi".into(),
    })];
    let client = CopilotClient::new("gpt-4o").with_processor(make_processor(events));
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("Hello")])
        .build();
    let stream = client.create_stream(req).await.unwrap();
    let chunks: Vec<CopilotStreamEvent> = stream.collect().await;
    assert_eq!(chunks.len(), 3); // refs + delta + done
}

#[test]
fn streaming_non_mapped_events_skipped() {
    let events = vec![
        agent_event(AgentEventKind::RunStarted {
            message: "start".into(),
        }),
        agent_event(AgentEventKind::FileChanged {
            path: "a.rs".into(),
            summary: "modified".into(),
        }),
    ];
    let stream = events_to_stream_events(&events, "gpt-4o");
    // refs + done only (RunStarted / FileChanged not mapped)
    assert_eq!(stream.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Tool use: Copilot tool definitions and results
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_def_to_copilot_creates_function_tool() {
    let def = CanonicalToolDef {
        name: "grep".into(),
        description: "Search file contents".into(),
        parameters_schema: json!({"type": "object", "properties": {"pattern": {"type": "string"}}}),
    };
    let tool = dialect::tool_def_to_copilot(&def);
    assert_eq!(tool.tool_type, CopilotToolType::Function);
    let func = tool.function.unwrap();
    assert_eq!(func.name, "grep");
    assert_eq!(func.description, "Search file contents");
}

#[test]
fn tool_def_from_copilot_extracts_function() {
    let tool = sample_tool_function("read_file");
    let def = dialect::tool_def_from_copilot(&tool).unwrap();
    assert_eq!(def.name, "read_file");
    assert!(!def.description.is_empty());
}

#[test]
fn tool_def_from_copilot_returns_none_for_confirmation() {
    let tool = CopilotTool {
        tool_type: CopilotToolType::Confirmation,
        function: None,
        confirmation: Some(CopilotConfirmation {
            id: "c1".into(),
            title: "Confirm".into(),
            message: "Sure?".into(),
            accepted: None,
        }),
    };
    assert!(dialect::tool_def_from_copilot(&tool).is_none());
}

#[test]
fn tool_def_roundtrip_canonical_to_copilot_and_back() {
    let original = CanonicalToolDef {
        name: "bash".into(),
        description: "Run a shell command".into(),
        parameters_schema: json!({"type": "object", "properties": {"cmd": {"type": "string"}}}),
    };
    let copilot_tool = dialect::tool_def_to_copilot(&original);
    let recovered = dialect::tool_def_from_copilot(&copilot_tool).unwrap();
    assert_eq!(recovered.name, original.name);
    assert_eq!(recovered.description, original.description);
    assert_eq!(recovered.parameters_schema, original.parameters_schema);
}

#[test]
fn tool_call_arguments_serialized_as_json_string() {
    let events = vec![agent_event(AgentEventKind::ToolCall {
        tool_name: "write".into(),
        tool_use_id: Some("c1".into()),
        parent_tool_use_id: None,
        input: json!({"path": "a.rs", "content": "fn main(){}"}),
    })];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let fc = resp.function_call.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&fc.arguments).unwrap();
    assert_eq!(parsed["path"], "a.rs");
}

#[test]
fn tool_call_without_id() {
    let events = vec![agent_event(AgentEventKind::ToolCall {
        tool_name: "search".into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    })];
    let receipt = mock_receipt(events);
    let resp = receipt_to_response(&receipt, "gpt-4o");
    let fc = resp.function_call.unwrap();
    assert_eq!(fc.name, "search");
    assert!(fc.id.is_none());
}

#[test]
fn tool_call_stream_function_call_arguments_valid_json() {
    let events = vec![agent_event(AgentEventKind::ToolCall {
        tool_name: "edit".into(),
        tool_use_id: Some("c2".into()),
        parent_tool_use_id: None,
        input: json!({"file": "test.rs", "line": 42}),
    })];
    let stream = events_to_stream_events(&events, "gpt-4o");
    match &stream[1] {
        CopilotStreamEvent::FunctionCall { function_call } => {
            let parsed: serde_json::Value = serde_json::from_str(&function_call.arguments).unwrap();
            assert_eq!(parsed["line"], 42);
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn tool_multiple_tools_in_builder() {
    let tools = vec![
        sample_tool_function("read_file"),
        sample_tool_function("write_file"),
        sample_tool_function("bash"),
    ];
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("do stuff")])
        .tools(tools)
        .build();
    assert_eq!(req.tools.unwrap().len(), 3);
}

#[test]
fn tool_confirmation_serde_roundtrip() {
    let conf = CopilotConfirmation {
        id: "conf-42".into(),
        title: "Delete all files?".into(),
        message: "This action cannot be undone.".into(),
        accepted: Some(true),
    };
    let json = serde_json::to_string(&conf).unwrap();
    let back: CopilotConfirmation = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "conf-42");
    assert_eq!(back.accepted, Some(true));
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn edge_empty_messages_ir() {
    let ir = messages_to_ir(&[]);
    assert!(ir.is_empty());
    let back = ir_to_messages(&ir);
    assert!(back.is_empty());
}

#[tokio::test]
async fn edge_no_processor_returns_error() {
    let client = CopilotClient::new("gpt-4o");
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let err = client.create(req).await.unwrap_err();
    assert!(matches!(err, ShimError::Internal(_)));
}

#[tokio::test]
async fn edge_no_processor_stream_returns_error() {
    let client = CopilotClient::new("gpt-4o");
    let req = CopilotRequestBuilder::new()
        .messages(vec![Message::user("test")])
        .build();
    let result = client.create_stream(req).await;
    assert!(result.is_err());
}

#[test]
fn edge_ir_usage_conversion() {
    let ir = IrUsage::from_io(500, 200);
    let (input, output, total) = ir_usage_to_tuple(&ir);
    assert_eq!(input, 500);
    assert_eq!(output, 200);
    assert_eq!(total, 700);
}

#[test]
fn edge_model_variations_known() {
    assert!(dialect::is_known_model("gpt-4o"));
    assert!(dialect::is_known_model("gpt-4o-mini"));
    assert!(dialect::is_known_model("o3-mini"));
    assert!(dialect::is_known_model("claude-sonnet-4"));
    assert!(!dialect::is_known_model("unknown-model-xyz"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional dialect mapping tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_to_canonical_model() {
    assert_eq!(dialect::to_canonical_model("gpt-4o"), "copilot/gpt-4o");
}

#[test]
fn dialect_from_canonical_model() {
    assert_eq!(dialect::from_canonical_model("copilot/gpt-4o"), "gpt-4o");
    assert_eq!(dialect::from_canonical_model("other-model"), "other-model");
}

#[test]
fn dialect_capability_manifest_has_streaming() {
    let m = dialect::capability_manifest();
    assert!(m.contains_key(&abp_core::Capability::Streaming));
}

#[test]
fn dialect_map_work_order_basic() {
    let wo = WorkOrderBuilder::new("Refactor module").build();
    let cfg = CopilotConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.messages.len(), 1);
    assert!(req.messages[0].content.contains("Refactor module"));
}

#[test]
fn dialect_map_work_order_with_system_prompt() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = CopilotConfig {
        system_prompt: Some("Be concise.".into()),
        ..CopilotConfig::default()
    };
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.messages.len(), 2);
    assert_eq!(req.messages[0].role, "system");
    assert_eq!(req.messages[0].content, "Be concise.");
}

#[test]
fn dialect_map_response_to_agent_events() {
    let resp = CopilotResponse {
        message: "Done!".into(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    };
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantMessage { text } if text == "Done!"
    ));
}

#[test]
fn dialect_map_stream_event_text_delta() {
    let event = CopilotStreamEvent::TextDelta {
        text: "hello".into(),
    };
    let mapped = dialect::map_stream_event(&event);
    assert_eq!(mapped.len(), 1);
    assert!(matches!(
        &mapped[0].kind,
        AgentEventKind::AssistantDelta { text } if text == "hello"
    ));
}

#[test]
fn dialect_map_stream_event_done() {
    let event = CopilotStreamEvent::Done {};
    let mapped = dialect::map_stream_event(&event);
    assert_eq!(mapped.len(), 1);
    assert!(matches!(
        &mapped[0].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn dialect_passthrough_roundtrip() {
    let event = CopilotStreamEvent::TextDelta {
        text: "hello".into(),
    };
    let wrapped = dialect::to_passthrough_event(&event);
    let recovered = dialect::from_passthrough_event(&wrapped).unwrap();
    assert_eq!(recovered, event);
}

#[test]
fn dialect_passthrough_fidelity_multiple_events() {
    let events = vec![
        CopilotStreamEvent::CopilotReferences { references: vec![] },
        CopilotStreamEvent::TextDelta {
            text: "hello".into(),
        },
        CopilotStreamEvent::FunctionCall {
            function_call: CopilotFunctionCall {
                name: "f".into(),
                arguments: "{}".into(),
                id: None,
            },
        },
        CopilotStreamEvent::Done {},
    ];
    assert!(dialect::verify_passthrough_fidelity(&events));
}

#[test]
fn lowering_roundtrip_with_references() {
    let refs = vec![sample_reference(CopilotReferenceType::File, "f-0")];
    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "check this".into(),
        name: None,
        copilot_references: refs,
    }];
    let ir = lowering::to_ir(&msgs);
    let back = lowering::from_ir(&ir);
    assert_eq!(back[0].copilot_references.len(), 1);
    assert_eq!(back[0].copilot_references[0].id, "f-0");
}

#[test]
fn lowering_extract_references() {
    let msgs = vec![
        CopilotMessage {
            role: "user".into(),
            content: "a".into(),
            name: None,
            copilot_references: vec![sample_reference(CopilotReferenceType::File, "f1")],
        },
        CopilotMessage {
            role: "user".into(),
            content: "b".into(),
            name: None,
            copilot_references: vec![sample_reference(CopilotReferenceType::Snippet, "s1")],
        },
    ];
    let ir = lowering::to_ir(&msgs);
    let refs = lowering::extract_references(&ir);
    assert_eq!(refs.len(), 2);
}

#[test]
fn shim_message_constructors() {
    let sys = Message::system("sys");
    assert_eq!(sys.role, "system");
    let usr = Message::user("usr");
    assert_eq!(usr.role, "user");
    let ast = Message::assistant("ast");
    assert_eq!(ast.role, "assistant");
}

#[test]
fn shim_message_serde_roundtrip() {
    let msg = Message::user("test content");
    let json = serde_json::to_string(&msg).unwrap();
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(back.role, "user");
    assert_eq!(back.content, "test content");
}

#[test]
fn mock_receipt_helpers() {
    let receipt = mock_receipt(vec![]);
    assert_eq!(receipt.backend.id, "mock");
    assert!(receipt.trace.is_empty());

    let usage = abp_core::UsageNormalized {
        input_tokens: Some(10),
        output_tokens: Some(5),
        ..Default::default()
    };
    let receipt_with = mock_receipt_with_usage(vec![], usage);
    assert_eq!(receipt_with.usage.input_tokens, Some(10));
}

#[test]
fn client_debug_impl() {
    let client = CopilotClient::new("gpt-4o");
    let debug = format!("{client:?}");
    assert!(debug.contains("gpt-4o"));
}

#[test]
fn client_model_accessor() {
    let client = CopilotClient::new("o1-mini");
    assert_eq!(client.model(), "o1-mini");
}
