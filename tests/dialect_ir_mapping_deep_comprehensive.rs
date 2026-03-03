// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Comprehensive dialect-to-IR mapping tests (120+).
//!
//! Covers dialect detection, IR normalization, round-trip fidelity,
//! content-block variants, role mapping, tool definitions, lossy mapping
//! detection, error handling, feature flags, and capability comparison.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_dialect::{Dialect, DialectDetector, DialectValidator};
use abp_mapping::{
    Fidelity, MappingError, MappingMatrix, MappingRegistry, MappingRule, features, known_rules,
    validate_mapping,
};
use serde_json::{Value, json};

// SDK dialect types
use abp_claude_sdk::dialect::ClaudeMessage;
use abp_claude_sdk::lowering as claude_ir;

use abp_codex_sdk::dialect::{
    CodexContentPart, CodexInputItem, CodexResponseItem, CodexUsage, ReasoningSummary,
};
use abp_codex_sdk::lowering as codex_ir;

use abp_copilot_sdk::dialect::{CopilotMessage, CopilotReference, CopilotReferenceType};
use abp_copilot_sdk::lowering as copilot_ir;

use abp_gemini_sdk::dialect::{GeminiContent, GeminiInlineData, GeminiPart};
use abp_gemini_sdk::lowering as gemini_ir;

use abp_kimi_sdk::dialect::{KimiFunctionCall, KimiMessage, KimiToolCall, KimiUsage};
use abp_kimi_sdk::lowering as kimi_ir;

use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};
use abp_openai_sdk::lowering as openai_ir;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn detector() -> DialectDetector {
    DialectDetector::new()
}

fn validator() -> DialectValidator {
    DialectValidator::new()
}

// ═══════════════════════════════════════════════════════════════════════
// §1  Dialect enum variants (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dialect_all_returns_six_variants() {
    let all = Dialect::all();
    assert_eq!(all.len(), 6);
}

#[test]
fn dialect_labels_are_human_readable() {
    assert_eq!(Dialect::OpenAi.label(), "OpenAI");
    assert_eq!(Dialect::Claude.label(), "Claude");
    assert_eq!(Dialect::Gemini.label(), "Gemini");
    assert_eq!(Dialect::Codex.label(), "Codex");
    assert_eq!(Dialect::Kimi.label(), "Kimi");
    assert_eq!(Dialect::Copilot.label(), "Copilot");
}

#[test]
fn dialect_display_matches_label() {
    for &d in Dialect::all() {
        assert_eq!(format!("{d}"), d.label());
    }
}

#[test]
fn dialect_serde_roundtrip_all_variants() {
    for &d in Dialect::all() {
        let json = serde_json::to_string(&d).unwrap();
        let back: Dialect = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
    }
}

#[test]
fn dialect_serde_snake_case_names() {
    assert_eq!(
        serde_json::to_string(&Dialect::OpenAi).unwrap(),
        "\"open_ai\""
    );
    assert_eq!(
        serde_json::to_string(&Dialect::Claude).unwrap(),
        "\"claude\""
    );
    assert_eq!(
        serde_json::to_string(&Dialect::Gemini).unwrap(),
        "\"gemini\""
    );
    assert_eq!(serde_json::to_string(&Dialect::Codex).unwrap(), "\"codex\"");
    assert_eq!(serde_json::to_string(&Dialect::Kimi).unwrap(), "\"kimi\"");
    assert_eq!(
        serde_json::to_string(&Dialect::Copilot).unwrap(),
        "\"copilot\""
    );
}

#[test]
fn dialect_clone_copy_eq_hash() {
    let d = Dialect::Claude;
    let d2 = d;
    assert_eq!(d, d2);
    let mut set = std::collections::HashSet::new();
    set.insert(d);
    assert!(set.contains(&d2));
}

// ═══════════════════════════════════════════════════════════════════════
// §2  Dialect detection from request payloads (18 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detect_openai_chat_request() {
    let v = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hello"}]
    });
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
    assert!(r.confidence > 0.4);
}

#[test]
fn detect_openai_response_with_choices() {
    let v = json!({
        "choices": [{"message": {"role": "assistant", "content": "hi"}}],
        "model": "gpt-4"
    });
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn detect_openai_with_temperature() {
    let v = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "test"}],
        "temperature": 0.7
    });
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::OpenAi);
}

#[test]
fn detect_claude_request_with_type_message() {
    let v = json!({
        "type": "message",
        "model": "claude-3-opus",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
    });
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn detect_claude_response_with_stop_reason() {
    let v = json!({
        "type": "message",
        "stop_reason": "end_turn",
        "content": [{"type": "text", "text": "done"}]
    });
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn detect_claude_messages_with_array_content() {
    let v = json!({
        "model": "claude-3-sonnet",
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hey"}]}]
    });
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Claude);
}

#[test]
fn detect_gemini_contents_with_parts() {
    let v = json!({
        "contents": [{"role": "user", "parts": [{"text": "hello"}]}]
    });
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_gemini_response_with_candidates() {
    let v = json!({
        "candidates": [{"content": {"parts": [{"text": "ok"}]}}]
    });
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_gemini_with_generation_config() {
    let v = json!({
        "contents": [{"parts": [{"text": "test"}]}],
        "generationConfig": {"temperature": 0.5}
    });
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Gemini);
}

#[test]
fn detect_codex_items_with_type_field() {
    let v = json!({
        "items": [{"type": "message", "role": "user", "content": "hello"}],
        "status": "completed"
    });
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Codex);
}

#[test]
fn detect_codex_response_object() {
    let v = json!({
        "object": "response",
        "items": [{"type": "message", "role": "assistant", "content": []}],
        "status": "completed"
    });
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Codex);
}

#[test]
fn detect_kimi_with_refs() {
    let v = json!({
        "refs": [{"url": "https://example.com"}],
        "messages": [{"role": "user", "content": "search for me"}]
    });
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Kimi);
}

#[test]
fn detect_kimi_with_search_plus() {
    let v = json!({
        "search_plus": true,
        "messages": [{"role": "user", "content": "search"}]
    });
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Kimi);
}

