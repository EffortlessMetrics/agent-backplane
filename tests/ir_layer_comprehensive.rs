// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive unit-level tests for the IR (Intermediate Representation) layer.
//!
//! 80+ tests covering construction, accessors, serde roundtrips, edge cases,
//! content block variants, conversation building, metadata, deterministic
//! serialization, and validation invariants.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use serde_json::{json, Value};

// ═══════════════════════════════════════════════════════════════════════════
// §1  IrConversation construction and accessors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn conversation_new_is_empty() {
    let conv = IrConversation::new();
    assert!(conv.is_empty());
    assert_eq!(conv.len(), 0);
    assert!(conv.messages.is_empty());
}

#[test]
fn conversation_default_is_empty() {
    let conv = IrConversation::default();
    assert!(conv.is_empty());
    assert_eq!(conv.len(), 0);
}

#[test]
fn conversation_from_messages() {
    let msgs = vec![
        IrMessage::text(IrRole::User, "hello"),
        IrMessage::text(IrRole::Assistant, "hi"),
    ];
    let conv = IrConversation::from_messages(msgs);
    assert_eq!(conv.len(), 2);
    assert!(!conv.is_empty());
}

#[test]
fn conversation_push_chaining() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "system prompt"))
        .push(IrMessage::text(IrRole::User, "query"))
        .push(IrMessage::text(IrRole::Assistant, "response"));
    assert_eq!(conv.len(), 3);
}

#[test]
fn conversation_system_message_found() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "be helpful"))
        .push(IrMessage::text(IrRole::User, "hello"));
    let sys = conv.system_message().unwrap();
    assert_eq!(sys.role, IrRole::System);
    assert_eq!(sys.text_content(), "be helpful");
}

#[test]
fn conversation_system_message_none_when_absent() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hello"));
    assert!(conv.system_message().is_none());
}

#[test]
fn conversation_last_assistant() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::Assistant, "first"))
        .push(IrMessage::text(IrRole::User, "middle"))
        .push(IrMessage::text(IrRole::Assistant, "last"));
    let last = conv.last_assistant().unwrap();
    assert_eq!(last.text_content(), "last");
}

#[test]
fn conversation_last_assistant_none() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "only user"));
    assert!(conv.last_assistant().is_none());
}

#[test]
fn conversation_last_message() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "first"))
        .push(IrMessage::text(IrRole::Assistant, "second"));
    assert_eq!(conv.last_message().unwrap().text_content(), "second");
}

#[test]
fn conversation_last_message_none() {
    let conv = IrConversation::new();
    assert!(conv.last_message().is_none());
}

#[test]
fn conversation_messages_by_role() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "q1"))
        .push(IrMessage::text(IrRole::Assistant, "a1"))
        .push(IrMessage::text(IrRole::User, "q2"))
        .push(IrMessage::text(IrRole::Assistant, "a2"));
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 2);
    assert_eq!(conv.messages_by_role(IrRole::Assistant).len(), 2);
    assert_eq!(conv.messages_by_role(IrRole::System).len(), 0);
    assert_eq!(conv.messages_by_role(IrRole::Tool).len(), 0);
}

#[test]
fn conversation_tool_calls_across_messages() {
    let msg1 = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "search".into(),
            input: json!({"q": "rust"}),
        }],
    );
    let msg2 = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t2".into(),
            name: "read_file".into(),
            input: json!({"path": "/tmp"}),
        }],
    );
    let conv = IrConversation::new().push(msg1).push(msg2);
    assert_eq!(conv.tool_calls().len(), 2);
}

#[test]
fn conversation_tool_calls_empty() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::Assistant, "no tool calls here"));
    assert!(conv.tool_calls().is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// §2  IrMessage construction and kind variants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn message_text_helper() {
    let msg = IrMessage::text(IrRole::User, "hello world");
    assert_eq!(msg.role, IrRole::User);
    assert_eq!(msg.content.len(), 1);
    assert!(msg.is_text_only());
    assert_eq!(msg.text_content(), "hello world");
}

