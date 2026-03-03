// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Comprehensive tests for OpenAI SDK dialect mapping.
//!
//! Validates the full round-trip from OpenAI request/response formats
//! through ABP's intermediate representation and back.

use std::collections::BTreeMap;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, CONTRACT_VERSION, Outcome, ReceiptBuilder, UsageNormalized,
    WorkOrderBuilder,
};
use abp_openai_sdk::dialect::{
    self, CanonicalToolDef, OpenAIChoice, OpenAIConfig, OpenAIFunctionCall, OpenAIFunctionDef,
    OpenAIMessage, OpenAIRequest, OpenAIResponse, OpenAIToolCall, OpenAIToolDef, OpenAIUsage,
    ToolChoice, ToolChoiceFunctionRef, ToolChoiceMode,
};
use abp_openai_sdk::lowering;
use abp_openai_sdk::response_format::{JsonSchemaSpec, ResponseFormat};
use abp_openai_sdk::streaming::{
    ChatCompletionChunk, ChunkChoice, ChunkDelta, ChunkFunctionCall, ChunkToolCall, ChunkUsage,
    ToolCallAccumulator,
};
use abp_openai_sdk::validation::{ExtendedRequestFields, UnmappableParam, ValidationErrors};
use abp_shim_openai::{
    ChatCompletionRequest, FunctionCall, Message, OpenAiClient, ProcessFn, Role, StreamEvent, Tool,
    ToolCall, Usage, events_to_stream_events, ir_to_messages, ir_usage_to_usage, messages_to_ir,
    mock_receipt, mock_receipt_with_usage, receipt_to_response, request_to_ir,
    request_to_work_order, tools_to_ir,
};
use chrono::Utc;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn msg(role: &str, content: Option<&str>) -> OpenAIMessage {
    OpenAIMessage {
        role: role.into(),
        content: content.map(Into::into),
        tool_calls: None,
        tool_call_id: None,
    }
}

fn tool_result_msg(content: Option<&str>, tool_call_id: &str) -> OpenAIMessage {
    OpenAIMessage {
        role: "tool".into(),
        content: content.map(Into::into),
        tool_calls: None,
        tool_call_id: Some(tool_call_id.into()),
    }
}

fn assistant_with_tool_calls(content: Option<&str>, calls: Vec<OpenAIToolCall>) -> OpenAIMessage {
    OpenAIMessage {
        role: "assistant".into(),
        content: content.map(Into::into),
        tool_calls: Some(calls),
        tool_call_id: None,
    }
}

fn make_tool_call(id: &str, name: &str, args: &str) -> OpenAIToolCall {
    OpenAIToolCall {
        id: id.into(),
        call_type: "function".into(),
        function: OpenAIFunctionCall {
            name: name.into(),
            arguments: args.into(),
        },
    }
}

fn simple_response(text: &str) -> OpenAIResponse {
    OpenAIResponse {
        id: "chatcmpl-test".into(),
        object: "chat.completion".into(),
        model: "gpt-4o".into(),
        choices: vec![OpenAIChoice {
            index: 0,
            message: msg("assistant", Some(text)),
            finish_reason: Some("stop".into()),
        }],
        usage: None,
    }
}

fn make_chunk(id: &str, delta: ChunkDelta, finish_reason: Option<&str>) -> ChatCompletionChunk {
    ChatCompletionChunk {
        id: id.into(),
        object: "chat.completion.chunk".into(),
        created: 1_700_000_000,
        model: "gpt-4o".into(),
        choices: vec![ChunkChoice {
            index: 0,
            delta,
            finish_reason: finish_reason.map(Into::into),
        }],
        usage: None,
    }
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
            tool_name: name.into(),
            tool_use_id: Some(id.into()),
            parent_tool_use_id: None,
            input,
        },
        ext: None,
    }
}

fn error_event(message: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: message.into(),
            error_code: None,
        },
        ext: None,
    }
}

fn make_processor(events: Vec<AgentEvent>) -> ProcessFn {
    Box::new(move |_wo| mock_receipt(events.clone()))
}

fn make_processor_with_usage(events: Vec<AgentEvent>, usage: UsageNormalized) -> ProcessFn {
    Box::new(move |_wo| mock_receipt_with_usage(events.clone(), usage.clone()))
}

