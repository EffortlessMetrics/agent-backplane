// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for [`abp_core::ir`] — the cross-dialect intermediate representation.

use abp_core::ir::*;
use serde_json::json;
use std::collections::BTreeMap;

// ═══════════════════════════════════════════════════════════════════════
// IrRole
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn role_serde_roundtrip() {
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
fn role_serializes_snake_case() {
    assert_eq!(
        serde_json::to_string(&IrRole::System).unwrap(),
        r#""system""#
    );
    assert_eq!(serde_json::to_string(&IrRole::User).unwrap(), r#""user""#);
    assert_eq!(
        serde_json::to_string(&IrRole::Assistant).unwrap(),
        r#""assistant""#
    );
    assert_eq!(serde_json::to_string(&IrRole::Tool).unwrap(), r#""tool""#);
}

// ═══════════════════════════════════════════════════════════════════════
// IrContentBlock – serde roundtrips
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn content_block_text_roundtrip() {
    let block = IrContentBlock::Text {
        text: "hello".into(),
    };
    let s = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&s).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_image_roundtrip() {
    let block = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "iVBOR...".into(),
    };
    let s = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&s).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_tool_use_roundtrip() {
    let block = IrContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "read_file".into(),
        input: json!({"path": "/tmp/foo.txt"}),
    };
    let s = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&s).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_tool_result_roundtrip() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: vec![IrContentBlock::Text {
            text: "file contents".into(),
        }],
        is_error: false,
    };
    let s = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&s).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_tool_result_error_roundtrip() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "tu_2".into(),
        content: vec![IrContentBlock::Text {
            text: "not found".into(),
        }],
        is_error: true,
    };
    let s = serde_json::to_string(&block).unwrap();
    assert!(s.contains(r#""is_error":true"#));
    let back: IrContentBlock = serde_json::from_str(&s).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_thinking_roundtrip() {
    let block = IrContentBlock::Thinking {
        text: "Let me reason…".into(),
    };
    let s = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&s).unwrap();
    assert_eq!(block, back);
}

#[test]
fn content_block_tagged_discriminator() {
    let block = IrContentBlock::Text { text: "hi".into() };
    let v: serde_json::Value = serde_json::to_value(&block).unwrap();
    assert_eq!(v["type"], "text");
}

// ═══════════════════════════════════════════════════════════════════════
// IrMessage construction
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn message_text_shortcut() {
    let msg = IrMessage::text(IrRole::User, "Hello!");
    assert_eq!(msg.role, IrRole::User);
    assert_eq!(msg.content.len(), 1);
    assert_eq!(msg.text_content(), "Hello!");
    assert!(msg.metadata.is_empty());
}

#[test]
fn message_new_with_multiple_blocks() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking { text: "hmm".into() },
            IrContentBlock::Text {
                text: "answer".into(),
            },
        ],
    );
    assert_eq!(msg.role, IrRole::Assistant);
    assert_eq!(msg.content.len(), 2);
}

#[test]
fn message_is_text_only_true() {
    let msg = IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text { text: "a".into() },
            IrContentBlock::Text { text: "b".into() },
        ],
    );
    assert!(msg.is_text_only());
}

#[test]
fn message_is_text_only_false() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text { text: "ok".into() },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "bash".into(),
                input: json!({}),
            },
        ],
    );
    assert!(!msg.is_text_only());
}

#[test]
fn message_text_content_concatenation() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Hello ".into(),
            },
            IrContentBlock::Thinking {
                text: "(skip)".into(),
            },
            IrContentBlock::Text {
                text: "World".into(),
            },
        ],
    );
    assert_eq!(msg.text_content(), "Hello World");
}

#[test]
fn message_tool_use_blocks() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "calling tools".into(),
            },
            IrContentBlock::ToolUse {
                id: "a".into(),
                name: "read".into(),
                input: json!({}),
            },
            IrContentBlock::ToolUse {
                id: "b".into(),
                name: "write".into(),
                input: json!({}),
            },
        ],
    );
    assert_eq!(msg.tool_use_blocks().len(), 2);
}

#[test]
fn message_with_metadata() {
    let mut meta = BTreeMap::new();
    meta.insert("vendor_id".into(), json!("abc-123"));
    let msg = IrMessage {
        role: IrRole::Assistant,
        content: vec![IrContentBlock::Text { text: "hi".into() }],
        metadata: meta,
    };
    assert_eq!(msg.metadata["vendor_id"], json!("abc-123"));
}

#[test]
fn message_serde_roundtrip() {
    let msg = IrMessage::text(IrRole::System, "You are helpful.");
    let s = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&s).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn message_empty_content() {
    let msg = IrMessage::new(IrRole::User, vec![]);
    assert!(msg.is_text_only()); // vacuously true
    assert_eq!(msg.text_content(), "");
    assert!(msg.tool_use_blocks().is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// IrConversation helpers
// ═══════════════════════════════════════════════════════════════════════

fn sample_conversation() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Be concise."))
        .push(IrMessage::text(IrRole::User, "Hi"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Sure".into(),
                },
                IrContentBlock::ToolUse {
                    id: "tc1".into(),
                    name: "read_file".into(),
                    input: json!({"path": "a.txt"}),
                },
            ],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tc1".into(),
                content: vec![IrContentBlock::Text {
                    text: "file data".into(),
                }],
                is_error: false,
            }],
        ))
        .push(IrMessage::text(IrRole::Assistant, "Done."))
}

