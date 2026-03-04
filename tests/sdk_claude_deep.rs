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
//! Comprehensive deep tests for Claude SDK dialect types and lowering.
//!
//! Covers all Claude message roles, request/response construction, streaming
//! events, tool use, Claude→IR lowering, IR→Claude lifting, serde roundtrips,
//! system prompt handling, content blocks, configuration, and edge cases.

use abp_claude_sdk::dialect::{
    self, CanonicalToolDef, ClaudeApiError, ClaudeCacheControl, ClaudeConfig, ClaudeContentBlock,
    ClaudeImageSource, ClaudeMessage, ClaudeMessageDelta, ClaudeRequest, ClaudeResponse,
    ClaudeStopReason, ClaudeStreamDelta, ClaudeStreamEvent, ClaudeSystemBlock, ClaudeToolDef,
    ClaudeUsage, ThinkingConfig,
};
use abp_claude_sdk::lowering;
use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_core::{AgentEventKind, WorkOrderBuilder};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn claude_msg(role: &str, content: &str) -> ClaudeMessage {
    ClaudeMessage {
        role: role.into(),
        content: content.into(),
    }
}

fn claude_blocks_msg(role: &str, blocks: &[ClaudeContentBlock]) -> ClaudeMessage {
    ClaudeMessage {
        role: role.into(),
        content: serde_json::to_string(blocks).unwrap(),
    }
}

fn make_response(content: Vec<ClaudeContentBlock>) -> ClaudeResponse {
    ClaudeResponse {
        id: "msg_test".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content,
        stop_reason: Some("end_turn".into()),
        usage: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Claude message roles
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn role_user_maps_to_ir_user() {
    let msgs = vec![claude_msg("user", "hi")];
    let conv = lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn role_assistant_maps_to_ir_assistant() {
    let msgs = vec![claude_msg("assistant", "hello")];
    let conv = lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
}

#[test]
fn unknown_role_defaults_to_user() {
    let msgs = vec![claude_msg("tool", "result")];
    let conv = lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn role_arbitrary_string_defaults_to_user() {
    let msgs = vec![claude_msg("something_else", "data")];
    let conv = lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn ir_assistant_maps_back_to_claude_assistant() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "yes")]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "assistant");
}

#[test]
fn ir_user_maps_back_to_claude_user() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "no")]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "user");
}

#[test]
fn ir_tool_role_maps_to_claude_user() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::Tool, "output")]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "user");
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. ClaudeRequest construction
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn request_from_work_order_basic() {
    let wo = WorkOrderBuilder::new("Summarize this code").build();
    let cfg = ClaudeConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
    assert!(req.messages[0].content.contains("Summarize this code"));
    assert_eq!(req.model, cfg.model);
    assert_eq!(req.max_tokens, cfg.max_tokens);
}

#[test]
fn request_uses_model_from_work_order_config() {
    let wo = WorkOrderBuilder::new("task")
        .model("claude-opus-4-20250514")
        .build();
    let cfg = ClaudeConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "claude-opus-4-20250514");
}

#[test]
fn request_defaults_to_config_model() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = ClaudeConfig {
        model: "claude-haiku-3-5-20241022".into(),
        ..ClaudeConfig::default()
    };
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.model, "claude-haiku-3-5-20241022");
}

#[test]
fn request_includes_system_prompt_from_config() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = ClaudeConfig {
        system_prompt: Some("Be concise".into()),
        ..ClaudeConfig::default()
    };
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.system, Some("Be concise".into()));
}

#[test]
fn request_system_none_when_not_configured() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = ClaudeConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert!(req.system.is_none());
}

#[test]
fn request_includes_thinking_config() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = ClaudeConfig {
        thinking: Some(ThinkingConfig::new(10000)),
        ..ClaudeConfig::default()
    };
    let req = dialect::map_work_order(&wo, &cfg);
    assert!(req.thinking.is_some());
    assert_eq!(req.thinking.unwrap().budget_tokens, 10000);
}

#[test]
fn request_max_tokens_from_config() {
    let wo = WorkOrderBuilder::new("task").build();
    let cfg = ClaudeConfig {
        max_tokens: 8192,
        ..ClaudeConfig::default()
    };
    let req = dialect::map_work_order(&wo, &cfg);
    assert_eq!(req.max_tokens, 8192);
}

#[test]
fn request_serde_roundtrip() {
    let req = ClaudeRequest {
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 4096,
        system: Some("system".into()),
        messages: vec![claude_msg("user", "hi")],
        thinking: Some(ThinkingConfig::new(5000)),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: ClaudeRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.model, req.model);
    assert_eq!(back.max_tokens, req.max_tokens);
    assert_eq!(back.system, req.system);
    assert_eq!(back.messages.len(), 1);
    assert!(back.thinking.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. ClaudeResponse parsing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn response_with_text_block() {
    let resp = make_response(vec![ClaudeContentBlock::Text {
        text: "Hello!".into(),
    }]);
    assert_eq!(resp.content.len(), 1);
    assert_eq!(resp.stop_reason, Some("end_turn".into()));
}

#[test]
fn response_with_usage() {
    let resp = ClaudeResponse {
        id: "msg_1".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![ClaudeContentBlock::Text { text: "ok".into() }],
        stop_reason: Some("end_turn".into()),
        usage: Some(ClaudeUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: Some(10),
            cache_read_input_tokens: Some(5),
        }),
    };
    let u = resp.usage.unwrap();
    assert_eq!(u.input_tokens, 100);
    assert_eq!(u.output_tokens, 50);
    assert_eq!(u.cache_creation_input_tokens, Some(10));
    assert_eq!(u.cache_read_input_tokens, Some(5));
}

#[test]
fn response_serde_roundtrip() {
    let resp = ClaudeResponse {
        id: "msg_42".into(),
        model: "claude-sonnet-4-20250514".into(),
        role: "assistant".into(),
        content: vec![
            ClaudeContentBlock::Text {
                text: "Sure".into(),
            },
            ClaudeContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "read".into(),
                input: json!({"path": "a.rs"}),
            },
        ],
        stop_reason: Some("tool_use".into()),
        usage: Some(ClaudeUsage {
            input_tokens: 200,
            output_tokens: 100,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: ClaudeResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back, resp);
}

#[test]
fn response_multiple_content_blocks() {
    let resp = make_response(vec![
        ClaudeContentBlock::Text {
            text: "I'll do that.".into(),
        },
        ClaudeContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "write_file".into(),
            input: json!({"path": "x.rs", "content": "fn main(){}"}),
        },
    ]);
    assert_eq!(resp.content.len(), 2);
}

#[test]
fn response_with_thinking_block() {
    let resp = make_response(vec![
        ClaudeContentBlock::Thinking {
            thinking: "Let me think...".into(),
            signature: Some("sig_abc".into()),
        },
        ClaudeContentBlock::Text {
            text: "Answer".into(),
        },
    ]);
    assert_eq!(resp.content.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Streaming events
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn stream_message_start_maps_to_run_started() {
    let event = ClaudeStreamEvent::MessageStart {
        message: make_response(vec![]),
    };
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0].kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn stream_message_stop_maps_to_run_completed() {
    let event = ClaudeStreamEvent::MessageStop {};
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn stream_text_delta_maps_to_assistant_delta() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta {
            text: "hello ".into(),
        },
    };
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "hello "),
        other => panic!("expected AssistantDelta, got {other:?}"),
    }
}

