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
//! Comprehensive shim roundtrip end-to-end tests.
//!
//! Validates every SDK shim crate through:
//!  1. Message construction roundtrips (native → IR → native)
//!  2. Tool definition roundtrips
//!  3. Streaming event mapping
//!  4. Error handling per shim
//!  5. Configuration / model name mapping
//!
//! Target: 100+ tests across all 6 shims.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{AgentEvent, AgentEventKind, Outcome, ReceiptBuilder, UsageNormalized};
use chrono::Utc;
use serde_json::json;

// ── Shared helpers ─────────────────────────────────────────────────────

fn evt_run_started() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "started".into(),
        },
        ext: None,
    }
}

fn evt_msg(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage { text: text.into() },
        ext: None,
    }
}

fn evt_delta(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta { text: text.into() },
        ext: None,
    }
}

fn evt_tool_call(name: &str, id: &str, args: serde_json::Value) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: name.into(),
            tool_use_id: Some(id.into()),
            parent_tool_use_id: None,
            input: args,
        },
        ext: None,
    }
}

fn evt_tool_result(name: &str, id: &str, output: serde_json::Value) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolResult {
            tool_name: name.into(),
            tool_use_id: Some(id.into()),
            output,
            is_error: false,
        },
        ext: None,
    }
}

fn evt_error(msg: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: msg.into(),
            error_code: Some(abp_error::ErrorCode::Internal),
        },
        ext: None,
    }
}

fn evt_warning(msg: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: msg.into(),
        },
        ext: None,
    }
}

fn evt_run_completed() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    }
}

fn evt_file_changed(path: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::FileChanged {
            path: path.into(),
            summary: "modified".into(),
        },
        ext: None,
    }
}

fn evt_cmd(cmd: &str, code: i32) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::CommandExecuted {
            command: cmd.into(),
            exit_code: Some(code),
            output_preview: Some("ok".into()),
        },
        ext: None,
    }
}

fn test_usage() -> UsageNormalized {
    UsageNormalized {
        input_tokens: Some(100),
        output_tokens: Some(50),
        ..Default::default()
    }
}

fn tool_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "File path" }
        },
        "required": ["path"]
    })
}

fn build_receipt(events: Vec<AgentEvent>) -> abp_core::Receipt {
    ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .usage(test_usage())
        .add_trace_event(evt_run_started())
        .add_trace_event(
            events
                .into_iter()
                .next()
                .unwrap_or_else(|| evt_msg("hello")),
        )
        .add_trace_event(evt_run_completed())
        .build()
}

fn build_receipt_multi(events: Vec<AgentEvent>) -> abp_core::Receipt {
    let mut b = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .usage(test_usage());
    for e in events {
        b = b.add_trace_event(e);
    }
    b.build()
}

// ═══════════════════════════════════════════════════════════════════════
// 1. OpenAI shim roundtrips
// ═══════════════════════════════════════════════════════════════════════

mod openai_roundtrip {
    use super::*;
    use abp_shim_openai::*;

    // ── Message construction roundtrips ──

