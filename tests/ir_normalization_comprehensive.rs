#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the IR (Intermediate Representation) crate.
//!
//! Covers: node construction, normalization passes, lowering to each dialect,
//! round-trip fidelity, validation rules, edge cases, serde, and determinism.

use abp_ir::{
    IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage,
    lower::{
        ir_role_to_dialect, lower_for_dialect, lower_to_claude, lower_to_codex, lower_to_copilot,
        lower_to_gemini, lower_to_kimi, lower_to_openai,
    },
    normalize::{
        dedup_system, extract_system, merge_adjacent_text, normalize, normalize_role,
        normalize_tool_schemas, sort_tools, strip_empty, strip_metadata, trim_text,
    },
};
use abp_sdk_types::Dialect;
use serde_json::json;
use std::collections::BTreeMap;

// ═══════════════════════════════════════════════════════════════════════════
// § 1 — IR Node Construction
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_role_variants_exist() {
    let _ = IrRole::System;
    let _ = IrRole::User;
    let _ = IrRole::Assistant;
    let _ = IrRole::Tool;
}

#[test]
fn ir_role_copy_semantics() {
    let r = IrRole::User;
    let r2 = r;
    assert_eq!(r, r2);
}

#[test]
fn ir_content_block_text() {
    let b = IrContentBlock::Text {
        text: "hello".into(),
    };
    if let IrContentBlock::Text { text } = &b {
        assert_eq!(text, "hello");
    } else {
        panic!("expected Text");
    }
}

#[test]
fn ir_content_block_image() {
    let b = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "iVBORw0KGgo=".into(),
    };
    if let IrContentBlock::Image { media_type, data } = &b {
        assert_eq!(media_type, "image/png");
        assert_eq!(data, "iVBORw0KGgo=");
    } else {
        panic!("expected Image");
    }
}

#[test]
fn ir_content_block_tool_use() {
    let b = IrContentBlock::ToolUse {
        id: "call_1".into(),
        name: "read_file".into(),
        input: json!({"path": "/tmp/a.rs"}),
    };
    if let IrContentBlock::ToolUse { id, name, input } = &b {
        assert_eq!(id, "call_1");
        assert_eq!(name, "read_file");
        assert_eq!(input["path"], "/tmp/a.rs");
    } else {
        panic!("expected ToolUse");
    }
}

#[test]
fn ir_content_block_tool_result() {
    let b = IrContentBlock::ToolResult {
        tool_use_id: "call_1".into(),
        content: vec![IrContentBlock::Text {
            text: "output".into(),
        }],
        is_error: false,
    };
    if let IrContentBlock::ToolResult {
        tool_use_id,
        content,
        is_error,
    } = &b
    {
        assert_eq!(tool_use_id, "call_1");
        assert_eq!(content.len(), 1);
        assert!(!is_error);
    } else {
        panic!("expected ToolResult");
    }
}

#[test]
fn ir_content_block_tool_result_error() {
    let b = IrContentBlock::ToolResult {
        tool_use_id: "call_2".into(),
        content: vec![IrContentBlock::Text {
            text: "not found".into(),
        }],
        is_error: true,
    };
    if let IrContentBlock::ToolResult { is_error, .. } = &b {
        assert!(is_error);
    }
}

#[test]
fn ir_content_block_thinking() {
    let b = IrContentBlock::Thinking {
        text: "hmm...".into(),
    };
    if let IrContentBlock::Thinking { text } = &b {
        assert_eq!(text, "hmm...");
    } else {
        panic!("expected Thinking");
    }
}

#[test]
fn ir_message_new() {
    let msg = IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Text { text: "hi".into() }],
    );
    assert_eq!(msg.role, IrRole::User);
    assert_eq!(msg.content.len(), 1);
    assert!(msg.metadata.is_empty());
}

#[test]
fn ir_message_text_shorthand() {
    let msg = IrMessage::text(IrRole::Assistant, "hello");
    assert_eq!(msg.role, IrRole::Assistant);
    assert_eq!(msg.text_content(), "hello");
}

#[test]
fn ir_message_is_text_only_true() {
    let msg = IrMessage::text(IrRole::User, "just text");
    assert!(msg.is_text_only());
}

#[test]
fn ir_message_is_text_only_false_with_tool_use() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "thinking".into(),
            },
            IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "search".into(),
                input: json!({}),
            },
        ],
    );
    assert!(!msg.is_text_only());
}

#[test]
fn ir_message_text_content_concatenates() {
    let msg = IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "hello ".into(),
            },
            IrContentBlock::Text {
                text: "world".into(),
            },
        ],
    );
    assert_eq!(msg.text_content(), "hello world");
}

#[test]
fn ir_message_text_content_skips_non_text() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "result: ".into(),
            },
            IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "search".into(),
                input: json!({}),
            },
            IrContentBlock::Text {
                text: "done".into(),
            },
        ],
    );
    assert_eq!(msg.text_content(), "result: done");
}

#[test]
fn ir_message_tool_use_blocks() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "let me".into(),
            },
            IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "read".into(),
                input: json!({}),
            },
            IrContentBlock::ToolUse {
                id: "c2".into(),
                name: "write".into(),
                input: json!({}),
            },
        ],
    );
    assert_eq!(msg.tool_use_blocks().len(), 2);
}

#[test]
fn ir_message_no_tool_use_blocks() {
    let msg = IrMessage::text(IrRole::User, "hi");
    assert!(msg.tool_use_blocks().is_empty());
}

#[test]
fn ir_message_metadata_is_btreemap() {
    let mut meta = BTreeMap::new();
    meta.insert("source".into(), json!("test"));
    meta.insert("aaa".into(), json!(1));
    let msg = IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text { text: "hi".into() }],
        metadata: meta,
    };
    // BTreeMap iteration order is sorted
    let keys: Vec<_> = msg.metadata.keys().collect();
    assert_eq!(keys, vec!["aaa", "source"]);
}

// ═══════════════════════════════════════════════════════════════════════════
// § 2 — IrConversation Construction & Accessors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn conversation_new_is_empty() {
    let conv = IrConversation::new();
    assert!(conv.is_empty());
    assert_eq!(conv.len(), 0);
}

#[test]
fn conversation_push_chaining() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::text(IrRole::Assistant, "hello"));
    assert_eq!(conv.len(), 2);
}

#[test]
fn conversation_from_messages() {
    let msgs = vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hi"),
    ];
    let conv = IrConversation::from_messages(msgs);
    assert_eq!(conv.len(), 2);
}

#[test]
fn conversation_system_message() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "be nice"))
        .push(IrMessage::text(IrRole::User, "hi"));
    let sys = conv.system_message().unwrap();
    assert_eq!(sys.text_content(), "be nice");
}

#[test]
fn conversation_system_message_none() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
    assert!(conv.system_message().is_none());
}

#[test]
fn conversation_last_assistant() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::Assistant, "first"))
        .push(IrMessage::text(IrRole::User, "q"))
        .push(IrMessage::text(IrRole::Assistant, "second"));
    assert_eq!(conv.last_assistant().unwrap().text_content(), "second");
}

#[test]
fn conversation_last_assistant_none() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
    assert!(conv.last_assistant().is_none());
}

#[test]
fn conversation_tool_calls() {
    let conv = IrConversation::new()
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "read".into(),
                input: json!({}),
            }],
        ))
        .push(IrMessage::text(IrRole::User, "q"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c2".into(),
                name: "write".into(),
                input: json!({}),
            }],
        ));
    assert_eq!(conv.tool_calls().len(), 2);
}