#[test]
fn stream_thinking_delta_has_ext() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::ThinkingDelta {
            thinking: "hmm".into(),
        },
    };
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    let ext = events[0].ext.as_ref().unwrap();
    assert_eq!(ext.get("thinking"), Some(&json!(true)));
}

#[test]
fn stream_input_json_delta_produces_no_events() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::InputJsonDelta {
            partial_json: r#"{"pa"#.into(),
        },
    };
    let events = dialect::map_stream_event(&event);
    assert!(events.is_empty());
}

#[test]
fn stream_signature_delta_produces_no_events() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::SignatureDelta {
            signature: "sig_part".into(),
        },
    };
    let events = dialect::map_stream_event(&event);
    assert!(events.is_empty());
}

#[test]
fn stream_content_block_start_tool_use() {
    let event = ClaudeStreamEvent::ContentBlockStart {
        index: 0,
        content_block: ClaudeContentBlock::ToolUse {
            id: "tu_5".into(),
            name: "bash".into(),
            input: json!({}),
        },
    };
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            ..
        } => {
            assert_eq!(tool_name, "bash");
            assert_eq!(tool_use_id.as_deref(), Some("tu_5"));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn stream_content_block_start_text_produces_no_events() {
    let event = ClaudeStreamEvent::ContentBlockStart {
        index: 0,
        content_block: ClaudeContentBlock::Text {
            text: String::new(),
        },
    };
    let events = dialect::map_stream_event(&event);
    assert!(events.is_empty());
}

#[test]
fn stream_content_block_stop_produces_no_events() {
    let event = ClaudeStreamEvent::ContentBlockStop { index: 0 };
    let events = dialect::map_stream_event(&event);
    assert!(events.is_empty());
}

#[test]
fn stream_ping_produces_no_events() {
    let event = ClaudeStreamEvent::Ping {};
    let events = dialect::map_stream_event(&event);
    assert!(events.is_empty());
}

#[test]
fn stream_message_delta_produces_no_events() {
    let event = ClaudeStreamEvent::MessageDelta {
        delta: ClaudeMessageDelta {
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
        },
        usage: None,
    };
    let events = dialect::map_stream_event(&event);
    assert!(events.is_empty());
}

#[test]
fn stream_error_maps_to_error_event() {
    let event = ClaudeStreamEvent::Error {
        error: ClaudeApiError {
            error_type: "overloaded_error".into(),
            message: "Overloaded".into(),
        },
    };
    let events = dialect::map_stream_event(&event);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::Error { message, .. } => {
            assert!(message.contains("overloaded_error"));
            assert!(message.contains("Overloaded"));
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Tool definitions
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_def_canonical_to_claude() {
    let canonical = CanonicalToolDef {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    let claude = dialect::tool_def_to_claude(&canonical);
    assert_eq!(claude.name, "read_file");
    assert_eq!(claude.description, "Read a file");
    assert_eq!(claude.input_schema, canonical.parameters_schema);
}

#[test]
fn tool_def_claude_to_canonical() {
    let claude_def = ClaudeToolDef {
        name: "grep".into(),
        description: "Search files".into(),
        input_schema: json!({"type": "object"}),
    };
    let canonical = dialect::tool_def_from_claude(&claude_def);
    assert_eq!(canonical.name, "grep");
    assert_eq!(canonical.parameters_schema, json!({"type": "object"}));
}

#[test]
fn tool_def_roundtrip() {
    let original = CanonicalToolDef {
        name: "edit".into(),
        description: "Edit a file".into(),
        parameters_schema: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"}
            },
            "required": ["path", "content"]
        }),
    };
    let claude = dialect::tool_def_to_claude(&original);
    let back = dialect::tool_def_from_claude(&claude);
    assert_eq!(back, original);
}

#[test]
fn tool_def_serde_roundtrip() {
    let def = ClaudeToolDef {
        name: "bash".into(),
        description: "Run a command".into(),
        input_schema: json!({"type": "object", "properties": {"command": {"type": "string"}}}),
    };
    let json_str = serde_json::to_string(&def).unwrap();
    let back: ClaudeToolDef = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back, def);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Tool use blocks
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_use_block_to_ir() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_abc".into(),
        name: "write".into(),
        input: json!({"content": "hello"}),
    }];
    let msgs = vec![claude_blocks_msg("assistant", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "tu_abc");
            assert_eq!(name, "write");
            assert_eq!(input, &json!({"content": "hello"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn tool_use_block_from_ir() {
    let ir_msg = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "tu_99".into(),
            name: "read".into(),
            input: json!({"path": "/tmp"}),
        }],
    );
    let conv = IrConversation::from_messages(vec![ir_msg]);
    let back = lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::ToolUse { id, name, .. } => {
            assert_eq!(id, "tu_99");
            assert_eq!(name, "read");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn tool_use_with_complex_input() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_x".into(),
        name: "edit".into(),
        input: json!({
            "path": "src/main.rs",
            "old_str": "fn old() {}",
            "new_str": "fn new() {}"
        }),
    }];
    let msgs = vec![claude_blocks_msg("assistant", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    let back = lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::ToolUse { input, .. } => {
            assert_eq!(input["path"], "src/main.rs");
            assert_eq!(input["old_str"], "fn old() {}");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn tool_use_empty_input() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_e".into(),
        name: "list".into(),
        input: json!({}),
    }];
    let msgs = vec![claude_blocks_msg("assistant", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => assert_eq!(input, &json!({})),
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Tool result blocks
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_result_with_content_to_ir() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_r1".into(),
        content: Some("file data".into()),
        is_error: None,
    }];
    let msgs = vec![claude_blocks_msg("user", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "tu_r1");
            assert_eq!(content.len(), 1);
            assert!(!is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn tool_result_none_content_to_ir() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_empty".into(),
        content: None,
        is_error: None,
    }];
    let msgs = vec![claude_blocks_msg("user", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { content, .. } => assert!(content.is_empty()),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn tool_result_is_error_true() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_err".into(),
        content: Some("not found".into()),
        is_error: Some(true),
    }];
    let msgs = vec![claude_blocks_msg("user", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { is_error, .. } => assert!(is_error),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn tool_result_is_error_false_explicit() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_ok".into(),
        content: Some("ok".into()),
        is_error: Some(false),
    }];
    let msgs = vec![claude_blocks_msg("user", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { is_error, .. } => assert!(!is_error),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn tool_result_roundtrip_preserves_error() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_re".into(),
        content: Some("error: boom".into()),
        is_error: Some(true),
    }];
    let msgs = vec![claude_blocks_msg("user", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    let back = lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::ToolResult {
            is_error,
            content,
            tool_use_id,
        } => {
            assert_eq!(*is_error, Some(true));
            assert_eq!(content.as_deref(), Some("error: boom"));
            assert_eq!(tool_use_id, "tu_re");
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn tool_result_from_ir_no_content_becomes_none() {
    let ir_msg = IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "tu_nc".into(),
            content: vec![],
            is_error: false,
        }],
    );
    let conv = IrConversation::from_messages(vec![ir_msg]);
    let back = lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::ToolResult {
            content, is_error, ..
        } => {
            assert!(content.is_none());
            assert!(is_error.is_none());
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn map_tool_result_helper_success() {
    let msg = dialect::map_tool_result("tu_s", "output text", false);
    assert_eq!(msg.role, "user");
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&msg.content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "tu_s");
            assert_eq!(content.as_deref(), Some("output text"));
            assert!(is_error.is_none());
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn map_tool_result_helper_error() {
    let msg = dialect::map_tool_result("tu_e", "failed", true);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&msg.content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::ToolResult { is_error, .. } => {
            assert_eq!(*is_error, Some(true));
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Claude → IR lowering (all content types)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lower_text_block() {
    let blocks = vec![ClaudeContentBlock::Text {
        text: "response text".into(),
    }];
    let msgs = vec![claude_blocks_msg("assistant", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::Text { text } => assert_eq!(text, "response text"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn lower_thinking_block() {
    let blocks = vec![ClaudeContentBlock::Thinking {
        thinking: "I should consider...".into(),
        signature: Some("sig_123".into()),
    }];
    let msgs = vec![claude_blocks_msg("assistant", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::Thinking { text } => assert_eq!(text, "I should consider..."),
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn lower_thinking_block_no_signature() {
    let blocks = vec![ClaudeContentBlock::Thinking {
        thinking: "reasoning".into(),
        signature: None,
    }];
    let msgs = vec![claude_blocks_msg("assistant", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::Thinking { text } => assert_eq!(text, "reasoning"),
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn lower_image_base64() {
    let blocks = vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Base64 {
            media_type: "image/jpeg".into(),
            data: "base64data==".into(),
        },
    }];
    let msgs = vec![claude_blocks_msg("user", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/jpeg");
            assert_eq!(data, "base64data==");
        }
        other => panic!("expected Image, got {other:?}"),
    }
}

#[test]
fn lower_image_url_becomes_text() {
    let blocks = vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Url {
            url: "https://example.com/img.png".into(),
        },
    }];
    let msgs = vec![claude_blocks_msg("user", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::Text { text } => {
            assert!(text.contains("https://example.com/img.png"));
        }
        other => panic!("expected Text for URL image, got {other:?}"),
    }
}

#[test]
fn lower_plain_text_content() {
    let msgs = vec![claude_msg("user", "just plain text")];
    let conv = lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].text_content(), "just plain text");
}

#[test]
fn lower_invalid_json_treated_as_plain_text() {
    let msgs = vec![claude_msg("assistant", "[not valid json")];
    let conv = lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].text_content(), "[not valid json");
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. IR → Claude lifting
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lift_text_message() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "question")]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].content, "question");
}

#[test]
fn lift_assistant_text() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "answer")]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "assistant");
    assert_eq!(back[0].content, "answer");
}

#[test]
fn lift_tool_use_produces_json() {
    let ir_msg = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "tu_lift".into(),
            name: "search".into(),
            input: json!({"q": "test"}),
        }],
    );
    let conv = IrConversation::from_messages(vec![ir_msg]);
    let back = lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    assert_eq!(parsed.len(), 1);
    assert!(matches!(&parsed[0], ClaudeContentBlock::ToolUse { .. }));
}

