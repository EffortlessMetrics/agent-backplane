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
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Deep comprehensive tests for the `abp-shim-copilot` crate covering the full
//! shim surface: request/response types, streaming, IR lowering roundtrips,
//! tool call mapping, capability mapping, error handling, and edge cases.

use abp_copilot_sdk::dialect::{
    self, CanonicalToolDef, CopilotConfig, CopilotConfirmation, CopilotError, CopilotFunctionCall,
    CopilotFunctionDef, CopilotMessage, CopilotReference, CopilotReferenceType, CopilotRequest,
    CopilotResponse, CopilotStreamEvent, CopilotTool, CopilotToolType, CopilotTurnEntry,
};
use abp_copilot_sdk::lowering;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, Capability, SupportLevel, UsageNormalized, WorkOrderBuilder,
};
use abp_shim_copilot::{
    CopilotClient, CopilotRequestBuilder, Message, ShimError, events_to_stream_events,
    ir_to_messages, ir_usage_to_tuple, messages_to_ir, mock_receipt, mock_receipt_with_usage,
    receipt_to_response, request_to_ir, request_to_work_order, response_to_ir,
};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;
use tokio_stream::StreamExt;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn make_processor(
    events: Vec<AgentEvent>,
) -> Box<dyn Fn(&abp_core::WorkOrder) -> abp_core::Receipt + Send + Sync> {
    Box::new(move |_wo| mock_receipt(events.clone()))
}

fn simple_copilot_request(text: &str) -> CopilotRequest {
    CopilotRequestBuilder::new()
        .messages(vec![Message::user(text)])
        .build()
}

fn make_reference(ref_type: CopilotReferenceType, id: &str) -> CopilotReference {
    CopilotReference {
        ref_type,
        id: id.into(),
        data: json!({}),
        metadata: None,
    }
}

fn make_function_call(name: &str, args: &str, id: Option<&str>) -> CopilotFunctionCall {
    CopilotFunctionCall {
        name: name.into(),
        arguments: args.into(),
        id: id.map(String::from),
    }
}

fn make_copilot_response(message: &str) -> CopilotResponse {
    CopilotResponse {
        message: message.into(),
        copilot_references: vec![],
        copilot_errors: vec![],
        copilot_confirmation: None,
        function_call: None,
    }
}