#[test]
fn conversation_messages_by_role() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "a"))
        .push(IrMessage::text(IrRole::Assistant, "b"))
        .push(IrMessage::text(IrRole::User, "c"));
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 2);
    assert_eq!(conv.messages_by_role(IrRole::Assistant).len(), 1);
    assert_eq!(conv.messages_by_role(IrRole::System).len(), 0);
}

#[test]
fn conversation_last_message() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "first"))
        .push(IrMessage::text(IrRole::Assistant, "last"));
    assert_eq!(conv.last_message().unwrap().text_content(), "last");
}

#[test]
fn conversation_last_message_empty() {
    let conv = IrConversation::new();
    assert!(conv.last_message().is_none());
}

#[test]
fn conversation_default_is_empty() {
    let conv = IrConversation::default();
    assert!(conv.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// § 3 — IrToolDefinition
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_definition_basic() {
    let tool = IrToolDefinition {
        name: "search".into(),
        description: "Search the web".into(),
        parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
    };
    assert_eq!(tool.name, "search");
    assert_eq!(tool.description, "Search the web");
}

#[test]
fn tool_definition_equality() {
    let a = IrToolDefinition {
        name: "f".into(),
        description: "d".into(),
        parameters: json!({}),
    };
    let b = a.clone();
    assert_eq!(a, b);
}

// ═══════════════════════════════════════════════════════════════════════════
// § 4 — IrUsage
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn usage_from_io() {
    let u = IrUsage::from_io(100, 50);
    assert_eq!(u.input_tokens, 100);
    assert_eq!(u.output_tokens, 50);
    assert_eq!(u.total_tokens, 150);
    assert_eq!(u.cache_read_tokens, 0);
    assert_eq!(u.cache_write_tokens, 0);
}

#[test]
fn usage_with_cache() {
    let u = IrUsage::with_cache(100, 50, 20, 10);
    assert_eq!(u.total_tokens, 150);
    assert_eq!(u.cache_read_tokens, 20);
    assert_eq!(u.cache_write_tokens, 10);
}

#[test]
fn usage_merge() {
    let a = IrUsage::from_io(100, 50);
    let b = IrUsage::with_cache(200, 100, 30, 20);
    let merged = a.merge(b);
    assert_eq!(merged.input_tokens, 300);
    assert_eq!(merged.output_tokens, 150);
    assert_eq!(merged.total_tokens, 450);
    assert_eq!(merged.cache_read_tokens, 30);
    assert_eq!(merged.cache_write_tokens, 20);
}

#[test]
fn usage_default_is_zero() {
    let u = IrUsage::default();
    assert_eq!(u.input_tokens, 0);
    assert_eq!(u.output_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

#[test]
fn usage_merge_identity() {
    let a = IrUsage::from_io(10, 20);
    let zero = IrUsage::default();
    assert_eq!(a.merge(zero), a);
}

#[test]
fn usage_copy_semantics() {
    let a = IrUsage::from_io(1, 2);
    let b = a;
    assert_eq!(a, b);
}

// ═══════════════════════════════════════════════════════════════════════════
// § 5 — Normalization: normalize_role
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn normalize_role_system() {
    assert_eq!(normalize_role("system"), Some(IrRole::System));
}

#[test]
fn normalize_role_developer_maps_to_system() {
    assert_eq!(normalize_role("developer"), Some(IrRole::System));
}

#[test]
fn normalize_role_user() {
    assert_eq!(normalize_role("user"), Some(IrRole::User));
}

#[test]
fn normalize_role_human_maps_to_user() {
    assert_eq!(normalize_role("human"), Some(IrRole::User));
}

#[test]
fn normalize_role_assistant() {
    assert_eq!(normalize_role("assistant"), Some(IrRole::Assistant));
}

#[test]
fn normalize_role_model_maps_to_assistant() {
    assert_eq!(normalize_role("model"), Some(IrRole::Assistant));
}

#[test]
fn normalize_role_bot_maps_to_assistant() {
    assert_eq!(normalize_role("bot"), Some(IrRole::Assistant));
}

#[test]
fn normalize_role_tool() {
    assert_eq!(normalize_role("tool"), Some(IrRole::Tool));
}

#[test]
fn normalize_role_function_maps_to_tool() {
    assert_eq!(normalize_role("function"), Some(IrRole::Tool));
}

#[test]
fn normalize_role_unknown_returns_none() {
    assert_eq!(normalize_role("narrator"), None);
    assert_eq!(normalize_role(""), None);
    assert_eq!(normalize_role("SYSTEM"), None);
    assert_eq!(normalize_role("User"), None);
}

// ═══════════════════════════════════════════════════════════════════════════
// § 6 — Normalization: dedup_system
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dedup_system_merges_multiple() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "First"))
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::text(IrRole::System, "Second"));
    let deduped = dedup_system(&conv);
    assert_eq!(deduped.messages_by_role(IrRole::System).len(), 1);
    assert_eq!(
        deduped.messages_by_role(IrRole::System)[0].text_content(),
        "First\nSecond"
    );
}

#[test]
fn dedup_system_preserves_order_of_non_system() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "sys"))
        .push(IrMessage::text(IrRole::User, "a"))
        .push(IrMessage::text(IrRole::Assistant, "b"))
        .push(IrMessage::text(IrRole::User, "c"));
    let deduped = dedup_system(&conv);
    assert_eq!(deduped.len(), 4); // 1 system + 3 non-system
    assert_eq!(deduped.messages[0].role, IrRole::System);
    assert_eq!(deduped.messages[1].text_content(), "a");
    assert_eq!(deduped.messages[2].text_content(), "b");
    assert_eq!(deduped.messages[3].text_content(), "c");
}

#[test]
fn dedup_system_no_system_messages() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::text(IrRole::Assistant, "hello"));
    let deduped = dedup_system(&conv);
    assert_eq!(deduped.len(), 2);
    assert!(deduped.messages_by_role(IrRole::System).is_empty());
}

#[test]
fn dedup_system_single_system_message_unchanged() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "sys"))
        .push(IrMessage::text(IrRole::User, "hi"));
    let deduped = dedup_system(&conv);
    assert_eq!(deduped.messages_by_role(IrRole::System).len(), 1);
    assert_eq!(
        deduped.messages_by_role(IrRole::System)[0].text_content(),
        "sys"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// § 7 — Normalization: trim_text
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn trim_text_leading_trailing_whitespace() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "  hello world  "));
    let trimmed = trim_text(&conv);
    assert_eq!(trimmed.messages[0].text_content(), "hello world");
}

#[test]
fn trim_text_leaves_non_text_untouched() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "  hi  ".into(),
            },
            IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "search".into(),
                input: json!({}),
            },
        ],
    ));
    let trimmed = trim_text(&conv);
    assert_eq!(trimmed.messages[0].content.len(), 2);
    if let IrContentBlock::Text { text } = &trimmed.messages[0].content[0] {
        assert_eq!(text, "hi");
    }
}

#[test]
fn trim_text_preserves_metadata() {
    let mut meta = BTreeMap::new();
    meta.insert("key".into(), json!("val"));
    let msg = IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text {
            text: "  hi  ".into(),
        }],
        metadata: meta,
    };
    let conv = IrConversation::from_messages(vec![msg]);
    let trimmed = trim_text(&conv);
    assert!(trimmed.messages[0].metadata.contains_key("key"));
}

// ═══════════════════════════════════════════════════════════════════════════
// § 8 — Normalization: strip_empty
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn strip_empty_removes_empty_messages() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::new(IrRole::Assistant, vec![]))
        .push(IrMessage::text(IrRole::User, "there"));
    let stripped = strip_empty(&conv);
    assert_eq!(stripped.len(), 2);
}