    #[test]
    fn user_message_to_ir_and_back() {
        let msgs = vec![Message::user("Hello world")];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.len(), 1);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[0].text_content(), "Hello world");
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].content.as_deref(), Some("Hello world"));
    }

    #[test]
    fn system_message_roundtrip() {
        let msgs = vec![Message::system("Be helpful"), Message::user("Hi")];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.len(), 2);
        assert_eq!(ir.messages[0].role, IrRole::System);
        assert_eq!(ir.messages[0].text_content(), "Be helpful");
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 2);
    }

    #[test]
    fn assistant_message_roundtrip() {
        let msgs = vec![Message::user("What is 2+2?"), Message::assistant("4")];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.len(), 2);
        assert_eq!(ir.messages[1].role, IrRole::Assistant);
        assert_eq!(ir.messages[1].text_content(), "4");
        let back = ir_to_messages(&ir);
        assert_eq!(back[1].content.as_deref(), Some("4"));
    }

    #[test]
    fn multi_turn_conversation_roundtrip() {
        let msgs = vec![
            Message::system("You are a math tutor"),
            Message::user("What is 2+2?"),
            Message::assistant("4"),
            Message::user("And 3+3?"),
        ];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.len(), 4);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 4);
        assert_eq!(back[0].content.as_deref(), Some("You are a math tutor"));
        assert_eq!(back[3].content.as_deref(), Some("And 3+3?"));
    }

    #[test]
    fn tool_message_roundtrip() {
        let msgs = vec![
            Message::user("Read file"),
            Message::tool("tool-1", "file contents here"),
        ];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.len(), 2);
        assert_eq!(ir.messages[1].role, IrRole::Tool);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 2);
    }

    #[test]
    fn empty_conversation_roundtrip() {
        let msgs: Vec<Message> = vec![];
        let ir = messages_to_ir(&msgs);
        assert!(ir.is_empty());
        let back = ir_to_messages(&ir);
        assert!(back.is_empty());
    }

    // ── Request/IR conversion ──

    #[test]
    fn request_to_ir_preserves_messages() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("test")])
            .build();
        let ir = request_to_ir(&req);
        assert_eq!(ir.len(), 1);
        assert_eq!(ir.messages[0].text_content(), "test");
    }

    #[test]
    fn request_to_work_order_captures_model() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o-mini")
            .messages(vec![Message::user("hello")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(wo.task, "hello");
    }

    // ── Tool definition roundtrips ──

    #[test]
    fn tool_def_to_ir_and_check() {
        let tool = Tool::function("read_file", "Read a file", tool_schema());
        let ir_tools = tools_to_ir(&[tool]);
        assert_eq!(ir_tools.len(), 1);
        assert_eq!(ir_tools[0].name, "read_file");
        assert_eq!(ir_tools[0].description, "Read a file");
        assert_eq!(ir_tools[0].parameters, tool_schema());
    }

    #[test]
    fn multiple_tools_to_ir() {
        let tools = vec![
            Tool::function("read_file", "Read a file", tool_schema()),
            Tool::function("write_file", "Write a file", tool_schema()),
            Tool::function("list_dir", "List directory", json!({})),
        ];
        let ir_tools = tools_to_ir(&tools);
        assert_eq!(ir_tools.len(), 3);
        assert_eq!(ir_tools[0].name, "read_file");
        assert_eq!(ir_tools[1].name, "write_file");
        assert_eq!(ir_tools[2].name, "list_dir");
    }

    #[test]
    fn tool_with_complex_schema() {
        let schema = json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "line_range": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "minItems": 2,
                    "maxItems": 2
                }
            },
            "required": ["path"]
        });
        let tool = Tool::function("view_file", "View file lines", schema.clone());
        let ir = tools_to_ir(&[tool]);
        assert_eq!(ir[0].parameters, schema);
    }

    // ── Streaming event mapping ──

    #[test]
    fn stream_events_from_deltas() {
        let events = vec![evt_delta("Hel"), evt_delta("lo!")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert!(stream.len() >= 3); // 2 deltas + stop
        let last = stream.last().unwrap();
        assert_eq!(last.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn stream_events_single_delta() {
        let events = vec![evt_delta("Complete response")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert!(!stream.is_empty());
    }

    #[test]
    fn stream_events_preserve_model() {
        let events = vec![evt_delta("hi")];
        let stream = events_to_stream_events(&events, "gpt-4-turbo");
        for se in &stream {
            assert_eq!(se.model, "gpt-4-turbo");
        }
    }

    #[test]
    fn stream_events_empty_input() {
        let events: Vec<AgentEvent> = vec![];
        let stream = events_to_stream_events(&events, "gpt-4o");
        // Should at least produce a stop event
        assert!(!stream.is_empty());
    }

    // ── Response construction ──

    #[test]
    fn receipt_to_response_basic() {
        let receipt = mock_receipt(vec![evt_msg("Hi there")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.model, "gpt-4o");
        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hi there"));
    }

    #[test]
    fn receipt_to_response_with_usage() {
        let receipt = mock_receipt_with_usage(vec![evt_msg("ok")], test_usage());
        let resp = receipt_to_response(&receipt, "gpt-4o");
        let u = resp.usage.unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    #[test]
    fn receipt_to_response_finish_reason() {
        let receipt = mock_receipt(vec![evt_msg("done")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    // ── Usage conversion ──

    #[test]
    fn ir_usage_to_openai_usage() {
        let ir = IrUsage::from_io(200, 100);
        let u = ir_usage_to_usage(&ir);
        assert_eq!(u.prompt_tokens, 200);
        assert_eq!(u.completion_tokens, 100);
        assert_eq!(u.total_tokens, 300);
    }

    // ── Error handling ──

    #[tokio::test]
    async fn client_without_processor_returns_error() {
        let client = OpenAiClient::new("gpt-4o");
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("test")])
            .build();
        let err = client.chat().completions().create(req).await.unwrap_err();
        assert!(matches!(err, ShimError::Internal(_)));
    }

    // ── Model name mapping ──

    #[test]
    fn model_name_gpt4o() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("x")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn model_name_gpt4_turbo() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4-turbo-2024-04-09")
            .messages(vec![Message::user("x")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo-2024-04-09"));
    }

    #[test]
    fn model_name_o1() {
        let req = ChatCompletionRequest::builder()
            .model("o1-preview")
            .messages(vec![Message::user("x")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("o1-preview"));
    }

    // ── Builder options ──

    #[test]
    fn builder_with_temperature() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("x")])
            .temperature(0.5)
            .build();
        assert_eq!(req.temperature, Some(0.5));
    }

    #[test]
    fn builder_with_max_tokens() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("x")])
            .max_tokens(2048)
            .build();
        assert_eq!(req.max_tokens, Some(2048));
    }

    #[test]
    fn builder_with_stream_flag() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("x")])
            .stream(true)
            .build();
        assert_eq!(req.stream, Some(true));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Claude shim roundtrips
// ═══════════════════════════════════════════════════════════════════════

mod claude_roundtrip {
    use super::*;
    use abp_shim_claude::*;

    fn make_request(model: &str, text: &str) -> MessageRequest {
        MessageRequest {
            model: model.into(),
            max_tokens: 4096,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text { text: text.into() }],
            }],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        }
    }

    // ── Message construction roundtrips ──

    #[test]
    fn text_content_block_to_ir_and_back() {
        let block = ContentBlock::Text {
            text: "Hello".into(),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert!(matches!(back, ContentBlock::Text { text } if text == "Hello"));
    }

    #[test]
    fn tool_use_content_block_roundtrip() {
        let block = ContentBlock::ToolUse {
            id: "tool-1".into(),
            name: "read_file".into(),
            input: json!({"path": "/tmp/test.txt"}),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert!(matches!(back, ContentBlock::ToolUse { name, .. } if name == "read_file"));
    }

    #[test]
    fn tool_result_content_block_roundtrip() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "tool-1".into(),
            content: Some("file contents".into()),
            is_error: Some(false),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert!(
            matches!(back, ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "tool-1")
        );
    }

    #[test]
    fn message_to_ir_preserves_role() {
        let msg = Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "Hello".into(),
            }],
        };
        let ir = message_to_ir(&msg);
        // ClaudeMessage has a role field
        assert_eq!(ir.role, "user");
    }

    #[test]
    fn multi_block_message_roundtrip() {
        let msg = Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: "Let me read that file.".into(),
                },
                ContentBlock::ToolUse {
                    id: "tc-1".into(),
                    name: "read_file".into(),
                    input: json!({"path": "test.rs"}),
                },
            ],
        };
        let ir = message_to_ir(&msg);
        assert_eq!(ir.role, "assistant");
        // Multi-block content is serialized to JSON string
        assert!(!ir.content.is_empty());
    }

    // ── Request conversion ──

    #[test]
    fn request_to_work_order_captures_model() {
        let req = make_request("claude-sonnet-4-20250514", "hello");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
        assert_eq!(wo.task, "hello");
    }

    #[test]
    fn request_to_claude_dialect() {
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 2048,
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "Explain Rust".into(),
                }],
            }],
            system: Some("Be concise".into()),
            temperature: Some(0.3),
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let dialect = request_to_claude(&req);
        assert_eq!(dialect.model, "claude-sonnet-4-20250514");
        assert_eq!(dialect.system.as_deref(), Some("Be concise"));
    }

    // ── Response construction ──

    #[test]
    fn response_from_events_basic() {
        let events = vec![evt_msg("Hi from Claude")];
        let resp = response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert_eq!(resp.model, "claude-sonnet-4-20250514");
        assert!(resp.content.iter().any(|b| matches!(
            b,
            ContentBlock::Text { text } if text == "Hi from Claude"
        )));
    }

    #[test]
    fn response_from_events_with_usage() {
        let usage = abp_claude_sdk::dialect::ClaudeUsage {
            input_tokens: 200,
            output_tokens: 80,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        };
        let events = vec![evt_msg("ok")];
        let resp = response_from_events(&events, "claude-sonnet-4-20250514", Some(&usage));
        assert_eq!(resp.usage.input_tokens, 200);
        assert_eq!(resp.usage.output_tokens, 80);
    }

    #[test]
    fn response_from_events_with_tool_call() {
        let events = vec![evt_tool_call("read_file", "tc-1", json!({"path": "x.rs"}))];
        let resp = response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert!(resp.content.iter().any(|b| matches!(
            b,
            ContentBlock::ToolUse { name, .. } if name == "read_file"
        )));
    }

    // ── Streaming ──

    #[tokio::test]
    async fn stream_produces_message_start_and_stop() {
        let client = AnthropicClient::new();
        let req = make_request("claude-sonnet-4-20250514", "Hello");
        let stream = client.create_stream(req).await.unwrap();
        let events = stream.collect_all().await;
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::MessageStart { .. }))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, StreamEvent::MessageStop {}))
        );
    }

    // ── Error handling ──

    #[tokio::test]
    async fn empty_messages_returns_error() {
        let client = AnthropicClient::new();
        let req = MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 4096,
            messages: vec![],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let err = client.create(req).await.unwrap_err();
        assert!(matches!(err, ShimError::InvalidRequest(_)));
    }

    // ── Model name mapping ──

    #[test]
    fn model_name_sonnet() {
        let req = make_request("claude-sonnet-4-20250514", "x");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
    }

    #[test]
    fn model_name_opus() {
        let req = make_request("claude-opus-4-20250514", "x");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("claude-opus-4-20250514"));
    }

    #[test]
    fn model_name_haiku() {
        let req = make_request("claude-3-haiku-20240307", "x");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("claude-3-haiku-20240307"));
    }

    // ── Tool definitions ──

    #[test]
    fn tool_def_roundtrip_via_sdk() {
        let canonical = abp_claude_sdk::dialect::CanonicalToolDef {
            name: "search".into(),
            description: "Search codebase".into(),
            parameters_schema: tool_schema(),
        };
        let claude_tool = abp_claude_sdk::dialect::tool_def_to_claude(&canonical);
        let back = abp_claude_sdk::dialect::tool_def_from_claude(&claude_tool);
        assert_eq!(back.name, "search");
        assert_eq!(back.description, "Search codebase");
        assert_eq!(back.parameters_schema, tool_schema());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Gemini shim roundtrips
// ═══════════════════════════════════════════════════════════════════════

mod gemini_roundtrip {
    use super::*;
    use abp_shim_gemini::*;

    fn make_request(model: &str, text: &str) -> GenerateContentRequest {
        GenerateContentRequest::new(model).add_content(Content::user(vec![Part::text(text)]))
    }

    // ── Message construction roundtrips ──

    #[test]
    fn text_part_construction() {
        let part = Part::text("Hello");
        assert!(matches!(part, Part::Text(ref s) if s == "Hello"));
    }

    #[test]
    fn function_call_part() {
        let part = Part::function_call("read_file", json!({"path": "x.rs"}));
        assert!(matches!(part, Part::FunctionCall { ref name, .. } if name == "read_file"));
    }

    #[test]
    fn function_response_part() {
        let part = Part::function_response("read_file", json!({"content": "data"}));
        assert!(matches!(part, Part::FunctionResponse { ref name, .. } if name == "read_file"));
    }

    #[test]
    fn content_user_construction() {
        let c = Content::user(vec![Part::text("test")]);
        assert_eq!(c.role, "user");
        assert_eq!(c.parts.len(), 1);
    }

    #[test]
    fn content_model_construction() {
        let c = Content::model(vec![Part::text("response")]);
        assert_eq!(c.role, "model");
    }

    // ── Request conversion ──

    #[test]
    fn request_to_dialect_preserves_model() {
        let req = make_request("gemini-2.5-flash", "hello");
        let dialect = to_dialect_request(&req);
        assert_eq!(dialect.model, "gemini-2.5-flash");
    }

    #[test]
    fn request_with_system_instruction() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .system_instruction(Content::user(vec![Part::text("Be helpful")]))
            .add_content(Content::user(vec![Part::text("Hi")]));
        let dialect = to_dialect_request(&req);
        assert!(dialect.system_instruction.is_some());
    }

    #[test]
    fn request_with_generation_config() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .add_content(Content::user(vec![Part::text("Hi")]))
            .generation_config(GenerationConfig {
                temperature: Some(0.9),
                max_output_tokens: Some(4096),
                ..Default::default()
            });
        assert_eq!(
            req.generation_config.as_ref().unwrap().temperature,
            Some(0.9)
        );
    }

    // ── Usage conversion ──

    #[test]
    fn usage_to_ir_and_back() {
        let meta = UsageMetadata {
            prompt_token_count: 150,
            candidates_token_count: 75,
            total_token_count: 225,
        };
        let ir = usage_to_ir(&meta);
        assert_eq!(ir.input_tokens, 150);
        assert_eq!(ir.output_tokens, 75);
        assert_eq!(ir.total_tokens, 225);
        let back = usage_from_ir(&ir);
        assert_eq!(back.prompt_token_count, 150);
        assert_eq!(back.candidates_token_count, 75);
        assert_eq!(back.total_token_count, 225);
    }

    #[test]
    fn usage_zero_values() {
        let meta = UsageMetadata {
            prompt_token_count: 0,
            candidates_token_count: 0,
            total_token_count: 0,
        };
        let ir = usage_to_ir(&meta);
        assert_eq!(ir.input_tokens, 0);
        let back = usage_from_ir(&ir);
        assert_eq!(back.total_token_count, 0);
    }

    // ── Tool definition roundtrip ──

    #[test]
    fn tool_def_roundtrip() {
        let canonical = abp_gemini_sdk::dialect::CanonicalToolDef {
            name: "execute_cmd".into(),
            description: "Execute a command".into(),
            parameters_schema: tool_schema(),
        };
        let gemini_decl = abp_gemini_sdk::dialect::tool_def_to_gemini(&canonical);
        let back = abp_gemini_sdk::dialect::tool_def_from_gemini(&gemini_decl);
        assert_eq!(back.name, "execute_cmd");
        assert_eq!(back.description, "Execute a command");
    }

    // ── Dialect response ──

    #[tokio::test]
    async fn client_generate_returns_response() {
        let client = PipelineClient::new("gemini-2.5-flash");
        let req = make_request("gemini-2.5-flash", "Hello");
        let resp = client.generate(req).await.unwrap();
        assert!(!resp.candidates.is_empty());
        assert!(resp.text().is_some());
    }

    #[tokio::test]
    async fn client_stream_returns_events() {
        let client = PipelineClient::new("gemini-2.5-flash");
        let req = make_request("gemini-2.5-flash", "Hello");
        let stream = client.generate_stream(req).await.unwrap();
        let events: Vec<_> = tokio_stream::StreamExt::collect(stream).await;
        assert!(!events.is_empty());
    }

    // ── Model name mapping ──

    #[test]
    fn model_name_flash() {
        let req = make_request("gemini-2.5-flash", "x");
        let d = to_dialect_request(&req);
        assert_eq!(d.model, "gemini-2.5-flash");
    }

    #[test]
    fn model_name_pro() {
        let req = make_request("gemini-2.5-pro-exp-03-25", "x");
        let d = to_dialect_request(&req);
        assert_eq!(d.model, "gemini-2.5-pro-exp-03-25");
    }

    // ── IR lowering ──

    #[test]
    fn ir_lowering_from_dialect() {
        let req = make_request("gemini-2.5-flash", "What is Rust?");
        let dialect = to_dialect_request(&req);
        let ir =
            abp_gemini_sdk::lowering::to_ir(&dialect.contents, dialect.system_instruction.as_ref());
        assert!(!ir.is_empty());
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[0].text_content(), "What is Rust?");
    }

    #[test]
    fn ir_lowering_with_system() {
        let req = GenerateContentRequest::new("gemini-2.5-flash")
            .system_instruction(Content::user(vec![Part::text("You are helpful.")]))
            .add_content(Content::user(vec![Part::text("Hi")]));
        let dialect = to_dialect_request(&req);
        let ir =
            abp_gemini_sdk::lowering::to_ir(&dialect.contents, dialect.system_instruction.as_ref());
        let sys = ir.system_message().expect("should have system");
        assert_eq!(sys.text_content(), "You are helpful.");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Codex shim roundtrips
// ═══════════════════════════════════════════════════════════════════════

mod codex_roundtrip {
    use super::*;
    use abp_shim_codex::*;

    fn make_request(model: &str, text: &str) -> abp_codex_sdk::dialect::CodexRequest {
        CodexRequestBuilder::new()
            .model(model)
            .input(vec![codex_message("user", text)])
            .build()
    }

    // ── Message construction roundtrips ──

    #[test]
    fn codex_message_construction() {
        let msg = codex_message("user", "Hello");
        assert!(matches!(
            msg,
            abp_codex_sdk::dialect::CodexInputItem::Message { ref role, ref content }
            if role == "user" && content == "Hello"
        ));
    }

    #[test]
    fn request_to_ir_preserves_content() {
        let req = make_request("codex-mini-latest", "Fix bug");
        let ir = request_to_ir(&req);
        assert_eq!(ir.len(), 1);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[0].text_content(), "Fix bug");
    }

    #[test]
    fn multi_message_request_to_ir() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![
                codex_message("system", "You are a coder"),
                codex_message("user", "Fix the test"),
            ])
            .build();
        let ir = request_to_ir(&req);
        assert_eq!(ir.len(), 2);
        assert_eq!(ir.messages[0].role, IrRole::System);
        assert_eq!(ir.messages[1].role, IrRole::User);
    }

    // ── Work order creation ──

    #[test]
    fn request_to_work_order_captures_model() {
        let req = make_request("codex-mini-latest", "Refactor");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
        assert_eq!(wo.task, "Refactor");
    }

    // ── Response construction ──

    #[test]
    fn receipt_to_response_basic() {
        let receipt = mock_receipt(vec![evt_msg("Here is the fix")]);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        assert_eq!(resp.model, "codex-mini-latest");
        assert!(!resp.output.is_empty());
    }

    #[test]
    fn receipt_to_response_with_usage() {
        let receipt = mock_receipt_with_usage(vec![evt_msg("ok")], test_usage());
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        let u = resp.usage.expect("should have usage");
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    // ── Streaming ──

    #[test]
    fn stream_events_from_deltas() {
        let events = vec![evt_delta("part1"), evt_delta("part2")];
        let stream = events_to_stream_events(&events, "codex-mini-latest");
        assert!(stream.len() >= 4); // created + 2 deltas + completed
    }

    #[test]
    fn stream_events_single() {
        let events = vec![evt_delta("full")];
        let stream = events_to_stream_events(&events, "codex-mini-latest");
        assert!(!stream.is_empty());
    }

    // ── Usage conversion ──

    #[test]
    fn ir_usage_to_codex_usage() {
        let ir = IrUsage::from_io(300, 150);
        let u = ir_usage_to_usage(&ir);
        assert_eq!(u.input_tokens, 300);
        assert_eq!(u.output_tokens, 150);
        assert_eq!(u.total_tokens, 450);
    }

    // ── Tool definitions ──

    #[test]
    fn tool_def_roundtrip() {
        let canonical = abp_codex_sdk::dialect::CanonicalToolDef {
            name: "bash".into(),
            description: "Run bash command".into(),
            parameters_schema: tool_schema(),
        };
        let codex_tool = abp_codex_sdk::dialect::tool_def_to_codex(&canonical);
        let back = abp_codex_sdk::dialect::tool_def_from_codex(&codex_tool);
        assert_eq!(back.name, "bash");
        assert_eq!(back.description, "Run bash command");
    }

    // ── Error handling ──

    #[tokio::test]
    async fn client_without_processor_returns_error() {
        let client = CodexClient::new("codex-mini-latest");
        let req = make_request("codex-mini-latest", "test");
        let err = client.create(req).await.unwrap_err();
        assert!(matches!(err, ShimError::Internal(_)));
    }

    // ── Model names ──

    #[test]
    fn model_name_codex_mini() {
        let req = make_request("codex-mini-latest", "x");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
    }

    #[test]
    fn model_name_o3() {
        let req = make_request("o3-mini", "x");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("o3-mini"));
    }

    // ── Builder options ──

    #[test]
    fn builder_with_temperature() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "x")])
            .temperature(0.2)
            .build();
        assert_eq!(req.temperature, Some(0.2));
    }

    #[test]
    fn builder_with_max_output_tokens() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "x")])
            .max_output_tokens(8192)
            .build();
        assert_eq!(req.max_output_tokens, Some(8192));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Kimi shim roundtrips