#[test]
fn detect_copilot_with_references() {
    let v = json!({
        "references": [{"type": "file", "id": "main.rs"}],
        "messages": [{"role": "user", "content": "explain this"}]
    });
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn detect_copilot_with_agent_mode() {
    let v = json!({
        "agent_mode": true,
        "messages": [{"role": "user", "content": "fix bug"}]
    });
    let r = detector().detect(&v).unwrap();
    assert_eq!(r.dialect, Dialect::Copilot);
}

#[test]
fn detect_returns_none_for_non_object() {
    assert!(detector().detect(&json!("hello")).is_none());
    assert!(detector().detect(&json!(42)).is_none());
    assert!(detector().detect(&json!([])).is_none());
    assert!(detector().detect(&json!(null)).is_none());
}

#[test]
fn detect_returns_none_for_empty_object() {
    assert!(detector().detect(&json!({})).is_none());
}

#[test]
fn detect_all_returns_multiple_matches_sorted() {
    // An OpenAI-ish payload that also has some Claude signals
    let v = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "temperature": 0.5
    });
    let results = detector().detect_all(&v);
    assert!(!results.is_empty());
    // Sorted by descending confidence
    for w in results.windows(2) {
        assert!(w[0].confidence >= w[1].confidence);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §3  IrMessage construction & helpers (12 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ir_message_text_constructor() {
    let msg = IrMessage::text(IrRole::User, "hello");
    assert_eq!(msg.role, IrRole::User);
    assert_eq!(msg.text_content(), "hello");
    assert!(msg.is_text_only());
    assert!(msg.metadata.is_empty());
}

#[test]
fn ir_message_new_with_multiple_blocks() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Here: ".into(),
            },
            IrContentBlock::Text {
                text: "done".into(),
            },
        ],
    );
    assert_eq!(msg.text_content(), "Here: done");
    assert!(msg.is_text_only());
}

#[test]
fn ir_message_is_text_only_false_for_mixed() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text { text: "ok".into() },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "read".into(),
                input: json!({}),
            },
        ],
    );
    assert!(!msg.is_text_only());
}

#[test]
fn ir_message_text_content_ignores_non_text() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text { text: "A".into() },
            IrContentBlock::Thinking { text: "hmm".into() },
            IrContentBlock::Text { text: "B".into() },
        ],
    );
    assert_eq!(msg.text_content(), "AB");
}

#[test]
fn ir_message_tool_use_blocks_extraction() {
    let tool = IrContentBlock::ToolUse {
        id: "t1".into(),
        name: "bash".into(),
        input: json!({"cmd": "ls"}),
    };
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Text { text: "ok".into() }, tool.clone()],
    );
    let tools = msg.tool_use_blocks();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0], &tool);
}

#[test]
fn ir_message_empty_content_is_text_only() {
    let msg = IrMessage::new(IrRole::User, vec![]);
    assert!(msg.is_text_only());
    assert_eq!(msg.text_content(), "");
}

#[test]
fn ir_message_metadata_round_trip() {
    let mut msg = IrMessage::text(IrRole::User, "hi");
    msg.metadata.insert("vendor_id".into(), json!("abc-123"));
    let json_str = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json_str).unwrap();
    assert_eq!(back.metadata["vendor_id"], json!("abc-123"));
}

#[test]
fn ir_message_metadata_skipped_when_empty() {
    let msg = IrMessage::text(IrRole::User, "hi");
    let v: Value = serde_json::to_value(&msg).unwrap();
    assert!(!v.as_object().unwrap().contains_key("metadata"));
}

#[test]
fn ir_message_serde_roundtrip_text() {
    let msg = IrMessage::text(IrRole::System, "Be concise.");
    let json_str = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json_str).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn ir_message_serde_roundtrip_tool_use() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "call_1".into(),
            name: "read_file".into(),
            input: json!({"path": "/tmp/a.txt"}),
        }],
    );
    let json_str = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json_str).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn ir_message_serde_roundtrip_tool_result() {
    let msg = IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "call_1".into(),
            content: vec![IrContentBlock::Text {
                text: "file contents".into(),
            }],
            is_error: false,
        }],
    );
    let json_str = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json_str).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn ir_message_serde_roundtrip_tool_result_error() {
    let msg = IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "call_2".into(),
            content: vec![IrContentBlock::Text {
                text: "permission denied".into(),
            }],
            is_error: true,
        }],
    );
    let back: IrMessage = serde_json::from_value(serde_json::to_value(&msg).unwrap()).unwrap();
    assert_eq!(msg, back);
}

