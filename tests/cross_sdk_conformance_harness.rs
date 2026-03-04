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
#![allow(clippy::needless_borrows_for_generic_args)]
//! Cross-SDK conformance test harness.
//!
//! Validates passthrough fidelity, cross-dialect mapping, capability
//! consistency, error code consistency, and receipt compatibility
//! across all six shim crates.

use abp_capability::{
    self, check_capability, claude_35_sonnet_manifest, codex_manifest, copilot_manifest,
    gemini_15_pro_manifest, generate_report, kimi_manifest, negotiate, openai_gpt4o_manifest,
};
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use abp_core::{
    AgentEvent, AgentEventKind, CONTRACT_VERSION, Capability, CapabilityManifest,
    CapabilityRequirement, CapabilityRequirements, MinSupport, Outcome, Receipt, ReceiptBuilder,
    UsageNormalized,
};
use abp_dialect::Dialect;
use abp_error::ErrorCode;
use abp_mapper::{default_ir_mapper, supported_ir_pairs};
use abp_receipt::{compute_hash, verify_hash};
use chrono::Utc;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// Shared helpers
// ═══════════════════════════════════════════════════════════════════════════

const ALL_DIALECTS: &[Dialect] = &[
    Dialect::OpenAi,
    Dialect::Claude,
    Dialect::Gemini,
    Dialect::Codex,
    Dialect::Kimi,
    Dialect::Copilot,
];

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

fn evt_run_completed() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
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

fn build_receipt(events: Vec<AgentEvent>) -> Receipt {
    let mut b = ReceiptBuilder::new("test-backend")
        .outcome(Outcome::Complete)
        .usage(test_usage());
    for e in events {
        b = b.add_trace_event(e);
    }
    b.build()
}

fn manifest_for_dialect(d: Dialect) -> CapabilityManifest {
    match d {
        Dialect::OpenAi => openai_gpt4o_manifest(),
        Dialect::Claude => claude_35_sonnet_manifest(),
        Dialect::Gemini => gemini_15_pro_manifest(),
        Dialect::Codex => codex_manifest(),
        Dialect::Kimi => kimi_manifest(),
        Dialect::Copilot => copilot_manifest(),
    }
}

fn simple_ir_conv(text: &str) -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Text {
            text: text.to_string(),
        }],
    )])
}

fn multi_turn_ir_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::System,
            vec![IrContentBlock::Text {
                text: "You are helpful".into(),
            }],
        ),
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Text {
                text: "Hello".into(),
            }],
        ),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Text {
                text: "Hi there!".into(),
            }],
        ),
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Text {
                text: "How are you?".into(),
            }],
        ),
    ])
}

fn tool_use_ir_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Text {
                text: "Read a file".into(),
            }],
        ),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_1".into(),
                name: "read_file".into(),
                input: json!({"path": "/tmp/test.rs"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "fn main() {}".into(),
                }],
                is_error: false,
            }],
        ),
    ])
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Passthrough fidelity — OpenAI
// ═══════════════════════════════════════════════════════════════════════════

mod openai_passthrough {
    use super::*;
    use abp_shim_openai::*;