// ═══════════════════════════════════════════════════════════════════════

mod kimi_roundtrip {
    use super::*;
    use abp_shim_kimi::*;

    fn make_request(model: &str, text: &str) -> abp_kimi_sdk::dialect::KimiRequest {
        KimiRequestBuilder::new()
            .model(model)
            .messages(vec![Message::user(text)])
            .build()
    }

    // ── Message construction roundtrips ──

    #[test]
    fn user_message_to_ir_and_back() {
        let msgs = vec![Message::user("Hello")];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.len(), 1);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[0].text_content(), "Hello");
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].content.as_deref(), Some("Hello"));
    }

    #[test]
    fn system_message_roundtrip() {
        let msgs = vec![Message::system("Be helpful"), Message::user("Hi")];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.len(), 2);
        assert_eq!(ir.messages[0].role, IrRole::System);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].content.as_deref(), Some("Be helpful"));
    }

    #[test]
    fn assistant_message_roundtrip() {
        let msgs = vec![Message::user("What is 1+1?"), Message::assistant("2")];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.messages[1].role, IrRole::Assistant);
        let back = ir_to_messages(&ir);
        assert_eq!(back[1].content.as_deref(), Some("2"));
    }

    #[test]
    fn multi_turn_roundtrip() {
        let msgs = vec![
            Message::system("Math tutor"),
            Message::user("2+2?"),
            Message::assistant("4"),
            Message::user("3+3?"),
        ];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.len(), 4);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 4);
    }

    #[test]
    fn empty_conversation() {
        let msgs: Vec<Message> = vec![];
        let ir = messages_to_ir(&msgs);
        assert!(ir.is_empty());
        let back = ir_to_messages(&ir);
        assert!(back.is_empty());
    }

    // ── Request conversion ──

    #[test]
    fn request_to_ir_preserves_content() {
        let req = make_request("moonshot-v1-8k", "Hello");
        let ir = request_to_ir(&req);
        assert_eq!(ir.messages[0].text_content(), "Hello");
    }

    #[test]
    fn request_to_work_order_captures_model() {
        let req = make_request("moonshot-v1-128k", "test");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-128k"));
        assert_eq!(wo.task, "test");
    }

    // ── Response construction ──

    #[test]
    fn receipt_to_response_basic() {
        let receipt = mock_receipt(vec![evt_msg("Hi from Kimi")]);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        assert_eq!(resp.model, "moonshot-v1-8k");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hi from Kimi")
        );
    }

    #[test]
    fn receipt_to_response_with_usage() {
        let receipt = mock_receipt_with_usage(vec![evt_msg("ok")], test_usage());
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        let u = resp.usage.expect("should have usage");
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.completion_tokens, 50);
        assert_eq!(u.total_tokens, 150);
    }

    // ── Streaming ──

    #[test]
    fn stream_chunks_from_deltas() {
        let events = vec![evt_delta("Hel"), evt_delta("lo!")];
        let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
        assert!(chunks.len() >= 3); // 2 deltas + stop
        let last = chunks.last().unwrap();
        assert_eq!(last.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn stream_chunks_model_preserved() {
        let events = vec![evt_delta("hi")];
        let chunks = events_to_stream_chunks(&events, "k1-32k-preview");
        for c in &chunks {
            assert_eq!(c.model, "k1-32k-preview");
        }
    }

    // ── Usage conversion ──

    #[test]
    fn ir_usage_to_kimi_usage() {
        let ir = IrUsage::from_io(500, 250);
        let u = ir_usage_to_usage(&ir);
        assert_eq!(u.prompt_tokens, 500);
        assert_eq!(u.completion_tokens, 250);
        assert_eq!(u.total_tokens, 750);
    }

    // ── Tool definitions ──

    #[test]
    fn tool_def_roundtrip() {
        let canonical = abp_kimi_sdk::dialect::CanonicalToolDef {
            name: "web_search".into(),
            description: "Search the web".into(),
            parameters_schema: tool_schema(),
        };
        let kimi_tool = abp_kimi_sdk::dialect::tool_def_to_kimi(&canonical);
        let back = abp_kimi_sdk::dialect::tool_def_from_kimi(&kimi_tool);
        assert_eq!(back.name, "web_search");
        assert_eq!(back.description, "Search the web");
    }

    // ── Error handling ──

    #[tokio::test]
    async fn client_without_processor_returns_error() {
        let client = KimiClient::new("moonshot-v1-8k");
        let req = make_request("moonshot-v1-8k", "test");
        let err = client.create(req).await.unwrap_err();
        assert!(matches!(err, ShimError::Internal(_)));
    }

    // ── Model names ──

    #[test]
    fn model_name_8k() {
        let req = make_request("moonshot-v1-8k", "x");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-8k"));
    }

    #[test]
    fn model_name_128k() {
        let req = make_request("moonshot-v1-128k", "x");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-128k"));
    }

    // ── Builder options ──

    #[test]
    fn builder_with_temperature() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("x")])
            .temperature(0.8)
            .build();
        assert_eq!(req.temperature, Some(0.8));
    }

    #[test]
    fn builder_with_max_tokens() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("x")])
            .max_tokens(4096)
            .build();
        assert_eq!(req.max_tokens, Some(4096));
    }

    #[test]
    fn builder_with_use_search() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("x")])
            .use_search(true)
            .build();
        assert_eq!(req.use_search, Some(true));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Copilot shim roundtrips