// ═══════════════════════════════════════════════════════════════════════
// §4  IrContentBlock variants (10 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn content_block_text_serde() {
    let block = IrContentBlock::Text {
        text: "hello".into(),
    };
    let v = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "text");
    assert_eq!(v["text"], "hello");
    let back: IrContentBlock = serde_json::from_value(v).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_image_serde() {
    let block = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "aGVsbG8=".into(),
    };
    let v = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "image");
    assert_eq!(v["media_type"], "image/png");
    let back: IrContentBlock = serde_json::from_value(v).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_tool_use_serde() {
    let block = IrContentBlock::ToolUse {
        id: "t1".into(),
        name: "bash".into(),
        input: json!({"command": "ls"}),
    };
    let v = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "tool_use");
    assert_eq!(v["name"], "bash");
    let back: IrContentBlock = serde_json::from_value(v).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_tool_result_serde() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "t1".into(),
        content: vec![IrContentBlock::Text {
            text: "output".into(),
        }],
        is_error: false,
    };
    let v = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "tool_result");
    assert!(!v["is_error"].as_bool().unwrap());
    let back: IrContentBlock = serde_json::from_value(v).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_thinking_serde() {
    let block = IrContentBlock::Thinking {
        text: "Let me think...".into(),
    };
    let v = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "thinking");
    assert_eq!(v["text"], "Let me think...");
    let back: IrContentBlock = serde_json::from_value(v).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_tool_result_nested_blocks() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "t1".into(),
        content: vec![
            IrContentBlock::Text {
                text: "line1".into(),
            },
            IrContentBlock::Image {
                media_type: "image/jpeg".into(),
                data: "abc=".into(),
            },
        ],
        is_error: false,
    };
    let v = serde_json::to_value(&block).unwrap();
    let back: IrContentBlock = serde_json::from_value(v).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_tool_result_empty_content() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "t99".into(),
        content: vec![],
        is_error: false,
    };
    let back: IrContentBlock =
        serde_json::from_value(serde_json::to_value(&block).unwrap()).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_text_empty_string() {
    let block = IrContentBlock::Text { text: "".into() };
    let back: IrContentBlock =
        serde_json::from_value(serde_json::to_value(&block).unwrap()).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_tool_use_complex_input() {
    let block = IrContentBlock::ToolUse {
        id: "c1".into(),
        name: "write_file".into(),
        input: json!({"path": "/a.txt", "content": "line1\nline2", "overwrite": true}),
    };
    let back: IrContentBlock =
        serde_json::from_value(serde_json::to_value(&block).unwrap()).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_thinking_unicode() {
    let block = IrContentBlock::Thinking {
        text: "思考中… 🤔".into(),
    };
    let back: IrContentBlock =
        serde_json::from_value(serde_json::to_value(&block).unwrap()).unwrap();
    assert_eq!(block, back);
}

// ═══════════════════════════════════════════════════════════════════════
// §5  IrRole mapping (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ir_role_serde_snake_case() {
    assert_eq!(
        serde_json::to_string(&IrRole::System).unwrap(),
        "\"system\""
    );
    assert_eq!(serde_json::to_string(&IrRole::User).unwrap(), "\"user\"");
    assert_eq!(
        serde_json::to_string(&IrRole::Assistant).unwrap(),
        "\"assistant\""
    );
    assert_eq!(serde_json::to_string(&IrRole::Tool).unwrap(), "\"tool\"");
}

#[test]
fn ir_role_roundtrip_all() {
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        let s = serde_json::to_string(&role).unwrap();
        let back: IrRole = serde_json::from_str(&s).unwrap();
        assert_eq!(role, back);
    }
}

#[test]
fn ir_role_copy_semantics() {
    let r = IrRole::User;
    let r2 = r;
    assert_eq!(r, r2);
}

#[test]
fn ir_role_hash_distinct() {
    let mut set = std::collections::HashSet::new();
    set.insert(IrRole::System);
    set.insert(IrRole::User);
    set.insert(IrRole::Assistant);
    set.insert(IrRole::Tool);
    assert_eq!(set.len(), 4);
}

#[test]
fn ir_role_debug_format() {
    assert_eq!(format!("{:?}", IrRole::System), "System");
    assert_eq!(format!("{:?}", IrRole::User), "User");
}

#[test]
fn ir_role_deserialization_rejects_unknown() {
    let result = serde_json::from_str::<IrRole>("\"admin\"");
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════
// §6  IrToolDefinition (5 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ir_tool_definition_serde_roundtrip() {
    let tool = IrToolDefinition {
        name: "read_file".into(),
        description: "Reads a file from disk".into(),
        parameters: json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"]
        }),
    };
    let json_str = serde_json::to_string(&tool).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&json_str).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn ir_tool_definition_empty_parameters() {
    let tool = IrToolDefinition {
        name: "noop".into(),
        description: "Does nothing".into(),
        parameters: json!({}),
    };
    let back: IrToolDefinition =
        serde_json::from_value(serde_json::to_value(&tool).unwrap()).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn ir_tool_definition_complex_schema() {
    let tool = IrToolDefinition {
        name: "search".into(),
        description: "Search codebase".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "max_results": {"type": "integer", "default": 10},
                "include_hidden": {"type": "boolean"}
            },
            "required": ["query"]
        }),
    };
    let back: IrToolDefinition =
        serde_json::from_value(serde_json::to_value(&tool).unwrap()).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn ir_tool_definition_clone_eq() {
    let a = IrToolDefinition {
        name: "x".into(),
        description: "y".into(),
        parameters: json!(null),
    };
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn ir_tool_definition_debug_is_readable() {
    let tool = IrToolDefinition {
        name: "test".into(),
        description: "desc".into(),
        parameters: json!({}),
    };
    let debug = format!("{tool:?}");
    assert!(debug.contains("test"));
    assert!(debug.contains("desc"));
}

// ═══════════════════════════════════════════════════════════════════════
// §7  IrConversation (8 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ir_conversation_new_is_empty() {
    let conv = IrConversation::new();
    assert!(conv.is_empty());
    assert_eq!(conv.len(), 0);
}

#[test]
fn ir_conversation_push_chaining() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "sys"))
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::text(IrRole::Assistant, "hello"));
    assert_eq!(conv.len(), 3);
}

#[test]
fn ir_conversation_system_message() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "instructions"))
        .push(IrMessage::text(IrRole::User, "hi"));
    let sys = conv.system_message().unwrap();
    assert_eq!(sys.text_content(), "instructions");
}

#[test]
fn ir_conversation_last_assistant() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "q"))
        .push(IrMessage::text(IrRole::Assistant, "a1"))
        .push(IrMessage::text(IrRole::User, "q2"))
        .push(IrMessage::text(IrRole::Assistant, "a2"));
    assert_eq!(conv.last_assistant().unwrap().text_content(), "a2");
}

#[test]
fn ir_conversation_messages_by_role() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "a"))
        .push(IrMessage::text(IrRole::Assistant, "b"))
        .push(IrMessage::text(IrRole::User, "c"));
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 2);
    assert_eq!(conv.messages_by_role(IrRole::Assistant).len(), 1);
    assert_eq!(conv.messages_by_role(IrRole::System).len(), 0);
}

#[test]
fn ir_conversation_tool_calls_across_messages() {
    let conv = IrConversation::new()
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "bash".into(),
                input: json!({}),
            }],
        ))
        .push(IrMessage::text(IrRole::Tool, "result"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t2".into(),
                name: "read".into(),
                input: json!({}),
            }],
        ));
    assert_eq!(conv.tool_calls().len(), 2);
}