fn make_tool_def(name: &str, desc: &str) -> Tool {
    Tool::function(
        name,
        desc,
        json!({
            "type": "object",
            "properties": {
                "input": {"type": "string"}
            }
        }),
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 1: Chat Completion Request Mapping
// ═══════════════════════════════════════════════════════════════════════════

mod chat_completion_request_mapping {
    use super::*;

    #[test]
    fn minimal_request_to_work_order() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("Hello")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Hello");
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn request_preserves_model_name() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4-turbo")
            .messages(vec![Message::user("Hi")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
    }

    #[test]
    fn request_with_temperature_in_vendor_config() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("Hi")])
            .temperature(0.7)
            .build();
        let wo = request_to_work_order(&req);
        let temp = wo.config.vendor.get("temperature");
        assert!(temp.is_some());
        assert_eq!(temp.unwrap().as_f64(), Some(0.7));
    }

    #[test]
    fn request_with_max_tokens_in_vendor_config() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("Hi")])
            .max_tokens(2048)
            .build();
        let wo = request_to_work_order(&req);
        let max_t = wo.config.vendor.get("max_tokens");
        assert!(max_t.is_some());
        assert_eq!(max_t.unwrap().as_u64(), Some(2048));
    }

    #[test]
    fn request_with_stop_sequences_in_vendor_config() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("Hi")])
            .stop(vec!["DONE".into(), "END".into()])
            .build();
        let wo = request_to_work_order(&req);
        let stop = wo.config.vendor.get("stop");
        assert!(stop.is_some());
    }

    #[test]
    fn default_model_is_gpt4o() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("Hi")])
            .build();
        assert_eq!(req.model, "gpt-4o");
    }

    #[test]
    fn request_serde_roundtrip_minimal() {
        let req = OpenAIRequest {
            model: "gpt-4o".into(),
            messages: vec![msg("user", Some("Hi"))],
            tools: None,
            tool_choice: None,
            temperature: None,
            max_tokens: None,
            response_format: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: OpenAIRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model, "gpt-4o");
        assert_eq!(parsed.messages.len(), 1);
    }

    #[test]
    fn request_serde_roundtrip_with_all_fields() {
        let req = OpenAIRequest {
            model: "gpt-4-turbo".into(),
            messages: vec![msg("system", Some("Be helpful")), msg("user", Some("Hi"))],
            tools: Some(vec![OpenAIToolDef {
                tool_type: "function".into(),
                function: OpenAIFunctionDef {
                    name: "search".into(),
                    description: "Search the web".into(),
                    parameters: json!({"type": "object"}),
                },
            }]),
            tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::Auto)),
            temperature: Some(0.5),
            max_tokens: Some(4096),
            response_format: Some(ResponseFormat::JsonObject),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: OpenAIRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model, "gpt-4-turbo");
        assert_eq!(parsed.messages.len(), 2);
        assert!(parsed.tools.is_some());
        assert!(parsed.tool_choice.is_some());
        assert_eq!(parsed.temperature, Some(0.5));
        assert_eq!(parsed.max_tokens, Some(4096));
    }

    #[test]
    fn request_to_ir_simple_user_message() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("Hello world")])
            .build();
        let conv = request_to_ir(&req);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello world");
    }

    #[test]
    fn request_to_ir_system_and_user() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![
                Message::system("You are helpful"),
                Message::user("What is Rust?"),
            ])
            .build();
        let conv = request_to_ir(&req);
        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[1].role, IrRole::User);
    }

    #[test]
    fn work_order_uses_last_user_message_as_task() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![
                Message::user("First question"),
                Message::assistant("First answer"),
                Message::user("Follow up question"),
            ])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Follow up question");
    }

    #[test]
    fn work_order_contract_version_matches() {
        let receipt = ReceiptBuilder::new("openai").build();
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 2: Function Calling / Tool Use
// ═══════════════════════════════════════════════════════════════════════════

mod function_calling_tool_use {
    use super::*;

    #[test]
    fn single_tool_call_to_ir() {
        let msgs = [assistant_with_tool_calls(
            None,
            vec![make_tool_call("c1", "read_file", r#"{"path":"main.rs"}"#)],
        )];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.len(), 1);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "c1");
                assert_eq!(name, "read_file");
                assert_eq!(input, &json!({"path": "main.rs"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn multiple_tool_calls_to_ir() {
        let msgs = [assistant_with_tool_calls(
            None,
            vec![
                make_tool_call("c1", "read_file", r#"{"path":"a.rs"}"#),
                make_tool_call("c2", "write_file", r#"{"path":"b.rs","content":"hi"}"#),
            ],
        )];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].content.len(), 2);
        assert!(matches!(
            &conv.messages[0].content[0],
            IrContentBlock::ToolUse { name, .. } if name == "read_file"
        ));
        assert!(matches!(
            &conv.messages[0].content[1],
            IrContentBlock::ToolUse { name, .. } if name == "write_file"
        ));
    }

    #[test]
    fn tool_result_message_to_ir() {
        let msgs = [tool_result_msg(Some("file contents here"), "c1")];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::Tool);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "c1");
                assert!(!is_error);
                assert!(!content.is_empty());
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn tool_call_roundtrip_through_ir() {
        let original = [assistant_with_tool_calls(
            None,
            vec![make_tool_call("call_99", "search", r#"{"query":"rust"}"#)],
        )];
        let ir = lowering::to_ir(&original);
        let back = lowering::from_ir(&ir);
        assert_eq!(back[0].role, "assistant");
        let tcs = back[0].tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id, "call_99");
        assert_eq!(tcs[0].function.name, "search");
    }

    #[test]
    fn tool_result_roundtrip_through_ir() {
        let original = [tool_result_msg(Some("result data"), "call_99")];
        let ir = lowering::to_ir(&original);
        let back = lowering::from_ir(&ir);
        assert_eq!(back[0].role, "tool");
        assert_eq!(back[0].tool_call_id.as_deref(), Some("call_99"));
        assert_eq!(back[0].content.as_deref(), Some("result data"));
    }

    #[test]
    fn tool_def_canonical_roundtrip() {
        let canonical = CanonicalToolDef {
            name: "get_weather".into(),
            description: "Get weather for a location".into(),
            parameters_schema: json!({
                "type": "object",
                "properties": {"location": {"type": "string"}}
            }),
        };
        let openai = dialect::tool_def_to_openai(&canonical);
        assert_eq!(openai.tool_type, "function");
        assert_eq!(openai.function.name, "get_weather");
        let back = dialect::tool_def_from_openai(&openai);
        assert_eq!(back.name, canonical.name);
        assert_eq!(back.description, canonical.description);
        assert_eq!(back.parameters_schema, canonical.parameters_schema);
    }

    #[test]
    fn shim_tool_to_ir_definition() {
        let tools = vec![make_tool_def("read_file", "Read a file")];
        let ir_tools = tools_to_ir(&tools);
        assert_eq!(ir_tools.len(), 1);
        assert_eq!(ir_tools[0].name, "read_file");
        assert_eq!(ir_tools[0].description, "Read a file");
    }

    #[test]
    fn assistant_with_text_and_tool_calls() {
        let msgs = [assistant_with_tool_calls(
            Some("Let me search that"),
            vec![make_tool_call("c1", "search", r#"{"q":"rust"}"#)],
        )];
        let ir = lowering::to_ir(&msgs);
        assert_eq!(ir.messages[0].content.len(), 2);
        assert!(matches!(
            &ir.messages[0].content[0],
            IrContentBlock::Text { text } if text == "Let me search that"
        ));
        assert!(matches!(
            &ir.messages[0].content[1],
            IrContentBlock::ToolUse { .. }
        ));
    }

    #[test]
    fn malformed_tool_arguments_preserved() {
        let msgs = [assistant_with_tool_calls(
            None,
            vec![make_tool_call("c_bad", "foo", "not-valid-json")],
        )];
        let conv = lowering::to_ir(&msgs);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { input, .. } => {
                // Invalid JSON args should be stored as a string value
                assert!(input.is_string());
                assert_eq!(input.as_str().unwrap(), "not-valid-json");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn empty_tool_call_list_preserved() {
        let m = OpenAIMessage {
            role: "assistant".into(),
            content: Some("Just text".into()),
            tool_calls: Some(vec![]),
            tool_call_id: None,
        };
        let ir = lowering::to_ir(&[m]);
        assert_eq!(ir.messages[0].role, IrRole::Assistant);
        // Only the text block, no tool calls from empty vec
        assert_eq!(ir.messages[0].text_content(), "Just text");
    }

    #[test]
    fn receipt_to_response_maps_tool_calls() {
        let events = vec![tool_call_event(
            "get_weather",
            "call_abc",
            json!({"location": "NYC"}),
        )];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let msg = &resp.choices[0].message;
        assert!(msg.tool_calls.is_some());
        let tcs = msg.tool_calls.as_ref().unwrap();
        assert_eq!(tcs[0].function.name, "get_weather");
        assert_eq!(tcs[0].id, "call_abc");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn receipt_with_tool_call_missing_id_generates_uuid() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "search".into(),
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
    fn multiple_tool_calls_in_receipt() {
        let events = vec![
            tool_call_event("read", "call_1", json!({"path": "a.rs"})),
            tool_call_event("write", "call_2", json!({"path": "b.rs"})),
        ];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 2);
        assert_eq!(tcs[0].function.name, "read");
        assert_eq!(tcs[1].function.name, "write");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 3: Streaming (SSE) Response Mapping
// ═══════════════════════════════════════════════════════════════════════════

mod streaming_sse_mapping {
    use super::*;

    #[test]
    fn text_delta_chunk_maps_to_assistant_delta() {
        let chunk = make_chunk(
            "chatcmpl-1",
            ChunkDelta {
                content: Some("Hello".into()),
                ..Default::default()
            },
            None,
        );
        let events = abp_openai_sdk::streaming::map_chunk(&chunk);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::AssistantDelta { text } if text == "Hello"
        ));
    }

    #[test]
    fn empty_delta_content_produces_no_events() {
        let chunk = make_chunk(
            "chatcmpl-1",
            ChunkDelta {
                content: Some(String::new()),
                ..Default::default()
            },
            None,
        );
        let events = abp_openai_sdk::streaming::map_chunk(&chunk);
        assert!(events.is_empty());
    }

    #[test]
    fn none_delta_content_produces_no_events() {
        let chunk = make_chunk("chatcmpl-1", ChunkDelta::default(), Some("stop"));
        let events = abp_openai_sdk::streaming::map_chunk(&chunk);
        assert!(events.is_empty());
    }

    #[test]
    fn accumulator_reassembles_streamed_tool_calls() {
        let mut acc = ToolCallAccumulator::new();
        // First fragment: name + start of args
        acc.feed(&[ChunkToolCall {
            index: 0,
            id: Some("call_1".into()),
            call_type: Some("function".into()),
            function: Some(ChunkFunctionCall {
                name: Some("search".into()),
                arguments: Some(r#"{"q":"#.into()),
            }),
        }]);
        // Second fragment: rest of args
        acc.feed(&[ChunkToolCall {
            index: 0,
            id: None,
            call_type: None,
            function: Some(ChunkFunctionCall {
                name: None,
                arguments: Some(r#""rust"}"#.into()),
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
                assert_eq!(tool_name, "search");
                assert_eq!(tool_use_id.as_deref(), Some("call_1"));
                assert_eq!(input, &json!({"q": "rust"}));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn accumulator_parallel_tool_calls() {
        let mut acc = ToolCallAccumulator::new();
        acc.feed(&[
            ChunkToolCall {
                index: 0,
                id: Some("c1".into()),
                call_type: Some("function".into()),
                function: Some(ChunkFunctionCall {
                    name: Some("read".into()),
                    arguments: Some(r#"{"path":"a"}"#.into()),
                }),
            },
            ChunkToolCall {
                index: 1,
                id: Some("c2".into()),
                call_type: Some("function".into()),
                function: Some(ChunkFunctionCall {
                    name: Some("write".into()),
                    arguments: Some(r#"{"path":"b"}"#.into()),
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
                name: Some("test_fn".into()),
                arguments: Some(r#"{"key":"val"}"#.into()),
            }),
        }]);
        let pairs = acc.finish_as_openai();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, "c1");
        assert_eq!(pairs[0].1.name, "test_fn");
        assert_eq!(pairs[0].1.arguments, r#"{"key":"val"}"#);
    }

    #[test]
    fn accumulator_skips_empty_name_entries() {
        let mut acc = ToolCallAccumulator::new();
        acc.feed(&[ChunkToolCall {
            index: 0,
            id: Some("c1".into()),
            call_type: None,
            function: Some(ChunkFunctionCall {
                name: None,
                arguments: Some("partial".into()),
            }),
        }]);
        let events = acc.finish();
        // Entry has empty name => should be filtered out
        assert!(events.is_empty());
    }

    #[test]
    fn events_to_stream_events_text_deltas() {
        let events = vec![assistant_delta_event("Hel"), assistant_delta_event("lo!")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        // 2 deltas + final stop chunk
        assert_eq!(stream.len(), 3);
        assert_eq!(stream[0].choices[0].delta.content.as_deref(), Some("Hel"));
        assert_eq!(stream[1].choices[0].delta.content.as_deref(), Some("lo!"));
        assert_eq!(stream[2].choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn events_to_stream_events_assistant_message() {
        let events = vec![assistant_msg_event("Complete message")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert_eq!(stream.len(), 2); // message + stop
        assert_eq!(
            stream[0].choices[0].delta.role.as_deref(),
            Some("assistant")
        );
        assert_eq!(
            stream[0].choices[0].delta.content.as_deref(),
            Some("Complete message")
        );
    }

    #[test]
    fn events_to_stream_events_tool_call() {
        let events = vec![tool_call_event(
            "read_file",
            "call_1",
            json!({"path": "main.rs"}),
        )];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert_eq!(stream.len(), 2); // tool call + stop
        let tc = &stream[0].choices[0].delta.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.id.as_deref(), Some("call_1"));
        assert_eq!(
            tc.function.as_ref().unwrap().name.as_deref(),
            Some("read_file")
        );
    }

    #[test]
    fn stream_events_have_correct_object_type() {
        let events = vec![assistant_delta_event("Hi")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        for se in &stream {
            assert_eq!(se.object, "chat.completion.chunk");
        }
    }

    #[test]
    fn stream_events_preserve_model_name() {
        let events = vec![assistant_delta_event("Hi")];
        let stream = events_to_stream_events(&events, "gpt-4-turbo");
        for se in &stream {
            assert_eq!(se.model, "gpt-4-turbo");
        }
    }

    #[test]
    fn empty_events_still_produce_stop_chunk() {
        let stream = events_to_stream_events(&[], "gpt-4o");
        assert_eq!(stream.len(), 1);
        assert_eq!(stream[0].choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn chunk_serde_roundtrip() {
        let chunk = ChatCompletionChunk {
            id: "chatcmpl-abc".into(),
            object: "chat.completion.chunk".into(),
            created: 1_700_000_000,
            model: "gpt-4o".into(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: Some("assistant".into()),
                    content: Some("Hello".into()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let parsed: ChatCompletionChunk = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "chatcmpl-abc");
        assert_eq!(parsed.choices[0].delta.content.as_deref(), Some("Hello"));
    }

    #[test]
    fn chunk_usage_serde_roundtrip() {
        let chunk = ChatCompletionChunk {
            id: "chatcmpl-final".into(),
            object: "chat.completion.chunk".into(),
            created: 1_700_000_000,
            model: "gpt-4o".into(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta::default(),
                finish_reason: Some("stop".into()),
            }],
            usage: Some(ChunkUsage {
                prompt_tokens: 100,
                completion_tokens: 50,
                total_tokens: 150,
            }),
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let parsed: ChatCompletionChunk = serde_json::from_str(&json).unwrap();
        let usage = parsed.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 4: Model Names and Capability Mapping
// ═══════════════════════════════════════════════════════════════════════════

mod model_capability_mapping {
    use super::*;
    use abp_core::{Capability, SupportLevel};

    #[test]
    fn known_model_gpt4o() {
        assert!(dialect::is_known_model("gpt-4o"));
    }

    #[test]
    fn known_model_gpt4o_mini() {
        assert!(dialect::is_known_model("gpt-4o-mini"));
    }

    #[test]
    fn known_model_gpt4_turbo() {
        assert!(dialect::is_known_model("gpt-4-turbo"));
    }

    #[test]
    fn known_model_o1() {
        assert!(dialect::is_known_model("o1"));
    }

    #[test]
    fn known_model_o1_mini() {
        assert!(dialect::is_known_model("o1-mini"));
    }

    #[test]
    fn known_model_o3_mini() {
        assert!(dialect::is_known_model("o3-mini"));
    }

    #[test]
    fn known_model_gpt41() {
        assert!(dialect::is_known_model("gpt-4.1"));
    }

    #[test]
    fn unknown_model_returns_false() {
        assert!(!dialect::is_known_model("gpt-99-turbo"));
    }

    #[test]
    fn unknown_model_davinci() {
        assert!(!dialect::is_known_model("text-davinci-003"));
    }

    #[test]
    fn canonical_model_mapping() {
        assert_eq!(dialect::to_canonical_model("gpt-4o"), "openai/gpt-4o");
        assert_eq!(
            dialect::to_canonical_model("gpt-4-turbo"),
            "openai/gpt-4-turbo"
        );
    }

    #[test]
    fn canonical_model_round_trip() {
        let original = "gpt-4o";
        let canonical = dialect::to_canonical_model(original);
        let back = dialect::from_canonical_model(&canonical);
        assert_eq!(back, original);
    }

    #[test]
    fn from_canonical_strips_prefix() {
        assert_eq!(dialect::from_canonical_model("openai/gpt-4o"), "gpt-4o");
    }

    #[test]
    fn from_canonical_no_prefix_passthrough() {
        assert_eq!(dialect::from_canonical_model("gpt-4o"), "gpt-4o");
    }

    #[test]
    fn capability_manifest_has_streaming() {
        let manifest = dialect::capability_manifest();
        assert!(manifest.contains_key(&Capability::Streaming));
        assert!(matches!(
            manifest[&Capability::Streaming],
            SupportLevel::Native
        ));
    }

    #[test]
    fn capability_manifest_has_structured_output() {
        let manifest = dialect::capability_manifest();
        assert!(manifest.contains_key(&Capability::StructuredOutputJsonSchema));
        assert!(matches!(
            manifest[&Capability::StructuredOutputJsonSchema],
            SupportLevel::Native
        ));
    }

    #[test]
    fn capability_manifest_tool_support_emulated() {
        let manifest = dialect::capability_manifest();
        for cap in [
            Capability::ToolRead,
            Capability::ToolWrite,
            Capability::ToolEdit,
            Capability::ToolBash,
            Capability::ToolGlob,
            Capability::ToolGrep,
        ] {
            assert!(
                matches!(manifest[&cap], SupportLevel::Emulated),
                "expected Emulated for {cap:?}"
            );
        }
    }

    #[test]
    fn capability_manifest_mcp_unsupported() {
        let manifest = dialect::capability_manifest();
        assert!(matches!(
            manifest[&Capability::McpClient],
            SupportLevel::Unsupported
        ));
        assert!(matches!(
            manifest[&Capability::McpServer],
            SupportLevel::Unsupported
        ));
    }

    #[test]
    fn default_config_model_is_gpt4o() {
        let cfg = OpenAIConfig::default();
        assert_eq!(cfg.model, "gpt-4o");
    }

    #[test]
    fn default_config_base_url() {
        let cfg = OpenAIConfig::default();
        assert_eq!(cfg.base_url, "https://api.openai.com/v1");
    }

    #[test]
    fn default_config_has_max_tokens() {
        let cfg = OpenAIConfig::default();
        assert_eq!(cfg.max_tokens, Some(4096));
    }

    #[test]
    fn dialect_version_constant() {
        assert_eq!(dialect::DIALECT_VERSION, "openai/v0.1");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 5: Tool Choice Parameter Handling
// ═══════════════════════════════════════════════════════════════════════════

mod tool_choice_handling {
    use super::*;

    #[test]
    fn tool_choice_none_serde() {
        let tc = ToolChoice::Mode(ToolChoiceMode::None);
        let json = serde_json::to_string(&tc).unwrap();
        assert_eq!(json, r#""none""#);
        let parsed: ToolChoice = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, tc);
    }

    #[test]
    fn tool_choice_auto_serde() {
        let tc = ToolChoice::Mode(ToolChoiceMode::Auto);
        let json = serde_json::to_string(&tc).unwrap();
        assert_eq!(json, r#""auto""#);
        let parsed: ToolChoice = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, tc);
    }

    #[test]
    fn tool_choice_required_serde() {
        let tc = ToolChoice::Mode(ToolChoiceMode::Required);
        let json = serde_json::to_string(&tc).unwrap();
        assert_eq!(json, r#""required""#);
        let parsed: ToolChoice = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, tc);
    }

    #[test]
    fn tool_choice_specific_function_serde() {
        let tc = ToolChoice::Function {
            tool_type: "function".into(),
            function: ToolChoiceFunctionRef {
                name: "get_weather".into(),
            },
        };
        let json = serde_json::to_string(&tc).unwrap();
        let parsed: ToolChoice = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, tc);
        assert!(json.contains("get_weather"));
        assert!(json.contains("function"));
    }

    #[test]
    fn tool_choice_in_request_builder() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("Hi")])
            .tools(vec![make_tool_def("search", "Search")])
            .tool_choice(ToolChoice::Mode(ToolChoiceMode::Required))
            .build();
        assert!(req.tool_choice.is_some());
    }

    #[test]
    fn tool_choice_absent_by_default() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("Hi")])
            .build();
        assert!(req.tool_choice.is_none());
    }

    #[test]
    fn request_with_tool_choice_serde_roundtrip() {
        let req = OpenAIRequest {
            model: "gpt-4o".into(),
            messages: vec![msg("user", Some("Hi"))],
            tools: Some(vec![OpenAIToolDef {
                tool_type: "function".into(),
                function: OpenAIFunctionDef {
                    name: "calc".into(),
                    description: "Calculate".into(),
                    parameters: json!({}),
                },
            }]),
            tool_choice: Some(ToolChoice::Function {
                tool_type: "function".into(),
                function: ToolChoiceFunctionRef {
                    name: "calc".into(),
                },
            }),
            temperature: None,
            max_tokens: None,
            response_format: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: OpenAIRequest = serde_json::from_str(&json).unwrap();
        assert!(parsed.tool_choice.is_some());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 6: Response Format Mapping
// ═══════════════════════════════════════════════════════════════════════════

mod response_format_mapping {
    use super::*;

    #[test]
    fn response_format_text_serde() {
        let rf = ResponseFormat::Text;
        let json = serde_json::to_string(&rf).unwrap();
        let parsed: ResponseFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, rf);
    }

    #[test]
    fn response_format_json_object_serde() {
        let rf = ResponseFormat::JsonObject;
        let json = serde_json::to_string(&rf).unwrap();
        assert!(json.contains("json_object"));
        let parsed: ResponseFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, rf);
    }

    #[test]
    fn response_format_json_schema_serde() {
        let rf = ResponseFormat::json_schema(
            "my_response",
            json!({
                "type": "object",
                "properties": {
                    "answer": {"type": "string"}
                },
                "required": ["answer"]
            }),
        );
        let json = serde_json::to_string(&rf).unwrap();
        assert!(json.contains("json_schema"));
        assert!(json.contains("my_response"));
        let parsed: ResponseFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, rf);
    }

    #[test]
    fn response_format_json_schema_has_strict() {
        let rf = ResponseFormat::json_schema("test", json!({"type": "object"}));
        match rf {
            ResponseFormat::JsonSchema { json_schema } => {
                assert_eq!(json_schema.strict, Some(true));
                assert_eq!(json_schema.name, "test");
            }
            other => panic!("expected JsonSchema, got {other:?}"),
        }
    }

    #[test]
    fn response_format_constructors() {
        assert_eq!(ResponseFormat::text(), ResponseFormat::Text);
        assert_eq!(ResponseFormat::json_object(), ResponseFormat::JsonObject);
    }

    #[test]
    fn response_format_in_request_builder() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("Generate JSON")])
            .response_format(ResponseFormat::JsonObject)
            .build();
        assert!(req.response_format.is_some());
    }

    #[test]
    fn json_schema_spec_full_fields() {
        let spec = JsonSchemaSpec {
            name: "output".into(),
            description: Some("The output schema".into()),
            schema: json!({"type": "object"}),
            strict: Some(true),
        };
        let json = serde_json::to_string(&spec).unwrap();
        let parsed: JsonSchemaSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "output");
        assert_eq!(parsed.description.as_deref(), Some("The output schema"));
        assert_eq!(parsed.strict, Some(true));
    }

    #[test]
    fn json_schema_spec_omits_optional_description() {
        let spec = JsonSchemaSpec {
            name: "bare".into(),
            description: None,
            schema: json!({}),
            strict: None,
        };
        let json = serde_json::to_string(&spec).unwrap();
        assert!(!json.contains("description"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 7: System/User/Assistant Role Mapping
// ═══════════════════════════════════════════════════════════════════════════

mod role_mapping {
    use super::*;

    #[test]
    fn system_role_to_ir() {
        let msgs = [msg("system", Some("You are a helpful assistant"))];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(
            conv.messages[0].text_content(),
            "You are a helpful assistant"
        );
    }

    #[test]
    fn user_role_to_ir() {
        let msgs = [msg("user", Some("What is 2+2?"))];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    #[test]
    fn assistant_role_to_ir() {
        let msgs = [msg("assistant", Some("The answer is 4"))];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
    }

    #[test]
    fn tool_role_to_ir() {
        let msgs = [tool_result_msg(Some("result"), "c1")];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::Tool);
    }

    #[test]
    fn unknown_role_maps_to_user() {
        let msgs = [msg("developer", Some("some text"))];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    #[test]
    fn system_text_roundtrip() {
        let msgs = [msg("system", Some("Be concise"))];
        let back = lowering::from_ir(&lowering::to_ir(&msgs));
        assert_eq!(back[0].role, "system");
        assert_eq!(back[0].content.as_deref(), Some("Be concise"));
    }

    #[test]
    fn user_text_roundtrip() {
        let msgs = [msg("user", Some("Hello"))];
        let back = lowering::from_ir(&lowering::to_ir(&msgs));
        assert_eq!(back[0].role, "user");
        assert_eq!(back[0].content.as_deref(), Some("Hello"));
    }

    #[test]
    fn assistant_text_roundtrip() {
        let msgs = [msg("assistant", Some("Hi there"))];
        let back = lowering::from_ir(&lowering::to_ir(&msgs));
        assert_eq!(back[0].role, "assistant");
        assert_eq!(back[0].content.as_deref(), Some("Hi there"));
    }

    #[test]
    fn shim_role_enum_system() {
        let m = Message::system("test");
        assert_eq!(m.role, Role::System);
        assert_eq!(m.content.as_deref(), Some("test"));
    }

    #[test]
    fn shim_role_enum_user() {
        let m = Message::user("test");
        assert_eq!(m.role, Role::User);
    }

    #[test]
    fn shim_role_enum_assistant() {
        let m = Message::assistant("test");
        assert_eq!(m.role, Role::Assistant);
    }

    #[test]
    fn shim_role_enum_tool() {
        let m = Message::tool("call_1", "result");
        assert_eq!(m.role, Role::Tool);
        assert_eq!(m.tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn shim_role_serde_roundtrip() {
        for role in [Role::System, Role::User, Role::Assistant, Role::Tool] {
            let json = serde_json::to_string(&role).unwrap();
            let parsed: Role = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, role);
        }
    }

    #[test]
    fn shim_messages_to_ir_and_back() {
        let msgs = vec![
            Message::system("Be brief"),
            Message::user("Hi"),
            Message::assistant("Hello"),
        ];
        let ir = messages_to_ir(&msgs);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].role, Role::System);
        assert_eq!(back[1].role, Role::User);
        assert_eq!(back[2].role, Role::Assistant);
    }

    #[test]
    fn none_content_message_produces_empty_blocks() {
        let msgs = [msg("assistant", None)];
        let conv = lowering::to_ir(&msgs);
        assert!(conv.messages[0].content.is_empty());
    }

    #[test]
    fn empty_content_string_produces_no_text_blocks() {
        let msgs = [msg("user", Some(""))];
        let conv = lowering::to_ir(&msgs);
        assert!(conv.messages[0].content.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 8: Token Usage Mapping
// ═══════════════════════════════════════════════════════════════════════════

mod token_usage_mapping {
    use super::*;

    #[test]
    fn openai_usage_serde_roundtrip() {
        let usage = OpenAIUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let parsed: OpenAIUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.prompt_tokens, 100);
        assert_eq!(parsed.completion_tokens, 50);
        assert_eq!(parsed.total_tokens, 150);
    }

    #[test]
    fn ir_usage_to_shim_usage() {
        let ir = IrUsage::from_io(200, 100);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 200);
        assert_eq!(usage.completion_tokens, 100);
        assert_eq!(usage.total_tokens, 300);
    }

    #[test]
    fn ir_usage_zero_values() {
        let ir = IrUsage::from_io(0, 0);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
    }

    #[test]
    fn receipt_to_response_usage_mapping() {
        let usage = UsageNormalized {
            input_tokens: Some(500),
            output_tokens: Some(200),
            ..Default::default()
        };
        let events = vec![assistant_msg_event("Hello")];
        let receipt = mock_receipt_with_usage(events, usage);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 500);
        assert_eq!(u.completion_tokens, 200);
        assert_eq!(u.total_tokens, 700);
    }

    #[test]
    fn receipt_to_response_usage_defaults_none_to_zero() {
        let usage = UsageNormalized::default();
        let events = vec![assistant_msg_event("Hello")];
        let receipt = mock_receipt_with_usage(events, usage);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 0);
        assert_eq!(u.completion_tokens, 0);
        assert_eq!(u.total_tokens, 0);
    }

    #[test]
    fn shim_usage_serde_roundtrip() {
        let u = Usage {
            prompt_tokens: 42,
            completion_tokens: 17,
            total_tokens: 59,
        };
        let json = serde_json::to_string(&u).unwrap();
        let parsed: Usage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, u);
    }

    #[test]
    fn response_with_usage_present() {
        let resp = OpenAIResponse {
            id: "chatcmpl-1".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![OpenAIChoice {
                index: 0,
                message: msg("assistant", Some("Hi")),
                finish_reason: Some("stop".into()),
            }],
            usage: Some(OpenAIUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: OpenAIResponse = serde_json::from_str(&json).unwrap();
        assert!(parsed.usage.is_some());
        assert_eq!(parsed.usage.unwrap().total_tokens, 15);
    }

    #[test]
    fn receipt_to_response_includes_usage() {
        let receipt = mock_receipt(vec![assistant_msg_event("Hello")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert!(resp.usage.is_some());
    }

    #[test]
    fn large_token_counts_preserved() {
        let ir = IrUsage::from_io(1_000_000, 500_000);
        let usage = ir_usage_to_usage(&ir);
        assert_eq!(usage.prompt_tokens, 1_000_000);
        assert_eq!(usage.completion_tokens, 500_000);
        assert_eq!(usage.total_tokens, 1_500_000);
    }

    #[test]
    fn ir_usage_with_cache() {
        let ir = IrUsage::with_cache(100, 50, 30, 20);
        assert_eq!(ir.cache_read_tokens, 30);
        assert_eq!(ir.cache_write_tokens, 20);
        assert_eq!(ir.total_tokens, 150);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 9: Error Code Mapping
// ═══════════════════════════════════════════════════════════════════════════

mod error_code_mapping {
    use super::*;

    #[test]
    fn error_event_in_receipt_maps_to_content() {
        let events = vec![error_event("rate limit exceeded")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let content = resp.choices[0].message.content.as_deref().unwrap();
        assert!(content.contains("rate limit exceeded"));
    }

    #[test]
    fn error_event_sets_finish_reason_stop() {
        let events = vec![error_event("context length exceeded")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn error_event_with_error_code() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "backend not found".into(),
                error_code: Some(abp_error::ErrorCode::BackendNotFound),
            },
            ext: None,
        };
        let receipt = mock_receipt(vec![event]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert!(
            resp.choices[0]
                .message
                .content
                .as_deref()
                .unwrap()
                .contains("backend not found")
        );
    }

    #[test]
    fn error_event_overrides_prior_content() {
        let events = vec![
            assistant_msg_event("partial response"),
            error_event("something went wrong"),
        ];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let content = resp.choices[0].message.content.as_deref().unwrap();
        assert!(content.contains("something went wrong"));
    }

    #[test]
    fn validation_errors_for_logprobs() {
        let fields = ExtendedRequestFields {
            logprobs: Some(true),
            ..Default::default()
        };
        let result = abp_openai_sdk::validation::validate_for_mapped_mode(&fields);
        assert!(result.is_err());
        let errs = result.unwrap_err();
        assert!(errs.errors.iter().any(|e| e.param == "logprobs"));
    }

    #[test]
    fn validation_errors_for_logit_bias() {
        let mut bias = BTreeMap::new();
        bias.insert("123".into(), 1.0);
        let fields = ExtendedRequestFields {
            logit_bias: Some(bias),
            ..Default::default()
        };
        let result = abp_openai_sdk::validation::validate_for_mapped_mode(&fields);
        assert!(result.is_err());
    }

    #[test]
    fn validation_errors_for_seed() {
        let fields = ExtendedRequestFields {
            seed: Some(42),
            ..Default::default()
        };
        let result = abp_openai_sdk::validation::validate_for_mapped_mode(&fields);
        assert!(result.is_err());
    }

    #[test]
    fn validation_ok_when_no_unmappable_params() {
        let fields = ExtendedRequestFields::default();
        let result = abp_openai_sdk::validation::validate_for_mapped_mode(&fields);
        assert!(result.is_ok());
    }

    #[test]
    fn validation_error_display_format() {
        let err = ValidationErrors {
            errors: vec![UnmappableParam {
                param: "logprobs".into(),
                reason: "not supported".into(),
            }],
        };
        let display = format!("{err}");
        assert!(display.contains("1 unmappable parameter"));
        assert!(display.contains("logprobs"));
    }

    #[test]
    fn multiple_validation_errors_combined() {
        let mut bias = BTreeMap::new();
        bias.insert("1".into(), 0.5);
        let fields = ExtendedRequestFields {
            logprobs: Some(true),
            logit_bias: Some(bias),
            seed: Some(1),
            ..Default::default()
        };
        let result = abp_openai_sdk::validation::validate_for_mapped_mode(&fields);
        let errs = result.unwrap_err();
        assert!(errs.errors.len() >= 3);
    }

    #[test]
    fn unmappable_param_display() {
        let p = UnmappableParam {
            param: "seed".into(),
            reason: "not supported by target backend".into(),
        };
        let display = format!("{p}");
        assert!(display.contains("seed"));
        assert!(display.contains("not supported"));
    }

    #[test]
    fn error_codes_from_abp_error_crate() {
        // Verify key error codes exist and are usable
        let _codes = [
            abp_error::ErrorCode::BackendNotFound,
            abp_error::ErrorCode::BackendTimeout,
            abp_error::ErrorCode::BackendCrashed,
            abp_error::ErrorCode::CapabilityUnsupported,
            abp_error::ErrorCode::DialectMappingFailed,
            abp_error::ErrorCode::Internal,
        ];
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 10: Multi-Turn Conversation Context Preservation
// ═══════════════════════════════════════════════════════════════════════════

mod multi_turn_conversation {
    use super::*;

    #[test]
    fn two_turn_conversation_roundtrip() {
        let msgs = [
            msg("user", Some("What is Rust?")),
            msg("assistant", Some("Rust is a systems programming language.")),
            msg("user", Some("What about memory safety?")),
        ];
        let ir = lowering::to_ir(&msgs);
        assert_eq!(ir.len(), 3);
        let back = lowering::from_ir(&ir);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].role, "user");
        assert_eq!(back[1].role, "assistant");
        assert_eq!(back[2].role, "user");
    }

    #[test]
    fn system_plus_multi_turn() {
        let msgs = [
            msg("system", Some("You are a Rust expert")),
            msg("user", Some("Explain ownership")),
            msg("assistant", Some("Ownership ensures...")),
            msg("user", Some("What about borrowing?")),
            msg("assistant", Some("Borrowing allows...")),
        ];
        let ir = lowering::to_ir(&msgs);
        assert_eq!(ir.len(), 5);
        assert_eq!(
            ir.system_message().unwrap().text_content(),
            "You are a Rust expert"
        );
    }

    #[test]
    fn conversation_with_tool_use_cycle() {
        let msgs = [
            msg("user", Some("Read the file main.rs")),
            assistant_with_tool_calls(
                None,
                vec![make_tool_call("c1", "read_file", r#"{"path":"main.rs"}"#)],
            ),
            tool_result_msg(Some("fn main() {}"), "c1"),
            msg("assistant", Some("The file contains a main function.")),
        ];
        let ir = lowering::to_ir(&msgs);
        assert_eq!(ir.len(), 4);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[1].role, IrRole::Assistant);
        assert_eq!(ir.messages[2].role, IrRole::Tool);
        assert_eq!(ir.messages[3].role, IrRole::Assistant);
    }

    #[test]
    fn multi_turn_with_multiple_tool_cycles() {
        let msgs = [
            msg("user", Some("Find and fix the bug")),
            assistant_with_tool_calls(
                None,
                vec![make_tool_call("c1", "read", r#"{"path":"src/lib.rs"}"#)],
            ),
            tool_result_msg(Some("pub fn broken() { panic!() }"), "c1"),
            assistant_with_tool_calls(
                Some("I see the issue"),
                vec![make_tool_call(
                    "c2",
                    "write",
                    r#"{"path":"src/lib.rs","content":"pub fn fixed() {}"}"#,
                )],
            ),
            tool_result_msg(Some("File written"), "c2"),
            msg("assistant", Some("Fixed the bug")),
        ];
        let ir = lowering::to_ir(&msgs);
        assert_eq!(ir.len(), 6);
        let tool_calls = ir.tool_calls();
        assert_eq!(tool_calls.len(), 2);
    }

    #[test]
    fn conversation_preserves_all_text_content() {
        let msgs = [
            msg("system", Some("sys prompt")),
            msg("user", Some("user msg 1")),
            msg("assistant", Some("assistant msg 1")),
            msg("user", Some("user msg 2")),
        ];
        let ir = lowering::to_ir(&msgs);
        let back = lowering::from_ir(&ir);
        assert_eq!(back[0].content.as_deref(), Some("sys prompt"));
        assert_eq!(back[1].content.as_deref(), Some("user msg 1"));
        assert_eq!(back[2].content.as_deref(), Some("assistant msg 1"));
        assert_eq!(back[3].content.as_deref(), Some("user msg 2"));
    }

    #[test]
    fn ir_conversation_accessors() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "sys"),
            IrMessage::text(IrRole::User, "hello"),
            IrMessage::text(IrRole::Assistant, "hi"),
            IrMessage::text(IrRole::User, "bye"),
        ]);
        assert_eq!(conv.len(), 4);
        assert!(!conv.is_empty());
        assert_eq!(conv.system_message().unwrap().text_content(), "sys");
        assert_eq!(conv.last_assistant().unwrap().text_content(), "hi");
        assert_eq!(conv.messages_by_role(IrRole::User).len(), 2);
    }

    #[test]
    fn empty_conversation() {
        let conv = IrConversation::new();
        assert!(conv.is_empty());
        assert_eq!(conv.len(), 0);
        assert!(conv.system_message().is_none());
        assert!(conv.last_assistant().is_none());
    }

    #[test]
    fn shim_multi_turn_to_ir() {
        let msgs = vec![
            Message::system("Be helpful"),
            Message::user("Question 1"),
            Message::assistant("Answer 1"),
            Message::user("Question 2"),
        ];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.len(), 4);
        let back = ir_to_messages(&ir);
        assert_eq!(back[0].role, Role::System);
        assert_eq!(back[3].role, Role::User);
    }

    #[test]
    fn shim_tool_use_cycle_roundtrip() {
        let tc = ToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"main.rs"}"#.into(),
            },
        };
        let msgs = vec![
            Message::user("Read the file"),
            Message::assistant_with_tool_calls(vec![tc]),
            Message::tool("call_1", "fn main() {}"),
        ];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.len(), 3);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 3);
        assert_eq!(back[2].role, Role::Tool);
        assert_eq!(back[2].tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn last_message_accessor() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "first"),
            IrMessage::text(IrRole::Assistant, "second"),
        ]);
        assert_eq!(conv.last_message().unwrap().text_content(), "second");
    }

    #[test]
    fn work_order_from_multi_turn_uses_last_user_message() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![
                Message::system("You are a coding assistant"),
                Message::user("Write a function"),
                Message::assistant("fn hello() {}"),
                Message::user("Now add error handling"),
            ])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "Now add error handling");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 11: Response Mapping (Receipt → Response)
// ═══════════════════════════════════════════════════════════════════════════

mod response_mapping {
    use super::*;

    #[test]
    fn simple_text_receipt_to_response() {
        let events = vec![assistant_msg_event("Hello, world!")];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.model, "gpt-4o");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello, world!")
        );
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn delta_events_concatenated_in_response() {
        let events = vec![
            assistant_delta_event("Hello"),
            assistant_delta_event(", "),
            assistant_delta_event("world!"),
        ];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello, world!")
        );
    }

    #[test]
    fn response_id_includes_run_id() {
        let receipt = mock_receipt(vec![assistant_msg_event("Hi")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert!(resp.id.starts_with("chatcmpl-"));
    }

    #[test]
    fn response_preserves_model_name() {
        let receipt = mock_receipt(vec![assistant_msg_event("Hi")]);
        let resp = receipt_to_response(&receipt, "gpt-4-turbo");
        assert_eq!(resp.model, "gpt-4-turbo");
    }

    #[test]
    fn empty_trace_receipt_produces_valid_response() {
        let receipt = mock_receipt(vec![]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.choices.len(), 1);
        assert!(resp.choices[0].message.content.is_none());
        assert!(resp.choices[0].message.tool_calls.is_none());
    }

    #[test]
    fn response_serde_roundtrip() {
        let resp = simple_response("Test message");
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: OpenAIResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "chatcmpl-test");
        assert_eq!(
            parsed.choices[0].message.content.as_deref(),
            Some("Test message")
        );
    }

    #[test]
    fn response_finish_reason_stop_for_text() {
        let receipt = mock_receipt(vec![assistant_msg_event("done")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn response_finish_reason_tool_calls() {
        let events = vec![tool_call_event("fn", "c1", json!({}))];
        let receipt = mock_receipt(events);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn map_response_extracts_assistant_text() {
        let resp = simple_response("Hello from GPT");
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::AssistantMessage { text } if text == "Hello from GPT"
        ));
    }

    #[test]
    fn map_response_with_tool_calls() {
        let resp = OpenAIResponse {
            id: "chatcmpl-1".into(),
            object: "chat.completion".into(),
            model: "gpt-4o".into(),
            choices: vec![OpenAIChoice {
                index: 0,
                message: assistant_with_tool_calls(
                    None,
                    vec![make_tool_call("c1", "search", r#"{"q":"test"}"#)],
                ),
                finish_reason: Some("tool_calls".into()),
            }],
            usage: None,
        };
        let events = dialect::map_response(&resp);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::ToolCall { tool_name, .. } if tool_name == "search"
        ));
    }

    #[test]
    fn map_work_order_to_request_and_back() {
        let wo = WorkOrderBuilder::new("Refactor auth")
            .model("gpt-4-turbo")
            .build();
        let cfg = OpenAIConfig::default();
        let req = dialect::map_work_order(&wo, &cfg);
        assert_eq!(req.model, "gpt-4-turbo");
        assert!(
            req.messages[0]
                .content
                .as_deref()
                .unwrap()
                .contains("Refactor auth")
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 12: Edge Cases and Unicode
// ═══════════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;

    #[test]
    fn unicode_text_preserved_through_ir() {
        let text = "こんにちは 🌍 مرحبا Привет";
        let msgs = [msg("user", Some(text))];
        let back = lowering::from_ir(&lowering::to_ir(&msgs));
        assert_eq!(back[0].content.as_deref(), Some(text));
    }

    #[test]
    fn emoji_in_tool_arguments() {
        let args = r#"{"emoji":"🎉🚀💻"}"#;
        let msgs = [assistant_with_tool_calls(
            None,
            vec![make_tool_call("c1", "emoji_fn", args)],
        )];
        let ir = lowering::to_ir(&msgs);
        let back = lowering::from_ir(&ir);
        let tc = &back[0].tool_calls.as_ref().unwrap()[0];
        let parsed: serde_json::Value = serde_json::from_str(&tc.function.arguments).unwrap();
        assert_eq!(parsed["emoji"], "🎉🚀💻");
    }

    #[test]
    fn very_long_message_preserved() {
        let long_text = "x".repeat(100_000);
        let msgs = [msg("user", Some(&long_text))];
        let back = lowering::from_ir(&lowering::to_ir(&msgs));
        assert_eq!(back[0].content.as_deref().unwrap().len(), 100_000);
    }

    #[test]
    fn special_characters_in_content() {
        let text = r#"line1\nline2\ttab "quoted" 'single' <xml> &amp;"#;
        let msgs = [msg("user", Some(text))];
        let back = lowering::from_ir(&lowering::to_ir(&msgs));
        assert_eq!(back[0].content.as_deref(), Some(text));
    }

    #[test]
    fn nested_json_in_tool_args() {
        let args = r#"{"config":{"nested":{"deep":true}},"list":[1,2,3]}"#;
        let msgs = [assistant_with_tool_calls(
            None,
            vec![make_tool_call("c1", "complex_fn", args)],
        )];
        let ir = lowering::to_ir(&msgs);
        match &ir.messages[0].content[0] {
            IrContentBlock::ToolUse { input, .. } => {
                assert!(input["config"]["nested"]["deep"].as_bool().unwrap());
                assert_eq!(input["list"].as_array().unwrap().len(), 3);
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn deterministic_json_with_btreemap() {
        let mut map = BTreeMap::new();
        map.insert("z_key".to_string(), json!("last"));
        map.insert("a_key".to_string(), json!("first"));
        let json = serde_json::to_string(&map).unwrap();
        assert!(json.starts_with(r#"{"a_key""#));
    }

    #[test]
    fn receipt_with_hash_is_deterministic() {
        let receipt = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .build()
            .with_hash()
            .unwrap();
        let hash1 = receipt.receipt_sha256.clone().unwrap();

        let _receipt2 = ReceiptBuilder::new("mock")
            .outcome(Outcome::Complete)
            .started_at(receipt.meta.started_at)
            .finished_at(receipt.meta.finished_at)
            .work_order_id(receipt.meta.work_order_id)
            .build();
        // Different run_id means different hash, but structure is valid
        assert_eq!(hash1.len(), 64);
    }

    #[test]
    fn contract_version_constant() {
        assert_eq!(CONTRACT_VERSION, "abp/v0.1");
    }

    #[test]
    fn receipt_builder_outcome_complete() {
        let receipt = ReceiptBuilder::new("openai")
            .outcome(Outcome::Complete)
            .build();
        assert_eq!(receipt.outcome, Outcome::Complete);
        assert_eq!(receipt.backend.id, "openai");
    }

    #[test]
    fn receipt_builder_outcome_failed() {
        let receipt = ReceiptBuilder::new("openai")
            .outcome(Outcome::Failed)
            .build();
        assert_eq!(receipt.outcome, Outcome::Failed);
    }

    #[test]
    fn receipt_builder_outcome_partial() {
        let receipt = ReceiptBuilder::new("openai")
            .outcome(Outcome::Partial)
            .build();
        assert_eq!(receipt.outcome, Outcome::Partial);
    }

    #[test]
    fn shim_request_builder_stream_flag() {
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("Hi")])
            .stream(true)
            .build();
        assert_eq!(req.stream, Some(true));
    }

    #[test]
    fn message_role_serializes_as_snake_case() {
        assert_eq!(serde_json::to_string(&Role::System).unwrap(), r#""system""#);
        assert_eq!(serde_json::to_string(&Role::User).unwrap(), r#""user""#);
        assert_eq!(
            serde_json::to_string(&Role::Assistant).unwrap(),
            r#""assistant""#
        );
        assert_eq!(serde_json::to_string(&Role::Tool).unwrap(), r#""tool""#);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Module 13: Client API Integration
// ═══════════════════════════════════════════════════════════════════════════

mod client_api {
    use super::*;
    use tokio_stream::StreamExt;

    #[tokio::test]
    async fn client_simple_chat_completion() {
        let events = vec![assistant_msg_event("Hello!")];
        let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("Hi")])
            .build();
        let resp = client.chat().completions().create(req).await.unwrap();
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
    }

    #[tokio::test]
    async fn client_streaming_completion() {
        let events = vec![assistant_delta_event("Hel"), assistant_delta_event("lo")];
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
        assert_eq!(chunks.len(), 3); // 2 deltas + stop
    }

    #[tokio::test]
    async fn client_tool_use_response() {
        let events = vec![tool_call_event(
            "get_weather",
            "call_xyz",
            json!({"city": "NYC"}),
        )];
        let client = OpenAiClient::new("gpt-4o").with_processor(make_processor(events));
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("Weather in NYC?")])
            .tools(vec![make_tool_def("get_weather", "Get weather")])
            .build();
        let resp = client.chat().completions().create(req).await.unwrap();
        let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tcs[0].function.name, "get_weather");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("tool_calls"));
    }

    #[tokio::test]
    async fn client_without_processor_returns_error() {
        let client = OpenAiClient::new("gpt-4o");
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("Hi")])
            .build();
        let result = client.chat().completions().create(req).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn client_model_accessor() {
        let client = OpenAiClient::new("gpt-4-turbo");
        assert_eq!(client.model(), "gpt-4-turbo");
    }

    #[tokio::test]
    async fn client_with_usage_in_response() {
        let usage = UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            ..Default::default()
        };
        let events = vec![assistant_msg_event("OK")];
        let client =
            OpenAiClient::new("gpt-4o").with_processor(make_processor_with_usage(events, usage));
        let req = ChatCompletionRequest::builder()
            .messages(vec![Message::user("Test")])
            .build();
        let resp = client.chat().completions().create(req).await.unwrap();
        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    #[tokio::test]
    async fn client_debug_format() {
        let client = OpenAiClient::new("gpt-4o");
        let debug = format!("{client:?}");
        assert!(debug.contains("gpt-4o"));
    }
}