#[test]
fn message_new_with_multiple_blocks() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Let me search.".into(),
            },
            IrContentBlock::ToolUse {
                id: "call_1".into(),
                name: "search".into(),
                input: json!({}),
            },
        ],
    );
    assert!(!msg.is_text_only());
    assert_eq!(msg.text_content(), "Let me search.");
}

#[test]
fn message_empty_content() {
    let msg = IrMessage::new(IrRole::User, vec![]);
    assert!(msg.is_text_only()); // vacuously true
    assert_eq!(msg.text_content(), "");
    assert!(msg.tool_use_blocks().is_empty());
}

#[test]
fn message_metadata_default_empty() {
    let msg = IrMessage::text(IrRole::User, "hi");
    assert!(msg.metadata.is_empty());
}

#[test]
fn message_metadata_custom() {
    let mut msg = IrMessage::text(IrRole::User, "hi");
    msg.metadata.insert("source".into(), json!("test-harness"));
    assert_eq!(msg.metadata.len(), 1);
    assert_eq!(msg.metadata["source"], json!("test-harness"));
}

#[test]
fn message_tool_use_blocks_filter() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "thinking...".into(),
            },
            IrContentBlock::ToolUse {
                id: "a".into(),
                name: "foo".into(),
                input: json!(null),
            },
            IrContentBlock::ToolUse {
                id: "b".into(),
                name: "bar".into(),
                input: json!(42),
            },
        ],
    );
    let tool_blocks = msg.tool_use_blocks();
    assert_eq!(tool_blocks.len(), 2);
}

#[test]
fn message_text_content_concatenation() {
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
fn message_text_content_skips_non_text() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "before".into(),
            },
            IrContentBlock::Thinking {
                text: "thinking step".into(),
            },
            IrContentBlock::Text {
                text: "after".into(),
            },
        ],
    );
    assert_eq!(msg.text_content(), "beforeafter");
}

#[test]
fn message_all_four_roles() {
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        let msg = IrMessage::text(role, "test");
        assert_eq!(msg.role, role);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  IrToolCall and IrToolResult (content blocks)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_use_block_fields() {
    let block = IrContentBlock::ToolUse {
        id: "call_123".into(),
        name: "read_file".into(),
        input: json!({"path": "src/main.rs"}),
    };
    if let IrContentBlock::ToolUse { id, name, input } = &block {
        assert_eq!(id, "call_123");
        assert_eq!(name, "read_file");
        assert_eq!(input["path"], "src/main.rs");
    } else {
        panic!("expected ToolUse");
    }
}

#[test]
fn tool_result_block_success() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "call_123".into(),
        content: vec![IrContentBlock::Text {
            text: "file contents here".into(),
        }],
        is_error: false,
    };
    if let IrContentBlock::ToolResult {
        tool_use_id,
        content,
        is_error,
    } = &block
    {
        assert_eq!(tool_use_id, "call_123");
        assert!(!is_error);
        assert_eq!(content.len(), 1);
    } else {
        panic!("expected ToolResult");
    }
}

#[test]
fn tool_result_block_error() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "call_456".into(),
        content: vec![IrContentBlock::Text {
            text: "permission denied".into(),
        }],
        is_error: true,
    };
    if let IrContentBlock::ToolResult { is_error, .. } = &block {
        assert!(is_error);
    } else {
        panic!("expected ToolResult");
    }
}

#[test]
fn tool_result_empty_content() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "call_789".into(),
        content: vec![],
        is_error: false,
    };
    if let IrContentBlock::ToolResult { content, .. } = &block {
        assert!(content.is_empty());
    } else {
        panic!("expected ToolResult");
    }
}

#[test]
fn tool_result_nested_multiple_blocks() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "call_nested".into(),
        content: vec![
            IrContentBlock::Text {
                text: "line 1".into(),
            },
            IrContentBlock::Text {
                text: "line 2".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "iVBOR...".into(),
            },
        ],
        is_error: false,
    };
    if let IrContentBlock::ToolResult { content, .. } = &block {
        assert_eq!(content.len(), 3);
    } else {
        panic!("expected ToolResult");
    }
}