#[test]
fn ir_conversation_last_message() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "first"))
        .push(IrMessage::text(IrRole::Assistant, "last"));
    assert_eq!(conv.last_message().unwrap().text_content(), "last");
}

#[test]
fn ir_conversation_serde_roundtrip() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "sys"))
        .push(IrMessage::text(IrRole::User, "hi"));
    let json_str = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json_str).unwrap();
    assert_eq!(conv, back);
}

// ═══════════════════════════════════════════════════════════════════════
// §8  IrUsage (5 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn ir_usage_from_io() {
    let u = IrUsage::from_io(100, 50);
    assert_eq!(u.input_tokens, 100);
    assert_eq!(u.output_tokens, 50);
    assert_eq!(u.total_tokens, 150);
    assert_eq!(u.cache_read_tokens, 0);
}

#[test]
fn ir_usage_with_cache() {
    let u = IrUsage::with_cache(200, 100, 50, 25);
    assert_eq!(u.total_tokens, 300);
    assert_eq!(u.cache_read_tokens, 50);
    assert_eq!(u.cache_write_tokens, 25);
}

#[test]
fn ir_usage_merge() {
    let a = IrUsage::from_io(10, 20);
    let b = IrUsage::from_io(30, 40);
    let c = a.merge(b);
    assert_eq!(c.input_tokens, 40);
    assert_eq!(c.output_tokens, 60);
    assert_eq!(c.total_tokens, 100);
}

#[test]
fn ir_usage_default_is_zero() {
    let u = IrUsage::default();
    assert_eq!(u.input_tokens, 0);
    assert_eq!(u.output_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

#[test]
fn ir_usage_serde_roundtrip() {
    let u = IrUsage::with_cache(100, 200, 10, 20);
    let back: IrUsage = serde_json::from_value(serde_json::to_value(&u).unwrap()).unwrap();
    assert_eq!(u, back);
}

// ═══════════════════════════════════════════════════════════════════════
// §9  OpenAI dialect → IR → dialect round-trip (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_text_message_to_ir_and_back() {
    let msgs = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("You are helpful.".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("hello".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let ir = openai_ir::to_ir(&msgs);
    assert_eq!(ir.len(), 2);
    assert_eq!(
        ir.system_message().unwrap().text_content(),
        "You are helpful."
    );

    let back = openai_ir::from_ir(&ir);
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[1].content.as_deref(), Some("hello"));
}

#[test]
fn openai_tool_call_to_ir_and_back() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "get_weather".into(),
                arguments: r#"{"city":"NYC"}"#.into(),
            },
        }]),
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&msgs);
    let tools = ir.tool_calls();
    assert_eq!(tools.len(), 1);

    let back = openai_ir::from_ir(&ir);
    assert_eq!(back[0].tool_calls.as_ref().unwrap().len(), 1);
    assert_eq!(
        back[0].tool_calls.as_ref().unwrap()[0].function.name,
        "get_weather"
    );
}

#[test]
fn openai_tool_result_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "tool".into(),
        content: Some("72°F".into()),
        tool_calls: None,
        tool_call_id: Some("call_1".into()),
    }];
    let ir = openai_ir::to_ir(&msgs);
    assert_eq!(ir.messages[0].role, IrRole::Tool);

    let back = openai_ir::from_ir(&ir);
    assert_eq!(back[0].role, "tool");
    assert_eq!(back[0].tool_call_id.as_deref(), Some("call_1"));
}

#[test]
fn openai_empty_messages_to_ir() {
    let ir = openai_ir::to_ir(&[]);
    assert!(ir.is_empty());
}

#[test]
fn openai_assistant_text_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: Some("Sure!".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&msgs);
    let back = openai_ir::from_ir(&ir);
    assert_eq!(back[0].content.as_deref(), Some("Sure!"));
}

#[test]
fn openai_multi_tool_calls_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![
            OpenAIToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "read".into(),
                    arguments: "{}".into(),
                },
            },
            OpenAIToolCall {
                id: "c2".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "write".into(),
                    arguments: "{}".into(),
                },
            },
        ]),
        tool_call_id: None,
    }];
    let ir = openai_ir::to_ir(&msgs);
    assert_eq!(ir.tool_calls().len(), 2);
    let back = openai_ir::from_ir(&ir);
    assert_eq!(back[0].tool_calls.as_ref().unwrap().len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// §10  Claude dialect → IR → dialect round-trip (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn claude_text_message_to_ir_and_back() {
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: "hello world".into(),
    }];
    let ir = claude_ir::to_ir(&msgs, None);
    assert_eq!(ir.len(), 1);
    assert_eq!(ir.messages[0].text_content(), "hello world");

    let back = claude_ir::from_ir(&ir);
    assert_eq!(back[0].role, "user");
}

#[test]
fn claude_system_prompt_to_ir() {
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: "hi".into(),
    }];
    let ir = claude_ir::to_ir(&msgs, Some("Be concise."));
    let sys = ir.system_message().unwrap();
    assert_eq!(sys.text_content(), "Be concise.");
}

#[test]
fn claude_extract_system_prompt() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "hi"));
    let sys = claude_ir::extract_system_prompt(&conv);
    assert_eq!(sys.unwrap(), "You are helpful.");
}

#[test]
fn claude_assistant_message_roundtrip() {
    let msgs = vec![ClaudeMessage {
        role: "assistant".into(),
        content: "I can help.".into(),
    }];
    let ir = claude_ir::to_ir(&msgs, None);
    assert_eq!(ir.messages[0].role, IrRole::Assistant);

    let back = claude_ir::from_ir(&ir);
    assert_eq!(back[0].role, "assistant");
}

#[test]
fn claude_empty_messages_to_ir() {
    let ir = claude_ir::to_ir(&[], None);
    assert!(ir.is_empty());
}

#[test]
fn claude_multi_turn_roundtrip() {
    let msgs = vec![
        ClaudeMessage {
            role: "user".into(),
            content: "q1".into(),
        },
        ClaudeMessage {
            role: "assistant".into(),
            content: "a1".into(),
        },
        ClaudeMessage {
            role: "user".into(),
            content: "q2".into(),
        },
    ];
    let ir = claude_ir::to_ir(&msgs, None);
    assert_eq!(ir.len(), 3);
    let back = claude_ir::from_ir(&ir);
    assert_eq!(back.len(), 3);
    assert_eq!(back[2].content, "q2");
}