#[test]
fn strip_empty_keeps_non_empty() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::text(IrRole::Assistant, "hello"));
    let stripped = strip_empty(&conv);
    assert_eq!(stripped.len(), 2);
}

#[test]
fn strip_empty_all_empty_yields_empty_conversation() {
    let conv = IrConversation::new()
        .push(IrMessage::new(IrRole::User, vec![]))
        .push(IrMessage::new(IrRole::Assistant, vec![]));
    let stripped = strip_empty(&conv);
    assert!(stripped.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// § 9 — Normalization: merge_adjacent_text
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn merge_adjacent_text_combines_consecutive() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "hello ".into(),
            },
            IrContentBlock::Text {
                text: "world".into(),
            },
        ],
    ));
    let merged = merge_adjacent_text(&conv);
    assert_eq!(merged.messages[0].content.len(), 1);
    assert_eq!(merged.messages[0].text_content(), "hello world");
}

#[test]
fn merge_adjacent_text_does_not_merge_across_non_text() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "before".into(),
            },
            IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "f".into(),
                input: json!({}),
            },
            IrContentBlock::Text {
                text: "after".into(),
            },
        ],
    ));
    let merged = merge_adjacent_text(&conv);
    assert_eq!(merged.messages[0].content.len(), 3);
}

#[test]
fn merge_adjacent_text_three_consecutive() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text { text: "a".into() },
            IrContentBlock::Text { text: "b".into() },
            IrContentBlock::Text { text: "c".into() },
        ],
    ));
    let merged = merge_adjacent_text(&conv);
    assert_eq!(merged.messages[0].content.len(), 1);
    assert_eq!(merged.messages[0].text_content(), "abc");
}

#[test]
fn merge_adjacent_text_single_block_unchanged() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "single"));
    let merged = merge_adjacent_text(&conv);
    assert_eq!(merged.messages[0].content.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// § 10 — Normalization: strip_metadata
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn strip_metadata_empty_keep_removes_all() {
    let mut meta = BTreeMap::new();
    meta.insert("vendor".into(), json!("openai"));
    meta.insert("trace_id".into(), json!("123"));
    let msg = IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text { text: "hi".into() }],
        metadata: meta,
    };
    let conv = IrConversation::from_messages(vec![msg]);
    let stripped = strip_metadata(&conv, &[]);
    assert!(stripped.messages[0].metadata.is_empty());
}

#[test]
fn strip_metadata_keeps_specified_keys() {
    let mut meta = BTreeMap::new();
    meta.insert("source".into(), json!("test"));
    meta.insert("vendor_id".into(), json!("abc"));
    meta.insert("trace".into(), json!("xyz"));
    let msg = IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text { text: "hi".into() }],
        metadata: meta,
    };
    let conv = IrConversation::from_messages(vec![msg]);
    let stripped = strip_metadata(&conv, &["source", "trace"]);
    assert_eq!(stripped.messages[0].metadata.len(), 2);
    assert!(stripped.messages[0].metadata.contains_key("source"));
    assert!(stripped.messages[0].metadata.contains_key("trace"));
    assert!(!stripped.messages[0].metadata.contains_key("vendor_id"));
}

#[test]
fn strip_metadata_no_metadata_is_noop() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
    let stripped = strip_metadata(&conv, &[]);
    assert!(stripped.messages[0].metadata.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// § 11 — Normalization: extract_system
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn extract_system_merges_all_system_messages() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Rule 1"))
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::text(IrRole::System, "Rule 2"));
    let (sys, rest) = extract_system(&conv);
    assert_eq!(sys.unwrap(), "Rule 1\nRule 2");
    assert_eq!(rest.len(), 1);
    assert!(rest.messages.iter().all(|m| m.role != IrRole::System));
}

#[test]
fn extract_system_none_when_no_system() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
    let (sys, _) = extract_system(&conv);
    assert!(sys.is_none());
}

#[test]
fn extract_system_skips_empty_system_messages() {
    let conv = IrConversation::new()
        .push(IrMessage::new(IrRole::System, vec![]))
        .push(IrMessage::text(IrRole::User, "hi"));
    let (sys, _) = extract_system(&conv);
    assert!(sys.is_none());
}

#[test]
fn extract_system_preserves_non_system_order() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "sys"))
        .push(IrMessage::text(IrRole::User, "first"))
        .push(IrMessage::text(IrRole::Assistant, "second"))
        .push(IrMessage::text(IrRole::User, "third"));
    let (_, rest) = extract_system(&conv);
    assert_eq!(rest.len(), 3);
    assert_eq!(rest.messages[0].text_content(), "first");
    assert_eq!(rest.messages[1].text_content(), "second");
    assert_eq!(rest.messages[2].text_content(), "third");
}

// ═══════════════════════════════════════════════════════════════════════════
// § 12 — Normalization: sort_tools & normalize_tool_schemas
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sort_tools_alphabetical() {
    let mut tools = vec![
        IrToolDefinition {
            name: "zebra".into(),
            description: "z".into(),
            parameters: json!({}),
        },
        IrToolDefinition {
            name: "apple".into(),
            description: "a".into(),
            parameters: json!({}),
        },
        IrToolDefinition {
            name: "mango".into(),
            description: "m".into(),
            parameters: json!({}),
        },
    ];
    sort_tools(&mut tools);
    assert_eq!(tools[0].name, "apple");
    assert_eq!(tools[1].name, "mango");
    assert_eq!(tools[2].name, "zebra");
}

#[test]
fn sort_tools_already_sorted() {
    let mut tools = vec![
        IrToolDefinition {
            name: "a".into(),
            description: "".into(),
            parameters: json!({}),
        },
        IrToolDefinition {
            name: "b".into(),
            description: "".into(),
            parameters: json!({}),
        },
    ];
    sort_tools(&mut tools);
    assert_eq!(tools[0].name, "a");
}

#[test]
fn sort_tools_empty() {
    let mut tools: Vec<IrToolDefinition> = vec![];
    sort_tools(&mut tools);
    assert!(tools.is_empty());
}

#[test]
fn normalize_tool_schemas_adds_missing_type() {
    let tools = vec![IrToolDefinition {
        name: "f".into(),
        description: "d".into(),
        parameters: json!({"properties": {"q": {"type": "string"}}}),
    }];
    let normalized = normalize_tool_schemas(&tools);
    assert_eq!(normalized[0].parameters["type"], "object");
}

#[test]
fn normalize_tool_schemas_preserves_existing_type() {
    let tools = vec![IrToolDefinition {
        name: "f".into(),
        description: "d".into(),
        parameters: json!({"type": "object", "properties": {}}),
    }];
    let normalized = normalize_tool_schemas(&tools);
    assert_eq!(normalized[0].parameters["type"], "object");
}

#[test]
fn normalize_tool_schemas_preserves_properties() {
    let tools = vec![IrToolDefinition {
        name: "f".into(),
        description: "d".into(),
        parameters: json!({"properties": {"q": {"type": "string"}}, "required": ["q"]}),
    }];
    let normalized = normalize_tool_schemas(&tools);
    assert_eq!(
        normalized[0].parameters["properties"]["q"]["type"],
        "string"
    );
    assert!(normalized[0].parameters["required"].is_array());
}