#[test]
fn tool_definition_construction() {
    let def = IrToolDefinition {
        name: "search".into(),
        description: "Search the codebase".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" }
            },
            "required": ["query"]
        }),
    };
    assert_eq!(def.name, "search");
    assert_eq!(def.description, "Search the codebase");
    assert!(def.parameters.is_object());
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  IR serde roundtrip (JSON)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn serde_roundtrip_ir_role() {
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        let json_str = serde_json::to_string(&role).unwrap();
        let back: IrRole = serde_json::from_str(&json_str).unwrap();
        assert_eq!(role, back);
    }
}

#[test]
fn serde_roundtrip_text_block() {
    let block = IrContentBlock::Text {
        text: "hello world".into(),
    };
    let json_str = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json_str).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_roundtrip_image_block() {
    let block = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "iVBORw0KGgo=".into(),
    };
    let json_str = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json_str).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_roundtrip_tool_use_block() {
    let block = IrContentBlock::ToolUse {
        id: "tc_1".into(),
        name: "grep".into(),
        input: json!({"pattern": "fn main", "path": "."}),
    };
    let json_str = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json_str).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_roundtrip_tool_result_block() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "tc_1".into(),
        content: vec![IrContentBlock::Text {
            text: "found 3 matches".into(),
        }],
        is_error: false,
    };
    let json_str = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json_str).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_roundtrip_thinking_block() {
    let block = IrContentBlock::Thinking {
        text: "Let me think about this step by step...".into(),
    };
    let json_str = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json_str).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_roundtrip_ir_message() {
    let msg = IrMessage::text(IrRole::User, "test message");
    let json_str = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json_str).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn serde_roundtrip_ir_message_with_metadata() {
    let mut msg = IrMessage::text(IrRole::Assistant, "response");
    msg.metadata.insert("model".into(), json!("gpt-4"));
    msg.metadata.insert("temperature".into(), json!(0.7));
    let json_str = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json_str).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn serde_roundtrip_ir_conversation() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "you are helpful"))
        .push(IrMessage::text(IrRole::User, "what is rust?"))
        .push(IrMessage::text(IrRole::Assistant, "A systems language."));
    let json_str = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json_str).unwrap();
    assert_eq!(conv, back);
}

#[test]
fn serde_roundtrip_empty_conversation() {
    let conv = IrConversation::new();
    let json_str = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json_str).unwrap();
    assert_eq!(conv, back);
}

#[test]
fn serde_roundtrip_tool_definition() {
    let def = IrToolDefinition {
        name: "list_files".into(),
        description: "Lists files in a directory".into(),
        parameters: json!({"type": "object", "properties": {}}),
    };
    let json_str = serde_json::to_string(&def).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&json_str).unwrap();
    assert_eq!(def, back);
}

#[test]
fn serde_roundtrip_ir_usage() {
    let usage = IrUsage::from_io(100, 50);
    let json_str = serde_json::to_string(&usage).unwrap();
    let back: IrUsage = serde_json::from_str(&json_str).unwrap();
    assert_eq!(usage, back);
}

#[test]
fn serde_roundtrip_ir_usage_with_cache() {
    let usage = IrUsage::with_cache(200, 80, 30, 10);
    let json_str = serde_json::to_string(&usage).unwrap();
    let back: IrUsage = serde_json::from_str(&json_str).unwrap();
    assert_eq!(usage, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// §5  Conversation with multiple message types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn conversation_mixed_roles() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Instructions"))
        .push(IrMessage::text(IrRole::User, "Please search"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "search".into(),
                input: json!({"q": "test"}),
            }],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "found it".into(),
                }],
                is_error: false,
            }],
        ))
        .push(IrMessage::text(IrRole::Assistant, "I found the result."));

    assert_eq!(conv.len(), 5);
    assert_eq!(conv.messages_by_role(IrRole::System).len(), 1);
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 1);
    assert_eq!(conv.messages_by_role(IrRole::Assistant).len(), 2);
    assert_eq!(conv.messages_by_role(IrRole::Tool).len(), 1);
    assert_eq!(conv.tool_calls().len(), 1);
}