// ═══════════════════════════════════════════════════════════════════════
// §11  Gemini dialect → IR → dialect round-trip (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn gemini_text_to_ir_and_back() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("hello".into())],
    }];
    let ir = gemini_ir::to_ir(&contents, None);
    assert_eq!(ir.len(), 1);
    assert_eq!(ir.messages[0].text_content(), "hello");

    let back = gemini_ir::from_ir(&ir);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
}

#[test]
fn gemini_system_instruction_to_ir() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("hi".into())],
    }];
    let sys = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Be brief.".into())],
    };
    let ir = gemini_ir::to_ir(&contents, Some(&sys));
    assert!(ir.system_message().is_some());
}

#[test]
fn gemini_extract_system_instruction() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "System prompt"))
        .push(IrMessage::text(IrRole::User, "hi"));
    let sys = gemini_ir::extract_system_instruction(&conv);
    assert!(sys.is_some());
}

#[test]
fn gemini_function_call_to_ir() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "get_weather".into(),
            args: json!({"city": "London"}),
        }],
    }];
    let ir = gemini_ir::to_ir(&contents, None);
    assert_eq!(ir.tool_calls().len(), 1);
}

#[test]
fn gemini_inline_data_to_ir() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/png".into(),
            data: "iVBOR...".into(),
        })],
    }];
    let ir = gemini_ir::to_ir(&contents, None);
    assert!(!ir.messages[0].is_text_only());
}

#[test]
fn gemini_empty_to_ir() {
    let ir = gemini_ir::to_ir(&[], None);
    assert!(ir.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// §12  Codex dialect → IR → dialect round-trip (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn codex_input_message_to_ir() {
    let items = vec![CodexInputItem::Message {
        role: "user".into(),
        content: "hello codex".into(),
    }];
    let ir = codex_ir::input_to_ir(&items);
    assert_eq!(ir.len(), 1);
    assert_eq!(ir.messages[0].text_content(), "hello codex");
}

#[test]
fn codex_response_message_to_ir_and_back() {
    let items = vec![CodexResponseItem::Message {
        role: "assistant".into(),
        content: vec![CodexContentPart::OutputText { text: "hi".into() }],
    }];
    let ir = codex_ir::to_ir(&items);
    assert_eq!(ir.messages[0].text_content(), "hi");

    let back = codex_ir::from_ir(&ir);
    assert!(!back.is_empty());
}

#[test]
fn codex_function_call_to_ir() {
    let items = vec![CodexResponseItem::FunctionCall {
        id: "fc1".into(),
        call_id: Some("call_1".into()),
        name: "bash".into(),
        arguments: r#"{"cmd": "ls"}"#.into(),
    }];
    let ir = codex_ir::to_ir(&items);
    assert_eq!(ir.tool_calls().len(), 1);
}

#[test]
fn codex_function_call_output_to_ir() {
    let items = vec![CodexResponseItem::FunctionCallOutput {
        call_id: "call_1".into(),
        output: "file.txt".into(),
    }];
    let ir = codex_ir::to_ir(&items);
    assert_eq!(ir.len(), 1);
}

#[test]
fn codex_reasoning_to_ir() {
    let items = vec![CodexResponseItem::Reasoning {
        summary: vec![ReasoningSummary {
            text: "I need to think about this.".into(),
        }],
    }];
    let ir = codex_ir::to_ir(&items);
    assert_eq!(ir.len(), 1);
}

#[test]
fn codex_usage_to_ir() {
    let usage = CodexUsage {
        input_tokens: 50,
        output_tokens: 100,
        total_tokens: 150,
    };
    let ir_usage = codex_ir::usage_to_ir(&usage);
    assert_eq!(ir_usage.input_tokens, 50);
    assert_eq!(ir_usage.output_tokens, 100);
    assert_eq!(ir_usage.total_tokens, 150);
}

// ═══════════════════════════════════════════════════════════════════════
// §13  Kimi dialect → IR → dialect round-trip (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn kimi_text_message_to_ir_and_back() {
    let msgs = vec![KimiMessage {
        role: "user".into(),
        content: Some("hello kimi".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let ir = kimi_ir::to_ir(&msgs);
    assert_eq!(ir.messages[0].text_content(), "hello kimi");

    let back = kimi_ir::from_ir(&ir);
    assert_eq!(back[0].content.as_deref(), Some("hello kimi"));
}

#[test]
fn kimi_tool_call_to_ir_and_back() {
    let msgs = vec![KimiMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![KimiToolCall {
            id: "k1".into(),
            call_type: "function".into(),
            function: KimiFunctionCall {
                name: "search".into(),
                arguments: r#"{"q":"rust"}"#.into(),
            },
        }]),
        tool_call_id: None,
    }];
    let ir = kimi_ir::to_ir(&msgs);
    assert_eq!(ir.tool_calls().len(), 1);

    let back = kimi_ir::from_ir(&ir);
    assert!(back[0].tool_calls.is_some());
}

#[test]
fn kimi_tool_result_roundtrip() {
    let msgs = vec![KimiMessage {
        role: "tool".into(),
        content: Some("result data".into()),
        tool_calls: None,
        tool_call_id: Some("k1".into()),
    }];
    let ir = kimi_ir::to_ir(&msgs);
    assert_eq!(ir.messages[0].role, IrRole::Tool);

    let back = kimi_ir::from_ir(&ir);
    assert_eq!(back[0].role, "tool");
}

#[test]
fn kimi_system_message_roundtrip() {
    let msgs = vec![KimiMessage {
        role: "system".into(),
        content: Some("You are Kimi.".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let ir = kimi_ir::to_ir(&msgs);
    assert_eq!(ir.system_message().unwrap().text_content(), "You are Kimi.");
}

#[test]
fn kimi_empty_to_ir() {
    let ir = kimi_ir::to_ir(&[]);
    assert!(ir.is_empty());
}

#[test]
fn kimi_usage_to_ir() {
    let usage = KimiUsage {
        prompt_tokens: 80,
        completion_tokens: 120,
        total_tokens: 200,
    };
    let ir_usage = kimi_ir::usage_to_ir(&usage);
    assert_eq!(ir_usage.input_tokens, 80);
    assert_eq!(ir_usage.output_tokens, 120);
    assert_eq!(ir_usage.total_tokens, 200);
}

// ═══════════════════════════════════════════════════════════════════════
// §14  Copilot dialect → IR → dialect round-trip (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn copilot_text_message_to_ir_and_back() {
    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "explain this code".into(),
        name: None,
        copilot_references: vec![],
    }];
    let ir = copilot_ir::to_ir(&msgs);
    assert_eq!(ir.messages[0].text_content(), "explain this code");

    let back = copilot_ir::from_ir(&ir);
    assert_eq!(back[0].content, "explain this code");
}

#[test]
fn copilot_with_references_roundtrip() {
    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "check this".into(),
        name: None,
        copilot_references: vec![CopilotReference {
            ref_type: CopilotReferenceType::File,
            id: "main.rs".into(),
            data: json!({"path": "src/main.rs"}),
            metadata: None,
        }],
    }];
    let ir = copilot_ir::to_ir(&msgs);
    assert!(!ir.is_empty());
}