// ═══════════════════════════════════════════════════════════════════════

mod copilot_roundtrip {
    use super::*;
    use abp_shim_copilot::*;

    fn make_request(model: &str, text: &str) -> abp_copilot_sdk::dialect::CopilotRequest {
        CopilotRequestBuilder::new()
            .model(model)
            .messages(vec![Message::user(text)])
            .build()
    }

    // ── Message construction roundtrips ──

    #[test]
    fn user_message_to_ir_and_back() {
        let msgs = vec![Message::user("Hello")];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.len(), 1);
        assert_eq!(ir.messages[0].role, IrRole::User);
        assert_eq!(ir.messages[0].text_content(), "Hello");
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].content, "Hello");
    }

    #[test]
    fn system_message_roundtrip() {
        let msgs = vec![Message::system("Be helpful"), Message::user("Hi")];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.len(), 2);
        assert_eq!(ir.messages[0].role, IrRole::System);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].content, "Be helpful");
    }

    #[test]
    fn assistant_message_roundtrip() {
        let msgs = vec![Message::user("Hi"), Message::assistant("Hello!")];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.messages[1].role, IrRole::Assistant);
        let back = ir_to_messages(&ir);
        assert_eq!(back[1].content, "Hello!");
    }

    #[test]
    fn multi_turn_roundtrip() {
        let msgs = vec![
            Message::system("helper"),
            Message::user("q1"),
            Message::assistant("a1"),
            Message::user("q2"),
        ];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.len(), 4);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 4);
    }

    #[test]
    fn empty_conversation() {
        let msgs: Vec<Message> = vec![];
        let ir = messages_to_ir(&msgs);
        assert!(ir.is_empty());
        let back = ir_to_messages(&ir);
        assert!(back.is_empty());
    }

    // ── Request conversion ──

    #[test]
    fn request_to_ir_preserves_content() {
        let req = make_request("gpt-4o", "Hello");
        let ir = request_to_ir(&req);
        assert_eq!(ir.messages[0].text_content(), "Hello");
    }

    #[test]
    fn request_to_work_order_captures_model() {
        let req = make_request("gpt-4o", "Refactor code");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
        assert_eq!(wo.task, "Refactor code");
    }

    // ── Response construction ──

    #[test]
    fn receipt_to_response_basic() {
        let receipt = mock_receipt(vec![evt_msg("Hi from Copilot")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.message, "Hi from Copilot");
    }

    #[test]
    fn receipt_to_response_with_usage() {
        let receipt = mock_receipt_with_usage(vec![evt_msg("ok")], test_usage());
        let resp = receipt_to_response(&receipt, "gpt-4o");
        // Copilot response doesn't expose usage the same way, just verify no panic
        assert!(!resp.message.is_empty());
    }

    // ── Streaming ──

    #[test]
    fn stream_events_from_deltas() {
        let events = vec![evt_delta("Hel"), evt_delta("lo!")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert_eq!(stream.len(), 4); // 1 refs + 2 deltas + 1 done
    }

    #[test]
    fn stream_events_single_delta() {
        let events = vec![evt_delta("complete")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert!(!stream.is_empty());
    }

    // ── Usage conversion ──

    #[test]
    fn copilot_ir_usage_to_tuple() {
        let ir = IrUsage::from_io(100, 50);
        let (input, output, total) = abp_shim_copilot::ir_usage_to_tuple(&ir);
        assert_eq!(input, 100);
        assert_eq!(output, 50);
        assert_eq!(total, 150);
    }

    #[test]
    fn copilot_ir_usage_zero() {
        let ir = IrUsage::from_io(0, 0);
        let (input, output, total) = abp_shim_copilot::ir_usage_to_tuple(&ir);
        assert_eq!(input, 0);
        assert_eq!(output, 0);
        assert_eq!(total, 0);
    }

    // ── Tool definitions ──

    #[test]
    fn tool_def_roundtrip() {
        let canonical = abp_copilot_sdk::dialect::CanonicalToolDef {
            name: "get_repo".into(),
            description: "Get repository info".into(),
            parameters_schema: tool_schema(),
        };
        let copilot_tool = abp_copilot_sdk::dialect::tool_def_to_copilot(&canonical);
        let back = abp_copilot_sdk::dialect::tool_def_from_copilot(&copilot_tool)
            .expect("should roundtrip");
        assert_eq!(back.name, "get_repo");
        assert_eq!(back.description, "Get repository info");
    }

    // ── Error handling ──

    #[tokio::test]
    async fn client_without_processor_returns_error() {
        let client = CopilotClient::new("gpt-4o");
        let req = make_request("gpt-4o", "test");
        let err = client.create(req).await.unwrap_err();
        assert!(matches!(err, ShimError::Internal(_)));
    }

    // ── Model names ──

    #[test]
    fn model_name_gpt4o() {
        let req = make_request("gpt-4o", "x");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn model_name_gpt4_turbo() {
        let req = make_request("gpt-4-turbo", "x");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4-turbo"));
    }

    // ── Builder options ──

    #[test]
    fn builder_with_references() {
        let refs = vec![abp_copilot_sdk::dialect::CopilotReference {
            ref_type: abp_copilot_sdk::dialect::CopilotReferenceType::File,
            id: "file-1".into(),
            data: json!({"path": "src/main.rs"}),
            metadata: None,
        }];
        let req = CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![Message::user("help")])
            .references(refs)
            .build();
        assert_eq!(req.references.len(), 1);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Cross-shim roundtrips (native A → IR → native B)
// ═══════════════════════════════════════════════════════════════════════

mod cross_shim {
    use super::*;

    #[test]
    fn openai_to_kimi_message_roundtrip() {
        let oai_msgs = vec![
            abp_shim_openai::Message::system("Be concise"),
            abp_shim_openai::Message::user("Hello"),
        ];
        let ir = abp_shim_openai::messages_to_ir(&oai_msgs);
        let kimi_msgs = abp_shim_kimi::ir_to_messages(&ir);
        assert_eq!(kimi_msgs.len(), 2);
        assert_eq!(kimi_msgs[0].content.as_deref(), Some("Be concise"));
        assert_eq!(kimi_msgs[1].content.as_deref(), Some("Hello"));
    }

    #[test]
    fn kimi_to_openai_message_roundtrip() {
        let kimi_msgs = vec![abp_shim_kimi::Message::user("What is Rust?")];
        let ir = abp_shim_kimi::messages_to_ir(&kimi_msgs);
        let oai_msgs = abp_shim_openai::ir_to_messages(&ir);
        assert_eq!(oai_msgs.len(), 1);
        assert_eq!(oai_msgs[0].content.as_deref(), Some("What is Rust?"));
    }

    #[test]
    fn openai_to_copilot_message_roundtrip() {
        let oai_msgs = vec![abp_shim_openai::Message::user("explain traits")];
        let ir = abp_shim_openai::messages_to_ir(&oai_msgs);
        let copilot_msgs = abp_shim_copilot::ir_to_messages(&ir);
        assert_eq!(copilot_msgs.len(), 1);
        assert_eq!(copilot_msgs[0].content, "explain traits");
    }

    #[test]
    fn copilot_to_kimi_message_roundtrip() {
        let copilot_msgs = vec![
            abp_shim_copilot::Message::system("helper"),
            abp_shim_copilot::Message::user("query"),
        ];
        let ir = abp_shim_copilot::messages_to_ir(&copilot_msgs);
        let kimi_msgs = abp_shim_kimi::ir_to_messages(&ir);
        assert_eq!(kimi_msgs.len(), 2);
        assert_eq!(kimi_msgs[0].content.as_deref(), Some("helper"));
    }

    #[test]
    fn receipt_to_openai_and_kimi_responses() {
        let receipt = build_receipt(vec![evt_msg("Shared response")]);
        let oai = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        let kimi = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        // Both should contain the same text
        assert!(
            oai.choices[0]
                .message
                .content
                .as_deref()
                .unwrap()
                .contains("response")
        );
        assert!(
            kimi.choices[0]
                .message
                .content
                .as_deref()
                .unwrap()
                .contains("response")
        );
    }

    #[test]
    fn receipt_to_claude_and_codex_responses() {
        let events = vec![evt_msg("cross-shim output")];
        let claude_resp =
            abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);
        let codex_receipt = abp_shim_codex::mock_receipt(vec![evt_msg("cross-shim output")]);
        let codex_resp = abp_shim_codex::receipt_to_response(&codex_receipt, "codex-mini-latest");
        assert!(!claude_resp.content.is_empty());
        assert!(!codex_resp.output.is_empty());
    }

    #[test]
    fn receipt_to_copilot_response() {
        let receipt = abp_shim_copilot::mock_receipt(vec![evt_msg("copilot says hi")]);
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.message, "copilot says hi");
    }

    #[test]
    fn all_six_shims_produce_valid_work_orders() {
        // OpenAI
        let oai_req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![abp_shim_openai::Message::user("task")])
            .build();
        let wo1 = abp_shim_openai::request_to_work_order(&oai_req);
        assert_eq!(wo1.task, "task");

        // Claude
        let claude_req = abp_shim_claude::MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
            max_tokens: 4096,
            messages: vec![abp_shim_claude::Message {
                role: abp_shim_claude::Role::User,
                content: vec![abp_shim_claude::ContentBlock::Text {
                    text: "task".into(),
                }],
            }],
            system: None,
            temperature: None,
            stop_sequences: None,
            thinking: None,
            stream: None,
        };
        let wo2 = abp_shim_claude::request_to_work_order(&claude_req);
        assert_eq!(wo2.task, "task");

        // Codex
        let codex_req = abp_shim_codex::CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![abp_shim_codex::codex_message("user", "task")])
            .build();
        let wo3 = abp_shim_codex::request_to_work_order(&codex_req);
        assert_eq!(wo3.task, "task");

        // Kimi
        let kimi_req = abp_shim_kimi::KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![abp_shim_kimi::Message::user("task")])
            .build();
        let wo4 = abp_shim_kimi::request_to_work_order(&kimi_req);
        assert_eq!(wo4.task, "task");

        // Copilot
        let copilot_req = abp_shim_copilot::CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![abp_shim_copilot::Message::user("task")])
            .build();
        let wo5 = abp_shim_copilot::request_to_work_order(&copilot_req);
        assert_eq!(wo5.task, "task");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Event handling edge cases