#[test]
fn conversation_with_thinking_and_tool_use() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking {
                text: "I should use the search tool.".into(),
            },
            IrContentBlock::Text {
                text: "Let me search.".into(),
            },
            IrContentBlock::ToolUse {
                id: "tc_think".into(),
                name: "search".into(),
                input: json!({"q": "foo"}),
            },
        ],
    );
    let conv = IrConversation::new().push(msg);
    assert_eq!(conv.tool_calls().len(), 1);
    assert!(!conv.messages[0].is_text_only());
}

#[test]
fn conversation_multiple_tool_calls_single_message() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "read".into(),
                input: json!({}),
            },
            IrContentBlock::ToolUse {
                id: "t2".into(),
                name: "write".into(),
                input: json!({}),
            },
            IrContentBlock::ToolUse {
                id: "t3".into(),
                name: "exec".into(),
                input: json!({}),
            },
        ],
    );
    assert_eq!(msg.tool_use_blocks().len(), 3);
}

#[test]
fn conversation_image_message() {
    let msg = IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "What is in this image?".into(),
            },
            IrContentBlock::Image {
                media_type: "image/jpeg".into(),
                data: "/9j/4AAQ...".into(),
            },
        ],
    );
    assert!(!msg.is_text_only());
    assert_eq!(msg.text_content(), "What is in this image?");
}

// ═══════════════════════════════════════════════════════════════════════════
// §6  IR normalization and validation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn usage_from_io_computes_total() {
    let usage = IrUsage::from_io(1000, 500);
    assert_eq!(usage.input_tokens, 1000);
    assert_eq!(usage.output_tokens, 500);
    assert_eq!(usage.total_tokens, 1500);
    assert_eq!(usage.cache_read_tokens, 0);
    assert_eq!(usage.cache_write_tokens, 0);
}

#[test]
fn usage_with_cache_computes_total() {
    let usage = IrUsage::with_cache(200, 100, 50, 25);
    assert_eq!(usage.total_tokens, 300);
    assert_eq!(usage.cache_read_tokens, 50);
    assert_eq!(usage.cache_write_tokens, 25);
}

#[test]
fn usage_merge_sums_all_fields() {
    let a = IrUsage::with_cache(100, 50, 10, 5);
    let b = IrUsage::with_cache(200, 80, 20, 10);
    let merged = a.merge(b);
    assert_eq!(merged.input_tokens, 300);
    assert_eq!(merged.output_tokens, 130);
    assert_eq!(merged.total_tokens, 430);
    assert_eq!(merged.cache_read_tokens, 30);
    assert_eq!(merged.cache_write_tokens, 15);
}

#[test]
fn usage_default_is_zero() {
    let usage = IrUsage::default();
    assert_eq!(usage.input_tokens, 0);
    assert_eq!(usage.output_tokens, 0);
    assert_eq!(usage.total_tokens, 0);
    assert_eq!(usage.cache_read_tokens, 0);
    assert_eq!(usage.cache_write_tokens, 0);
}

#[test]
fn usage_merge_with_default_is_identity() {
    let usage = IrUsage::from_io(42, 58);
    let merged = usage.merge(IrUsage::default());
    assert_eq!(usage, merged);
}

#[test]
fn usage_merge_is_commutative() {
    let a = IrUsage::from_io(10, 20);
    let b = IrUsage::from_io(30, 40);
    assert_eq!(a.merge(b), b.merge(a));
}

#[test]
fn usage_merge_is_associative() {
    let a = IrUsage::from_io(1, 2);
    let b = IrUsage::from_io(3, 4);
    let c = IrUsage::from_io(5, 6);
    assert_eq!(a.merge(b).merge(c), a.merge(b.merge(c)));
}