#[test]
fn lift_thinking_produces_json() {
    let ir_msg = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "thinking...".into(),
        }],
    );
    let conv = IrConversation::from_messages(vec![ir_msg]);
    let back = lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::Thinking {
            thinking,
            signature,
        } => {
            assert_eq!(thinking, "thinking...");
            assert!(signature.is_none());
        }
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn lift_image_produces_json() {
    let ir_msg = IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "abcd".into(),
        }],
    );
    let conv = IrConversation::from_messages(vec![ir_msg]);
    let back = lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::Image {
            source: ClaudeImageSource::Base64 { media_type, data },
        } => {
            assert_eq!(media_type, "image/png");
            assert_eq!(data, "abcd");
        }
        other => panic!("expected Image Base64, got {other:?}"),
    }
}

#[test]
fn lift_skips_system_messages() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "instructions"),
        IrMessage::text(IrRole::User, "hello"),
        IrMessage::text(IrRole::Assistant, "hi"),
    ]);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[1].role, "assistant");
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Roundtrip: Claude → IR → Claude
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_user_text() {
    let msgs = vec![claude_msg("user", "Hello world")];
    let conv = lowering::to_ir(&msgs, None);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content, "Hello world");
}

#[test]
fn roundtrip_assistant_text() {
    let msgs = vec![claude_msg("assistant", "I'll help you")];
    let conv = lowering::to_ir(&msgs, None);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].role, "assistant");
    assert_eq!(back[0].content, "I'll help you");
}