// ═══════════════════════════════════════════════════════════════════════════
// § 13 — Full normalization pipeline
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn full_pipeline_dedup_trim_merge_strip() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "  sys1  "))
        .push(IrMessage::text(IrRole::User, "  hello  "))
        .push(IrMessage::text(IrRole::System, "  sys2  "))
        .push(IrMessage::new(IrRole::Assistant, vec![]));
    let result = normalize(&conv);
    // dedup joins system texts with \n before trim; trim only strips outer whitespace
    assert_eq!(result.messages_by_role(IrRole::System).len(), 1);
    assert_eq!(
        result.messages_by_role(IrRole::System)[0].text_content(),
        "sys1  \n  sys2"
    );
    assert_eq!(
        result.messages_by_role(IrRole::User)[0].text_content(),
        "hello"
    );
    // Empty assistant should be stripped
    assert_eq!(result.messages_by_role(IrRole::Assistant).len(), 0);
}

#[test]
fn full_pipeline_idempotent() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "  a  "))
        .push(IrMessage::text(IrRole::User, " b "))
        .push(IrMessage::text(IrRole::System, " c "));
    let once = normalize(&conv);
    let twice = normalize(&once);
    assert_eq!(once, twice);
}

#[test]
fn full_pipeline_merges_adjacent_text() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "hello ".into(),
            },
            IrContentBlock::Text {
                text: "world".into(),
            },
        ],
    ));
    // Pipeline: dedup → trim → merge → strip_empty
    // trim happens before merge, so "hello " → "hello" and "world" → "world", then merged.
    let result = normalize(&conv);
    assert_eq!(result.messages[0].content.len(), 1);
    assert_eq!(result.messages[0].text_content(), "helloworld");
}

#[test]
fn full_pipeline_empty_conversation() {
    let conv = IrConversation::new();
    let result = normalize(&conv);
    assert!(result.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// § 14 — Lowering: Role mapping
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_role_mapping() {
    assert_eq!(
        ir_role_to_dialect(IrRole::System, Dialect::OpenAi),
        "system"
    );
    assert_eq!(ir_role_to_dialect(IrRole::User, Dialect::OpenAi), "user");
    assert_eq!(
        ir_role_to_dialect(IrRole::Assistant, Dialect::OpenAi),
        "assistant"
    );
    assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::OpenAi), "tool");
}

#[test]
fn claude_role_mapping() {
    assert_eq!(
        ir_role_to_dialect(IrRole::System, Dialect::Claude),
        "system"
    );
    assert_eq!(ir_role_to_dialect(IrRole::User, Dialect::Claude), "user");
    assert_eq!(
        ir_role_to_dialect(IrRole::Assistant, Dialect::Claude),
        "assistant"
    );
    assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::Claude), "user");
}

#[test]
fn gemini_role_mapping() {
    assert_eq!(
        ir_role_to_dialect(IrRole::System, Dialect::Gemini),
        "system"
    );
    assert_eq!(ir_role_to_dialect(IrRole::User, Dialect::Gemini), "user");
    assert_eq!(
        ir_role_to_dialect(IrRole::Assistant, Dialect::Gemini),
        "model"
    );
    assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::Gemini), "user");
}

#[test]
fn kimi_matches_openai_roles() {
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        assert_eq!(
            ir_role_to_dialect(role, Dialect::Kimi),
            ir_role_to_dialect(role, Dialect::OpenAi)
        );
    }
}

#[test]
fn codex_matches_openai_roles() {
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        assert_eq!(
            ir_role_to_dialect(role, Dialect::Codex),
            ir_role_to_dialect(role, Dialect::OpenAi)
        );
    }
}

#[test]
fn copilot_matches_openai_roles() {
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        assert_eq!(
            ir_role_to_dialect(role, Dialect::Copilot),
            ir_role_to_dialect(role, Dialect::OpenAi)
        );
    }
}

#[test]
fn all_dialects_map_user_to_user() {
    for d in Dialect::all() {
        assert_eq!(ir_role_to_dialect(IrRole::User, *d), "user");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// § 15 — Lowering: OpenAI
// ═══════════════════════════════════════════════════════════════════════════

fn sample_conv() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::text(IrRole::User, "Hi"))
        .push(IrMessage::text(IrRole::Assistant, "Hello!"))
}

fn sample_tools() -> Vec<IrToolDefinition> {
    vec![IrToolDefinition {
        name: "calc".into(),
        description: "Math evaluator".into(),
        parameters: json!({"type": "object", "properties": {"expr": {"type": "string"}}}),
    }]
}

#[test]
fn openai_basic_structure() {
    let lowered = lower_to_openai(&sample_conv(), &[]);
    let msgs = lowered["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[1]["role"], "user");
    assert_eq!(msgs[2]["role"], "assistant");
}

#[test]
fn openai_with_tools() {
    let lowered = lower_to_openai(&sample_conv(), &sample_tools());
    let tools = lowered["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["type"], "function");
    assert_eq!(tools[0]["function"]["name"], "calc");
}

#[test]
fn openai_no_tools_omits_field() {
    let lowered = lower_to_openai(&sample_conv(), &[]);
    assert!(lowered.get("tools").is_none());
}

#[test]
fn openai_tool_use_produces_tool_calls() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Let me check.".into(),
            },
            IrContentBlock::ToolUse {
                id: "call_1".into(),
                name: "read_file".into(),
                input: json!({"path": "/tmp/a.rs"}),
            },
        ],
    ));
    let lowered = lower_to_openai(&conv, &[]);
    let msg = &lowered["messages"][0];
    assert_eq!(msg["content"], "Let me check.");
    assert_eq!(msg["tool_calls"][0]["id"], "call_1");
    assert_eq!(msg["tool_calls"][0]["function"]["name"], "read_file");
}

#[test]
fn openai_tool_result_produces_tool_message() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "call_1".into(),
            content: vec![IrContentBlock::Text {
                text: "file contents".into(),
            }],
            is_error: false,
        }],
    ));
    let lowered = lower_to_openai(&conv, &[]);
    let msg = &lowered["messages"][0];
    assert_eq!(msg["role"], "tool");
    assert_eq!(msg["tool_call_id"], "call_1");
    assert_eq!(msg["content"], "file contents");
}

#[test]
fn openai_multiple_tool_results_expand() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Tool,
        vec![
            IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![IrContentBlock::Text {
                    text: "res1".into(),
                }],
                is_error: false,
            },
            IrContentBlock::ToolResult {
                tool_use_id: "c2".into(),
                content: vec![IrContentBlock::Text {
                    text: "res2".into(),
                }],
                is_error: false,
            },
        ],
    ));
    let lowered = lower_to_openai(&conv, &[]);
    let msgs = lowered["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0]["tool_call_id"], "c1");
    assert_eq!(msgs[1]["tool_call_id"], "c2");
}

// ═══════════════════════════════════════════════════════════════════════════
// § 16 — Lowering: Claude
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_extracts_system_to_top_level() {
    let lowered = lower_to_claude(&sample_conv(), &[]);
    assert_eq!(lowered["system"], "You are helpful.");
    let msgs = lowered["messages"].as_array().unwrap();
    assert!(msgs.iter().all(|m| m["role"] != "system"));
}

#[test]
fn claude_no_system_omits_field() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
    let lowered = lower_to_claude(&conv, &[]);
    assert!(lowered.get("system").is_none());
}

#[test]
fn claude_tool_schema_uses_input_schema() {
    let lowered = lower_to_claude(&sample_conv(), &sample_tools());
    let tool = &lowered["tools"][0];
    assert!(tool.get("input_schema").is_some());
    assert!(tool.get("parameters").is_none());
}

#[test]
fn claude_content_blocks_are_typed() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text { text: "hi".into() },
            IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "search".into(),
                input: json!({"q": "rust"}),
            },
        ],
    ));
    let lowered = lower_to_claude(&conv, &[]);
    let content = lowered["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "tool_use");
}