// ═══════════════════════════════════════════════════════════════════════════
// §7  Content block types (text, image, tool_use, tool_result, thinking)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn content_block_text_variant() {
    let block = IrContentBlock::Text {
        text: "hello".into(),
    };
    assert!(matches!(block, IrContentBlock::Text { .. }));
}

#[test]
fn content_block_image_variant() {
    let block = IrContentBlock::Image {
        media_type: "image/webp".into(),
        data: "UklGR...".into(),
    };
    assert!(matches!(block, IrContentBlock::Image { .. }));
}

#[test]
fn content_block_tool_use_variant() {
    let block = IrContentBlock::ToolUse {
        id: "id".into(),
        name: "tool".into(),
        input: json!(null),
    };
    assert!(matches!(block, IrContentBlock::ToolUse { .. }));
}

#[test]
fn content_block_tool_result_variant() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "id".into(),
        content: vec![],
        is_error: false,
    };
    assert!(matches!(block, IrContentBlock::ToolResult { .. }));
}

#[test]
fn content_block_thinking_variant() {
    let block = IrContentBlock::Thinking { text: "hmm".into() };
    assert!(matches!(block, IrContentBlock::Thinking { .. }));
}

#[test]
fn content_block_text_empty_string() {
    let block = IrContentBlock::Text {
        text: String::new(),
    };
    if let IrContentBlock::Text { text } = &block {
        assert!(text.is_empty());
    }
}

#[test]
fn content_block_image_svg_type() {
    let block = IrContentBlock::Image {
        media_type: "image/svg+xml".into(),
        data: "PHN2Zy...".into(),
    };
    if let IrContentBlock::Image { media_type, .. } = &block {
        assert_eq!(media_type, "image/svg+xml");
    }
}

#[test]
fn content_block_tool_use_with_complex_input() {
    let block = IrContentBlock::ToolUse {
        id: "complex".into(),
        name: "create_file".into(),
        input: json!({
            "path": "/tmp/test.rs",
            "content": "fn main() {}",
            "options": {
                "overwrite": true,
                "permissions": 644
            }
        }),
    };
    if let IrContentBlock::ToolUse { input, .. } = &block {
        assert!(input.is_object());
        assert_eq!(input["options"]["overwrite"], true);
    }
}

#[test]
fn content_block_clone_equality() {
    let block = IrContentBlock::ToolUse {
        id: "c1".into(),
        name: "test".into(),
        input: json!({"a": 1}),
    };
    let cloned = block.clone();
    assert_eq!(block, cloned);
}

// ═══════════════════════════════════════════════════════════════════════════
// §8  System message handling
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn system_message_is_first_in_conversation() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "system"))
        .push(IrMessage::text(IrRole::User, "user"));
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert!(conv.system_message().is_some());
}

#[test]
fn system_message_returns_first_occurrence() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "first system"))
        .push(IrMessage::text(IrRole::User, "user"))
        .push(IrMessage::text(IrRole::System, "second system"));
    let sys = conv.system_message().unwrap();
    assert_eq!(sys.text_content(), "first system");
}

#[test]
fn system_message_can_have_metadata() {
    let mut msg = IrMessage::text(IrRole::System, "Be a helpful assistant.");
    msg.metadata.insert("priority".into(), json!("high"));
    let conv = IrConversation::new().push(msg);
    let sys = conv.system_message().unwrap();
    assert_eq!(sys.metadata["priority"], "high");
}

#[test]
fn system_message_multiblock() {
    let msg = IrMessage::new(
        IrRole::System,
        vec![
            IrContentBlock::Text {
                text: "Rule 1: Be helpful. ".into(),
            },
            IrContentBlock::Text {
                text: "Rule 2: Be safe.".into(),
            },
        ],
    );
    assert!(msg.is_text_only());
    assert_eq!(msg.text_content(), "Rule 1: Be helpful. Rule 2: Be safe.");
}