#[test]
fn copilot_assistant_message_roundtrip() {
    let msgs = vec![CopilotMessage {
        role: "assistant".into(),
        content: "Here is the explanation.".into(),
        name: None,
        copilot_references: vec![],
    }];
    let ir = copilot_ir::to_ir(&msgs);
    let back = copilot_ir::from_ir(&ir);
    assert_eq!(back[0].role, "assistant");
}

#[test]
fn copilot_system_message_roundtrip() {
    let msgs = vec![CopilotMessage {
        role: "system".into(),
        content: "You are a coding assistant.".into(),
        name: None,
        copilot_references: vec![],
    }];
    let ir = copilot_ir::to_ir(&msgs);
    assert_eq!(
        ir.system_message().unwrap().text_content(),
        "You are a coding assistant."
    );
}

#[test]
fn copilot_empty_to_ir() {
    let ir = copilot_ir::to_ir(&[]);
    assert!(ir.is_empty());
}

#[test]
fn copilot_extract_references_from_ir() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
    let refs = copilot_ir::extract_references(&conv);
    assert!(refs.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// §15  Cross-dialect IR normalization (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_and_claude_user_text_normalize_to_same_ir() {
    let oai = openai_ir::to_ir(&[OpenAIMessage {
        role: "user".into(),
        content: Some("hello".into()),
        tool_calls: None,
        tool_call_id: None,
    }]);
    let claude = claude_ir::to_ir(
        &[ClaudeMessage {
            role: "user".into(),
            content: "hello".into(),
        }],
        None,
    );
    assert_eq!(oai.messages[0].role, claude.messages[0].role);
    assert_eq!(
        oai.messages[0].text_content(),
        claude.messages[0].text_content()
    );
}

#[test]
fn kimi_and_openai_share_tool_call_structure() {
    let oai = openai_ir::to_ir(&[OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "x".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "test".into(),
                arguments: "{}".into(),
            },
        }]),
        tool_call_id: None,
    }]);
    let kimi = kimi_ir::to_ir(&[KimiMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![KimiToolCall {
            id: "x".into(),
            call_type: "function".into(),
            function: KimiFunctionCall {
                name: "test".into(),
                arguments: "{}".into(),
            },
        }]),
        tool_call_id: None,
    }]);
    assert_eq!(oai.tool_calls().len(), kimi.tool_calls().len());
}

#[test]
fn gemini_and_copilot_user_text_normalize_equivalently() {
    let gem = gemini_ir::to_ir(
        &[GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("same question".into())],
        }],
        None,
    );
    let cop = copilot_ir::to_ir(&[CopilotMessage {
        role: "user".into(),
        content: "same question".into(),
        name: None,
        copilot_references: vec![],
    }]);
    assert_eq!(
        gem.messages[0].text_content(),
        cop.messages[0].text_content()
    );
}

#[test]
fn all_dialects_produce_user_role_for_user() {
    let roles: Vec<IrRole> = vec![
        openai_ir::to_ir(&[OpenAIMessage {
            role: "user".into(),
            content: Some("x".into()),
            tool_calls: None,
            tool_call_id: None,
        }])
        .messages[0]
            .role,
        claude_ir::to_ir(
            &[ClaudeMessage {
                role: "user".into(),
                content: "x".into(),
            }],
            None,
        )
        .messages[0]
            .role,
        gemini_ir::to_ir(
            &[GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("x".into())],
            }],
            None,
        )
        .messages[0]
            .role,
        kimi_ir::to_ir(&[KimiMessage {
            role: "user".into(),
            content: Some("x".into()),
            tool_calls: None,
            tool_call_id: None,
        }])
        .messages[0]
            .role,
        copilot_ir::to_ir(&[CopilotMessage {
            role: "user".into(),
            content: "x".into(),
            name: None,
            copilot_references: vec![],
        }])
        .messages[0]
            .role,
    ];
    for r in &roles {
        assert_eq!(*r, IrRole::User);
    }
}

#[test]
fn all_dialects_produce_assistant_role() {
    let roles: Vec<IrRole> = vec![
        openai_ir::to_ir(&[OpenAIMessage {
            role: "assistant".into(),
            content: Some("x".into()),
            tool_calls: None,
            tool_call_id: None,
        }])
        .messages[0]
            .role,
        claude_ir::to_ir(
            &[ClaudeMessage {
                role: "assistant".into(),
                content: "x".into(),
            }],
            None,
        )
        .messages[0]
            .role,
        kimi_ir::to_ir(&[KimiMessage {
            role: "assistant".into(),
            content: Some("x".into()),
            tool_calls: None,
            tool_call_id: None,
        }])
        .messages[0]
            .role,
        copilot_ir::to_ir(&[CopilotMessage {
            role: "assistant".into(),
            content: "x".into(),
            name: None,
            copilot_references: vec![],
        }])
        .messages[0]
            .role,
    ];
    for r in &roles {
        assert_eq!(*r, IrRole::Assistant);
    }
}