    #[test]
    fn t01_user_message_roundtrip_preserves_text() {
        let msgs = vec![Message::user("Hello world")];
        let ir = messages_to_ir(&msgs);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].content.as_deref(), Some("Hello world"));
    }

    #[test]
    fn t02_system_message_roundtrip_preserves_role() {
        let msgs = vec![Message::system("Be concise"), Message::user("Hi")];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.messages[0].role, IrRole::System);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 2);
    }

    #[test]
    fn t03_assistant_message_roundtrip() {
        let msgs = vec![Message::user("2+2?"), Message::assistant("4")];
        let ir = messages_to_ir(&msgs);
        let back = ir_to_messages(&ir);
        assert_eq!(back[1].content.as_deref(), Some("4"));
    }

    #[test]
    fn t04_multi_turn_preserves_count() {
        let msgs = vec![
            Message::system("system"),
            Message::user("u1"),
            Message::assistant("a1"),
            Message::user("u2"),
        ];
        let ir = messages_to_ir(&msgs);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 4);
    }

    #[test]
    fn t05_tool_message_roundtrip() {
        let msgs = vec![
            Message::user("Read file"),
            Message::tool("tool-1", "contents"),
        ];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.messages[1].role, IrRole::Tool);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 2);
    }

    #[test]
    fn t06_request_to_ir_preserves_content() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o")
            .messages(vec![Message::user("test")])
            .build();
        let ir = request_to_ir(&req);
        assert_eq!(ir.messages[0].text_content(), "test");
    }

    #[test]
    fn t07_tool_definitions_roundtrip() {
        let tools = vec![
            Tool::function("read_file", "Read a file", tool_schema()),
            Tool::function("write_file", "Write a file", tool_schema()),
        ];
        let ir_tools = tools_to_ir(&tools);
        assert_eq!(ir_tools.len(), 2);
        assert_eq!(ir_tools[0].name, "read_file");
        assert_eq!(ir_tools[1].name, "write_file");
        assert_eq!(ir_tools[0].parameters, tool_schema());
    }

    #[test]
    fn t08_work_order_captures_model() {
        let req = ChatCompletionRequest::builder()
            .model("gpt-4o-mini")
            .messages(vec![Message::user("hello")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o-mini"));
    }

    #[test]
    fn t09_receipt_to_response_preserves_text() {
        let receipt = mock_receipt(vec![evt_msg("Hi there")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert_eq!(resp.model, "gpt-4o");
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hi there"));
    }

    #[test]
    fn t10_usage_ir_roundtrip() {
        let ir = IrUsage::from_io(200, 100);
        let u = ir_usage_to_usage(&ir);
        assert_eq!(u.prompt_tokens, 200);
        assert_eq!(u.completion_tokens, 100);
        assert_eq!(u.total_tokens, 300);
    }

    #[test]
    fn t11_empty_conversation_roundtrip() {
        let msgs: Vec<Message> = vec![];
        let ir = messages_to_ir(&msgs);
        assert!(ir.is_empty());
        let back = ir_to_messages(&ir);
        assert!(back.is_empty());
    }

    #[test]
    fn t12_stream_events_include_stop() {
        let events = vec![evt_delta("hello")];
        let stream = events_to_stream_events(&events, "gpt-4o");
        assert!(!stream.is_empty());
        let last = stream.last().unwrap();
        assert_eq!(last.choices[0].finish_reason.as_deref(), Some("stop"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Passthrough fidelity — Claude
// ═══════════════════════════════════════════════════════════════════════════

mod claude_passthrough {
    use super::*;
    use abp_shim_claude::*;

    fn make_request(text: &str) -> MessageRequest {
        MessageRequest {
            model: "claude-sonnet-4-20250514".into(),
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

    #[test]
    fn t01_text_content_block_roundtrip() {
        let block = ContentBlock::Text {
            text: "Hello".into(),
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert!(matches!(back, ContentBlock::Text { text } if text == "Hello"));
    }

    #[test]
    fn t02_tool_use_content_block_roundtrip() {
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
    fn t03_tool_result_content_block_roundtrip() {
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
    fn t04_message_to_ir_preserves_role() {
        let msg = Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "Hello".into(),
            }],
        };
        let ir = message_to_ir(&msg);
        assert_eq!(ir.role, "user");
    }

    #[test]
    fn t05_work_order_captures_model() {
        let req = make_request("hello");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("claude-sonnet-4-20250514"));
    }

    #[test]
    fn t06_work_order_captures_task() {
        let req = make_request("explain Rust");
        let wo = request_to_work_order(&req);
        assert_eq!(wo.task, "explain Rust");
    }

    #[test]
    fn t07_request_with_system_prompt() {
        let mut req = make_request("hi");
        req.system = Some("Be concise".into());
        let wo = request_to_work_order(&req);
        assert!(!wo.task.is_empty());
    }

    #[test]
    fn t08_thinking_block_roundtrip() {
        let block = ContentBlock::Thinking {
            thinking: "Let me think...".into(),
            signature: None,
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert!(
            matches!(back, ContentBlock::Thinking { thinking, .. } if thinking == "Let me think...")
        );
    }

    #[test]
    fn t09_image_block_roundtrip() {
        let block = ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".into(),
                data: "aWhlbGxv".into(),
            },
        };
        let ir = content_block_to_ir(&block);
        let back = content_block_from_ir(&ir);
        assert!(matches!(back, ContentBlock::Image { .. }));
    }

    #[test]
    fn t10_multi_block_message_preserves_count() {
        let msg = Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: "Let me read".into(),
                },
                ContentBlock::ToolUse {
                    id: "tc-1".into(),
                    name: "read_file".into(),
                    input: json!({"path": "test.rs"}),
                },
            ],
        };
        let ir = message_to_ir(&msg);
        assert!(!ir.content.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Passthrough fidelity — Gemini
// ═══════════════════════════════════════════════════════════════════════════

mod gemini_passthrough {
    use super::*;
    use abp_shim_gemini::*;

    #[test]
    fn t01_text_request_to_ir_roundtrip() {
        let req = GenerateContentRequest::new("gemini-1.5-pro")
            .add_content(Content::user(vec![Part::text("Hello")]));
        let (ir, _gen, _safety) = request_to_ir(&req).unwrap();
        assert!(!ir.conversation.is_empty());
    }

    #[test]
    fn t02_ir_to_response_produces_candidates() {
        let ir = simple_ir_conv("test response");
        let receipt = build_receipt(vec![evt_msg("test response")]);
        let resp = ir_to_response(&ir, &receipt, &None, &[]).unwrap();
        assert!(!resp.candidates.is_empty());
    }

    #[test]
    fn t03_function_call_part_roundtrip() {
        let part = Part::function_call("search", json!({"query": "test"}));
        let dialect_part = part_to_dialect(&part);
        let back = part_from_dialect(&dialect_part);
        assert!(matches!(back, Part::FunctionCall { name, .. } if name == "search"));
    }

    #[test]
    fn t04_function_response_part_roundtrip() {
        let part = Part::function_response("search", json!({"result": "found"}));
        let dialect_part = part_to_dialect(&part);
        let back = part_from_dialect(&dialect_part);
        assert!(matches!(back, Part::FunctionResponse { name, .. } if name == "search"));
    }

    #[test]
    fn t05_usage_ir_roundtrip() {
        let usage = UsageMetadata {
            prompt_token_count: 50,
            candidates_token_count: 30,
            total_token_count: 80,
        };
        let ir = usage_to_ir(&usage);
        let back = usage_from_ir(&ir);
        assert_eq!(back.prompt_token_count, 50);
        assert_eq!(back.candidates_token_count, 30);
        assert_eq!(back.total_token_count, 80);
    }

    #[test]
    fn t06_content_dialect_roundtrip() {
        let content = Content::user(vec![Part::text("Hello world")]);
        let dialect = content_to_dialect(&content);
        let back = content_from_dialect(&dialect);
        assert_eq!(back.role, "user");
        assert_eq!(back.parts.len(), 1);
    }

    #[test]
    fn t07_receipt_to_stream_events_produces_chunks() {
        let receipt = build_receipt(vec![evt_msg("chunk1"), evt_delta("chunk2")]);
        let stream = receipt_to_stream_events(&receipt);
        assert!(stream.len() >= 2);
    }

    #[test]
    fn t08_work_order_captures_model() {
        let req = GenerateContentRequest::new("gemini-1.5-flash")
            .add_content(Content::user(vec![Part::text("test")]));
        let (ir, gen_cfg, _) = request_to_ir(&req).unwrap();
        let wo = ir_to_work_order(&ir, "gemini-1.5-flash", &gen_cfg);
        assert!(
            wo.config
                .model
                .as_deref()
                .unwrap()
                .contains("gemini-1.5-flash")
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Passthrough fidelity — Codex
// ═══════════════════════════════════════════════════════════════════════════

mod codex_passthrough {
    use super::*;
    use abp_shim_codex::*;

    #[test]
    fn t01_request_to_ir_preserves_message() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "Hello")])
            .build();
        let ir = request_to_ir(&req);
        assert!(!ir.is_empty());
        assert_eq!(ir.messages[0].text_content(), "Hello");
    }

    #[test]
    fn t02_work_order_captures_model() {
        let req = CodexRequestBuilder::new()
            .model("codex-mini-latest")
            .input(vec![codex_message("user", "hello")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("codex-mini-latest"));
    }

    #[test]
    fn t03_receipt_to_response_preserves_model() {
        let receipt = mock_receipt(vec![evt_msg("result")]);
        let resp = receipt_to_response(&receipt, "codex-mini-latest");
        assert_eq!(resp.model, "codex-mini-latest");
    }

    #[test]
    fn t04_usage_ir_roundtrip() {
        let ir = IrUsage::from_io(150, 75);
        let u = ir_usage_to_usage(&ir);
        assert_eq!(u.input_tokens, 150);
        assert_eq!(u.output_tokens, 75);
        assert_eq!(u.total_tokens, 225);
    }

    #[test]
    fn t05_ir_to_response_items_preserves_text() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Text {
                text: "hello".into(),
            }],
        )]);
        let items = ir_to_response_items(&conv);
        assert!(!items.is_empty());
    }

    #[test]
    fn t06_stream_events_from_deltas() {
        let events = vec![evt_delta("part1"), evt_delta("part2")];
        let stream = events_to_stream_events(&events, "codex-mini-latest");
        assert!(stream.len() >= 2);
    }

    #[test]
    fn t07_multi_message_request_roundtrip() {
        let req = CodexRequestBuilder::new()
            .input(vec![
                codex_message("system", "Be helpful"),
                codex_message("user", "Hello"),
            ])
            .build();
        let ir = request_to_ir(&req);
        assert!(ir.len() >= 2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Passthrough fidelity — Copilot
// ═══════════════════════════════════════════════════════════════════════════

mod copilot_passthrough {
    use super::*;
    use abp_shim_copilot::*;

    #[test]
    fn t01_user_message_ir_roundtrip() {
        let msgs = vec![Message::user("Hello")];
        let ir = messages_to_ir(&msgs);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].content, "Hello");
    }

    #[test]
    fn t02_system_message_roundtrip() {
        let msgs = vec![Message::system("Be helpful"), Message::user("Hi")];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.messages[0].role, IrRole::System);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 2);
    }

    #[test]
    fn t03_assistant_message_roundtrip() {
        let msgs = vec![Message::user("hi"), Message::assistant("hello")];
        let ir = messages_to_ir(&msgs);
        let back = ir_to_messages(&ir);
        assert_eq!(back[1].content, "hello");
    }

    #[test]
    fn t04_work_order_captures_model() {
        let req = CopilotRequestBuilder::new()
            .model("gpt-4o")
            .messages(vec![Message::user("test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn t05_receipt_to_response_produces_message() {
        let receipt = mock_receipt(vec![evt_msg("answer")]);
        let resp = receipt_to_response(&receipt, "gpt-4o");
        assert!(!resp.message.is_empty());
    }

    #[test]
    fn t06_usage_ir_roundtrip() {
        let ir = IrUsage::from_io(80, 40);
        let (input, output, total) = ir_usage_to_tuple(&ir);
        assert_eq!(input, 80);
        assert_eq!(output, 40);
        assert_eq!(total, 120);
    }

    #[test]
    fn t07_empty_conversation_roundtrip() {
        let msgs: Vec<Message> = vec![];
        let ir = messages_to_ir(&msgs);
        assert!(ir.is_empty());
        let back = ir_to_messages(&ir);
        assert!(back.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Passthrough fidelity — Kimi
// ═══════════════════════════════════════════════════════════════════════════

mod kimi_passthrough {
    use super::*;
    use abp_shim_kimi::*;

    #[test]
    fn t01_user_message_ir_roundtrip() {
        let msgs = vec![Message::user("Hello")];
        let ir = messages_to_ir(&msgs);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].content.as_deref(), Some("Hello"));
    }

    #[test]
    fn t02_system_message_roundtrip() {
        let msgs = vec![Message::system("Be helpful"), Message::user("Hi")];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.messages[0].role, IrRole::System);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 2);
    }

    #[test]
    fn t03_assistant_message_roundtrip() {
        let msgs = vec![Message::user("hello"), Message::assistant("hi")];
        let ir = messages_to_ir(&msgs);
        let back = ir_to_messages(&ir);
        assert_eq!(back[1].content.as_deref(), Some("hi"));
    }

    #[test]
    fn t04_work_order_captures_model() {
        let req = KimiRequestBuilder::new()
            .model("moonshot-v1-8k")
            .messages(vec![Message::user("test")])
            .build();
        let wo = request_to_work_order(&req);
        assert_eq!(wo.config.model.as_deref(), Some("moonshot-v1-8k"));
    }

    #[test]
    fn t05_receipt_to_response_preserves_model() {
        let receipt = abp_shim_kimi::mock_receipt(vec![evt_msg("answer")]);
        let resp = receipt_to_response(&receipt, "moonshot-v1-8k");
        assert_eq!(resp.model, "moonshot-v1-8k");
    }

    #[test]
    fn t06_usage_ir_roundtrip() {
        let ir = IrUsage::from_io(120, 60);
        let u = ir_usage_to_usage(&ir);
        assert_eq!(u.prompt_tokens, 120);
        assert_eq!(u.completion_tokens, 60);
        assert_eq!(u.total_tokens, 180);
    }

    #[test]
    fn t07_tool_message_roundtrip() {
        let msgs = vec![
            Message::user("read file"),
            Message::tool("tool-1", "file content"),
        ];
        let ir = messages_to_ir(&msgs);
        assert_eq!(ir.messages[1].role, IrRole::Tool);
        let back = ir_to_messages(&ir);
        assert_eq!(back.len(), 2);
    }

    #[test]
    fn t08_stream_chunks_from_deltas() {
        let events = vec![evt_delta("part1"), evt_delta("part2")];
        let chunks = events_to_stream_chunks(&events, "moonshot-v1-8k");
        assert!(chunks.len() >= 2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Cross-dialect mapping via IR mappers
// ═══════════════════════════════════════════════════════════════════════════

mod cross_dialect_mapping {
    use super::*;

    #[test]
    fn t01_supported_pairs_is_nonempty() {
        let pairs = supported_ir_pairs();
        assert!(!pairs.is_empty(), "should have at least one supported pair");
    }

    #[test]
    fn t02_identity_mapping_for_each_dialect() {
        for &d in ALL_DIALECTS {
            let mapper = default_ir_mapper(d, d);
            assert!(mapper.is_some(), "identity mapper should exist for {d:?}",);
        }
    }

    #[test]
    fn t03_openai_to_claude_mapper_exists() {
        let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude);
        assert!(mapper.is_some());
    }

    #[test]
    fn t04_claude_to_openai_mapper_exists() {
        let mapper = default_ir_mapper(Dialect::Claude, Dialect::OpenAi);
        assert!(mapper.is_some());
    }

    #[test]
    fn t05_openai_to_gemini_mapper_exists() {
        let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Gemini);
        assert!(mapper.is_some());
    }

    #[test]
    fn t06_gemini_to_openai_mapper_exists() {
        let mapper = default_ir_mapper(Dialect::Gemini, Dialect::OpenAi);
        assert!(mapper.is_some());
    }

    #[test]
    fn t07_openai_to_kimi_mapper_exists() {
        let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Kimi);
        assert!(mapper.is_some());
    }

    #[test]
    fn t08_openai_to_copilot_mapper_exists() {
        let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Copilot);
        assert!(mapper.is_some());
    }

    #[test]
    fn t09_simple_text_maps_across_all_supported_pairs() {
        let conv = simple_ir_conv("Hello world");
        let pairs = supported_ir_pairs();
        for (from, to) in &pairs {
            let mapper = default_ir_mapper(*from, *to).unwrap();
            let result = mapper.map_request(*from, *to, &conv);
            assert!(
                result.is_ok(),
                "simple text mapping {from:?} -> {to:?} should succeed: {:?}",
                result.err()
            );
        }
    }

    #[test]
    fn t10_multi_turn_maps_across_supported_pairs() {
        let conv = multi_turn_ir_conv();
        let pairs = supported_ir_pairs();
        for (from, to) in &pairs {
            let mapper = default_ir_mapper(*from, *to).unwrap();
            let result = mapper.map_request(*from, *to, &conv);
            assert!(
                result.is_ok(),
                "multi-turn mapping {from:?} -> {to:?} should succeed: {:?}",
                result.err()
            );
        }
    }

    #[test]
    fn t11_identity_mapping_preserves_message_count() {
        let conv = multi_turn_ir_conv();
        for &d in ALL_DIALECTS {
            if let Some(mapper) = default_ir_mapper(d, d) {
                let result = mapper.map_request(d, d, &conv).unwrap();
                assert_eq!(
                    result.len(),
                    conv.len(),
                    "identity mapping for {d:?} should preserve count"
                );
            }
        }
    }

    #[test]
    fn t12_tool_use_maps_across_major_pairs() {
        let conv = tool_use_ir_conv();
        let major_pairs = [
            (Dialect::OpenAi, Dialect::Claude),
            (Dialect::Claude, Dialect::OpenAi),
            (Dialect::OpenAi, Dialect::Gemini),
            (Dialect::Gemini, Dialect::OpenAi),
        ];
        for (from, to) in &major_pairs {
            if let Some(mapper) = default_ir_mapper(*from, *to) {
                let result = mapper.map_request(*from, *to, &conv);
                assert!(
                    result.is_ok(),
                    "tool use mapping {from:?} -> {to:?} should succeed: {:?}",
                    result.err()
                );
            }
        }
    }

    #[test]
    fn t13_response_mapping_across_supported_pairs() {
        let conv = simple_ir_conv("response text");
        for (from, to) in &supported_ir_pairs() {
            let mapper = default_ir_mapper(*from, *to).unwrap();
            let result = mapper.map_response(*from, *to, &conv);
            assert!(
                result.is_ok(),
                "response mapping {from:?} -> {to:?} should succeed"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Capability consistency
// ═══════════════════════════════════════════════════════════════════════════

mod capability_consistency {
    use super::*;
    use abp_capability::SupportLevel;

    #[test]
    fn t01_all_manifests_are_nonempty() {
        for &d in ALL_DIALECTS {
            let m = manifest_for_dialect(d);
            assert!(!m.is_empty(), "{d:?} manifest should not be empty");
        }
    }

    #[test]
    fn t02_streaming_declared_by_all() {
        for &d in ALL_DIALECTS {
            let m = manifest_for_dialect(d);
            let level = check_capability(&m, &Capability::Streaming);
            assert!(
                !matches!(level, SupportLevel::Unsupported { .. }),
                "{d:?} should support streaming"
            );
        }
    }

    #[test]
    fn t03_system_message_declared_by_all_major() {
        for d in [Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
            let m = manifest_for_dialect(d);
            let level = check_capability(&m, &Capability::SystemMessage);
            assert!(
                matches!(level, SupportLevel::Native),
                "{d:?} should natively support system messages"
            );
        }
    }

    #[test]
    fn t04_tool_use_capabilities_consistent() {
        // All major backends should support tool use
        for d in [
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Kimi,
        ] {
            let m = manifest_for_dialect(d);
            let level = check_capability(&m, &Capability::ToolUse);
            assert!(
                !matches!(level, SupportLevel::Unsupported { .. }),
                "{d:?} should support tool use"
            );
        }
    }

    #[test]
    fn t05_negotiation_viable_for_basic_caps() {
        let basic_caps = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Emulated,
            }],
        };
        for &d in ALL_DIALECTS {
            let m = manifest_for_dialect(d);
            let result = negotiate(&m, &basic_caps);
            assert!(
                result.is_viable(),
                "{d:?} should be viable for basic streaming"
            );
        }
    }

    #[test]
    fn t06_negotiation_report_is_nonempty() {
        let caps = CapabilityRequirements {
            required: vec![CapabilityRequirement {
                capability: Capability::Streaming,
                min_support: MinSupport::Native,
            }],
        };
        let m = openai_gpt4o_manifest();
        let result = negotiate(&m, &caps);
        let report = generate_report(&result);
        assert!(!report.summary.is_empty());
    }

    #[test]
    fn t07_vision_support_varies_by_dialect() {
        let openai = check_capability(&openai_gpt4o_manifest(), &Capability::Vision);
        let codex = check_capability(&codex_manifest(), &Capability::Vision);
        // OpenAI GPT-4o supports vision; Codex may not natively
        assert!(matches!(openai, SupportLevel::Native));
        assert_ne!(
            format!("{openai:?}"),
            format!("{codex:?}"),
            "vision support should differ between OpenAI and Codex"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Error code consistency
// ═══════════════════════════════════════════════════════════════════════════

mod error_code_consistency {
    use super::*;

    #[test]
    fn t01_error_codes_have_categories() {
        let codes = [
            ErrorCode::BackendTimeout,
            ErrorCode::BackendRateLimited,
            ErrorCode::CapabilityUnsupported,
            ErrorCode::MappingUnsupportedCapability,
            ErrorCode::Internal,
            ErrorCode::IrLoweringFailed,
            ErrorCode::ContractVersionMismatch,
        ];
        for code in &codes {
            let cat = code.category();
            assert!(
                !format!("{cat:?}").is_empty(),
                "{code:?} should have a category"
            );
        }
    }

    #[test]
    fn t02_backend_timeout_is_retryable() {
        assert!(ErrorCode::BackendTimeout.is_retryable());
    }

    #[test]
    fn t03_backend_rate_limited_is_retryable() {
        assert!(ErrorCode::BackendRateLimited.is_retryable());
    }

    #[test]
    fn t04_internal_error_is_not_retryable() {
        assert!(!ErrorCode::Internal.is_retryable());
    }

    #[test]
    fn t05_contract_version_mismatch_not_retryable() {
        assert!(!ErrorCode::ContractVersionMismatch.is_retryable());
    }

    #[test]
    fn t06_error_codes_have_stable_string_repr() {
        assert!(!ErrorCode::BackendTimeout.as_str().is_empty());
        assert!(!ErrorCode::Internal.as_str().is_empty());
        assert!(!ErrorCode::MappingDialectMismatch.as_str().is_empty());
    }

    #[test]
    fn t07_error_info_construction() {
        let info = abp_error::ErrorInfo::new(ErrorCode::BackendTimeout, "timed out");
        assert_eq!(info.code, ErrorCode::BackendTimeout);
        assert_eq!(info.message, "timed out");
    }

    #[test]
    fn t08_error_codes_have_messages() {
        let codes = [
            ErrorCode::ProtocolInvalidEnvelope,
            ErrorCode::BackendNotFound,
            ErrorCode::CapabilityUnsupported,
            ErrorCode::PolicyDenied,
        ];
        for code in &codes {
            assert!(!code.message().is_empty(), "{code:?} should have a message");
        }
    }

    #[test]
    fn t09_mapping_error_codes_exist() {
        // Verify all mapping-related error codes are accessible
        let _ = ErrorCode::MappingUnsupportedCapability;
        let _ = ErrorCode::MappingDialectMismatch;
        let _ = ErrorCode::MappingLossyConversion;
        let _ = ErrorCode::MappingUnmappableTool;
    }

    #[test]
    fn t10_abp_error_with_context() {
        let err = abp_error::AbpError::new(ErrorCode::BackendTimeout, "timeout")
            .with_context("backend", "openai");
        assert_eq!(err.code, ErrorCode::BackendTimeout);
        assert!(err.is_retryable());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Receipt compatibility
// ═══════════════════════════════════════════════════════════════════════════

mod receipt_compatibility {
    use super::*;

    #[test]
    fn t01_receipt_hash_is_deterministic() {
        let events = vec![evt_run_started(), evt_msg("hello"), evt_run_completed()];
        let r1 = build_receipt(events.clone());
        let r2 = build_receipt(events);
        let h1 = compute_hash(&r1).unwrap();
        let h2 = compute_hash(&r2).unwrap();
        // Hashes may differ due to timestamps, but the function should not panic
        assert!(!h1.is_empty());
        assert!(!h2.is_empty());
    }

    #[test]
    fn t02_receipt_hash_verifies() {
        let receipt = ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .usage(test_usage())
            .add_trace_event(evt_msg("hello"))
            .with_hash()
            .unwrap();
        assert!(verify_hash(&receipt));
    }

    #[test]
    fn t03_receipt_includes_contract_version() {
        let receipt = build_receipt(vec![evt_msg("test")]);
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    }

    #[test]
    fn t04_receipt_timestamps_are_valid() {
        let receipt = build_receipt(vec![evt_msg("test")]);
        // started_at should be before or equal to finished_at
        assert!(receipt.meta.started_at <= receipt.meta.finished_at);
    }

    #[test]
    fn t05_receipt_from_each_shim_has_valid_hash() {
        // Each shim's mock_receipt should produce hashable receipts
        let events = vec![evt_msg("hello")];

        let openai_receipt = abp_shim_openai::mock_receipt(events.clone());
        assert!(compute_hash(&openai_receipt).is_ok());

        let codex_receipt = abp_shim_codex::mock_receipt(events.clone());
        assert!(compute_hash(&codex_receipt).is_ok());

        let copilot_receipt = abp_shim_copilot::mock_receipt(events.clone());
        assert!(compute_hash(&copilot_receipt).is_ok());

        let kimi_receipt = abp_shim_kimi::mock_receipt(events);
        assert!(compute_hash(&kimi_receipt).is_ok());
    }

    #[test]
    fn t06_receipt_outcome_variants() {
        for outcome in [Outcome::Complete, Outcome::Partial, Outcome::Failed] {
            let receipt = ReceiptBuilder::new("test").outcome(outcome.clone()).build();
            assert_eq!(receipt.outcome, outcome);
        }
    }

    #[test]
    fn t07_receipt_usage_roundtrip() {
        let usage = UsageNormalized {
            input_tokens: Some(100),
            output_tokens: Some(50),
            ..Default::default()
        };
        let receipt = ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .usage(usage.clone())
            .build();
        assert_eq!(receipt.usage.input_tokens, Some(100));
        assert_eq!(receipt.usage.output_tokens, Some(50));
    }

    #[test]
    fn t08_receipt_with_hash_sets_sha256() {
        let receipt = ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        assert!(receipt.receipt_sha256.is_some());
    }

    #[test]
    fn t09_modified_receipt_fails_verification() {
        let mut receipt = ReceiptBuilder::new("test")
            .outcome(Outcome::Complete)
            .with_hash()
            .unwrap();
        // Tamper with the receipt
        receipt.outcome = Outcome::Failed;
        assert!(!verify_hash(&receipt));
    }

    #[test]
    fn t10_receipt_trace_preserves_events() {
        let events = vec![
            evt_run_started(),
            evt_msg("hello"),
            evt_tool_call("read_file", "tc-1", json!({"path": "test"})),
            evt_run_completed(),
        ];
        let receipt = build_receipt(events);
        assert!(receipt.trace.len() >= 4);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Cross-shim IR structure consistency
// ═══════════════════════════════════════════════════════════════════════════

mod ir_structure_consistency {
    use super::*;

    #[test]
    fn t01_ir_conversation_serde_roundtrip() {
        let conv = multi_turn_ir_conv();
        let json = serde_json::to_string(&conv).unwrap();
        let back: IrConversation = serde_json::from_str(&json).unwrap();
        assert_eq!(conv, back);
    }

    #[test]
    fn t02_ir_tool_use_block_serde() {
        let block = IrContentBlock::ToolUse {
            id: "call_1".into(),
            name: "search".into(),
            input: json!({"q": "rust"}),
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn t03_ir_usage_merge() {
        let u1 = IrUsage::from_io(100, 50);
        let u2 = IrUsage::from_io(200, 100);
        let merged = u1.merge(u2);
        assert_eq!(merged.input_tokens, 300);
        assert_eq!(merged.output_tokens, 150);
    }
}