// ═══════════════════════════════════════════════════════════════════════════
// §9  Multi-turn conversation building
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn multi_turn_basic_exchange() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "What is 2+2?"))
        .push(IrMessage::text(IrRole::Assistant, "4"))
        .push(IrMessage::text(IrRole::User, "And 3+3?"))
        .push(IrMessage::text(IrRole::Assistant, "6"));
    assert_eq!(conv.len(), 4);
    assert_eq!(conv.last_assistant().unwrap().text_content(), "6");
}

#[test]
fn multi_turn_with_tool_loop() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "Find the file"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "search".into(),
                input: json!({"q": "main.rs"}),
            }],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "src/main.rs".into(),
                }],
                is_error: false,
            }],
        ))
        .push(IrMessage::text(IrRole::Assistant, "Found at src/main.rs"));
    assert_eq!(conv.len(), 4);
    assert_eq!(conv.tool_calls().len(), 1);
    assert_eq!(
        conv.last_assistant().unwrap().text_content(),
        "Found at src/main.rs"
    );
}

#[test]
fn multi_turn_alternating_roles_preserved() {
    let roles = [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::User,
        IrRole::Assistant,
    ];
    let mut conv = IrConversation::new();
    for (i, role) in roles.iter().enumerate() {
        conv = conv.push(IrMessage::text(*role, format!("msg {i}")));
    }
    for (i, msg) in conv.messages.iter().enumerate() {
        assert_eq!(msg.role, roles[i]);
    }
}

#[test]
fn multi_turn_large_conversation() {
    let mut conv = IrConversation::new();
    for i in 0..50 {
        let role = if i % 2 == 0 {
            IrRole::User
        } else {
            IrRole::Assistant
        };
        conv = conv.push(IrMessage::text(role, format!("turn {i}")));
    }
    assert_eq!(conv.len(), 50);
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 25);
    assert_eq!(conv.messages_by_role(IrRole::Assistant).len(), 25);
}

#[test]
fn multi_turn_tool_error_recovery() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "delete /tmp/x"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "d1".into(),
                name: "delete_file".into(),
                input: json!({"path": "/tmp/x"}),
            }],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "d1".into(),
                content: vec![IrContentBlock::Text {
                    text: "EACCES: permission denied".into(),
                }],
                is_error: true,
            }],
        ))
        .push(IrMessage::text(
            IrRole::Assistant,
            "Permission denied. Try sudo?",
        ));
    assert_eq!(conv.len(), 4);
    // Verify the tool result is an error
    if let IrContentBlock::ToolResult { is_error, .. } = &conv.messages[2].content[0] {
        assert!(is_error);
    } else {
        panic!("expected tool result");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §10  IR deterministic serialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn deterministic_serialization_metadata_order() {
    let mut msg = IrMessage::text(IrRole::User, "test");
    msg.metadata.insert("zzz".into(), json!(1));
    msg.metadata.insert("aaa".into(), json!(2));
    msg.metadata.insert("mmm".into(), json!(3));

    let json1 = serde_json::to_string(&msg).unwrap();
    let json2 = serde_json::to_string(&msg).unwrap();
    assert_eq!(json1, json2);

    // BTreeMap guarantees alphabetical key order
    let v: Value = serde_json::from_str(&json1).unwrap();
    let keys: Vec<&str> = v["metadata"]
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();
    assert_eq!(keys, vec!["aaa", "mmm", "zzz"]);
}

#[test]
fn deterministic_serialization_conversation() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "a"))
        .push(IrMessage::text(IrRole::Assistant, "b"));
    let s1 = serde_json::to_string(&conv).unwrap();
    let s2 = serde_json::to_string(&conv).unwrap();
    assert_eq!(s1, s2);
}

#[test]
fn deterministic_serialization_usage() {
    let usage = IrUsage::with_cache(100, 200, 10, 20);
    let s1 = serde_json::to_string(&usage).unwrap();
    let s2 = serde_json::to_string(&usage).unwrap();
    assert_eq!(s1, s2);
}