// ═══════════════════════════════════════════════════════════════════════

mod event_edge_cases {
    use super::*;

    #[test]
    fn openai_stream_with_tool_call_events() {
        let events = vec![
            evt_tool_call("read_file", "tc-1", json!({"path": "x.rs"})),
            evt_tool_result("read_file", "tc-1", json!("contents")),
            evt_msg("Done reading"),
        ];
        let stream = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");
        assert!(!stream.is_empty());
    }

    #[test]
    fn kimi_stream_with_tool_call_events() {
        let events = vec![
            evt_tool_call("search", "tc-2", json!({"q": "rust"})),
            evt_msg("Found results"),
        ];
        let chunks = abp_shim_kimi::events_to_stream_chunks(&events, "moonshot-v1-8k");
        assert!(!chunks.is_empty());
    }

    #[test]
    fn codex_stream_with_error_event() {
        let events = vec![evt_error("something went wrong")];
        let stream = abp_shim_codex::events_to_stream_events(&events, "codex-mini-latest");
        assert!(!stream.is_empty());
    }

    #[test]
    fn openai_stream_with_warning_event() {
        let events = vec![evt_warning("rate limit approaching"), evt_delta("text")];
        let stream = abp_shim_openai::events_to_stream_events(&events, "gpt-4o");
        assert!(!stream.is_empty());
    }