#[test]
fn roundtrip_tool_use() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_rt".into(),
        name: "glob".into(),
        input: json!({"pattern": "*.rs"}),
    }];
    let msgs = vec![claude_blocks_msg("assistant", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    let back = lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "tu_rt");
            assert_eq!(name, "glob");
            assert_eq!(input, &json!({"pattern": "*.rs"}));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn roundtrip_tool_result_with_content() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_rt2".into(),
        content: Some("data here".into()),
        is_error: None,
    }];
    let msgs = vec![claude_blocks_msg("user", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    let back = lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            assert_eq!(tool_use_id, "tu_rt2");
            assert_eq!(content.as_deref(), Some("data here"));
            assert!(is_error.is_none());
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn roundtrip_thinking_block() {
    let blocks = vec![ClaudeContentBlock::Thinking {
        thinking: "Let me reason step by step".into(),
        signature: None,
    }];
    let msgs = vec![claude_blocks_msg("assistant", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    let back = lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::Thinking { thinking, .. } => {
            assert_eq!(thinking, "Let me reason step by step");
        }
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn roundtrip_image_base64() {
    let blocks = vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Base64 {
            media_type: "image/gif".into(),
            data: "R0lG...".into(),
        },
    }];
    let msgs = vec![claude_blocks_msg("user", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    let back = lowering::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::Image {
            source: ClaudeImageSource::Base64 { media_type, data },
        } => {
            assert_eq!(media_type, "image/gif");
            assert_eq!(data, "R0lG...");
        }
        other => panic!("expected Image Base64, got {other:?}"),
    }
}

#[test]
fn roundtrip_multi_turn_conversation() {
    let msgs = vec![
        claude_msg("user", "Hello"),
        claude_msg("assistant", "Hi there"),
        claude_msg("user", "Do something"),
        claude_msg("assistant", "Done"),
    ];
    let conv = lowering::to_ir(&msgs, None);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 4);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[0].content, "Hello");
    assert_eq!(back[1].role, "assistant");
    assert_eq!(back[1].content, "Hi there");
    assert_eq!(back[2].role, "user");
    assert_eq!(back[2].content, "Do something");
    assert_eq!(back[3].role, "assistant");
    assert_eq!(back[3].content, "Done");
}

#[test]
fn roundtrip_tool_call_then_result_conversation() {
    let tool_use = vec![ClaudeContentBlock::ToolUse {
        id: "tu_conv".into(),
        name: "search".into(),
        input: json!({"query": "test"}),
    }];
    let tool_result = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_conv".into(),
        content: Some("found 3 matches".into()),
        is_error: None,
    }];
    let msgs = vec![
        claude_msg("user", "Find tests"),
        claude_blocks_msg("assistant", &tool_use),
        claude_blocks_msg("user", &tool_result),
        claude_msg("assistant", "I found 3 matches."),
    ];
    let conv = lowering::to_ir(&msgs, None);
    assert_eq!(conv.len(), 4);
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 4);
    assert_eq!(back[0].content, "Find tests");
    assert_eq!(back[3].content, "I found 3 matches.");
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Serde roundtrip for all Claude types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn serde_text_block() {
    let block = ClaudeContentBlock::Text {
        text: "hello".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn serde_tool_use_block() {
    let block = ClaudeContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "read".into(),
        input: json!({"path": "a.rs"}),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn serde_tool_result_block() {
    let block = ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: Some("data".into()),
        is_error: Some(true),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn serde_tool_result_minimal() {
    let block = ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_min".into(),
        content: None,
        is_error: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(!json.contains("content"));
    assert!(!json.contains("is_error"));
    let back: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn serde_thinking_block() {
    let block = ClaudeContentBlock::Thinking {
        thinking: "hmm".into(),
        signature: Some("sig".into()),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn serde_thinking_no_signature() {
    let block = ClaudeContentBlock::Thinking {
        thinking: "reason".into(),
        signature: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(!json.contains("signature"));
    let back: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn serde_image_base64() {
    let block = ClaudeContentBlock::Image {
        source: ClaudeImageSource::Base64 {
            media_type: "image/png".into(),
            data: "abc123".into(),
        },
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn serde_image_url() {
    let block = ClaudeContentBlock::Image {
        source: ClaudeImageSource::Url {
            url: "https://example.com/img.png".into(),
        },
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ClaudeContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn serde_usage() {
    let u = ClaudeUsage {
        input_tokens: 100,
        output_tokens: 200,
        cache_creation_input_tokens: Some(10),
        cache_read_input_tokens: Some(5),
    };
    let json = serde_json::to_string(&u).unwrap();
    let back: ClaudeUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(back, u);
}

#[test]
fn serde_usage_no_cache() {
    let u = ClaudeUsage {
        input_tokens: 50,
        output_tokens: 25,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    };
    let json = serde_json::to_string(&u).unwrap();
    assert!(!json.contains("cache_creation"));
    assert!(!json.contains("cache_read"));
    let back: ClaudeUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(back, u);
}

#[test]
fn serde_stream_message_start() {
    let event = ClaudeStreamEvent::MessageStart {
        message: make_response(vec![]),
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, event);
}

#[test]
fn serde_stream_content_block_start() {
    let event = ClaudeStreamEvent::ContentBlockStart {
        index: 0,
        content_block: ClaudeContentBlock::Text {
            text: String::new(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, event);
}

#[test]
fn serde_stream_text_delta() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta {
            text: "word".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, event);
}

#[test]
fn serde_stream_input_json_delta() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 1,
        delta: ClaudeStreamDelta::InputJsonDelta {
            partial_json: r#"{"path":"#.into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, event);
}

#[test]
fn serde_stream_thinking_delta() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::ThinkingDelta {
            thinking: "consider".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, event);
}

#[test]
fn serde_stream_signature_delta() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::SignatureDelta {
            signature: "sig_data".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, event);
}

#[test]
fn serde_stream_content_block_stop() {
    let event = ClaudeStreamEvent::ContentBlockStop { index: 2 };
    let json = serde_json::to_string(&event).unwrap();
    let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, event);
}

#[test]
fn serde_stream_message_delta() {
    let event = ClaudeStreamEvent::MessageDelta {
        delta: ClaudeMessageDelta {
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
        },
        usage: Some(ClaudeUsage {
            input_tokens: 10,
            output_tokens: 20,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, event);
}

#[test]
fn serde_stream_message_stop() {
    let event = ClaudeStreamEvent::MessageStop {};
    let json = serde_json::to_string(&event).unwrap();
    let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, event);
}

#[test]
fn serde_stream_ping() {
    let event = ClaudeStreamEvent::Ping {};
    let json = serde_json::to_string(&event).unwrap();
    let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, event);
}

#[test]
fn serde_stream_error() {
    let event = ClaudeStreamEvent::Error {
        error: ClaudeApiError {
            error_type: "invalid_request_error".into(),
            message: "Bad request".into(),
        },
    };
    let json = serde_json::to_string(&event).unwrap();
    let back: ClaudeStreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(back, event);
}

#[test]
fn serde_stop_reason_enum() {
    for reason in [
        ClaudeStopReason::EndTurn,
        ClaudeStopReason::ToolUse,
        ClaudeStopReason::MaxTokens,
        ClaudeStopReason::StopSequence,
    ] {
        let json = serde_json::to_string(&reason).unwrap();
        let back: ClaudeStopReason = serde_json::from_str(&json).unwrap();
        assert_eq!(back, reason);
    }
}

#[test]
fn serde_system_block() {
    let block = ClaudeSystemBlock::Text {
        text: "Be helpful".into(),
        cache_control: Some(ClaudeCacheControl::ephemeral()),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: ClaudeSystemBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn serde_system_block_no_cache() {
    let block = ClaudeSystemBlock::Text {
        text: "instructions".into(),
        cache_control: None,
    };
    let json = serde_json::to_string(&block).unwrap();
    assert!(!json.contains("cache_control"));
    let back: ClaudeSystemBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn serde_claude_config() {
    let cfg = ClaudeConfig {
        api_key: "sk-ant-test".into(),
        base_url: "https://api.anthropic.com/v1".into(),
        model: "claude-sonnet-4-20250514".into(),
        max_tokens: 4096,
        system_prompt: Some("Be helpful".into()),
        thinking: Some(ThinkingConfig::new(8000)),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: ClaudeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.api_key, cfg.api_key);
    assert_eq!(back.model, cfg.model);
    assert_eq!(back.max_tokens, cfg.max_tokens);
    assert_eq!(back.system_prompt, cfg.system_prompt);
    assert_eq!(back.thinking.unwrap().budget_tokens, 8000);
}

#[test]
fn serde_thinking_config() {
    let tc = ThinkingConfig::new(16000);
    let json = serde_json::to_string(&tc).unwrap();
    let back: ThinkingConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tc);
    assert_eq!(back.thinking_type, "enabled");
    assert_eq!(back.budget_tokens, 16000);
}

#[test]
fn serde_api_error() {
    let err = ClaudeApiError {
        error_type: "rate_limit_error".into(),
        message: "Too many requests".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: ClaudeApiError = serde_json::from_str(&json).unwrap();
    assert_eq!(back, err);
}

#[test]
fn serde_message_delta() {
    let delta = ClaudeMessageDelta {
        stop_reason: Some("stop_sequence".into()),
        stop_sequence: Some("\n\nHuman:".into()),
    };
    let json = serde_json::to_string(&delta).unwrap();
    let back: ClaudeMessageDelta = serde_json::from_str(&json).unwrap();
    assert_eq!(back, delta);
}

#[test]
fn serde_message_delta_minimal() {
    let delta = ClaudeMessageDelta {
        stop_reason: None,
        stop_sequence: None,
    };
    let json = serde_json::to_string(&delta).unwrap();
    let back: ClaudeMessageDelta = serde_json::from_str(&json).unwrap();
    assert_eq!(back, delta);
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. System prompt handling
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn system_prompt_prepends_ir_system_message() {
    let msgs = vec![claude_msg("user", "question")];
    let conv = lowering::to_ir(&msgs, Some("You are helpful"));
    assert_eq!(conv.len(), 2);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[0].text_content(), "You are helpful");
    assert_eq!(conv.messages[1].role, IrRole::User);
}

#[test]
fn empty_system_prompt_skipped() {
    let msgs = vec![claude_msg("user", "hi")];
    let conv = lowering::to_ir(&msgs, Some(""));
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn none_system_prompt_skipped() {
    let msgs = vec![claude_msg("user", "hi")];
    let conv = lowering::to_ir(&msgs, None);
    assert_eq!(conv.len(), 1);
}

#[test]
fn extract_system_prompt_from_ir() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be concise"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    let sys = lowering::extract_system_prompt(&conv);
    assert_eq!(sys.as_deref(), Some("Be concise"));
}

#[test]
fn extract_system_prompt_none_when_absent() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
    let sys = lowering::extract_system_prompt(&conv);
    assert!(sys.is_none());
}

#[test]
fn system_prompt_roundtrip_via_extract() {
    let msgs = vec![claude_msg("user", "hello")];
    let conv = lowering::to_ir(&msgs, Some("Be helpful and concise"));
    let sys = lowering::extract_system_prompt(&conv);
    assert_eq!(sys.as_deref(), Some("Be helpful and concise"));
    let back = lowering::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Content blocks (mixed, multiple)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mixed_text_and_tool_use() {
    let blocks = vec![
        ClaudeContentBlock::Text {
            text: "I'll read that file.".into(),
        },
        ClaudeContentBlock::ToolUse {
            id: "tu_mix".into(),
            name: "read_file".into(),
            input: json!({"path": "lib.rs"}),
        },
    ];
    let msgs = vec![claude_blocks_msg("assistant", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].content.len(), 2);
    assert!(matches!(
        &conv.messages[0].content[0],
        IrContentBlock::Text { .. }
    ));
    assert!(matches!(
        &conv.messages[0].content[1],
        IrContentBlock::ToolUse { .. }
    ));
}

#[test]
fn multiple_text_blocks() {
    let blocks = vec![
        ClaudeContentBlock::Text {
            text: "Part 1. ".into(),
        },
        ClaudeContentBlock::Text {
            text: "Part 2.".into(),
        },
    ];
    let msgs = vec![claude_blocks_msg("assistant", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].content.len(), 2);
}

#[test]
fn multiple_tool_uses_in_one_message() {
    let blocks = vec![
        ClaudeContentBlock::ToolUse {
            id: "tu_a".into(),
            name: "read".into(),
            input: json!({"path": "a.rs"}),
        },
        ClaudeContentBlock::ToolUse {
            id: "tu_b".into(),
            name: "read".into(),
            input: json!({"path": "b.rs"}),
        },
    ];
    let msgs = vec![claude_blocks_msg("assistant", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    let tool_calls = conv.tool_calls();
    assert_eq!(tool_calls.len(), 2);
}

#[test]
fn thinking_then_text_then_tool_use() {
    let blocks = vec![
        ClaudeContentBlock::Thinking {
            thinking: "Let me plan this...".into(),
            signature: None,
        },
        ClaudeContentBlock::Text {
            text: "I'll edit the file.".into(),
        },
        ClaudeContentBlock::ToolUse {
            id: "tu_complex".into(),
            name: "edit".into(),
            input: json!({"path": "main.rs"}),
        },
    ];
    let msgs = vec![claude_blocks_msg("assistant", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].content.len(), 3);
    assert!(matches!(
        &conv.messages[0].content[0],
        IrContentBlock::Thinking { .. }
    ));
    assert!(matches!(
        &conv.messages[0].content[1],
        IrContentBlock::Text { .. }
    ));
    assert!(matches!(
        &conv.messages[0].content[2],
        IrContentBlock::ToolUse { .. }
    ));
}

#[test]
fn image_and_text_blocks() {
    let blocks = vec![
        ClaudeContentBlock::Text {
            text: "Here's the image:".into(),
        },
        ClaudeContentBlock::Image {
            source: ClaudeImageSource::Base64 {
                media_type: "image/png".into(),
                data: "iVBOR".into(),
            },
        },
    ];
    let msgs = vec![claude_blocks_msg("user", &blocks)];
    let conv = lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].content.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_messages_to_ir() {
    let conv = lowering::to_ir(&[], None);
    assert!(conv.is_empty());
}

#[test]
fn empty_messages_from_ir() {
    let conv = IrConversation::new();
    let back = lowering::from_ir(&conv);
    assert!(back.is_empty());
}

#[test]
fn empty_content_string() {
    let msgs = vec![claude_msg("user", "")];
    let conv = lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].text_content(), "");
}

#[test]
fn whitespace_only_content() {
    let msgs = vec![claude_msg("user", "   ")];
    let conv = lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].text_content(), "   ");
}

#[test]
fn unicode_content_roundtrip() {
    let msgs = vec![claude_msg("user", "こんにちは 🌍 émojis 中文")];
    let conv = lowering::to_ir(&msgs, None);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].content, "こんにちは 🌍 émojis 中文");
}

#[test]
fn newlines_preserved() {
    let msgs = vec![claude_msg("user", "line1\nline2\nline3")];
    let conv = lowering::to_ir(&msgs, None);
    let back = lowering::from_ir(&conv);
    assert_eq!(back[0].content, "line1\nline2\nline3");
}

#[test]
fn very_long_content() {
    let long_text = "a".repeat(100_000);
    let msgs = vec![claude_msg("user", &long_text)];
    let conv = lowering::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].text_content().len(), 100_000);
}

#[test]
fn special_chars_in_content() {
    let msgs = vec![claude_msg("user", r#"{"key": "value", "arr": [1,2,3]}"#)];
    let conv = lowering::to_ir(&msgs, None);
    assert!(
        conv.messages[0]
            .text_content()
            .contains(r#"{"key": "value""#)
    );
}

#[test]
fn single_message_only_system() {
    let conv = lowering::to_ir(&[], Some("Only system"));
    assert_eq!(conv.len(), 1);
    assert_eq!(conv.messages[0].role, IrRole::System);
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Configuration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn default_config_values() {
    let cfg = ClaudeConfig::default();
    assert_eq!(cfg.model, "claude-sonnet-4-20250514");
    assert_eq!(cfg.max_tokens, 4096);
    assert!(cfg.base_url.contains("anthropic.com"));
    assert!(cfg.api_key.is_empty());
    assert!(cfg.system_prompt.is_none());
    assert!(cfg.thinking.is_none());
}

#[test]
fn thinking_config_constructor() {
    let tc = ThinkingConfig::new(10000);
    assert_eq!(tc.thinking_type, "enabled");
    assert_eq!(tc.budget_tokens, 10000);
}

#[test]
fn cache_control_ephemeral() {
    let cc = ClaudeCacheControl::ephemeral();
    assert_eq!(cc.cache_type, "ephemeral");
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. Model name mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn to_canonical_model_prefixes() {
    assert_eq!(
        dialect::to_canonical_model("claude-sonnet-4-20250514"),
        "anthropic/claude-sonnet-4-20250514"
    );
}

#[test]
fn from_canonical_model_strips_prefix() {
    assert_eq!(
        dialect::from_canonical_model("anthropic/claude-sonnet-4-20250514"),
        "claude-sonnet-4-20250514"
    );
}

#[test]
fn from_canonical_model_passthrough_without_prefix() {
    assert_eq!(
        dialect::from_canonical_model("claude-sonnet-4-20250514"),
        "claude-sonnet-4-20250514"
    );
}

#[test]
fn model_canonical_roundtrip() {
    let model = "claude-opus-4-20250514";
    let canonical = dialect::to_canonical_model(model);
    let back = dialect::from_canonical_model(&canonical);
    assert_eq!(back, model);
}

#[test]
fn known_models() {
    assert!(dialect::is_known_model("claude-sonnet-4-20250514"));
    assert!(dialect::is_known_model("claude-opus-4-20250514"));
    assert!(dialect::is_known_model("claude-haiku-3-5-20241022"));
    assert!(dialect::is_known_model("claude-4-latest"));
    assert!(!dialect::is_known_model("gpt-4"));
    assert!(!dialect::is_known_model("unknown-model"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. Stop reason mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_stop_reason_end_turn() {
    assert_eq!(
        dialect::parse_stop_reason("end_turn"),
        Some(ClaudeStopReason::EndTurn)
    );
}

#[test]
fn parse_stop_reason_tool_use() {
    assert_eq!(
        dialect::parse_stop_reason("tool_use"),
        Some(ClaudeStopReason::ToolUse)
    );
}

#[test]
fn parse_stop_reason_max_tokens() {
    assert_eq!(
        dialect::parse_stop_reason("max_tokens"),
        Some(ClaudeStopReason::MaxTokens)
    );
}

#[test]
fn parse_stop_reason_stop_sequence() {
    assert_eq!(
        dialect::parse_stop_reason("stop_sequence"),
        Some(ClaudeStopReason::StopSequence)
    );
}

#[test]
fn parse_stop_reason_unknown() {
    assert_eq!(dialect::parse_stop_reason("something_else"), None);
}

#[test]
fn map_stop_reason_all() {
    assert_eq!(
        dialect::map_stop_reason(ClaudeStopReason::EndTurn),
        "end_turn"
    );
    assert_eq!(
        dialect::map_stop_reason(ClaudeStopReason::ToolUse),
        "tool_use"
    );
    assert_eq!(
        dialect::map_stop_reason(ClaudeStopReason::MaxTokens),
        "max_tokens"
    );
    assert_eq!(
        dialect::map_stop_reason(ClaudeStopReason::StopSequence),
        "stop_sequence"
    );
}

#[test]
fn stop_reason_roundtrip() {
    for reason in [
        ClaudeStopReason::EndTurn,
        ClaudeStopReason::ToolUse,
        ClaudeStopReason::MaxTokens,
        ClaudeStopReason::StopSequence,
    ] {
        let s = dialect::map_stop_reason(reason);
        let back = dialect::parse_stop_reason(s).unwrap();
        assert_eq!(back, reason);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. map_response
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn map_response_text_event() {
    let resp = make_response(vec![ClaudeContentBlock::Text { text: "Hi!".into() }]);
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "Hi!"),
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
}

#[test]
fn map_response_tool_use_event() {
    let resp = make_response(vec![ClaudeContentBlock::ToolUse {
        id: "tu_mr".into(),
        name: "write_file".into(),
        input: json!({"path": "x.rs"}),
    }]);
    let events = dialect::map_response(&resp);
    match &events[0].kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => {
            assert_eq!(tool_name, "write_file");
            assert_eq!(tool_use_id.as_deref(), Some("tu_mr"));
            assert_eq!(input, &json!({"path": "x.rs"}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

#[test]
fn map_response_tool_result_event() {
    let resp = make_response(vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_res".into(),
        content: Some("result data".into()),
        is_error: None,
    }]);
    let events = dialect::map_response(&resp);
    match &events[0].kind {
        AgentEventKind::ToolResult {
            tool_use_id,
            output,
            is_error,
            ..
        } => {
            assert_eq!(tool_use_id.as_deref(), Some("tu_res"));
            assert_eq!(output, &json!("result data"));
            assert!(!is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn map_response_tool_result_error_event() {
    let resp = make_response(vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_err".into(),
        content: Some("error message".into()),
        is_error: Some(true),
    }]);
    let events = dialect::map_response(&resp);
    match &events[0].kind {
        AgentEventKind::ToolResult { is_error, .. } => assert!(is_error),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn map_response_thinking_event_has_ext() {
    let resp = make_response(vec![ClaudeContentBlock::Thinking {
        thinking: "reasoning...".into(),
        signature: Some("sig_abc".into()),
    }]);
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 1);
    match &events[0].kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "reasoning..."),
        other => panic!("expected AssistantMessage, got {other:?}"),
    }
    let ext = events[0].ext.as_ref().unwrap();
    assert_eq!(ext.get("thinking"), Some(&json!(true)));
    assert_eq!(ext.get("signature"), Some(&json!("sig_abc")));
}

#[test]
fn map_response_thinking_no_signature_ext() {
    let resp = make_response(vec![ClaudeContentBlock::Thinking {
        thinking: "hmm".into(),
        signature: None,
    }]);
    let events = dialect::map_response(&resp);
    let ext = events[0].ext.as_ref().unwrap();
    assert_eq!(ext.get("thinking"), Some(&json!(true)));
    assert!(ext.get("signature").is_none());
}

#[test]
fn map_response_image_block_produces_no_event() {
    let resp = make_response(vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Base64 {
            media_type: "image/png".into(),
            data: "xyz".into(),
        },
    }]);
    let events = dialect::map_response(&resp);
    assert!(events.is_empty());
}

#[test]
fn map_response_multiple_blocks() {
    let resp = make_response(vec![
        ClaudeContentBlock::Text {
            text: "here".into(),
        },
        ClaudeContentBlock::ToolUse {
            id: "tu_m".into(),
            name: "search".into(),
            input: json!({}),
        },
    ]);
    let events = dialect::map_response(&resp);
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantMessage { .. }
    ));
    assert!(matches!(&events[1].kind, AgentEventKind::ToolCall { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. Passthrough fidelity
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn passthrough_text_delta_roundtrip() {
    let event = ClaudeStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ClaudeStreamDelta::TextDelta {
            text: "hello".into(),
        },
    };
    let wrapped = dialect::to_passthrough_event(&event);
    let back = dialect::from_passthrough_event(&wrapped);
    assert_eq!(back.as_ref(), Some(&event));
}

#[test]
fn passthrough_message_start_roundtrip() {
    let event = ClaudeStreamEvent::MessageStart {
        message: make_response(vec![]),
    };
    let wrapped = dialect::to_passthrough_event(&event);
    let back = dialect::from_passthrough_event(&wrapped);
    assert_eq!(back.as_ref(), Some(&event));
}

#[test]
fn passthrough_error_roundtrip() {
    let event = ClaudeStreamEvent::Error {
        error: ClaudeApiError {
            error_type: "api_error".into(),
            message: "Internal error".into(),
        },
    };
    let wrapped = dialect::to_passthrough_event(&event);
    let back = dialect::from_passthrough_event(&wrapped);
    assert_eq!(back.as_ref(), Some(&event));
}

#[test]
fn passthrough_has_dialect_marker() {
    let event = ClaudeStreamEvent::Ping {};
    let wrapped = dialect::to_passthrough_event(&event);
    let ext = wrapped.ext.as_ref().unwrap();
    assert_eq!(ext.get("dialect"), Some(&json!("claude")));
}

#[test]
fn passthrough_has_raw_message() {
    let event = ClaudeStreamEvent::Ping {};
    let wrapped = dialect::to_passthrough_event(&event);
    let ext = wrapped.ext.as_ref().unwrap();
    assert!(ext.contains_key("raw_message"));
}

#[test]
fn verify_passthrough_fidelity_all_event_types() {
    let events = vec![
        ClaudeStreamEvent::MessageStart {
            message: make_response(vec![]),
        },
        ClaudeStreamEvent::ContentBlockStart {
            index: 0,
            content_block: ClaudeContentBlock::Text {
                text: String::new(),
            },
        },
        ClaudeStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ClaudeStreamDelta::TextDelta { text: "hi".into() },
        },
        ClaudeStreamEvent::ContentBlockStop { index: 0 },
        ClaudeStreamEvent::MessageDelta {
            delta: ClaudeMessageDelta {
                stop_reason: Some("end_turn".into()),
                stop_sequence: None,
            },
            usage: None,
        },
        ClaudeStreamEvent::MessageStop {},
        ClaudeStreamEvent::Ping {},
    ];
    assert!(dialect::verify_passthrough_fidelity(&events));
}

#[test]
fn from_passthrough_returns_none_for_non_passthrough() {
    use abp_core::AgentEvent;
    use chrono::Utc;

    let event = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext: None,
    };
    assert!(dialect::from_passthrough_event(&event).is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 20. Capability manifest
// ═══════════════════════════════════════════════════════════════════════════

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
fn capability_manifest_has_tool_capabilities() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::ToolRead),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::ToolWrite),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::ToolEdit),
        Some(SupportLevel::Native)
    ));
    assert!(matches!(
        m.get(&Capability::ToolBash),
        Some(SupportLevel::Native)
    ));
}

#[test]
fn capability_manifest_mcp_server_unsupported() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::McpServer),
        Some(SupportLevel::Unsupported)
    ));
}

#[test]
fn capability_manifest_checkpointing_emulated() {
    use abp_core::{Capability, SupportLevel};
    let m = dialect::capability_manifest();
    assert!(matches!(
        m.get(&Capability::Checkpointing),
        Some(SupportLevel::Emulated)
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// 21. Constants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dialect_version_constant() {
    assert_eq!(dialect::DIALECT_VERSION, "claude/v0.1");
}

#[test]
fn default_model_constant() {
    assert_eq!(dialect::DEFAULT_MODEL, "claude-sonnet-4-20250514");
}

#[test]
fn backend_name_constant() {
    assert_eq!(abp_claude_sdk::BACKEND_NAME, "sidecar:claude");
}

// ═══════════════════════════════════════════════════════════════════════════
// 22. Work order with snippets
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn work_order_with_snippets_appended() {
    use abp_core::{ContextPacket, ContextSnippet};
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![ContextSnippet {
            name: "main.rs".into(),
            content: "fn main() {}".into(),
        }],
    };
    let wo = WorkOrderBuilder::new("Review this code")
        .context(ctx)
        .build();
    let cfg = ClaudeConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert!(req.messages[0].content.contains("Review this code"));
    assert!(req.messages[0].content.contains("main.rs"));
    assert!(req.messages[0].content.contains("fn main() {}"));
}

#[test]
fn work_order_with_multiple_snippets() {
    use abp_core::{ContextPacket, ContextSnippet};
    let ctx = ContextPacket {
        files: vec![],
        snippets: vec![
            ContextSnippet {
                name: "a.rs".into(),
                content: "fn a() {}".into(),
            },
            ContextSnippet {
                name: "b.rs".into(),
                content: "fn b() {}".into(),
            },
        ],
    };
    let wo = WorkOrderBuilder::new("Analyze").context(ctx).build();
    let cfg = ClaudeConfig::default();
    let req = dialect::map_work_order(&wo, &cfg);
    assert!(req.messages[0].content.contains("a.rs"));
    assert!(req.messages[0].content.contains("b.rs"));
}