#[test]
fn claude_thinking_block() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "let me think".into(),
        }],
    ));
    let lowered = lower_to_claude(&conv, &[]);
    let block = &lowered["messages"][0]["content"][0];
    assert_eq!(block["type"], "thinking");
    assert_eq!(block["thinking"], "let me think");
}

#[test]
fn claude_image_block() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "base64data".into(),
        }],
    ));
    let lowered = lower_to_claude(&conv, &[]);
    let block = &lowered["messages"][0]["content"][0];
    assert_eq!(block["type"], "image");
    assert_eq!(block["source"]["type"], "base64");
    assert_eq!(block["source"]["media_type"], "image/png");
}

#[test]
fn claude_tool_result_block() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![IrContentBlock::Text {
                text: "result".into(),
            }],
            is_error: true,
        }],
    ));
    let lowered = lower_to_claude(&conv, &[]);
    let block = &lowered["messages"][0]["content"][0];
    assert_eq!(block["type"], "tool_result");
    assert_eq!(block["tool_use_id"], "c1");
    assert_eq!(block["is_error"], true);
}

// ═══════════════════════════════════════════════════════════════════════════
// § 17 — Lowering: Gemini
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn gemini_system_instruction() {
    let lowered = lower_to_gemini(&sample_conv(), &[]);
    assert_eq!(
        lowered["system_instruction"]["parts"][0]["text"],
        "You are helpful."
    );
}

#[test]
fn gemini_no_system_omits_instruction() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
    let lowered = lower_to_gemini(&conv, &[]);
    assert!(lowered.get("system_instruction").is_none());
}

#[test]
fn gemini_assistant_becomes_model() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::text(IrRole::Assistant, "hello"));
    let lowered = lower_to_gemini(&conv, &[]);
    let contents = lowered["contents"].as_array().unwrap();
    assert_eq!(contents[1]["role"], "model");
}

#[test]
fn gemini_thinking_blocks_skipped() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking { text: "hmm".into() },
            IrContentBlock::Text {
                text: "answer".into(),
            },
        ],
    ));
    let lowered = lower_to_gemini(&conv, &[]);
    let parts = lowered["contents"][0]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0]["text"], "answer");
}

#[test]
fn gemini_function_call() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "c1".into(),
            name: "search".into(),
            input: json!({"q": "rust"}),
        }],
    ));
    let lowered = lower_to_gemini(&conv, &[]);
    let part = &lowered["contents"][0]["parts"][0];
    assert_eq!(part["functionCall"]["name"], "search");
    assert_eq!(part["functionCall"]["args"]["q"], "rust");
}

#[test]
fn gemini_function_response() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "search".into(),
            content: vec![IrContentBlock::Text {
                text: "found it".into(),
            }],
            is_error: false,
        }],
    ));
    let lowered = lower_to_gemini(&conv, &[]);
    let part = &lowered["contents"][0]["parts"][0];
    assert_eq!(part["functionResponse"]["name"], "search");
    assert_eq!(part["functionResponse"]["response"]["result"], "found it");
}

#[test]
fn gemini_tools_function_declarations() {
    let lowered = lower_to_gemini(&sample_conv(), &sample_tools());
    let decls = lowered["tools"][0]["function_declarations"]
        .as_array()
        .unwrap();
    assert_eq!(decls.len(), 1);
    assert_eq!(decls[0]["name"], "calc");
}

#[test]
fn gemini_image_inline_data() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/jpeg".into(),
            data: "base64data".into(),
        }],
    ));
    let lowered = lower_to_gemini(&conv, &[]);
    let part = &lowered["contents"][0]["parts"][0];
    assert_eq!(part["inline_data"]["mime_type"], "image/jpeg");
    assert_eq!(part["inline_data"]["data"], "base64data");
}

// ═══════════════════════════════════════════════════════════════════════════
// § 18 — Lowering: Convenience aliases (kimi, codex, copilot)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn kimi_matches_openai_output() {
    let conv = sample_conv();
    let tools = sample_tools();
    assert_eq!(lower_to_kimi(&conv, &tools), lower_to_openai(&conv, &tools));
}

#[test]
fn codex_matches_openai_output() {
    let conv = sample_conv();
    let tools = sample_tools();
    assert_eq!(
        lower_to_codex(&conv, &tools),
        lower_to_openai(&conv, &tools)
    );
}

#[test]
fn copilot_matches_openai_output() {
    let conv = sample_conv();
    let tools = sample_tools();
    assert_eq!(
        lower_to_copilot(&conv, &tools),
        lower_to_openai(&conv, &tools)
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// § 19 — Lowering: lower_for_dialect dispatch
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lower_for_dialect_all_produce_objects() {
    let conv = sample_conv();
    let tools = sample_tools();
    for d in Dialect::all() {
        let lowered = lower_for_dialect(*d, &conv, &tools);
        assert!(lowered.is_object(), "{}: expected JSON object", d.label());
    }
}

#[test]
fn lower_for_dialect_no_tools_omits_tools() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
    for d in Dialect::all() {
        let lowered = lower_for_dialect(*d, &conv, &[]);
        assert!(
            lowered.get("tools").is_none(),
            "{}: tools should be omitted",
            d.label()
        );
    }
}

#[test]
fn lower_for_dialect_dispatch_matches_specific_functions() {
    let conv = sample_conv();
    let tools = sample_tools();
    assert_eq!(
        lower_for_dialect(Dialect::OpenAi, &conv, &tools),
        lower_to_openai(&conv, &tools)
    );
    assert_eq!(
        lower_for_dialect(Dialect::Claude, &conv, &tools),
        lower_to_claude(&conv, &tools)
    );
    assert_eq!(
        lower_for_dialect(Dialect::Gemini, &conv, &tools),
        lower_to_gemini(&conv, &tools)
    );
    assert_eq!(
        lower_for_dialect(Dialect::Kimi, &conv, &tools),
        lower_to_kimi(&conv, &tools)
    );
    assert_eq!(
        lower_for_dialect(Dialect::Codex, &conv, &tools),
        lower_to_codex(&conv, &tools)
    );
    assert_eq!(
        lower_for_dialect(Dialect::Copilot, &conv, &tools),
        lower_to_copilot(&conv, &tools)
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// § 20 — Round-trip: normalize → lower preserves semantics
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_normalize_then_lower_openai() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "  sys  "))
        .push(IrMessage::text(IrRole::User, "  hi  "))
        .push(IrMessage::text(IrRole::Assistant, " hello "));
    let normalized = normalize(&conv);
    let lowered = lower_to_openai(&normalized, &[]);
    let msgs = lowered["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["content"], "sys");
    assert_eq!(msgs[1]["content"], "hi");
    assert_eq!(msgs[2]["content"], "hello");
}

#[test]
fn roundtrip_normalize_then_lower_claude() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "  sys  "))
        .push(IrMessage::text(IrRole::User, "  hi  "));
    let normalized = normalize(&conv);
    let lowered = lower_to_claude(&normalized, &[]);
    assert_eq!(lowered["system"], "sys");
}

#[test]
fn roundtrip_normalize_then_lower_gemini() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "  sys  "))
        .push(IrMessage::text(IrRole::User, "  hi  "));
    let normalized = normalize(&conv);
    let lowered = lower_to_gemini(&normalized, &[]);
    assert_eq!(lowered["system_instruction"]["parts"][0]["text"], "sys");
}