    #[test]
    fn claude_response_from_mixed_events() {
        let events = vec![
            evt_run_started(),
            evt_msg("text output"),
            evt_file_changed("src/main.rs"),
            evt_cmd("cargo test", 0),
            evt_run_completed(),
        ];
        let resp = abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);
        assert!(!resp.content.is_empty());
    }

    #[test]
    fn copilot_stream_with_mixed_events() {
        let events = vec![
            evt_delta("part1"),
            evt_file_changed("test.rs"),
            evt_delta("part2"),
        ];
        let stream = abp_shim_copilot::events_to_stream_events(&events, "gpt-4o");
        assert!(!stream.is_empty());
    }

    #[test]
    fn openai_response_from_receipt_with_tool_calls() {
        let receipt = build_receipt_multi(vec![
            evt_run_started(),
            evt_tool_call("bash", "tc-1", json!({"cmd": "ls"})),
            evt_tool_result("bash", "tc-1", json!("file1.rs\nfile2.rs")),
            evt_msg("Listed files"),
            evt_run_completed(),
        ]);
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        assert!(!resp.choices.is_empty());
    }

    #[test]
    fn kimi_response_from_receipt_with_error_event() {
        let receipt = build_receipt_multi(vec![evt_error("timeout"), evt_msg("partial output")]);
        let resp = abp_shim_kimi::receipt_to_response(&receipt, "moonshot-v1-8k");
        assert!(!resp.choices.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Serialization fidelity
// ═══════════════════════════════════════════════════════════════════════

mod serde_fidelity {
    use super::*;

    #[test]
    fn openai_request_json_roundtrip() {
        let req = abp_shim_openai::ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![abp_shim_openai::Message::user("test")])
            .build();
        let json = serde_json::to_string(&req).unwrap();
        let back: abp_shim_openai::ChatCompletionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, "gpt-4o");
        assert_eq!(back.messages.len(), 1);
    }

    #[test]
    fn openai_response_json_roundtrip() {
        let receipt = abp_shim_openai::mock_receipt(vec![evt_msg("hello")]);
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        let json = serde_json::to_string(&resp).unwrap();
        let back: abp_shim_openai::ChatCompletionResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model, "gpt-4o");
    }

    #[test]
    fn claude_content_block_json_roundtrip() {
        let block = abp_shim_claude::ContentBlock::Text {
            text: "hello".into(),
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: abp_shim_claude::ContentBlock = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, abp_shim_claude::ContentBlock::Text { text } if text == "hello"));
    }

    #[test]
    fn gemini_content_json_roundtrip() {
        let c = abp_shim_gemini::Content::user(vec![abp_shim_gemini::Part::text("test")]);
        let json = serde_json::to_string(&c).unwrap();
        let back: abp_shim_gemini::Content = serde_json::from_str(&json).unwrap();
        assert_eq!(back.role, "user");
        assert_eq!(back.parts.len(), 1);
    }

    #[test]
    fn kimi_message_json_roundtrip() {
        let msg = abp_shim_kimi::Message::user("hello");
        let json = serde_json::to_string(&msg).unwrap();
        let back: abp_shim_kimi::Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back.content.as_deref(), Some("hello"));
    }

    #[test]
    fn copilot_message_json_roundtrip() {
        let msg = abp_shim_copilot::Message::user("hello");
        let json = serde_json::to_string(&msg).unwrap();
        let back: abp_shim_copilot::Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back.content, "hello");
    }

    #[test]
    fn no_abp_framing_in_openai_response() {
        let receipt = abp_shim_openai::mock_receipt(vec![evt_msg("hi")]);
        let resp = abp_shim_openai::receipt_to_response(&receipt, "gpt-4o");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(!json.contains("work_order"));
        assert!(!json.contains("receipt_sha256"));
    }

    #[test]
    fn no_abp_framing_in_claude_response() {
        let events = vec![evt_msg("hi")];
        let resp = abp_shim_claude::response_from_events(&events, "claude-sonnet-4-20250514", None);
        let json = serde_json::to_string(&resp).unwrap();
        assert!(!json.contains("work_order"));
        assert!(!json.contains("receipt_sha256"));
    }

    #[test]
    fn no_abp_framing_in_copilot_response() {
        let receipt = abp_shim_copilot::mock_receipt(vec![evt_msg("hi")]);
        let resp = abp_shim_copilot::receipt_to_response(&receipt, "gpt-4o");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(!json.contains("work_order"));
        assert!(!json.contains("receipt_sha256"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. IR layer property checks
// ═══════════════════════════════════════════════════════════════════════

mod ir_properties {
    use super::*;

    #[test]
    fn ir_conversation_len() {
        let ir = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "one"),
            IrMessage::text(IrRole::Assistant, "two"),
        ]);
        assert_eq!(ir.len(), 2);
        assert!(!ir.is_empty());
    }

    #[test]
    fn ir_conversation_system_message() {
        let ir = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "system prompt"),
            IrMessage::text(IrRole::User, "hi"),
        ]);
        let sys = ir.system_message().unwrap();
        assert_eq!(sys.text_content(), "system prompt");
    }

    #[test]
    fn ir_conversation_last_assistant() {
        let ir = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "q1"),
            IrMessage::text(IrRole::Assistant, "a1"),
            IrMessage::text(IrRole::User, "q2"),
            IrMessage::text(IrRole::Assistant, "a2"),
        ]);
        assert_eq!(ir.last_assistant().unwrap().text_content(), "a2");
    }

    #[test]
    fn ir_conversation_messages_by_role() {
        let ir = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "q1"),
            IrMessage::text(IrRole::Assistant, "a1"),
            IrMessage::text(IrRole::User, "q2"),
        ]);
        assert_eq!(ir.messages_by_role(IrRole::User).len(), 2);
        assert_eq!(ir.messages_by_role(IrRole::Assistant).len(), 1);
    }

    #[test]
    fn ir_conversation_tool_calls() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tc-1".into(),
                name: "read_file".into(),
                input: json!({}),
            }],
        );
        let ir = IrConversation::from_messages(vec![msg]);
        assert_eq!(ir.tool_calls().len(), 1);
    }

    #[test]
    fn ir_message_text_content_concatenation() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Hello ".into(),
                },
                IrContentBlock::Text {
                    text: "World".into(),
                },
            ],
        );
        assert_eq!(msg.text_content(), "Hello World");
    }

    #[test]
    fn ir_message_is_text_only() {
        let text_msg = IrMessage::text(IrRole::User, "hello");
        assert!(text_msg.is_text_only());

        let mixed_msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "text".into(),
                },
                IrContentBlock::ToolUse {
                    id: "t".into(),
                    name: "f".into(),
                    input: json!({}),
                },
            ],
        );
        assert!(!mixed_msg.is_text_only());
    }

    #[test]
    fn ir_usage_from_io() {
        let u = IrUsage::from_io(100, 200);
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 200);
        assert_eq!(u.total_tokens, 300);
        assert_eq!(u.cache_read_tokens, 0);
        assert_eq!(u.cache_write_tokens, 0);
    }

    #[test]
    fn ir_conversation_push_chaining() {
        let ir = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "first"))
            .push(IrMessage::text(IrRole::Assistant, "second"));
        assert_eq!(ir.len(), 2);
    }

    #[test]
    fn ir_empty_conversation() {
        let ir = IrConversation::new();
        assert!(ir.is_empty());
        assert_eq!(ir.len(), 0);
        assert!(ir.system_message().is_none());
        assert!(ir.last_assistant().is_none());
        assert!(ir.last_message().is_none());
    }
}