#[test]
fn codex_input_and_openai_normalize_same_user_text() {
    let oai = openai_ir::to_ir(&[OpenAIMessage {
        role: "user".into(),
        content: Some("q".into()),
        tool_calls: None,
        tool_call_id: None,
    }]);
    let codex = codex_ir::input_to_ir(&[CodexInputItem::Message {
        role: "user".into(),
        content: "q".into(),
    }]);
    assert_eq!(
        oai.messages[0].text_content(),
        codex.messages[0].text_content()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// §16  Validation (8 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_openai_valid_request() {
    let v = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let result = validator().validate(&v, Dialect::OpenAi);
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn validate_openai_missing_model() {
    let v = json!({
        "messages": [{"role": "user", "content": "hi"}]
    });
    let result = validator().validate(&v, Dialect::OpenAi);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.path == "/model"));
}

#[test]
fn validate_claude_valid_request() {
    let v = json!({
        "model": "claude-3-opus",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let result = validator().validate(&v, Dialect::Claude);
    assert!(result.valid);
}

#[test]
fn validate_claude_message_type_response_no_model_ok() {
    let v = json!({
        "type": "message",
        "content": [{"type": "text", "text": "hello"}]
    });
    let result = validator().validate(&v, Dialect::Claude);
    assert!(result.valid);
}

#[test]
fn validate_gemini_valid_contents() {
    let v = json!({
        "contents": [{"role": "user", "parts": [{"text": "hi"}]}]
    });
    let result = validator().validate(&v, Dialect::Gemini);
    assert!(result.valid);
}

#[test]
fn validate_gemini_missing_parts() {
    let v = json!({
        "contents": [{"role": "user"}]
    });
    let result = validator().validate(&v, Dialect::Gemini);
    assert!(!result.valid);
}

#[test]
fn validate_non_object_always_fails() {
    let result = validator().validate(&json!("not an object"), Dialect::OpenAi);
    assert!(!result.valid);
    assert!(result.errors[0].path == "/");
}

#[test]
fn validate_codex_items_without_type() {
    let v = json!({
        "items": [{"role": "user", "content": "hi"}]
    });
    let result = validator().validate(&v, Dialect::Codex);
    assert!(!result.valid);
}

// ═══════════════════════════════════════════════════════════════════════
// §17  Feature flags per dialect (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn feature_constants_are_defined() {
    assert_eq!(features::TOOL_USE, "tool_use");
    assert_eq!(features::STREAMING, "streaming");
    assert_eq!(features::THINKING, "thinking");
    assert_eq!(features::IMAGE_INPUT, "image_input");
    assert_eq!(features::CODE_EXEC, "code_exec");
}

#[test]
fn known_rules_registry_not_empty() {
    let reg = known_rules();
    assert!(!reg.is_empty());
}

#[test]
fn known_rules_cover_openai_tool_use() {
    let reg = known_rules();
    let rule = reg.lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE);
    assert!(rule.is_some());
}

#[test]
fn known_rules_cover_claude_thinking() {
    let reg = known_rules();
    let rule = reg.lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING);
    assert!(rule.is_some());
}

#[test]
fn known_rules_cover_gemini_image_input() {
    let reg = known_rules();
    let rule = reg.lookup(Dialect::Gemini, Dialect::Claude, features::IMAGE_INPUT);
    assert!(rule.is_some());
}

#[test]
fn known_rules_cover_all_dialect_pairs_for_tool_use() {
    let reg = known_rules();
    for &src in Dialect::all() {
        for &tgt in Dialect::all() {
            if src != tgt {
                let rule = reg.lookup(src, tgt, features::TOOL_USE);
                assert!(rule.is_some(), "missing tool_use rule: {src} -> {tgt}");
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §18  Lossy mapping detection (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_lossless_check() {
    let f = Fidelity::Lossless;
    assert!(f.is_lossless());
    assert!(!f.is_unsupported());
}

#[test]
fn fidelity_lossy_labeled_check() {
    let f = Fidelity::LossyLabeled {
        warning: "thinking blocks dropped".into(),
    };
    assert!(!f.is_lossless());
    assert!(!f.is_unsupported());
}

#[test]
fn fidelity_unsupported_check() {
    let f = Fidelity::Unsupported {
        reason: "no image support".into(),
    };
    assert!(!f.is_lossless());
    assert!(f.is_unsupported());
}

#[test]
fn thinking_claude_to_openai_is_lossy() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::Claude, Dialect::OpenAi, features::THINKING)
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
}

#[test]
fn tool_use_openai_to_claude_is_lossless() {
    let reg = known_rules();
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, features::TOOL_USE)
        .unwrap();
    assert!(rule.fidelity.is_lossless());
}

#[test]
fn validate_mapping_reports_per_feature() {
    let reg = known_rules();
    let feats = vec![
        features::TOOL_USE.to_string(),
        features::THINKING.to_string(),
    ];
    let results = validate_mapping(&reg, Dialect::Claude, Dialect::OpenAi, &feats);
    assert_eq!(results.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// §19  MappingMatrix and MappingRegistry (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn mapping_matrix_from_registry() {
    let reg = known_rules();
    let matrix = MappingMatrix::from_registry(&reg);
    assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Claude));
}

#[test]
fn mapping_matrix_set_and_get() {
    let mut matrix = MappingMatrix::new();
    matrix.set(Dialect::OpenAi, Dialect::Gemini, true);
    assert!(matrix.is_supported(Dialect::OpenAi, Dialect::Gemini));
    assert!(!matrix.is_supported(Dialect::Gemini, Dialect::OpenAi));
}

#[test]
fn mapping_registry_insert_and_lookup() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "custom_feature".into(),
        fidelity: Fidelity::Lossless,
    });
    let rule = reg.lookup(Dialect::OpenAi, Dialect::Claude, "custom_feature");
    assert!(rule.is_some());
    assert!(rule.unwrap().fidelity.is_lossless());
}