#[test]
fn metadata_skipped_when_empty() {
    let msg = IrMessage::text(IrRole::User, "no metadata");
    let json_str = serde_json::to_string(&msg).unwrap();
    let v: Value = serde_json::from_str(&json_str).unwrap();
    assert!(v.get("metadata").is_none());
}

#[test]
fn metadata_present_when_nonempty() {
    let mut msg = IrMessage::text(IrRole::User, "with metadata");
    msg.metadata.insert("key".into(), json!("value"));
    let json_str = serde_json::to_string(&msg).unwrap();
    let v: Value = serde_json::from_str(&json_str).unwrap();
    assert!(v.get("metadata").is_some());
}

#[test]
fn content_block_tagged_discriminator() {
    let block = IrContentBlock::Text { text: "hi".into() };
    let v: Value = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "text");
}

#[test]
fn content_block_tool_use_tag() {
    let block = IrContentBlock::ToolUse {
        id: "x".into(),
        name: "y".into(),
        input: json!(null),
    };
    let v: Value = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "tool_use");
}

#[test]
fn content_block_tool_result_tag() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "x".into(),
        content: vec![],
        is_error: false,
    };
    let v: Value = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "tool_result");
}

#[test]
fn content_block_image_tag() {
    let block = IrContentBlock::Image {
        media_type: "image/gif".into(),
        data: "R0lGODlh".into(),
    };
    let v: Value = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "image");
}

#[test]
fn content_block_thinking_tag() {
    let block = IrContentBlock::Thinking {
        text: "thoughts".into(),
    };
    let v: Value = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "thinking");
}

#[test]
fn role_serde_snake_case() {
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
fn deserialize_role_from_snake_case() {
    let sys: IrRole = serde_json::from_str("\"system\"").unwrap();
    let user: IrRole = serde_json::from_str("\"user\"").unwrap();
    let asst: IrRole = serde_json::from_str("\"assistant\"").unwrap();
    let tool: IrRole = serde_json::from_str("\"tool\"").unwrap();
    assert_eq!(sys, IrRole::System);
    assert_eq!(user, IrRole::User);
    assert_eq!(asst, IrRole::Assistant);
    assert_eq!(tool, IrRole::Tool);
}

#[test]
fn conversation_serde_preserves_message_order() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "first"))
        .push(IrMessage::text(IrRole::Assistant, "second"))
        .push(IrMessage::text(IrRole::User, "third"));
    let json_str = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json_str).unwrap();
    for (orig, decoded) in conv.messages.iter().zip(back.messages.iter()) {
        assert_eq!(orig.text_content(), decoded.text_content());
        assert_eq!(orig.role, decoded.role);
    }
}

#[test]
fn tool_definition_serde_roundtrip_complex_schema() {
    let def = IrToolDefinition {
        name: "complex_tool".into(),
        description: "A tool with a complex schema".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "minLength": 1},
                "count": {"type": "integer", "minimum": 0},
                "tags": {
                    "type": "array",
                    "items": {"type": "string"}
                },
                "config": {
                    "type": "object",
                    "properties": {
                        "verbose": {"type": "boolean"}
                    }
                }
            },
            "required": ["name"]
        }),
    };
    let json_str = serde_json::to_string_pretty(&def).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&json_str).unwrap();
    assert_eq!(def, back);
}

#[test]
fn conversation_equality() {
    let a = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "hello"))
        .push(IrMessage::text(IrRole::Assistant, "hi"));
    let b = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "hello"))
        .push(IrMessage::text(IrRole::Assistant, "hi"));
    assert_eq!(a, b);
}

#[test]
fn conversation_inequality_different_content() {
    let a = IrConversation::new().push(IrMessage::text(IrRole::User, "hello"));
    let b = IrConversation::new().push(IrMessage::text(IrRole::User, "goodbye"));
    assert_ne!(a, b);
}