fn make_copilot_message(role: &str, content: &str) -> CopilotMessage {
    CopilotMessage {
        role: role.into(),
        content: content.into(),
        name: None,
        copilot_references: vec![],
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: Message constructors
// ═══════════════════════════════════════════════════════════════════════════

mod message_constructors {
    use super::*;

    #[test]
    fn system_message_has_correct_role() {
        let msg = Message::system("You are helpful.");
        assert_eq!(msg.role, "system");
        assert_eq!(msg.content, "You are helpful.");
        assert!(msg.name.is_none());
        assert!(msg.copilot_references.is_empty());
    }

    #[test]
    fn user_message_has_correct_role() {
        let msg = Message::user("Hello");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "Hello");
    }

    #[test]
    fn assistant_message_has_correct_role() {
        let msg = Message::assistant("Sure!");
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.content, "Sure!");
    }

    #[test]
    fn user_with_refs_attaches_references() {
        let refs = vec![make_reference(CopilotReferenceType::File, "file-0")];
        let msg = Message::user_with_refs("Check this", refs.clone());
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "Check this");
        assert_eq!(msg.copilot_references.len(), 1);
        assert_eq!(msg.copilot_references[0].id, "file-0");
    }

    #[test]
    fn user_with_empty_refs() {
        let msg = Message::user_with_refs("No refs", vec![]);
        assert!(msg.copilot_references.is_empty());
    }

    #[test]
    fn system_message_with_empty_content() {
        let msg = Message::system("");
        assert_eq!(msg.role, "system");
        assert!(msg.content.is_empty());
    }

    #[test]
    fn message_accepts_string_type() {
        let owned = String::from("Owned string");
        let msg = Message::user(owned);
        assert_eq!(msg.content, "Owned string");
    }

    #[test]
    fn message_serde_roundtrip() {
        let msg = Message::user("Hello");
        let json = serde_json::to_string(&msg).unwrap();
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back.role, "user");
        assert_eq!(back.content, "Hello");
    }

    #[test]
    fn message_with_name_serialized() {
        let mut msg = Message::user("Hi");
        msg.name = Some("alice".into());
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["name"], "alice");
    }

    #[test]
    fn message_without_name_omits_field() {
        let msg = Message::user("Hi");
        let json = serde_json::to_value(&msg).unwrap();
        assert!(json.get("name").is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: Request builder
// ═══════════════════════════════════════════════════════════════════════════

mod request_builder {
    use super::*;

    #[test]
    fn default_model_is_gpt4o() {
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .build();
        assert_eq!(req.model, "gpt-4o");
    }

    #[test]
    fn custom_model_override() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4-turbo")
            .messages(vec![Message::user("test")])
            .build();
        assert_eq!(req.model, "gpt-4-turbo");
    }

    #[test]
    fn builder_with_tools() {
        let tool = CopilotTool {
            tool_type: CopilotToolType::Function,
            function: Some(CopilotFunctionDef {
                name: "read_file".into(),
                description: "Read a file".into(),
                parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            }),
            confirmation: None,
        };
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("test")])
            .tools(vec![tool])
            .build();
        assert!(req.tools.is_some());
        assert_eq!(req.tools.unwrap().len(), 1);
    }

    #[test]
    fn builder_with_turn_history() {
        let history = vec![CopilotTurnEntry {
            request: "What is Rust?".into(),
            response: "A systems language.".into(),
        }];
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("Tell me more")])
            .turn_history(history)
            .build();
        assert_eq!(req.turn_history.len(), 1);
        assert_eq!(req.turn_history[0].request, "What is Rust?");
    }

    #[test]
    fn builder_with_references() {
        let refs = vec![make_reference(CopilotReferenceType::Repository, "repo-0")];
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::user("Check repo")])
            .references(refs)
            .build();
        assert_eq!(req.references.len(), 1);
    }

    #[test]
    fn builder_messages_converted_correctly() {
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::system("Be concise"), Message::user("Hello")])
            .build();
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].role, "system");
        assert_eq!(req.messages[1].role, "user");
    }

    #[test]
    fn builder_default_state() {
        let builder = CopilotRequestBuilder::new();
        let req = builder.build();
        assert_eq!(req.model, "gpt-4o");
        assert!(req.messages.is_empty());
        assert!(req.tools.is_none());
        assert!(req.turn_history.is_empty());
        assert!(req.references.is_empty());
    }

    #[test]
    fn builder_chaining_all_fields() {
        let req = CopilotRequestBuilder::new()
            .model("o3-mini")
            .messages(vec![Message::user("hi")])
            .tools(vec![])
            .turn_history(vec![])
            .references(vec![])
            .build();
        assert_eq!(req.model, "o3-mini");
    }

    #[test]
    fn builder_preserves_message_references() {
        let refs = vec![make_reference(CopilotReferenceType::File, "f1")];
        let msg = Message::user_with_refs("Read", refs);
        let req = CopilotRequestBuilder::new().messages(vec![msg]).build();
        assert_eq!(req.messages[0].copilot_references.len(), 1);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: IR conversion
// ═══════════════════════════════════════════════════════════════════════════

mod ir_conversion {
    use super::*;

    #[test]
    fn request_to_ir_basic() {
        let req = simple_copilot_request("Hello");
        let conv = request_to_ir(&req);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello");
    }

    #[test]
    fn request_to_ir_with_system_message() {
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::system("Be concise"), Message::user("Hi")])
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
                Message::user("Q1"),
                Message::assistant("A1"),
                Message::user("Q2"),
            ])
            .build();
        let conv = request_to_ir(&req);
        assert_eq!(conv.len(), 3);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[1].role, IrRole::Assistant);
        assert_eq!(conv.messages[2].role, IrRole::User);
    }

    #[test]
    fn messages_to_ir_roundtrip() {
        let messages = vec![
            Message::system("System"),
            Message::user("User"),
            Message::assistant("Assistant"),
        ];
        let conv = messages_to_ir(&messages);
        assert_eq!(conv.len(), 3);
        let back = ir_to_messages(&conv);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].role, "system");
        assert_eq!(back[0].content, "System");
        assert_eq!(back[1].role, "user");
        assert_eq!(back[2].role, "assistant");
    }

    #[test]
    fn ir_to_messages_empty() {
        let conv = IrConversation::from_messages(vec![]);
        let msgs = ir_to_messages(&conv);
        assert!(msgs.is_empty());
    }

    #[test]
    fn messages_to_ir_empty() {
        let conv = messages_to_ir(&[]);
        assert!(conv.is_empty());
    }

    #[test]
    fn response_to_ir_basic() {
        let resp = make_copilot_response("Hello!");
        let conv = response_to_ir(&resp);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        assert_eq!(conv.messages[0].text_content(), "Hello!");
    }

    #[test]
    fn response_to_ir_empty_message() {
        let resp = make_copilot_response("");
        let conv = response_to_ir(&resp);
        assert!(conv.is_empty());
    }

    #[test]
    fn ir_usage_tuple_basic() {
        let usage = IrUsage::from_io(100, 50);
        let (input, output, total) = ir_usage_to_tuple(&usage);
        assert_eq!(input, 100);
        assert_eq!(output, 50);
        assert_eq!(total, 150);
    }

    #[test]
    fn ir_usage_tuple_zero() {
        let usage = IrUsage::from_io(0, 0);
        let (input, output, total) = ir_usage_to_tuple(&usage);
        assert_eq!(input, 0);
        assert_eq!(output, 0);
        assert_eq!(total, 0);
    }

    #[test]
    fn ir_usage_tuple_large_values() {
        let usage = IrUsage::from_io(1_000_000, 500_000);
        let (_input, _output, total) = ir_usage_to_tuple(&usage);
        assert_eq!(total, 1_500_000);
    }

    #[test]
    fn message_name_preserved_through_ir() {
        let mut msg = Message::user("Hi");
        msg.name = Some("alice".into());
        let conv = messages_to_ir(&[msg]);
        let back = ir_to_messages(&conv);
        assert_eq!(back[0].name.as_deref(), Some("alice"));
    }

    #[test]
    fn message_references_preserved_through_ir() {
        let refs = vec![CopilotReference {
            ref_type: CopilotReferenceType::File,
            id: "file-0".into(),
            data: json!({"path": "main.rs"}),
            metadata: None,
        }];
        let msg = Message::user_with_refs("Check", refs);
        let conv = messages_to_ir(&[msg]);
        let back = ir_to_messages(&conv);
        assert_eq!(back[0].copilot_references.len(), 1);
        assert_eq!(back[0].copilot_references[0].id, "file-0");
    }

    #[test]
    fn response_references_preserved_through_ir() {
        let refs = vec![CopilotReference {
            ref_type: CopilotReferenceType::WebSearchResult,
            id: "web-0".into(),
            data: json!({"url": "https://example.com"}),
            metadata: None,
        }];
        let resp = CopilotResponse {
            message: "Found it".into(),
            copilot_references: refs,
            copilot_errors: vec![],
            copilot_confirmation: None,
            function_call: None,
        };
        let conv = response_to_ir(&resp);
        let back = ir_to_messages(&conv);
        assert_eq!(back[0].copilot_references.len(), 1);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: Work order conversion
// ═══════════════════════════════════════════════════════════════════════════

mod work_order_conversion {
    use super::*;

    #[test]
    fn request_to_work_order_basic() {
        let req = simple_copilot_request("Hello world");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Hello world");
    }

    #[test]
    fn request_to_work_order_model_preserved() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4-turbo")
            .messages(vec![Message::user("test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
    }

    #[test]
    fn request_to_work_order_uses_last_user_message() {
        let req = CopilotRequestBuilder::new()
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
    fn request_to_work_order_default_model() {
        let req = simple_copilot_request("test");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn request_to_work_order_system_only_fallback() {
        let req = CopilotRequestBuilder::new()
            .messages(vec![Message::system("System only")])
            .build();
        let wo = request_to_work_order(&req);
        // No user message, should use fallback
        assert_eq!(wo.task, "copilot completion");
    }

    #[test]
    fn request_to_work_order_empty_messages() {
        let req = CopilotRequestBuilder::new().build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "copilot completion");
    }

    #[test]
    fn work_order_has_uuid() {
        let req = simple_copilot_request("test");
        let wo = request_to_work_order(&req);
        assert!(!wo.id.is_nil());
    }

    #[test]
    fn request_to_work_order_o3_mini_model() {
        let req = CopilotRequestBuilder::new()
            .model("o3-mini")
            .messages(vec![Message::user("test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("o3-mini"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: Receipt to response
// ═══════════════════════════════════════════════════════════════════════════

mod receipt_to_response_tests {
    use super::*;

    #[test]
    fn receipt_with_assistant_message() {
        let events = vec![make_event(AgentEventKind::AssistantMessage {
            text: "Hello!".into(),
        })];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.message, "Hello!");
        assert!(resp.copilot_errors.is_empty());
        assert!(resp.function_call.is_none());
    }

    #[test]
    fn receipt_with_deltas_concatenated() {
        let events = vec![
            make_event(AgentEventKind::AssistantDelta { text: "Hel".into() }),
            make_event(AgentEventKind::AssistantDelta { text: "lo!".into() }),
        ];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.message, "Hello!");
    }

    #[test]
    fn receipt_with_tool_call() {
        let events = vec![make_event(AgentEventKind::ToolCall {
            tool_name: "read_file".into(),
            tool_use_id: Some("call_abc".into()),
            parent_tool_use_id: None,
            input: json!({"path": "src/main.rs"}),
        })];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let fc = resp.function_call.unwrap();
        assert_eq!(fc.name, "read_file");
        assert_eq!(fc.id.as_deref(), Some("call_abc"));
        assert!(fc.arguments.contains("main.rs"));
    }

    #[test]
    fn receipt_with_error() {
        let events = vec![make_event(AgentEventKind::Error {
            message: "rate limit exceeded".into(),
            error_code: None,
        })];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.copilot_errors.len(), 1);
        assert!(resp.copilot_errors[0].message.contains("rate limit"));
        assert_eq!(resp.copilot_errors[0].error_type, "backend_error");
    }

    #[test]
    fn receipt_with_error_code() {
        let events = vec![make_event(AgentEventKind::Error {
            message: "forbidden".into(),
            error_code: Some(abp_error::ErrorCode::BackendTimeout),
        })];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert!(resp.copilot_errors[0].code.is_some());
    }

    #[test]
    fn receipt_empty_trace() {
        let receipt = mock_receipt(vec![]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert!(resp.message.is_empty());
        assert!(resp.copilot_errors.is_empty());
        assert!(resp.function_call.is_none());
    }

    #[test]
    fn receipt_message_then_tool_call() {
        let events = vec![
            make_event(AgentEventKind::AssistantMessage {
                text: "Let me read that.".into(),
            }),
            make_event(AgentEventKind::ToolCall {
                tool_name: "read_file".into(),
                tool_use_id: Some("call_1".into()),
                parent_tool_use_id: None,
                input: json!({"path": "file.rs"}),
            }),
        ];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.message, "Let me read that.");
        assert!(resp.function_call.is_some());
    }

    #[test]
    fn receipt_multiple_errors() {
        let events = vec![
            make_event(AgentEventKind::Error {
                message: "error one".into(),
                error_code: None,
            }),
            make_event(AgentEventKind::Error {
                message: "error two".into(),
                error_code: None,
            }),
        ];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.copilot_errors.len(), 2);
    }

    #[test]
    fn receipt_last_tool_call_wins() {
        let events = vec![
            make_event(AgentEventKind::ToolCall {
                tool_name: "first_tool".into(),
                tool_use_id: Some("call_1".into()),
                parent_tool_use_id: None,
                input: json!({}),
            }),
            make_event(AgentEventKind::ToolCall {
                tool_name: "second_tool".into(),
                tool_use_id: Some("call_2".into()),
                parent_tool_use_id: None,
                input: json!({}),
            }),
        ];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let fc = resp.function_call.unwrap();
        assert_eq!(fc.name, "second_tool");
    }

    #[test]
    fn receipt_ignores_run_started() {
        let events = vec![
            make_event(AgentEventKind::RunStarted {
                message: "starting".into(),
            }),
            make_event(AgentEventKind::AssistantMessage {
                text: "Hello".into(),
            }),
        ];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.message, "Hello");
    }

    #[test]
    fn receipt_with_usage() {
        let usage = UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            ..Default::default()
        };
        let events = vec![make_event(AgentEventKind::AssistantMessage {
            text: "Done".into(),
        })];
        let receipt = mock_receipt_with_usage(events, usage);
        assert_eq!(receipt.usage.input_tokens, Some(100));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: Stream events
// ═══════════════════════════════════════════════════════════════════════════

mod stream_events {
    use super::*;

    #[test]
    fn stream_starts_with_references() {
        let events = vec![make_event(AgentEventKind::AssistantDelta {
            text: "hi".into(),
        })];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert!(matches!(
            &stream[0],
            CopilotStreamEvent::CopilotReferences { references } if references.is_empty()
        ));
    }

    #[test]
    fn stream_ends_with_done() {
        let events = vec![make_event(AgentEventKind::AssistantDelta {
            text: "hi".into(),
        })];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert!(matches!(
            stream.last().unwrap(),
            CopilotStreamEvent::Done {}
        ));
    }

    #[test]
    fn stream_delta_events() {
        let events = vec![
            make_event(AgentEventKind::AssistantDelta { text: "Hel".into() }),
            make_event(AgentEventKind::AssistantDelta { text: "lo".into() }),
        ];
        let stream = events_to_stream_events(&events, "gpt-4o");
        // references + 2 deltas + done
        assert_eq!(stream.len(), 4);
        assert!(matches!(&stream[1], CopilotStreamEvent::TextDelta { text } if text == "Hel"));
        assert!(matches!(&stream[2], CopilotStreamEvent::TextDelta { text } if text == "lo"));
    }

    #[test]
    fn stream_assistant_message_becomes_delta() {
        let events = vec![make_event(AgentEventKind::AssistantMessage {
            text: "Full message".into(),
        })];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert!(matches!(
            &stream[1],
            CopilotStreamEvent::TextDelta { text } if text == "Full message"
        ));
    }

    #[test]
    fn stream_tool_call_event() {
        let events = vec![make_event(AgentEventKind::ToolCall {
            tool_name: "search".into(),
            tool_use_id: Some("call_1".into()),
            parent_tool_use_id: None,
            input: json!({"query": "rust"}),
        })];
        let stream = events_to_stream_events(&events, "gpt-4o");
        match &stream[1] {
            CopilotStreamEvent::FunctionCall { function_call } => {
                assert_eq!(function_call.name, "search");
                assert_eq!(function_call.id.as_deref(), Some("call_1"));
            }
            other => panic!("Expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn stream_error_event() {
        let events = vec![make_event(AgentEventKind::Error {
            message: "boom".into(),
            error_code: None,
        })];
        let stream = events_to_stream_events(&events, "gpt-4o");
        match &stream[1] {
            CopilotStreamEvent::CopilotErrors { errors } => {
                assert_eq!(errors.len(), 1);
                assert_eq!(errors[0].message, "boom");
            }
            other => panic!("Expected CopilotErrors, got {other:?}"),
        }
    }

    #[test]
    fn stream_empty_events_just_refs_and_done() {
        let stream = events_to_stream_events(&[], "gpt-4o");
        assert_eq!(stream.len(), 2);
        assert!(matches!(
            &stream[0],
            CopilotStreamEvent::CopilotReferences { .. }
        ));
        assert!(matches!(&stream[1], CopilotStreamEvent::Done {}));
    }

    #[test]
    fn stream_mixed_events_order() {
        let events = vec![
            make_event(AgentEventKind::AssistantDelta { text: "A".into() }),
            make_event(AgentEventKind::Error {
                message: "warn".into(),
                error_code: None,
            }),
            make_event(AgentEventKind::AssistantDelta { text: "B".into() }),
        ];
        let stream = events_to_stream_events(&events, "gpt-4o");
        // refs + delta + error + delta + done
        assert_eq!(stream.len(), 5);
        assert!(matches!(&stream[1], CopilotStreamEvent::TextDelta { .. }));
        assert!(matches!(
            &stream[2],
            CopilotStreamEvent::CopilotErrors { .. }
        ));
        assert!(matches!(&stream[3], CopilotStreamEvent::TextDelta { .. }));
    }

    #[test]
    fn stream_ignores_run_events() {
        let events = vec![
            make_event(AgentEventKind::RunStarted {
                message: "start".into(),
            }),
            make_event(AgentEventKind::AssistantDelta { text: "hi".into() }),
            make_event(AgentEventKind::RunCompleted {
                message: "done".into(),
            }),
        ];
        let stream = events_to_stream_events(&events, "gpt-4o");
        // refs + delta + done (run events are ignored in stream mapping)
        assert_eq!(stream.len(), 3);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: Client
// ═══════════════════════════════════════════════════════════════════════════

mod client_tests {
    use super::*;

    #[tokio::test]
    async fn client_create_simple() {
        let events = vec![make_event(AgentEventKind::AssistantMessage {
            text: "Hello!".into(),
        })];
        let client = CopilotClient::new("gpt-4o").with_processor(make_processor(events));
        let req = simple_copilot_request("Hi");
        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.message, "Hello!");
    }

    #[tokio::test]
    async fn client_create_stream_simple() {
        let events = vec![
            make_event(AgentEventKind::AssistantDelta { text: "Hel".into() }),
            make_event(AgentEventKind::AssistantDelta { text: "lo!".into() }),
        ];
        let client = CopilotClient::new("gpt-4o").with_processor(make_processor(events));
        let req = simple_copilot_request("Hi");
        let stream = client.create_stream(req).await.unwrap();
        let chunks: Vec<CopilotStreamEvent> = stream.collect().await;
        assert_eq!(chunks.len(), 4);
    }

    #[tokio::test]
    async fn client_no_processor_create_error() {
        let client = CopilotClient::new("gpt-4o");
        let req = simple_copilot_request("test");
        let err = client.create(req).await.unwrap_err();
        assert!(matches!(err, ShimError::Internal(_)));
    }

    #[tokio::test]
    async fn client_no_processor_stream_error() {
        let client = CopilotClient::new("gpt-4o");
        let req = simple_copilot_request("test");
        let result = client.create_stream(req).await;
        assert!(result.is_err());
    }

    #[test]
    fn client_model_accessor() {
        let client = CopilotClient::new("gpt-4-turbo");
        assert_eq!(client.model(), "gpt-4-turbo");
    }

    #[test]
    fn client_debug_format() {
        let client = CopilotClient::new("gpt-4o");
        let debug = format!("{client:?}");
        assert!(debug.contains("CopilotClient"));
        assert!(debug.contains("gpt-4o"));
    }

    #[tokio::test]
    async fn client_with_tool_call_response() {
        let events = vec![make_event(AgentEventKind::ToolCall {
            tool_name: "write_file".into(),
            tool_use_id: Some("call_xyz".into()),
            parent_tool_use_id: None,
            input: json!({"path": "out.txt", "content": "hello"}),
        })];
        let client = CopilotClient::new("gpt-4o").with_processor(make_processor(events));
        let req = simple_copilot_request("Write a file");
        let resp = client.create(req).await.unwrap();
        let fc = resp.function_call.unwrap();
        assert_eq!(fc.name, "write_file");
    }

    #[tokio::test]
    async fn client_stream_with_function_call() {
        let events = vec![make_event(AgentEventKind::ToolCall {
            tool_name: "search".into(),
            tool_use_id: Some("call_1".into()),
            parent_tool_use_id: None,
            input: json!({"q": "rust"}),
        })];
        let client = CopilotClient::new("gpt-4o").with_processor(make_processor(events));
        let req = simple_copilot_request("Search");
        let stream = client.create_stream(req).await.unwrap();
        let chunks: Vec<CopilotStreamEvent> = stream.collect().await;
        // refs + function_call + done
        assert_eq!(chunks.len(), 3);
        assert!(matches!(
            &chunks[1],
            CopilotStreamEvent::FunctionCall { .. }
        ));
    }

    #[tokio::test]
    async fn client_stream_with_error() {
        let events = vec![make_event(AgentEventKind::Error {
            message: "timeout".into(),
            error_code: None,
        })];
        let client = CopilotClient::new("gpt-4o").with_processor(make_processor(events));
        let req = simple_copilot_request("test");
        let stream = client.create_stream(req).await.unwrap();
        let chunks: Vec<CopilotStreamEvent> = stream.collect().await;
        assert!(
            chunks
                .iter()
                .any(|c| matches!(c, CopilotStreamEvent::CopilotErrors { .. }))
        );
    }

    #[tokio::test]
    async fn client_multi_turn_request() {
        let events = vec![make_event(AgentEventKind::AssistantMessage {
            text: "4".into(),
        })];
        let client = CopilotClient::new("gpt-4o").with_processor(make_processor(events));
        let req = CopilotRequestBuilder::new()
            .messages(vec![
                Message::user("2+2?"),
                Message::assistant("Calculating..."),
                Message::user("Just the number"),
            ])
            .build();
        let resp = client.create(req).await.unwrap();
        assert_eq!(resp.message, "4");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: Dialect types
// ═══════════════════════════════════════════════════════════════════════════

mod dialect_types {
    use super::*;

    #[test]
    fn dialect_version() {
        assert_eq!(dialect::DIALECT_VERSION, "copilot/v0.1");
    }

    #[test]
    fn default_model_constant() {
        assert_eq!(dialect::DEFAULT_MODEL, "gpt-4o");
    }

    #[test]
    fn to_canonical_model() {
        let canonical = dialect::to_canonical_model("gpt-4o");
        assert_eq!(canonical, "copilot/gpt-4o");
    }

    #[test]
    fn from_canonical_model_strips_prefix() {
        let vendor = dialect::from_canonical_model("copilot/gpt-4o");
        assert_eq!(vendor, "gpt-4o");
    }

    #[test]
    fn from_canonical_model_no_prefix() {
        let vendor = dialect::from_canonical_model("gpt-4o");
        assert_eq!(vendor, "gpt-4o");
    }

    #[test]
    fn canonical_roundtrip() {
        let model = "o3-mini";
        let canonical = dialect::to_canonical_model(model);
        let back = dialect::from_canonical_model(&canonical);
        assert_eq!(back, model);
    }

    #[test]
    fn is_known_model_true() {
        assert!(dialect::is_known_model("gpt-4o"));
        assert!(dialect::is_known_model("gpt-4-turbo"));
        assert!(dialect::is_known_model("o1"));
        assert!(dialect::is_known_model("o3-mini"));
        assert!(dialect::is_known_model("claude-sonnet-4"));
    }

    #[test]
    fn is_known_model_false() {
        assert!(!dialect::is_known_model("unknown-model"));
        assert!(!dialect::is_known_model("gpt-5"));
    }

    #[test]
    fn copilot_config_default() {
        let cfg = CopilotConfig::default();
        assert!(cfg.base_url.contains("githubcopilot"));
        assert_eq!(cfg.model, "gpt-4o");
        assert!(cfg.token.is_empty());
        assert!(cfg.system_prompt.is_none());
    }

    #[test]
    fn copilot_config_serde() {
        let cfg = CopilotConfig {
            token: "tok_123".into(),
            base_url: "https://api.example.com".into(),
            model: "gpt-4o".into(),
            system_prompt: Some("Be helpful".into()),
        };
        let json = serde_json::to_value(&cfg).unwrap();
        assert_eq!(json["token"], "tok_123");
        assert_eq!(json["system_prompt"], "Be helpful");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: Tool definitions
// ═══════════════════════════════════════════════════════════════════════════

mod tool_definitions {
    use super::*;

    #[test]
    fn tool_def_to_copilot_function() {
        let def = CanonicalToolDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        };
        let tool = dialect::tool_def_to_copilot(&def);
        assert_eq!(tool.tool_type, CopilotToolType::Function);
        let func = tool.function.unwrap();
        assert_eq!(func.name, "read_file");
        assert_eq!(func.description, "Read a file");
        assert!(tool.confirmation.is_none());
    }

    #[test]
    fn tool_def_from_copilot_function() {
        let tool = CopilotTool {
            tool_type: CopilotToolType::Function,
            function: Some(CopilotFunctionDef {
                name: "search".into(),
                description: "Search code".into(),
                parameters: json!({"type": "object"}),
            }),
            confirmation: None,
        };
        let def = dialect::tool_def_from_copilot(&tool).unwrap();
        assert_eq!(def.name, "search");
        assert_eq!(def.description, "Search code");
    }

    #[test]
    fn tool_def_from_copilot_confirmation_returns_none() {
        let tool = CopilotTool {
            tool_type: CopilotToolType::Confirmation,
            function: None,
            confirmation: Some(CopilotConfirmation {
                id: "conf-1".into(),
                title: "Approve?".into(),
                message: "Allow file write?".into(),
                accepted: None,
            }),
        };
        assert!(dialect::tool_def_from_copilot(&tool).is_none());
    }

    #[test]
    fn tool_def_from_copilot_no_function_returns_none() {
        let tool = CopilotTool {
            tool_type: CopilotToolType::Function,
            function: None,
            confirmation: None,
        };
        assert!(dialect::tool_def_from_copilot(&tool).is_none());
    }

    #[test]
    fn tool_def_roundtrip() {
        let def = CanonicalToolDef {
            name: "write_file".into(),
            description: "Write a file".into(),
            parameters_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"]
            }),
        };
        let tool = dialect::tool_def_to_copilot(&def);
        let back = dialect::tool_def_from_copilot(&tool).unwrap();
        assert_eq!(back.name, def.name);
        assert_eq!(back.description, def.description);
        assert_eq!(back.parameters_schema, def.parameters_schema);
    }

    #[test]
    fn copilot_tool_serde_roundtrip() {
        let tool = CopilotTool {
            tool_type: CopilotToolType::Function,
            function: Some(CopilotFunctionDef {
                name: "test".into(),
                description: "A test".into(),
                parameters: json!({}),
            }),
            confirmation: None,
        };
        let json = serde_json::to_string(&tool).unwrap();
        let back: CopilotTool = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tool_type, CopilotToolType::Function);
        assert_eq!(back.function.unwrap().name, "test");
    }

    #[test]
    fn confirmation_tool_serde() {
        let tool = CopilotTool {
            tool_type: CopilotToolType::Confirmation,
            function: None,
            confirmation: Some(CopilotConfirmation {
                id: "conf-1".into(),
                title: "Delete file?".into(),
                message: "This will permanently delete the file.".into(),
                accepted: Some(true),
            }),
        };
        let json = serde_json::to_value(&tool).unwrap();
        assert_eq!(json["type"], "confirmation");
        let conf = &json["confirmation"];
        assert_eq!(conf["accepted"], true);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: Capability manifest
// ═══════════════════════════════════════════════════════════════════════════

mod capability_manifest {
    use super::*;

    #[test]
    fn manifest_has_streaming_native() {
        let m = dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::Streaming),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn manifest_has_tool_read_emulated() {
        let m = dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::ToolRead),
            Some(SupportLevel::Emulated)
        ));
    }

    #[test]
    fn manifest_has_tool_write_emulated() {
        let m = dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::ToolWrite),
            Some(SupportLevel::Emulated)
        ));
    }

    #[test]
    fn manifest_glob_unsupported() {
        let m = dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::ToolGlob),
            Some(SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn manifest_grep_unsupported() {
        let m = dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::ToolGrep),
            Some(SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn manifest_web_search_native() {
        let m = dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::ToolWebSearch),
            Some(SupportLevel::Native)
        ));
    }

    #[test]
    fn manifest_mcp_unsupported() {
        let m = dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::McpClient),
            Some(SupportLevel::Unsupported)
        ));
        assert!(matches!(
            m.get(&Capability::McpServer),
            Some(SupportLevel::Unsupported)
        ));
    }

    #[test]
    fn manifest_hooks_emulated() {
        let m = dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::HooksPreToolUse),
            Some(SupportLevel::Emulated)
        ));
        assert!(matches!(
            m.get(&Capability::HooksPostToolUse),
            Some(SupportLevel::Emulated)
        ));
    }

    #[test]
    fn manifest_json_schema_emulated() {
        let m = dialect::capability_manifest();
        assert!(matches!(
            m.get(&Capability::StructuredOutputJsonSchema),
            Some(SupportLevel::Emulated)
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: Dialect mapping
// ═══════════════════════════════════════════════════════════════════════════

mod dialect_mapping {
    use super::*;

    #[test]
    fn map_work_order_basic() {
        let wo = WorkOrderBuilder::new("Fix the bug").build();
        let cfg = CopilotConfig::default();
        let req = dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
        assert_eq!(req.messages[0].content, "Fix the bug");
    }

    #[test]
    fn map_work_order_with_system_prompt() {
        let wo = WorkOrderBuilder::new("task").build();
        let cfg = CopilotConfig {
            system_prompt: Some("You are a coding assistant.".into()),
            ..CopilotConfig::default()
        };
        let req = dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].role, "system");
        assert_eq!(req.messages[0].content, "You are a coding assistant.");
    }

    #[test]
    fn map_work_order_model_override() {
        let wo = WorkOrderBuilder::new("task").model("gpt-4-turbo").build();
        let cfg = CopilotConfig::default();
        let req = dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.model, "gpt-4-turbo");
    }

    #[test]
    fn map_work_order_config_model_fallback() {
        let wo = WorkOrderBuilder::new("task").build();
        let cfg = CopilotConfig {
            model: "o1-mini".into(),
            ..CopilotConfig::default()
        };
        let req = dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.model, "o1-mini");
    }

    #[test]
    fn map_response_assistant_message() {
        let resp = make_copilot_response("Hello!");
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Hello!"),
            other => panic!("Expected AssistantMessage, got {other:?}"),
        }
    }

    #[test]
    fn map_response_empty_message_no_event() {
        let resp = make_copilot_response("");
        let events = dialect::map_response(&resp);
        assert!(events.is_empty());
    }

    #[test]
    fn map_response_with_errors() {
        let resp = CopilotResponse {
            message: String::new(),
            copilot_references: vec![],
            copilot_errors: vec![CopilotError {
                error_type: "auth_error".into(),
                message: "invalid token".into(),
                code: Some("401".into()),
                identifier: None,
            }],
            copilot_confirmation: None,
            function_call: None,
        };
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::Error { message, .. } => {
                assert!(message.contains("auth_error"));
                assert!(message.contains("invalid token"));
            }
            other => panic!("Expected Error, got {other:?}"),
        }
    }

    #[test]
    fn map_response_with_function_call() {
        let resp = CopilotResponse {
            message: String::new(),
            copilot_references: vec![],
            copilot_errors: vec![],
            copilot_confirmation: None,
            function_call: Some(make_function_call(
                "read_file",
                r#"{"path":"src/main.rs"}"#,
                Some("call_123"),
            )),
        };
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("call_123"));
            }
            other => panic!("Expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn map_response_with_confirmation() {
        let resp = CopilotResponse {
            message: String::new(),
            copilot_references: vec![],
            copilot_errors: vec![],
            copilot_confirmation: Some(CopilotConfirmation {
                id: "conf-1".into(),
                title: "Approve deletion".into(),
                message: "Delete temp files?".into(),
                accepted: None,
            }),
            function_call: None,
        };
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::Warning { message } => {
                assert!(message.contains("Confirmation required"));
                assert!(message.contains("Approve deletion"));
            }
            other => panic!("Expected Warning, got {other:?}"),
        }
        assert!(events[0].ext.is_some());
    }

    #[test]
    fn map_response_combined() {
        let resp = CopilotResponse {
            message: "Working on it".into(),
            copilot_references: vec![],
            copilot_errors: vec![CopilotError {
                error_type: "warning".into(),
                message: "slow response".into(),
                code: None,
                identifier: None,
            }],
            copilot_confirmation: None,
            function_call: Some(make_function_call("tool", "{}", Some("c1"))),
        };
        let events = dialect::map_response(&resp);
        // message + error + tool call
        assert_eq!(events.len(), 3);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: Stream event mapping
// ═══════════════════════════════════════════════════════════════════════════

mod stream_event_mapping {
    use super::*;

    #[test]
    fn map_text_delta() {
        let event = CopilotStreamEvent::TextDelta {
            text: "hello".into(),
        };
        let mapped = dialect::map_stream_event(&event);
        assert_eq!(mapped.len(), 1);
        match &mapped[0].kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "hello"),
            other => panic!("Expected AssistantDelta, got {other:?}"),
        }
    }

    #[test]
    fn map_function_call_event() {
        let event = CopilotStreamEvent::FunctionCall {
            function_call: make_function_call("search", r#"{"q":"test"}"#, Some("call_1")),
        };
        let mapped = dialect::map_stream_event(&event);
        assert_eq!(mapped.len(), 1);
        match &mapped[0].kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                assert_eq!(tool_name, "search");
                assert_eq!(tool_use_id.as_deref(), Some("call_1"));
                assert_eq!(input["q"], "test");
            }
            other => panic!("Expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn map_confirmation_event() {
        let event = CopilotStreamEvent::CopilotConfirmation {
            confirmation: CopilotConfirmation {
                id: "conf-1".into(),
                title: "Allow?".into(),
                message: "Allow file write?".into(),
                accepted: None,
            },
        };
        let mapped = dialect::map_stream_event(&event);
        assert_eq!(mapped.len(), 1);
        match &mapped[0].kind {
            AgentEventKind::Warning { message } => {
                assert!(message.contains("Allow?"));
            }
            other => panic!("Expected Warning, got {other:?}"),
        }
    }

    #[test]
    fn map_errors_event() {
        let event = CopilotStreamEvent::CopilotErrors {
            errors: vec![
                CopilotError {
                    error_type: "rate_limit".into(),
                    message: "too many requests".into(),
                    code: None,
                    identifier: None,
                },
                CopilotError {
                    error_type: "timeout".into(),
                    message: "request timed out".into(),
                    code: None,
                    identifier: None,
                },
            ],
        };
        let mapped = dialect::map_stream_event(&event);
        assert_eq!(mapped.len(), 2);
    }

    #[test]
    fn map_references_empty() {
        let event = CopilotStreamEvent::CopilotReferences { references: vec![] };
        let mapped = dialect::map_stream_event(&event);
        assert!(mapped.is_empty());
    }

    #[test]
    fn map_references_nonempty() {
        let event = CopilotStreamEvent::CopilotReferences {
            references: vec![make_reference(CopilotReferenceType::File, "f1")],
        };
        let mapped = dialect::map_stream_event(&event);
        assert_eq!(mapped.len(), 1);
        match &mapped[0].kind {
            AgentEventKind::RunStarted { message } => {
                assert!(message.contains("1 reference"));
            }
            other => panic!("Expected RunStarted, got {other:?}"),
        }
    }

    #[test]
    fn map_done_event() {
        let event = CopilotStreamEvent::Done {};
        let mapped = dialect::map_stream_event(&event);
        assert_eq!(mapped.len(), 1);
        match &mapped[0].kind {
            AgentEventKind::RunCompleted { message } => {
                assert!(message.contains("completed"));
            }
            other => panic!("Expected RunCompleted, got {other:?}"),
        }
    }

    #[test]
    fn map_function_call_invalid_json_args() {
        let event = CopilotStreamEvent::FunctionCall {
            function_call: make_function_call("tool", "not valid json", Some("c1")),
        };
        let mapped = dialect::map_stream_event(&event);
        assert_eq!(mapped.len(), 1);
        // Invalid JSON becomes a string value
        match &mapped[0].kind {
            AgentEventKind::ToolCall { input, .. } => {
                assert!(input.is_string());
            }
            other => panic!("Expected ToolCall, got {other:?}"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: Passthrough fidelity
// ═══════════════════════════════════════════════════════════════════════════

mod passthrough_fidelity {
    use super::*;

    #[test]
    fn text_delta_passthrough() {
        let event = CopilotStreamEvent::TextDelta {
            text: "hello".into(),
        };
        let wrapped = dialect::to_passthrough_event(&event);
        let ext = wrapped.ext.as_ref().unwrap();
        assert_eq!(ext["dialect"], "copilot");
        assert!(ext.contains_key("raw_message"));

        let recovered = dialect::from_passthrough_event(&wrapped).unwrap();
        assert_eq!(recovered, event);
    }

    #[test]
    fn function_call_passthrough() {
        let event = CopilotStreamEvent::FunctionCall {
            function_call: make_function_call("search", r#"{"q":"test"}"#, Some("c1")),
        };
        let wrapped = dialect::to_passthrough_event(&event);
        let recovered = dialect::from_passthrough_event(&wrapped).unwrap();
        assert_eq!(recovered, event);
    }

    #[test]
    fn done_passthrough() {
        let event = CopilotStreamEvent::Done {};
        let wrapped = dialect::to_passthrough_event(&event);
        let recovered = dialect::from_passthrough_event(&wrapped).unwrap();
        assert_eq!(recovered, event);
    }

    #[test]
    fn error_passthrough() {
        let event = CopilotStreamEvent::CopilotErrors {
            errors: vec![CopilotError {
                error_type: "backend_error".into(),
                message: "boom".into(),
                code: Some("500".into()),
                identifier: Some("err-1".into()),
            }],
        };
        let wrapped = dialect::to_passthrough_event(&event);
        let recovered = dialect::from_passthrough_event(&wrapped).unwrap();
        assert_eq!(recovered, event);
    }

    #[test]
    fn confirmation_passthrough() {
        let event = CopilotStreamEvent::CopilotConfirmation {
            confirmation: CopilotConfirmation {
                id: "conf-1".into(),
                title: "Approve?".into(),
                message: "Allow?".into(),
                accepted: Some(true),
            },
        };
        let wrapped = dialect::to_passthrough_event(&event);
        let recovered = dialect::from_passthrough_event(&wrapped).unwrap();
        assert_eq!(recovered, event);
    }

    #[test]
    fn from_passthrough_no_ext_returns_none() {
        let event = make_event(AgentEventKind::AssistantDelta { text: "hi".into() });
        assert!(dialect::from_passthrough_event(&event).is_none());
    }

    #[test]
    fn from_passthrough_no_raw_message_returns_none() {
        let mut ext = BTreeMap::new();
        ext.insert("dialect".into(), json!("copilot"));
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "hi".into() },
            ext: Some(ext),
        };
        assert!(dialect::from_passthrough_event(&event).is_none());
    }

    #[test]
    fn verify_passthrough_fidelity_all_events() {
        let events = vec![
            CopilotStreamEvent::CopilotReferences {
                references: vec![make_reference(CopilotReferenceType::File, "f1")],
            },
            CopilotStreamEvent::TextDelta {
                text: "hello".into(),
            },
            CopilotStreamEvent::FunctionCall {
                function_call: make_function_call("tool", "{}", Some("c1")),
            },
            CopilotStreamEvent::CopilotErrors {
                errors: vec![CopilotError {
                    error_type: "err".into(),
                    message: "msg".into(),
                    code: None,
                    identifier: None,
                }],
            },
            CopilotStreamEvent::Done {},
        ];
        assert!(dialect::verify_passthrough_fidelity(&events));
    }

    #[test]
    fn verify_passthrough_fidelity_empty() {
        assert!(dialect::verify_passthrough_fidelity(&[]));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: Lowering
// ═══════════════════════════════════════════════════════════════════════════

mod lowering_tests {
    use super::*;

    #[test]
    fn to_ir_user_message() {
        let msgs = vec![make_copilot_message("user", "Hello")];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello");
    }

    #[test]
    fn to_ir_system_message() {
        let msgs = vec![make_copilot_message("system", "Be helpful")];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::System);
    }

    #[test]
    fn to_ir_assistant_message() {
        let msgs = vec![make_copilot_message("assistant", "Sure!")];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
    }

    #[test]
    fn to_ir_unknown_role_defaults_user() {
        let msgs = vec![make_copilot_message("developer", "hi")];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    #[test]
    fn from_ir_tool_role_becomes_user() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::Text {
                text: "result".into(),
            }],
        )]);
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].role, "user");
    }

    #[test]
    fn roundtrip_preserves_content() {
        let msgs = vec![
            make_copilot_message("system", "Sys"),
            make_copilot_message("user", "Usr"),
            make_copilot_message("assistant", "Asst"),
        ];
        let conv = lowering::to_ir(&msgs);
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].content, "Sys");
        assert_eq!(back[1].content, "Usr");
        assert_eq!(back[2].content, "Asst");
    }

    #[test]
    fn empty_content_roundtrip() {
        let msgs = vec![make_copilot_message("user", "")];
        let conv = lowering::to_ir(&msgs);
        let back = lowering::from_ir(&conv);
        assert!(back[0].content.is_empty());
    }

    #[test]
    fn extract_references_from_ir() {
        let msgs = vec![CopilotMessage {
            role: "user".into(),
            content: "Check".into(),
            name: None,
            copilot_references: vec![
                make_reference(CopilotReferenceType::File, "f1"),
                make_reference(CopilotReferenceType::Snippet, "s1"),
            ],
        }];
        let conv = lowering::to_ir(&msgs);
        let refs = lowering::extract_references(&conv);
        assert_eq!(refs.len(), 2);
    }

    #[test]
    fn extract_references_empty_when_none() {
        let msgs = vec![make_copilot_message("user", "no refs")];
        let conv = lowering::to_ir(&msgs);
        let refs = lowering::extract_references(&conv);
        assert!(refs.is_empty());
    }

    #[test]
    fn thinking_block_to_copilot_text() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "reasoning...".into(),
            }],
        )]);
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].content, "reasoning...");
    }

    #[test]
    fn name_metadata_roundtrip() {
        let msgs = vec![CopilotMessage {
            role: "user".into(),
            content: "Hi".into(),
            name: Some("bob".into()),
            copilot_references: vec![],
        }];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(
            conv.messages[0]
                .metadata
                .get("copilot_name")
                .and_then(|v| v.as_str()),
            Some("bob")
        );
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].name.as_deref(), Some("bob"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: Serialization
// ═══════════════════════════════════════════════════════════════════════════

mod serde_tests {
    use super::*;

    #[test]
    fn copilot_request_serde_roundtrip() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![Message::user("Hello")])
            .build();
        let json = serde_json::to_string(&req).unwrap();
        let back: CopilotRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, "gpt-4o");
        assert_eq!(back.messages[0].content, "Hello");
    }

    #[test]
    fn copilot_response_serde_roundtrip() {
        let resp = CopilotResponse {
            message: "Hello!".into(),
            copilot_references: vec![make_reference(CopilotReferenceType::File, "f1")],
            copilot_errors: vec![],
            copilot_confirmation: None,
            function_call: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: CopilotResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.message, "Hello!");
        assert_eq!(back.copilot_references.len(), 1);
    }

    #[test]
    fn copilot_stream_event_text_delta_serde() {
        let event = CopilotStreamEvent::TextDelta {
            text: "chunk".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn copilot_stream_event_done_serde() {
        let event = CopilotStreamEvent::Done {};
        let json = serde_json::to_string(&event).unwrap();
        let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn copilot_stream_event_function_call_serde() {
        let event = CopilotStreamEvent::FunctionCall {
            function_call: make_function_call("read", r#"{"p":"a"}"#, Some("c1")),
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn copilot_error_serde() {
        let err = CopilotError {
            error_type: "backend_error".into(),
            message: "server error".into(),
            code: Some("500".into()),
            identifier: Some("err-abc".into()),
        };
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["type"], "backend_error");
        assert_eq!(json["code"], "500");
        let back: CopilotError = serde_json::from_value(json).unwrap();
        assert_eq!(back.error_type, "backend_error");
    }

    #[test]
    fn copilot_reference_serde() {
        let r = CopilotReference {
            ref_type: CopilotReferenceType::Repository,
            id: "repo-0".into(),
            data: json!({"owner": "octocat", "name": "hello"}),
            metadata: Some({
                let mut m = BTreeMap::new();
                m.insert("label".into(), json!("My Repo"));
                m
            }),
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: CopilotReference = serde_json::from_str(&json).unwrap();
        assert_eq!(back.ref_type, CopilotReferenceType::Repository);
        assert_eq!(back.id, "repo-0");
        assert!(back.metadata.is_some());
    }

    #[test]
    fn copilot_turn_entry_serde() {
        let entry = CopilotTurnEntry {
            request: "What is Rust?".into(),
            response: "A systems language.".into(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: CopilotTurnEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, entry);
    }

    #[test]
    fn copilot_function_call_serde() {
        let fc = make_function_call("tool", r#"{"key":"value"}"#, Some("call_1"));
        let json = serde_json::to_string(&fc).unwrap();
        let back: CopilotFunctionCall = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "tool");
        assert_eq!(back.id.as_deref(), Some("call_1"));
    }

    #[test]
    fn copilot_function_call_no_id_serde() {
        let fc = make_function_call("tool", "{}", None);
        let json = serde_json::to_value(&fc).unwrap();
        // id should be omitted or null
        assert!(json.get("id").is_none() || json["id"].is_null());
    }

    #[test]
    fn copilot_message_serde_roundtrip() {
        let msg = CopilotMessage {
            role: "user".into(),
            content: "Hello".into(),
            name: Some("alice".into()),
            copilot_references: vec![make_reference(CopilotReferenceType::File, "f1")],
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: CopilotMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn reference_type_serde_variants() {
        let types = vec![
            (CopilotReferenceType::File, "file"),
            (CopilotReferenceType::Snippet, "snippet"),
            (CopilotReferenceType::Repository, "repository"),
            (CopilotReferenceType::WebSearchResult, "web_search_result"),
        ];
        for (variant, expected_str) in types {
            let json = serde_json::to_value(&variant).unwrap();
            assert_eq!(json.as_str().unwrap(), expected_str);
        }
    }

    #[test]
    fn tool_type_serde_variants() {
        let json = serde_json::to_value(CopilotToolType::Function).unwrap();
        assert_eq!(json, "function");
        let json = serde_json::to_value(CopilotToolType::Confirmation).unwrap();
        assert_eq!(json, "confirmation");
    }

    #[test]
    fn stream_event_references_serde() {
        let event = CopilotStreamEvent::CopilotReferences {
            references: vec![make_reference(CopilotReferenceType::Snippet, "s1")],
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn stream_event_confirmation_serde() {
        let event = CopilotStreamEvent::CopilotConfirmation {
            confirmation: CopilotConfirmation {
                id: "c1".into(),
                title: "OK?".into(),
                message: "Proceed?".into(),
                accepted: Some(false),
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: CopilotStreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: Error handling
// ═══════════════════════════════════════════════════════════════════════════

mod error_handling {
    use super::*;

    #[test]
    fn shim_error_invalid_request_display() {
        let err = ShimError::InvalidRequest("bad input".into());
        assert!(err.to_string().contains("invalid request"));
        assert!(err.to_string().contains("bad input"));
    }

    #[test]
    fn shim_error_internal_display() {
        let err = ShimError::Internal("something broke".into());
        assert!(err.to_string().contains("internal error"));
    }

    #[test]
    fn shim_error_serde_from_json_error() {
        let json_err = serde_json::from_str::<String>("not valid json").unwrap_err();
        let err = ShimError::from(json_err);
        assert!(matches!(err, ShimError::Serde(_)));
        assert!(err.to_string().contains("serde error"));
    }

    #[test]
    fn shim_error_debug_format() {
        let err = ShimError::InvalidRequest("test".into());
        let debug = format!("{err:?}");
        assert!(debug.contains("InvalidRequest"));
    }

    #[tokio::test]
    async fn client_error_is_internal_not_serde() {
        let client = CopilotClient::new("gpt-4o");
        let req = simple_copilot_request("test");
        let err = client.create(req).await.unwrap_err();
        assert!(matches!(err, ShimError::Internal(_)));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module: Edge cases
// ═══════════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;

    #[test]
    fn empty_messages_to_ir() {
        let conv = messages_to_ir(&[]);
        assert!(conv.is_empty());
    }

    #[test]
    fn very_long_content() {
        let long_text = "a".repeat(100_000);
        let msg = Message::user(&long_text);
        let conv = messages_to_ir(&[msg]);
        let back = ir_to_messages(&conv);
        assert_eq!(back[0].content.len(), 100_000);
    }

    #[test]
    fn unicode_content_preserved() {
        let msg = Message::user("Hello \u{1F600} \u{4E16}\u{754C}");
        let conv = messages_to_ir(&[msg]);
        let back = ir_to_messages(&conv);
        assert!(back[0].content.contains('\u{1F600}'));
        assert!(back[0].content.contains('\u{4E16}'));
    }

    #[test]
    fn special_characters_in_tool_args() {
        let events = vec![make_event(AgentEventKind::ToolCall {
            tool_name: "write".into(),
            tool_use_id: Some("c1".into()),
            parent_tool_use_id: None,
            input: json!({"content": "line1\nline2\ttab"}),
        })];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let fc = resp.function_call.unwrap();
        assert!(fc.arguments.contains("line1"));
    }

    #[test]
    fn multiple_references_different_types() {
        let refs = vec![
            make_reference(CopilotReferenceType::File, "f1"),
            make_reference(CopilotReferenceType::Snippet, "s1"),
            make_reference(CopilotReferenceType::Repository, "r1"),
            make_reference(CopilotReferenceType::WebSearchResult, "w1"),
        ];
        let msg = Message::user_with_refs("Check all", refs);
        let conv = messages_to_ir(&[msg]);
        let back = ir_to_messages(&conv);
        assert_eq!(back[0].copilot_references.len(), 4);
    }

    #[test]
    fn reference_with_metadata() {
        let mut metadata = BTreeMap::new();
        metadata.insert("label".into(), json!("main file"));
        metadata.insert("uri".into(), json!("file:///src/main.rs"));
        let r = CopilotReference {
            ref_type: CopilotReferenceType::File,
            id: "f1".into(),
            data: json!({"path": "src/main.rs"}),
            metadata: Some(metadata),
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: CopilotReference = serde_json::from_str(&json).unwrap();
        let meta = back.metadata.unwrap();
        assert_eq!(meta["label"], "main file");
    }

    #[test]
    fn tool_call_no_id() {
        let events = vec![make_event(AgentEventKind::ToolCall {
            tool_name: "tool".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        })];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let fc = resp.function_call.unwrap();
        assert!(fc.id.is_none());
    }

    #[test]
    fn mock_receipt_has_valid_structure() {
        let receipt = mock_receipt(vec![]);
        assert!(!receipt.meta.run_id.is_nil());
        assert_eq!(receipt.meta.contract_version, abp_core::CONTRACT_VERSION);
        assert_eq!(receipt.backend.id, "mock");
    }

    #[test]
    fn mock_receipt_with_custom_usage() {
        let usage = UsageNormalized {
            input_tokens: Some(1000),
            output_tokens: Some(500),
            cache_read_tokens: Some(100),
            cache_write_tokens: Some(50),
            request_units: Some(1),
            estimated_cost_usd: Some(0.01),
        };
        let receipt = mock_receipt_with_usage(vec![], usage);
        assert_eq!(receipt.usage.input_tokens, Some(1000));
        assert_eq!(receipt.usage.estimated_cost_usd, Some(0.01));
    }

    #[test]
    fn confirmation_accepted_none() {
        let conf = CopilotConfirmation {
            id: "c1".into(),
            title: "T".into(),
            message: "M".into(),
            accepted: None,
        };
        let json = serde_json::to_value(&conf).unwrap();
        assert!(json.get("accepted").is_none());
    }

    #[test]
    fn response_with_all_fields() {
        let resp = CopilotResponse {
            message: "Done".into(),
            copilot_references: vec![make_reference(CopilotReferenceType::File, "f1")],
            copilot_errors: vec![CopilotError {
                error_type: "warn".into(),
                message: "slow".into(),
                code: None,
                identifier: None,
            }],
            copilot_confirmation: Some(CopilotConfirmation {
                id: "c1".into(),
                title: "OK?".into(),
                message: "Proceed?".into(),
                accepted: Some(true),
            }),
            function_call: Some(make_function_call("tool", "{}", Some("c1"))),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: CopilotResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.message, "Done");
        assert_eq!(back.copilot_references.len(), 1);
        assert_eq!(back.copilot_errors.len(), 1);
        assert!(back.copilot_confirmation.is_some());
        assert!(back.function_call.is_some());
    }

    #[test]
    fn request_with_all_fields() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![Message::system("sys"), Message::user("hi")])
            .tools(vec![CopilotTool {
                tool_type: CopilotToolType::Function,
                function: Some(CopilotFunctionDef {
                    name: "t".into(),
                    description: "d".into(),
                    parameters: json!({}),
                }),
                confirmation: None,
            }])
            .turn_history(vec![CopilotTurnEntry {
                request: "q".into(),
                response: "a".into(),
            }])
            .references(vec![make_reference(CopilotReferenceType::File, "f1")])
            .build();
        let json = serde_json::to_string(&req).unwrap();
        let back: CopilotRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.messages.len(), 2);
        assert!(back.tools.is_some());
        assert_eq!(back.turn_history.len(), 1);
        assert_eq!(back.references.len(), 1);
    }

    #[test]
    fn re_exported_types_accessible() {
        // Verify re-exports from abp-shim-copilot
        let _def = abp_shim_copilot::CopilotFunctionDef {
            name: "test".into(),
            description: "test".into(),
            parameters: json!({}),
        };
        let _tt = abp_shim_copilot::CopilotToolType::Function;
    }
}