#[test]
fn mapping_registry_lookup_missing_returns_none() {
    let reg = MappingRegistry::new();
    assert!(
        reg.lookup(Dialect::OpenAi, Dialect::Claude, "nonexistent")
            .is_none()
    );
}

#[test]
fn mapping_registry_len_and_iter() {
    let reg = known_rules();
    let count_iter = reg.iter().count();
    assert_eq!(reg.len(), count_iter);
    assert!(reg.len() > 0);
}

#[test]
fn mapping_registry_rank_targets() {
    let reg = known_rules();
    let ranked = reg.rank_targets(Dialect::OpenAi, &[features::TOOL_USE, features::STREAMING]);
    assert!(!ranked.is_empty());
    // Sorted by descending supported feature count
    for w in ranked.windows(2) {
        assert!(w[0].1 >= w[1].1);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §20  Error handling for unmappable requests (5 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn mapping_error_feature_unsupported() {
    let err = MappingError::FeatureUnsupported {
        feature: "exotic_tool".into(),
        from: Dialect::OpenAi,
        to: Dialect::Copilot,
    };
    let debug = format!("{err:?}");
    assert!(debug.contains("exotic_tool"));
}

#[test]
fn mapping_error_fidelity_loss() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "thinking blocks discarded".into(),
    };
    let debug = format!("{err:?}");
    assert!(debug.contains("thinking"));
}

#[test]
fn mapping_error_dialect_mismatch() {
    let err = MappingError::DialectMismatch {
        from: Dialect::OpenAi,
        to: Dialect::Gemini,
    };
    let debug = format!("{err:?}");
    assert!(debug.contains("OpenAi"));
}

#[test]
fn mapping_error_invalid_input() {
    let err = MappingError::InvalidInput {
        reason: "messages array is null".into(),
    };
    let debug = format!("{err:?}");
    assert!(debug.contains("null"));
}

#[test]
fn validate_mapping_with_unsupported_feature() {
    let reg = MappingRegistry::new(); // empty — no rules
    let feats = vec!["nonexistent_feature".to_string()];
    let results = validate_mapping(&reg, Dialect::OpenAi, Dialect::Claude, &feats);
    assert_eq!(results.len(), 1);
    assert!(results[0].fidelity.is_unsupported());
}

// ═══════════════════════════════════════════════════════════════════════
// §21  Dialect capabilities vs IR capabilities (5 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn copilot_reference_types_serialize() {
    let rt = CopilotReferenceType::File;
    let json = serde_json::to_value(&rt).unwrap();
    assert!(json.is_string());
    let back: CopilotReferenceType = serde_json::from_value(json).unwrap();
    assert_eq!(rt, back);
}

#[test]
fn copilot_all_reference_types_roundtrip() {
    for rt in [
        CopilotReferenceType::File,
        CopilotReferenceType::Snippet,
        CopilotReferenceType::Repository,
        CopilotReferenceType::WebSearchResult,
    ] {
        let v = serde_json::to_value(&rt).unwrap();
        let back: CopilotReferenceType = serde_json::from_value(v).unwrap();
        assert_eq!(rt, back);
    }
}

#[test]
fn gemini_function_response_to_ir() {
    let contents = vec![GeminiContent {
        role: "function".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "get_weather".into(),
            response: json!({"temp": 72}),
        }],
    }];
    let ir = gemini_ir::to_ir(&contents, None);
    assert_eq!(ir.len(), 1);
}

#[test]
fn codex_content_part_output_text() {
    let part = CodexContentPart::OutputText {
        text: "result".into(),
    };
    let v = serde_json::to_value(&part).unwrap();
    assert_eq!(v["type"], "output_text");
    assert_eq!(v["text"], "result");
    let back: CodexContentPart = serde_json::from_value(v).unwrap();
    // CodexContentPart doesn't derive PartialEq, check via re-serialization
    let v2 = serde_json::to_value(&back).unwrap();
    assert_eq!(serde_json::to_value(&part).unwrap(), v2);
}

#[test]
fn openai_function_call_serialization() {
    let fc = OpenAIFunctionCall {
        name: "bash".into(),
        arguments: r#"{"cmd":"ls"}"#.into(),
    };
    let v = serde_json::to_value(&fc).unwrap();
    assert_eq!(v["name"], "bash");
    let back: OpenAIFunctionCall = serde_json::from_value(v).unwrap();
    assert_eq!(fc.name, back.name);
    assert_eq!(fc.arguments, back.arguments);
}

// ═══════════════════════════════════════════════════════════════════════
// §22  Detection evidence and confidence (4 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn detection_evidence_is_populated() {
    let v = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}],
        "temperature": 0.5
    });
    let r = detector().detect(&v).unwrap();
    assert!(!r.evidence.is_empty());
}

#[test]
fn detection_confidence_bounded_zero_to_one() {
    let payloads = vec![
        json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]}),
        json!({"type": "message", "model": "claude-3-opus", "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]}),
        json!({"contents": [{"parts": [{"text": "hi"}]}]}),
    ];
    for p in &payloads {
        let r = detector().detect(p).unwrap();
        assert!(r.confidence >= 0.0 && r.confidence <= 1.0);
    }
}

#[test]
fn detect_all_empty_for_non_object() {
    let results = detector().detect_all(&json!(42));
    assert!(results.is_empty());
}

#[test]
fn detect_all_empty_for_empty_object() {
    let results = detector().detect_all(&json!({}));
    assert!(results.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// §23  Validation edge cases (3 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validate_openai_message_missing_role() {
    let v = json!({
        "model": "gpt-4",
        "messages": [{"content": "no role here"}]
    });
    let result = validator().validate(&v, Dialect::OpenAi);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| e.path.contains("role")));
}

#[test]
fn validate_kimi_message_missing_role() {
    let v = json!({
        "messages": [{"content": "no role"}]
    });
    let result = validator().validate(&v, Dialect::Kimi);
    assert!(!result.valid);
}

#[test]
fn validate_copilot_message_missing_role() {
    let v = json!({
        "messages": [{"content": "no role"}]
    });
    let result = validator().validate(&v, Dialect::Copilot);
    assert!(!result.valid);
}