#[test]
fn conversation_inequality_different_roles() {
    let a = IrConversation::new().push(IrMessage::text(IrRole::User, "hello"));
    let b = IrConversation::new().push(IrMessage::text(IrRole::Assistant, "hello"));
    assert_ne!(a, b);
}

#[test]
fn conversation_inequality_different_length() {
    let a = IrConversation::new().push(IrMessage::text(IrRole::User, "hello"));
    let b = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "hello"))
        .push(IrMessage::text(IrRole::Assistant, "hi"));
    assert_ne!(a, b);
}

#[test]
fn usage_copy_semantics() {
    let usage = IrUsage::from_io(10, 20);
    let copy = usage;
    assert_eq!(usage, copy);
}

#[test]
fn message_clone_independence() {
    let original = IrMessage::text(IrRole::User, "original");
    let mut cloned = original.clone();
    cloned.metadata.insert("added".into(), json!("after clone"));
    assert!(original.metadata.is_empty());
    assert_eq!(cloned.metadata.len(), 1);
}

#[test]
fn conversation_clone_independence() {
    let original = IrConversation::new().push(IrMessage::text(IrRole::User, "msg"));
    let cloned = original.clone();
    assert_eq!(original, cloned);
    // Push to a new conversation built from original shouldn't affect cloned
    let extended = original.push(IrMessage::text(IrRole::Assistant, "reply"));
    assert_ne!(extended, cloned);
}

#[test]
fn serde_roundtrip_full_conversation_with_all_block_types() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "You are helpful."))
        .push(IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text {
                    text: "Describe this image:".into(),
                },
                IrContentBlock::Image {
                    media_type: "image/png".into(),
                    data: "base64data==".into(),
                },
            ],
        ))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "User wants image description.".into(),
                },
                IrContentBlock::Text {
                    text: "I see a cat.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "v1".into(),
                    name: "vision_analyze".into(),
                    input: json!({"detail": "high"}),
                },
            ],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "v1".into(),
                content: vec![IrContentBlock::Text {
                    text: "Orange tabby cat, indoor setting.".into(),
                }],
                is_error: false,
            }],
        ))
        .push(IrMessage::text(
            IrRole::Assistant,
            "The image shows an orange tabby cat indoors.",
        ));

    let json_str = serde_json::to_string_pretty(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json_str).unwrap();
    assert_eq!(conv, back);
    assert_eq!(back.len(), 5);
    assert_eq!(back.tool_calls().len(), 1);
    assert!(back.system_message().is_some());
}

#[test]
fn deserialize_from_raw_json_text_block() {
    let raw = r#"{"type":"text","text":"hello"}"#;
    let block: IrContentBlock = serde_json::from_str(raw).unwrap();
    assert!(matches!(block, IrContentBlock::Text { text } if text == "hello"));
}

#[test]
fn deserialize_from_raw_json_tool_use_block() {
    let raw = r#"{"type":"tool_use","id":"t1","name":"grep","input":{"q":"test"}}"#;
    let block: IrContentBlock = serde_json::from_str(raw).unwrap();
    assert!(
        matches!(block, IrContentBlock::ToolUse { id, name, .. } if id == "t1" && name == "grep")
    );
}

#[test]
fn deserialize_from_raw_json_message() {
    let raw = r#"{"role":"user","content":[{"type":"text","text":"hi"}]}"#;
    let msg: IrMessage = serde_json::from_str(raw).unwrap();
    assert_eq!(msg.role, IrRole::User);
    assert_eq!(msg.text_content(), "hi");
    assert!(msg.metadata.is_empty());
}

#[test]
fn deserialize_from_raw_json_conversation() {
    let raw = r#"{"messages":[{"role":"user","content":[{"type":"text","text":"q"}]},{"role":"assistant","content":[{"type":"text","text":"a"}]}]}"#;
    let conv: IrConversation = serde_json::from_str(raw).unwrap();
    assert_eq!(conv.len(), 2);
}