#[test]
fn roundtrip_openai_compatible_dialects_identical() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, " sys "))
        .push(IrMessage::text(IrRole::User, " hi "));
    let normalized = normalize(&conv);
    let oa = lower_to_openai(&normalized, &[]);
    let ki = lower_to_kimi(&normalized, &[]);
    let cx = lower_to_codex(&normalized, &[]);
    let cp = lower_to_copilot(&normalized, &[]);
    assert_eq!(oa, ki);
    assert_eq!(oa, cx);
    assert_eq!(oa, cp);
}

// ═══════════════════════════════════════════════════════════════════════════
// § 21 — Edge cases: Empty / special content
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn edge_empty_conversation_lower_openai() {
    let conv = IrConversation::new();
    let lowered = lower_to_openai(&conv, &[]);
    assert_eq!(lowered["messages"].as_array().unwrap().len(), 0);
}

#[test]
fn edge_empty_conversation_lower_claude() {
    let conv = IrConversation::new();
    let lowered = lower_to_claude(&conv, &[]);
    assert_eq!(lowered["messages"].as_array().unwrap().len(), 0);
}

#[test]
fn edge_empty_conversation_lower_gemini() {
    let conv = IrConversation::new();
    let lowered = lower_to_gemini(&conv, &[]);
    assert_eq!(lowered["contents"].as_array().unwrap().len(), 0);
}

#[test]
fn edge_empty_text_message() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, ""));
    let lowered = lower_to_openai(&conv, &[]);
    assert_eq!(lowered["messages"][0]["content"], "");
}

#[test]
fn edge_whitespace_only_text() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "   "));
    let trimmed = trim_text(&conv);
    assert_eq!(trimmed.messages[0].text_content(), "");
}

#[test]
fn edge_unicode_content() {
    let text = "こんにちは世界 🌍 Ñoño café résumé";
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, text));
    let lowered = lower_to_openai(&conv, &[]);
    assert_eq!(lowered["messages"][0]["content"], text);
}

#[test]
fn edge_emoji_heavy_content() {
    let text = "🎉🎊🎈🎁🎆🎇✨🎄🎋🎍🎏🎐🎑🎀";
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, text));
    let normalized = normalize(&conv);
    assert_eq!(normalized.messages[0].text_content(), text);
}

#[test]
fn edge_newlines_in_content() {
    let text = "line1\nline2\nline3";
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, text));
    let lowered = lower_to_openai(&conv, &[]);
    assert_eq!(lowered["messages"][0]["content"], text);
}

#[test]
fn edge_special_chars_in_tool_name() {
    let tool = IrToolDefinition {
        name: "my-tool_v2.0".into(),
        description: "desc".into(),
        parameters: json!({}),
    };
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
    let lowered = lower_to_openai(&conv, &[tool]);
    assert_eq!(lowered["tools"][0]["function"]["name"], "my-tool_v2.0");
}

#[test]
fn edge_large_content() {
    let text = "x".repeat(100_000);
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, &text));
    let lowered = lower_to_openai(&conv, &[]);
    assert_eq!(
        lowered["messages"][0]["content"].as_str().unwrap().len(),
        100_000
    );
}

#[test]
fn edge_many_messages() {
    let mut conv = IrConversation::new();
    for i in 0..100 {
        let role = if i % 2 == 0 {
            IrRole::User
        } else {
            IrRole::Assistant
        };
        conv = conv.push(IrMessage::text(role, format!("msg_{i}")));
    }
    assert_eq!(conv.len(), 100);
    let lowered = lower_to_openai(&conv, &[]);
    assert_eq!(lowered["messages"].as_array().unwrap().len(), 100);
}

#[test]
fn edge_nested_json_in_tool_input() {
    let input = json!({
        "query": {"nested": {"deeply": {"value": [1, 2, 3]}}},
        "options": {"timeout": 30}
    });
    let block = IrContentBlock::ToolUse {
        id: "c1".into(),
        name: "search".into(),
        input: input.clone(),
    };
    let conv = IrConversation::new().push(IrMessage::new(IrRole::Assistant, vec![block]));
    let lowered = lower_to_openai(&conv, &[]);
    let args_str = lowered["messages"][0]["tool_calls"][0]["function"]["arguments"]
        .as_str()
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(args_str).unwrap();
    assert_eq!(parsed["query"]["nested"]["deeply"]["value"][0], 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// § 22 — Serde serialization of IR types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn serde_ir_role_roundtrip() {
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        let json = serde_json::to_string(&role).unwrap();
        let back: IrRole = serde_json::from_str(&json).unwrap();
        assert_eq!(role, back);
    }
}

#[test]
fn serde_ir_role_snake_case() {
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
fn serde_ir_content_block_text_roundtrip() {
    let block = IrContentBlock::Text {
        text: "hello".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_ir_content_block_text_has_type_tag() {
    let block = IrContentBlock::Text {
        text: "hello".into(),
    };
    let val: serde_json::Value = serde_json::to_value(&block).unwrap();
    assert_eq!(val["type"], "text");
}

#[test]
fn serde_ir_content_block_tool_use_roundtrip() {
    let block = IrContentBlock::ToolUse {
        id: "c1".into(),
        name: "search".into(),
        input: json!({"q": "rust"}),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_ir_content_block_tool_result_roundtrip() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "c1".into(),
        content: vec![IrContentBlock::Text {
            text: "result".into(),
        }],
        is_error: false,
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_ir_content_block_image_roundtrip() {
    let block = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "base64data".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_ir_content_block_thinking_roundtrip() {
    let block = IrContentBlock::Thinking { text: "hmm".into() };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_ir_message_roundtrip() {
    let msg = IrMessage::text(IrRole::User, "hello");
    let json = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn serde_ir_message_with_metadata_roundtrip() {
    let mut meta = BTreeMap::new();
    meta.insert("source".into(), json!("test"));
    let msg = IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text { text: "hi".into() }],
        metadata: meta,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn serde_ir_message_empty_metadata_skipped() {
    let msg = IrMessage::text(IrRole::User, "hi");
    let val: serde_json::Value = serde_json::to_value(&msg).unwrap();
    assert!(val.get("metadata").is_none());
}

#[test]
fn serde_ir_conversation_roundtrip() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "sys"))
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::text(IrRole::Assistant, "hello"));
    let json = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(conv, back);
}

#[test]
fn serde_ir_tool_definition_roundtrip() {
    let tool = IrToolDefinition {
        name: "search".into(),
        description: "Search".into(),
        parameters: json!({"type": "object"}),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn serde_ir_usage_roundtrip() {
    let usage = IrUsage::with_cache(100, 50, 20, 10);
    let json = serde_json::to_string(&usage).unwrap();
    let back: IrUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(usage, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// § 23 — Content block type tags (serde tag = "type")
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn content_block_type_tag_text() {
    let block = IrContentBlock::Text { text: "hi".into() };
    let val: serde_json::Value = serde_json::to_value(&block).unwrap();
    assert_eq!(val["type"], "text");
}

#[test]
fn content_block_type_tag_image() {
    let block = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "d".into(),
    };
    let val: serde_json::Value = serde_json::to_value(&block).unwrap();
    assert_eq!(val["type"], "image");
}

#[test]
fn content_block_type_tag_tool_use() {
    let block = IrContentBlock::ToolUse {
        id: "c1".into(),
        name: "f".into(),
        input: json!({}),
    };
    let val: serde_json::Value = serde_json::to_value(&block).unwrap();
    assert_eq!(val["type"], "tool_use");
}

#[test]
fn content_block_type_tag_tool_result() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "c1".into(),
        content: vec![],
        is_error: false,
    };
    let val: serde_json::Value = serde_json::to_value(&block).unwrap();
    assert_eq!(val["type"], "tool_result");
}

#[test]
fn content_block_type_tag_thinking() {
    let block = IrContentBlock::Thinking { text: "hmm".into() };
    let val: serde_json::Value = serde_json::to_value(&block).unwrap();
    assert_eq!(val["type"], "thinking");
}

// ═══════════════════════════════════════════════════════════════════════════
// § 24 — Deterministic serialization (BTreeMap)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn btreemap_metadata_serializes_sorted() {
    let mut meta = BTreeMap::new();
    meta.insert("z_key".into(), json!("z"));
    meta.insert("a_key".into(), json!("a"));
    meta.insert("m_key".into(), json!("m"));
    let msg = IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text { text: "hi".into() }],
        metadata: meta,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let a_pos = json.find("a_key").unwrap();
    let m_pos = json.find("m_key").unwrap();
    let z_pos = json.find("z_key").unwrap();
    assert!(a_pos < m_pos);
    assert!(m_pos < z_pos);
}

#[test]
fn deterministic_serialization_same_output() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "sys"))
        .push(IrMessage::text(IrRole::User, "hi"));
    let json1 = serde_json::to_string(&conv).unwrap();
    let json2 = serde_json::to_string(&conv).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn deterministic_lowering_same_output() {
    let conv = sample_conv();
    let tools = sample_tools();
    let a = lower_to_openai(&conv, &tools);
    let b = lower_to_openai(&conv, &tools);
    assert_eq!(a, b);
}

// ═══════════════════════════════════════════════════════════════════════════
// § 25 — Equality and Clone
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_role_equality() {
    assert_eq!(IrRole::User, IrRole::User);
    assert_ne!(IrRole::User, IrRole::Assistant);
}

#[test]
fn ir_content_block_equality() {
    let a = IrContentBlock::Text { text: "hi".into() };
    let b = IrContentBlock::Text { text: "hi".into() };
    let c = IrContentBlock::Text { text: "bye".into() };
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn ir_message_equality() {
    let a = IrMessage::text(IrRole::User, "hi");
    let b = IrMessage::text(IrRole::User, "hi");
    let c = IrMessage::text(IrRole::User, "bye");
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn ir_message_equality_different_role() {
    let a = IrMessage::text(IrRole::User, "hi");
    let b = IrMessage::text(IrRole::Assistant, "hi");
    assert_ne!(a, b);
}

#[test]
fn ir_conversation_equality() {
    let a = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::text(IrRole::Assistant, "hello"));
    let b = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::text(IrRole::Assistant, "hello"));
    assert_eq!(a, b);
}

#[test]
fn ir_conversation_equality_different_order() {
    let a = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::text(IrRole::Assistant, "hello"));
    let b = IrConversation::new()
        .push(IrMessage::text(IrRole::Assistant, "hello"))
        .push(IrMessage::text(IrRole::User, "hi"));
    assert_ne!(a, b);
}