#[test]
fn conversation_system_message() {
    let conv = sample_conversation();
    let sys = conv.system_message().unwrap();
    assert_eq!(sys.text_content(), "Be concise.");
}

#[test]
fn conversation_last_assistant() {
    let conv = sample_conversation();
    let last = conv.last_assistant().unwrap();
    assert_eq!(last.text_content(), "Done.");
}

#[test]
fn conversation_tool_calls() {
    let conv = sample_conversation();
    let calls = conv.tool_calls();
    assert_eq!(calls.len(), 1);
}

#[test]
fn conversation_len_and_empty() {
    let conv = IrConversation::new();
    assert!(conv.is_empty());
    assert_eq!(conv.len(), 0);

    let conv = sample_conversation();
    assert!(!conv.is_empty());
    assert_eq!(conv.len(), 5);
}

#[test]
fn conversation_messages_by_role() {
    let conv = sample_conversation();
    assert_eq!(conv.messages_by_role(IrRole::Assistant).len(), 2);
    assert_eq!(conv.messages_by_role(IrRole::Tool).len(), 1);
    assert_eq!(conv.messages_by_role(IrRole::System).len(), 1);
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 1);
}

#[test]
fn conversation_last_message() {
    let conv = sample_conversation();
    assert_eq!(conv.last_message().unwrap().role, IrRole::Assistant);
}

#[test]
fn conversation_no_system() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
    assert!(conv.system_message().is_none());
}

#[test]
fn conversation_no_assistant() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
    assert!(conv.last_assistant().is_none());
}

#[test]
fn conversation_from_messages() {
    let msgs = vec![
        IrMessage::text(IrRole::User, "a"),
        IrMessage::text(IrRole::Assistant, "b"),
    ];
    let conv = IrConversation::from_messages(msgs);
    assert_eq!(conv.len(), 2);
}

#[test]
fn conversation_serde_roundtrip() {
    let conv = sample_conversation();
    let s = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&s).unwrap();
    assert_eq!(conv, back);
}

// ═══════════════════════════════════════════════════════════════════════
// IrToolDefinition
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn tool_definition_simple() {
    let td = IrToolDefinition {
        name: "read_file".into(),
        description: "Read a file".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            },
            "required": ["path"]
        }),
    };
    assert_eq!(td.name, "read_file");
}

#[test]
fn tool_definition_complex_schema() {
    let td = IrToolDefinition {
        name: "execute_command".into(),
        description: "Execute a shell command with options".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The command to run" },
                "args": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "env": {
                    "type": "object",
                    "additionalProperties": { "type": "string" }
                },
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 300000
                }
            },
            "required": ["command"]
        }),
    };
    let s = serde_json::to_string(&td).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&s).unwrap();
    assert_eq!(td, back);
}

#[test]
fn tool_definition_serde_roundtrip() {
    let td = IrToolDefinition {
        name: "search".into(),
        description: "Search codebase".into(),
        parameters: json!({"type": "object", "properties": {}}),
    };
    let s = serde_json::to_string(&td).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&s).unwrap();
    assert_eq!(td, back);
}

// ═══════════════════════════════════════════════════════════════════════
// IrUsage
// ═══════════════════════════════════════════════════════════════════════

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
    let u = IrUsage::with_cache(200, 80, 50, 20);
    assert_eq!(u.total_tokens, 280);
    assert_eq!(u.cache_read_tokens, 50);
    assert_eq!(u.cache_write_tokens, 20);
}

#[test]
fn usage_default_is_zero() {
    let u = IrUsage::default();
    assert_eq!(u.input_tokens, 0);
    assert_eq!(u.output_tokens, 0);
    assert_eq!(u.total_tokens, 0);
    assert_eq!(u.cache_read_tokens, 0);
    assert_eq!(u.cache_write_tokens, 0);
}

#[test]
fn usage_merge() {
    let a = IrUsage::with_cache(100, 50, 10, 5);
    let b = IrUsage::with_cache(200, 80, 20, 10);
    let m = a.merge(b);
    assert_eq!(m.input_tokens, 300);
    assert_eq!(m.output_tokens, 130);
    assert_eq!(m.total_tokens, 430);
    assert_eq!(m.cache_read_tokens, 30);
    assert_eq!(m.cache_write_tokens, 15);
}

#[test]
fn usage_serde_roundtrip() {
    let u = IrUsage::with_cache(500, 200, 100, 50);
    let s = serde_json::to_string(&u).unwrap();
    let back: IrUsage = serde_json::from_str(&s).unwrap();
    assert_eq!(u, back);
}