#[test]
fn ir_conversation_clone() {
    let a = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::text(IrRole::Assistant, "hello"));
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn ir_usage_equality() {
    let a = IrUsage::from_io(10, 20);
    let b = IrUsage::from_io(10, 20);
    assert_eq!(a, b);
}

#[test]
fn ir_usage_inequality() {
    let a = IrUsage::from_io(10, 20);
    let b = IrUsage::from_io(10, 21);
    assert_ne!(a, b);
}

// ═══════════════════════════════════════════════════════════════════════════
// § 26 — Complex conversation scenarios
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn complex_tool_use_flow_openai() {
    let conv = IrConversation::new()
        .push(IrMessage::text(
            IrRole::System,
            "You are a coding assistant.",
        ))
        .push(IrMessage::text(IrRole::User, "Read my file"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "I'll read it.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "call_abc".into(),
                    name: "read_file".into(),
                    input: json!({"path": "main.rs"}),
                },
            ],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_abc".into(),
                content: vec![IrContentBlock::Text {
                    text: "fn main() {}".into(),
                }],
                is_error: false,
            }],
        ))
        .push(IrMessage::text(
            IrRole::Assistant,
            "Your file contains a main function.",
        ));

    let lowered = lower_to_openai(&conv, &[]);
    let msgs = lowered["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 5);
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[1]["role"], "user");
    assert_eq!(msgs[2]["role"], "assistant");
    assert!(msgs[2].get("tool_calls").is_some());
    assert_eq!(msgs[3]["role"], "tool");
    assert_eq!(msgs[4]["role"], "assistant");
}

#[test]
fn complex_tool_use_flow_claude() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "sys"))
        .push(IrMessage::text(IrRole::User, "do it"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "run".into(),
                input: json!({"cmd": "ls"}),
            }],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![IrContentBlock::Text {
                    text: "file.txt".into(),
                }],
                is_error: false,
            }],
        ));

    let lowered = lower_to_claude(&conv, &[]);
    assert_eq!(lowered["system"], "sys");
    let msgs = lowered["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 3); // user, assistant, tool (mapped to user role)
    // Tool-role maps to "user" in Claude
    assert_eq!(msgs[2]["role"], "user");
    assert_eq!(msgs[2]["content"][0]["type"], "tool_result");
}

#[test]
fn complex_multi_turn_with_images() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "What's in this image?"))
        .push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "iVBORw0KGgo=".into(),
            }],
        ))
        .push(IrMessage::text(IrRole::Assistant, "I see a cat."));

    // All dialects should handle this
    for d in Dialect::all() {
        let lowered = lower_for_dialect(*d, &conv, &[]);
        assert!(lowered.is_object(), "{} failed", d.label());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// § 27 — Normalization pipeline properties
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn normalize_preserves_message_count_for_non_empty() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "a"))
        .push(IrMessage::text(IrRole::Assistant, "b"));
    let result = normalize(&conv);
    assert_eq!(result.len(), 2);
}

#[test]
fn normalize_reduces_system_to_one() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "a"))
        .push(IrMessage::text(IrRole::System, "b"))
        .push(IrMessage::text(IrRole::System, "c"))
        .push(IrMessage::text(IrRole::User, "hi"));
    let result = normalize(&conv);
    assert_eq!(result.messages_by_role(IrRole::System).len(), 1);
}

#[test]
fn normalize_system_at_position_zero() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::text(IrRole::System, "sys"));
    let result = normalize(&conv);
    if !result.messages_by_role(IrRole::System).is_empty() {
        assert_eq!(result.messages[0].role, IrRole::System);
    }
}

#[test]
fn normalize_trim_removes_only_whitespace() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "  hello world  "));
    let result = normalize(&conv);
    assert_eq!(result.messages[0].text_content(), "hello world");
}

// ═══════════════════════════════════════════════════════════════════════════
// § 28 — Deserialization from JSON
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deserialize_ir_role_from_string() {
    let role: IrRole = serde_json::from_str("\"user\"").unwrap();
    assert_eq!(role, IrRole::User);
}

#[test]
fn deserialize_ir_content_block_from_json() {
    let json = r#"{"type":"text","text":"hello"}"#;
    let block: IrContentBlock = serde_json::from_str(json).unwrap();
    assert_eq!(
        block,
        IrContentBlock::Text {
            text: "hello".into()
        }
    );
}

#[test]
fn deserialize_ir_message_without_metadata() {
    let json = r#"{"role":"user","content":[{"type":"text","text":"hi"}]}"#;
    let msg: IrMessage = serde_json::from_str(json).unwrap();
    assert_eq!(msg.role, IrRole::User);
    assert!(msg.metadata.is_empty());
}

#[test]
fn deserialize_ir_conversation_from_json() {
    let json = r#"{"messages":[{"role":"user","content":[{"type":"text","text":"hi"}]}]}"#;
    let conv: IrConversation = serde_json::from_str(json).unwrap();
    assert_eq!(conv.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// § 29 — Cross-dialect lowering comparison
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_and_claude_both_have_messages_array() {
    let conv = sample_conv();
    let oa = lower_to_openai(&conv, &[]);
    let cl = lower_to_claude(&conv, &[]);
    assert!(oa["messages"].is_array());
    assert!(cl["messages"].is_array());
}

#[test]
fn gemini_uses_contents_not_messages() {
    let conv = sample_conv();
    let gm = lower_to_gemini(&conv, &[]);
    assert!(gm.get("messages").is_none());
    assert!(gm["contents"].is_array());
}

#[test]
fn claude_tool_use_content_inline() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "c1".into(),
            name: "f".into(),
            input: json!({"x": 1}),
        }],
    ));
    let cl = lower_to_claude(&conv, &[]);
    let block = &cl["messages"][0]["content"][0];
    // Claude has inline input, not stringified
    assert!(block["input"].is_object());
}

#[test]
fn openai_tool_use_arguments_stringified() {
    let conv = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "c1".into(),
            name: "f".into(),
            input: json!({"x": 1}),
        }],
    ));
    let oa = lower_to_openai(&conv, &[]);
    let args = &oa["messages"][0]["tool_calls"][0]["function"]["arguments"];
    // OpenAI stringifies arguments
    assert!(args.is_string());
}

// ═══════════════════════════════════════════════════════════════════════════
// § 30 — Additional edge cases and misc
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_result_with_empty_content() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "c1".into(),
        content: vec![],
        is_error: false,
    };
    let conv = IrConversation::new().push(IrMessage::new(IrRole::Tool, vec![block]));
    let lowered = lower_to_openai(&conv, &[]);
    assert_eq!(lowered["messages"][0]["content"], "");
}

#[test]
fn multiple_tools_in_lowered_output() {
    let tools = vec![
        IrToolDefinition {
            name: "read".into(),
            description: "Read".into(),
            parameters: json!({"type": "object"}),
        },
        IrToolDefinition {
            name: "write".into(),
            description: "Write".into(),
            parameters: json!({"type": "object"}),
        },
    ];
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
    let lowered = lower_to_openai(&conv, &tools);
    assert_eq!(lowered["tools"].as_array().unwrap().len(), 2);
}

#[test]
fn normalize_then_extract_system() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "  rule1  "))
        .push(IrMessage::text(IrRole::User, "q"))
        .push(IrMessage::text(IrRole::System, "  rule2  "));
    let normalized = normalize(&conv);
    // dedup joins with \n before trim, so inner whitespace from originals remains
    let (sys, rest) = extract_system(&normalized);
    assert_eq!(sys.unwrap(), "rule1  \n  rule2");
    assert_eq!(rest.len(), 1);
}

#[test]
fn message_with_only_thinking() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "deep thought".into(),
        }],
    );
    assert!(!msg.is_text_only());
    assert_eq!(msg.text_content(), "");
}

#[test]
fn message_with_only_image() {
    let msg = IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "data".into(),
        }],
    );
    assert!(!msg.is_text_only());
    assert_eq!(msg.text_content(), "");
}

#[test]
fn conversation_tool_calls_empty_when_no_tools() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::text(IrRole::Assistant, "hello"));
    assert!(conv.tool_calls().is_empty());
}

#[test]
fn normalize_role_case_sensitive() {
    assert_eq!(normalize_role("System"), None);
    assert_eq!(normalize_role("USER"), None);
    assert_eq!(normalize_role("Assistant"), None);
    assert_eq!(normalize_role("TOOL"), None);
}

#[test]
fn content_block_clone() {
    let a = IrContentBlock::Text { text: "hi".into() };
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn tool_definition_clone() {
    let a = IrToolDefinition {
        name: "f".into(),
        description: "d".into(),
        parameters: json!({"type": "object"}),
    };
    let b = a.clone();
    assert_eq!(a, b);
}

#[test]
fn conversation_is_empty_after_strip_all_empty() {
    let conv = IrConversation::new()
        .push(IrMessage::new(IrRole::User, vec![]))
        .push(IrMessage::new(IrRole::Assistant, vec![]));
    let stripped = strip_empty(&conv);
    assert!(stripped.is_empty());
    assert_eq!(stripped.len(), 0);
}

#[test]
fn dedup_system_with_no_non_system_messages() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "a"))
        .push(IrMessage::text(IrRole::System, "b"));
    let deduped = dedup_system(&conv);
    assert_eq!(deduped.len(), 1);
    assert_eq!(deduped.messages[0].text_content(), "a\nb");
}

#[test]
fn trim_text_tabs_and_newlines() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "\t\nhello\n\t"));
    let trimmed = trim_text(&conv);
    assert_eq!(trimmed.messages[0].text_content(), "hello");
}

#[test]
fn openai_text_only_assistant_no_tool_calls() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::Assistant, "just text"));
    let lowered = lower_to_openai(&conv, &[]);
    let msg = &lowered["messages"][0];
    assert_eq!(msg["content"], "just text");
    assert!(msg.get("tool_calls").is_none());
}

#[test]
fn gemini_no_tools_omits_field() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
    let lowered = lower_to_gemini(&conv, &[]);
    assert!(lowered.get("tools").is_none());
}

#[test]
fn claude_no_tools_omits_field() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
    let lowered = lower_to_claude(&conv, &[]);
    assert!(lowered.get("tools").is_none());
}

#[test]
fn normalize_tool_schemas_empty_input() {
    let normalized = normalize_tool_schemas(&[]);
    assert!(normalized.is_empty());
}

#[test]
fn sort_tools_single_element() {
    let mut tools = vec![IrToolDefinition {
        name: "only".into(),
        description: "d".into(),
        parameters: json!({}),
    }];
    sort_tools(&mut tools);
    assert_eq!(tools[0].name, "only");
}

#[test]
fn ir_message_empty_content() {
    let msg = IrMessage::new(IrRole::User, vec![]);
    assert!(msg.is_text_only()); // vacuously true: all zero blocks are Text
    assert_eq!(msg.text_content(), "");
    assert!(msg.tool_use_blocks().is_empty());
}

#[test]
fn ir_conversation_messages_by_role_tool() {
    let conv = IrConversation::new()
        .push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![],
                is_error: false,
            }],
        ))
        .push(IrMessage::text(IrRole::User, "ok"));
    assert_eq!(conv.messages_by_role(IrRole::Tool).len(), 1);
}

#[test]
fn edge_backslash_and_quotes_in_text() {
    let text = r#"He said "hello" and path is C:\Users\test"#;
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, text));
    let json = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(back.messages[0].text_content(), text);
}

#[test]
fn edge_null_bytes_in_text() {
    let text = "hello\0world";
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, text));
    let json = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(back.messages[0].text_content(), text);
}

#[test]
fn edge_rtl_text() {
    let text = "مرحبا بالعالم";
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, text));
    let normalized = normalize(&conv);
    assert_eq!(normalized.messages[0].text_content(), text);
}

#[test]
fn edge_mixed_content_types_in_single_message() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking {
                text: "thinking...".into(),
            },
            IrContentBlock::Text {
                text: "Here's the result.".into(),
            },
            IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "exec".into(),
                input: json!({"cmd": "ls"}),
            },
        ],
    );
    assert!(!msg.is_text_only());
    assert_eq!(msg.text_content(), "Here's the result.");
    assert_eq!(msg.tool_use_blocks().len(), 1);
}
