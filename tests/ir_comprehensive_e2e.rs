// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive end-to-end tests for the IR (Intermediate Representation) layer.
//!
//! Covers construction, manipulation, serialization roundtrips, cross-dialect
//! representation, and edge cases for all IR types.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use serde_json::json;

// ─── Helpers ────────────────────────────────────────────────────────────

fn simple_tool_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string" }
        },
        "required": ["path"]
    })
}

fn tool_use_block(id: &str, name: &str, input: serde_json::Value) -> IrContentBlock {
    IrContentBlock::ToolUse {
        id: id.to_string(),
        name: name.to_string(),
        input,
    }
}

fn tool_result_block(tool_use_id: &str, text: &str, is_error: bool) -> IrContentBlock {
    IrContentBlock::ToolResult {
        tool_use_id: tool_use_id.to_string(),
        content: vec![IrContentBlock::Text {
            text: text.to_string(),
        }],
        is_error,
    }
}

fn sample_image_block() -> IrContentBlock {
    IrContentBlock::Image {
        media_type: "image/png".to_string(),
        data: "iVBORw0KGgoAAAANSUhEUg==".to_string(),
    }
}

fn serde_roundtrip<
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
>(
    val: &T,
) {
    let json = serde_json::to_string(val).expect("serialize");
    let back: T = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(*val, back);
}

// ═══════════════════════════════════════════════════════════════════════
// 1. IrRole
// ═══════════════════════════════════════════════════════════════════════

mod role {
    use super::*;

    #[test]
    fn all_variants_exist() {
        let _s = IrRole::System;
        let _u = IrRole::User;
        let _a = IrRole::Assistant;
        let _t = IrRole::Tool;
    }

    #[test]
    fn clone_and_copy() {
        let r = IrRole::User;
        let r2 = r;
        assert_eq!(r, r2);
    }

    #[test]
    fn debug_format() {
        assert_eq!(format!("{:?}", IrRole::System), "System");
        assert_eq!(format!("{:?}", IrRole::Tool), "Tool");
    }

    #[test]
    fn hash_in_collections() {
        let mut set = std::collections::HashSet::new();
        set.insert(IrRole::User);
        set.insert(IrRole::User);
        assert_eq!(set.len(), 1);
        set.insert(IrRole::Assistant);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn serde_roundtrip_all_roles() {
        for role in [
            IrRole::System,
            IrRole::User,
            IrRole::Assistant,
            IrRole::Tool,
        ] {
            serde_roundtrip(&role);
        }
    }

    #[test]
    fn serde_snake_case_names() {
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
    fn deserialize_from_snake_case() {
        assert_eq!(
            serde_json::from_str::<IrRole>("\"system\"").unwrap(),
            IrRole::System
        );
        assert_eq!(
            serde_json::from_str::<IrRole>("\"tool\"").unwrap(),
            IrRole::Tool
        );
    }

    #[test]
    fn invalid_role_fails() {
        assert!(serde_json::from_str::<IrRole>("\"admin\"").is_err());
    }

    #[test]
    fn equality_and_inequality() {
        assert_eq!(IrRole::User, IrRole::User);
        assert_ne!(IrRole::User, IrRole::System);
        assert_ne!(IrRole::Assistant, IrRole::Tool);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. IrContentBlock
// ═══════════════════════════════════════════════════════════════════════

mod content_block {
    use super::*;

    #[test]
    fn text_block() {
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
    fn image_block() {
        let b = sample_image_block();
        if let IrContentBlock::Image { media_type, data } = &b {
            assert_eq!(media_type, "image/png");
            assert!(!data.is_empty());
        } else {
            panic!("expected Image");
        }
    }

    #[test]
    fn tool_use_block_construction() {
        let b = tool_use_block("tu_1", "read_file", json!({"path": "/a.txt"}));
        if let IrContentBlock::ToolUse { id, name, input } = &b {
            assert_eq!(id, "tu_1");
            assert_eq!(name, "read_file");
            assert_eq!(input["path"], "/a.txt");
        } else {
            panic!("expected ToolUse");
        }
    }

    #[test]
    fn tool_result_success() {
        let b = tool_result_block("tu_1", "file contents", false);
        if let IrContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } = &b
        {
            assert_eq!(tool_use_id, "tu_1");
            assert!(!is_error);
            assert_eq!(content.len(), 1);
        } else {
            panic!("expected ToolResult");
        }
    }

    #[test]
    fn tool_result_error() {
        let b = tool_result_block("tu_2", "not found", true);
        if let IrContentBlock::ToolResult { is_error, .. } = &b {
            assert!(is_error);
        } else {
            panic!("expected ToolResult");
        }
    }

    #[test]
    fn thinking_block() {
        let b = IrContentBlock::Thinking {
            text: "Let me think...".into(),
        };
        if let IrContentBlock::Thinking { text } = &b {
            assert_eq!(text, "Let me think...");
        } else {
            panic!("expected Thinking");
        }
    }

    #[test]
    fn serde_text_roundtrip() {
        serde_roundtrip(&IrContentBlock::Text {
            text: "hello world".into(),
        });
    }

    #[test]
    fn serde_image_roundtrip() {
        serde_roundtrip(&sample_image_block());
    }

    #[test]
    fn serde_tool_use_roundtrip() {
        serde_roundtrip(&tool_use_block("id1", "grep", json!({"q": "foo"})));
    }

    #[test]
    fn serde_tool_result_roundtrip() {
        serde_roundtrip(&tool_result_block("id1", "result text", false));
    }

    #[test]
    fn serde_thinking_roundtrip() {
        serde_roundtrip(&IrContentBlock::Thinking { text: "hmm".into() });
    }

    #[test]
    fn text_tagged_as_type_text() {
        let v: serde_json::Value =
            serde_json::to_value(IrContentBlock::Text { text: "hi".into() }).unwrap();
        assert_eq!(v["type"], "text");
    }

    #[test]
    fn image_tagged_as_type_image() {
        let v: serde_json::Value = serde_json::to_value(sample_image_block()).unwrap();
        assert_eq!(v["type"], "image");
    }

    #[test]
    fn tool_use_tagged_as_type_tool_use() {
        let v = serde_json::to_value(tool_use_block("x", "y", json!({}))).unwrap();
        assert_eq!(v["type"], "tool_use");
    }

    #[test]
    fn tool_result_tagged_as_type_tool_result() {
        let v = serde_json::to_value(tool_result_block("x", "y", false)).unwrap();
        assert_eq!(v["type"], "tool_result");
    }

    #[test]
    fn thinking_tagged_as_type_thinking() {
        let v = serde_json::to_value(IrContentBlock::Thinking { text: "t".into() }).unwrap();
        assert_eq!(v["type"], "thinking");
    }

    #[test]
    fn clone_equality() {
        let b = tool_use_block("a", "b", json!(1));
        assert_eq!(b.clone(), b);
    }

    #[test]
    fn nested_tool_result_content() {
        let nested = IrContentBlock::ToolResult {
            tool_use_id: "outer".into(),
            content: vec![
                IrContentBlock::Text {
                    text: "line1".into(),
                },
                sample_image_block(),
            ],
            is_error: false,
        };
        if let IrContentBlock::ToolResult { content, .. } = &nested {
            assert_eq!(content.len(), 2);
        }
        serde_roundtrip(&nested);
    }

    #[test]
    fn empty_text_block() {
        let b = IrContentBlock::Text {
            text: String::new(),
        };
        serde_roundtrip(&b);
    }

    #[test]
    fn tool_use_with_complex_input() {
        let input = json!({
            "query": "SELECT *",
            "params": [1, "two", null, true],
            "nested": {"a": {"b": {"c": 42}}}
        });
        let b = tool_use_block("complex_1", "sql_query", input);
        serde_roundtrip(&b);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. IrMessage
// ═══════════════════════════════════════════════════════════════════════

mod message {
    use super::*;

    #[test]
    fn text_message() {
        let m = IrMessage::text(IrRole::User, "hello");
        assert_eq!(m.role, IrRole::User);
        assert_eq!(m.text_content(), "hello");
    }

    #[test]
    fn new_with_blocks() {
        let m = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text { text: "a".into() },
                IrContentBlock::Text { text: "b".into() },
            ],
        );
        assert_eq!(m.content.len(), 2);
    }

    #[test]
    fn text_content_concatenation() {
        let m = IrMessage::new(
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
        assert_eq!(m.text_content(), "hello world");
    }

    #[test]
    fn text_content_skips_non_text() {
        let m = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text { text: "hi".into() },
                tool_use_block("1", "grep", json!({})),
                IrContentBlock::Text {
                    text: " there".into(),
                },
            ],
        );
        assert_eq!(m.text_content(), "hi there");
    }

    #[test]
    fn is_text_only_true() {
        let m = IrMessage::text(IrRole::User, "pure text");
        assert!(m.is_text_only());
    }

    #[test]
    fn is_text_only_false_with_tool_use() {
        let m = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "calling".into(),
                },
                tool_use_block("1", "tool", json!({})),
            ],
        );
        assert!(!m.is_text_only());
    }

    #[test]
    fn is_text_only_false_with_image() {
        let m = IrMessage::new(IrRole::User, vec![sample_image_block()]);
        assert!(!m.is_text_only());
    }

    #[test]
    fn is_text_only_empty_content() {
        let m = IrMessage::new(IrRole::User, vec![]);
        assert!(m.is_text_only());
    }

    #[test]
    fn tool_use_blocks_extraction() {
        let m = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text { text: "a".into() },
                tool_use_block("1", "tool_a", json!({})),
                tool_use_block("2", "tool_b", json!({})),
                IrContentBlock::Thinking { text: "hmm".into() },
            ],
        );
        let tool_uses = m.tool_use_blocks();
        assert_eq!(tool_uses.len(), 2);
    }

    #[test]
    fn tool_use_blocks_empty_when_none() {
        let m = IrMessage::text(IrRole::User, "no tools");
        assert!(m.tool_use_blocks().is_empty());
    }

    #[test]
    fn metadata_empty_by_default() {
        let m = IrMessage::text(IrRole::User, "test");
        assert!(m.metadata.is_empty());
    }

    #[test]
    fn metadata_can_be_set() {
        let mut m = IrMessage::text(IrRole::User, "test");
        m.metadata.insert("vendor".to_string(), json!("anthropic"));
        assert_eq!(m.metadata["vendor"], "anthropic");
    }

    #[test]
    fn metadata_skipped_when_empty_in_serialization() {
        let m = IrMessage::text(IrRole::User, "test");
        let v = serde_json::to_value(&m).unwrap();
        assert!(v.get("metadata").is_none());
    }

    #[test]
    fn metadata_present_when_non_empty_in_serialization() {
        let mut m = IrMessage::text(IrRole::User, "test");
        m.metadata.insert("key".to_string(), json!(42));
        let v = serde_json::to_value(&m).unwrap();
        assert!(v.get("metadata").is_some());
    }

    #[test]
    fn serde_roundtrip_text_message() {
        serde_roundtrip(&IrMessage::text(IrRole::User, "hello"));
    }

    #[test]
    fn serde_roundtrip_message_with_metadata() {
        let mut m = IrMessage::text(IrRole::System, "sys");
        m.metadata.insert("model".to_string(), json!("gpt-4"));
        m.metadata.insert("temp".to_string(), json!(0.7));
        serde_roundtrip(&m);
    }

    #[test]
    fn serde_roundtrip_multiblock_message() {
        let m = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "think".into(),
                },
                IrContentBlock::Text {
                    text: "answer".into(),
                },
                tool_use_block("t1", "exec", json!({"cmd": "ls"})),
            ],
        );
        serde_roundtrip(&m);
    }

    #[test]
    fn message_for_all_roles() {
        for role in [
            IrRole::System,
            IrRole::User,
            IrRole::Assistant,
            IrRole::Tool,
        ] {
            let m = IrMessage::text(role, "msg");
            assert_eq!(m.role, role);
            serde_roundtrip(&m);
        }
    }

    #[test]
    fn system_message_creation() {
        let m = IrMessage::text(IrRole::System, "You are a helpful assistant.");
        assert_eq!(m.role, IrRole::System);
        assert!(m.is_text_only());
    }

    #[test]
    fn tool_role_message() {
        let m = IrMessage::new(
            IrRole::Tool,
            vec![tool_result_block("tu_1", "result data", false)],
        );
        assert_eq!(m.role, IrRole::Tool);
    }

    #[test]
    fn text_content_empty_when_no_text_blocks() {
        let m = IrMessage::new(IrRole::User, vec![sample_image_block()]);
        assert_eq!(m.text_content(), "");
    }

    #[test]
    fn clone_preserves_metadata() {
        let mut m = IrMessage::text(IrRole::User, "x");
        m.metadata.insert("k".to_string(), json!("v"));
        let cloned = m.clone();
        assert_eq!(cloned.metadata["k"], "v");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. IrToolDefinition
// ═══════════════════════════════════════════════════════════════════════

mod tool_definition {
    use super::*;

    #[test]
    fn basic_construction() {
        let td = IrToolDefinition {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: simple_tool_schema(),
        };
        assert_eq!(td.name, "read_file");
    }

    #[test]
    fn serde_roundtrip() {
        let td = IrToolDefinition {
            name: "write_file".into(),
            description: "Write to a file".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"]
            }),
        };
        super::serde_roundtrip(&td);
    }

    #[test]
    fn empty_parameters() {
        let td = IrToolDefinition {
            name: "noop".into(),
            description: "Does nothing".into(),
            parameters: json!({}),
        };
        super::serde_roundtrip(&td);
    }

    #[test]
    fn null_parameters() {
        let td = IrToolDefinition {
            name: "noop".into(),
            description: "No params".into(),
            parameters: serde_json::Value::Null,
        };
        super::serde_roundtrip(&td);
    }

    #[test]
    fn complex_nested_parameters() {
        let td = IrToolDefinition {
            name: "complex_tool".into(),
            description: "A complex tool".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "config": {
                        "type": "object",
                        "properties": {
                            "nested": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "key": {"type": "string"},
                                        "value": {"type": "integer"}
                                    }
                                }
                            }
                        }
                    }
                }
            }),
        };
        super::serde_roundtrip(&td);
    }

    #[test]
    fn equality() {
        let a = IrToolDefinition {
            name: "t".into(),
            description: "d".into(),
            parameters: json!({}),
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn inequality_name() {
        let a = IrToolDefinition {
            name: "a".into(),
            description: "d".into(),
            parameters: json!({}),
        };
        let b = IrToolDefinition {
            name: "b".into(),
            description: "d".into(),
            parameters: json!({}),
        };
        assert_ne!(a, b);
    }

    #[test]
    fn json_field_names() {
        let td = IrToolDefinition {
            name: "tool".into(),
            description: "desc".into(),
            parameters: json!(null),
        };
        let v = serde_json::to_value(&td).unwrap();
        assert!(v.get("name").is_some());
        assert!(v.get("description").is_some());
        assert!(v.get("parameters").is_some());
    }

    #[test]
    fn deserialize_from_json_literal() {
        let td: IrToolDefinition = serde_json::from_value(json!({
            "name": "exec",
            "description": "Execute command",
            "parameters": {"type": "object"}
        }))
        .unwrap();
        assert_eq!(td.name, "exec");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. IrConversation
// ═══════════════════════════════════════════════════════════════════════

mod conversation {
    use super::*;

    #[test]
    fn new_is_empty() {
        let c = IrConversation::new();
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn default_is_empty() {
        let c = IrConversation::default();
        assert!(c.is_empty());
    }

    #[test]
    fn push_increments_len() {
        let c = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::Assistant, "hello"));
        assert_eq!(c.len(), 2);
        assert!(!c.is_empty());
    }

    #[test]
    fn push_chaining() {
        let c = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "u"))
            .push(IrMessage::text(IrRole::Assistant, "a"));
        assert_eq!(c.len(), 3);
    }

    #[test]
    fn from_messages() {
        let msgs = vec![
            IrMessage::text(IrRole::User, "a"),
            IrMessage::text(IrRole::Assistant, "b"),
        ];
        let c = IrConversation::from_messages(msgs);
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn system_message_found() {
        let c = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "instructions"))
            .push(IrMessage::text(IrRole::User, "hi"));
        assert_eq!(c.system_message().unwrap().text_content(), "instructions");
    }

    #[test]
    fn system_message_none_when_absent() {
        let c = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        assert!(c.system_message().is_none());
    }

    #[test]
    fn system_message_returns_first() {
        let c = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "first"))
            .push(IrMessage::text(IrRole::System, "second"));
        assert_eq!(c.system_message().unwrap().text_content(), "first");
    }

    #[test]
    fn last_assistant() {
        let c = IrConversation::new()
            .push(IrMessage::text(IrRole::Assistant, "first"))
            .push(IrMessage::text(IrRole::User, "middle"))
            .push(IrMessage::text(IrRole::Assistant, "last"));
        assert_eq!(c.last_assistant().unwrap().text_content(), "last");
    }

    #[test]
    fn last_assistant_none() {
        let c = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        assert!(c.last_assistant().is_none());
    }

    #[test]
    fn last_message() {
        let c = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "first"))
            .push(IrMessage::text(IrRole::Assistant, "last"));
        assert_eq!(c.last_message().unwrap().text_content(), "last");
    }

    #[test]
    fn last_message_none_when_empty() {
        assert!(IrConversation::new().last_message().is_none());
    }

    #[test]
    fn messages_by_role() {
        let c = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "u1"))
            .push(IrMessage::text(IrRole::Assistant, "a1"))
            .push(IrMessage::text(IrRole::User, "u2"))
            .push(IrMessage::text(IrRole::Assistant, "a2"));
        assert_eq!(c.messages_by_role(IrRole::User).len(), 2);
        assert_eq!(c.messages_by_role(IrRole::Assistant).len(), 2);
        assert_eq!(c.messages_by_role(IrRole::System).len(), 0);
    }

    #[test]
    fn tool_calls_across_messages() {
        let c = IrConversation::new()
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    tool_use_block("t1", "read", json!({})),
                    tool_use_block("t2", "write", json!({})),
                ],
            ))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![tool_result_block("t1", "ok", false)],
            ))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![tool_use_block("t3", "exec", json!({}))],
            ));
        assert_eq!(c.tool_calls().len(), 3);
    }

    #[test]
    fn tool_calls_empty() {
        let c = IrConversation::new().push(IrMessage::text(IrRole::User, "no tools"));
        assert!(c.tool_calls().is_empty());
    }

    #[test]
    fn serde_roundtrip_empty() {
        serde_roundtrip(&IrConversation::new());
    }

    #[test]
    fn serde_roundtrip_populated() {
        let c = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::Assistant, "hello"));
        serde_roundtrip(&c);
    }

    #[test]
    fn serde_roundtrip_with_tools() {
        let c = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "read /etc/hosts"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![tool_use_block(
                    "tu1",
                    "read_file",
                    json!({"path": "/etc/hosts"}),
                )],
            ))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![tool_result_block("tu1", "127.0.0.1 localhost", false)],
            ))
            .push(IrMessage::text(IrRole::Assistant, "Here is the content."));
        serde_roundtrip(&c);
    }

    #[test]
    fn direct_messages_field_access() {
        let c = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        assert_eq!(c.messages.len(), 1);
        assert_eq!(c.messages[0].role, IrRole::User);
    }

    #[test]
    fn equality_same_messages() {
        let a = IrConversation::new().push(IrMessage::text(IrRole::User, "x"));
        let b = IrConversation::new().push(IrMessage::text(IrRole::User, "x"));
        assert_eq!(a, b);
    }

    #[test]
    fn inequality_different_messages() {
        let a = IrConversation::new().push(IrMessage::text(IrRole::User, "x"));
        let b = IrConversation::new().push(IrMessage::text(IrRole::User, "y"));
        assert_ne!(a, b);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. IrUsage
// ═══════════════════════════════════════════════════════════════════════

mod usage {
    use super::*;

    #[test]
    fn default_all_zero() {
        let u = IrUsage::default();
        assert_eq!(u.input_tokens, 0);
        assert_eq!(u.output_tokens, 0);
        assert_eq!(u.total_tokens, 0);
        assert_eq!(u.cache_read_tokens, 0);
        assert_eq!(u.cache_write_tokens, 0);
    }

    #[test]
    fn from_io_computes_total() {
        let u = IrUsage::from_io(100, 50);
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 50);
        assert_eq!(u.total_tokens, 150);
        assert_eq!(u.cache_read_tokens, 0);
        assert_eq!(u.cache_write_tokens, 0);
    }

    #[test]
    fn from_io_zero() {
        let u = IrUsage::from_io(0, 0);
        assert_eq!(u.total_tokens, 0);
    }

    #[test]
    fn with_cache() {
        let u = IrUsage::with_cache(200, 100, 50, 25);
        assert_eq!(u.input_tokens, 200);
        assert_eq!(u.output_tokens, 100);
        assert_eq!(u.total_tokens, 300);
        assert_eq!(u.cache_read_tokens, 50);
        assert_eq!(u.cache_write_tokens, 25);
    }

    #[test]
    fn merge_sums_all_fields() {
        let a = IrUsage::with_cache(100, 50, 10, 5);
        let b = IrUsage::with_cache(200, 100, 20, 10);
        let merged = a.merge(b);
        assert_eq!(merged.input_tokens, 300);
        assert_eq!(merged.output_tokens, 150);
        assert_eq!(merged.total_tokens, 450);
        assert_eq!(merged.cache_read_tokens, 30);
        assert_eq!(merged.cache_write_tokens, 15);
    }

    #[test]
    fn merge_with_default() {
        let a = IrUsage::from_io(10, 20);
        let merged = a.merge(IrUsage::default());
        assert_eq!(merged, a);
    }

    #[test]
    fn merge_associativity() {
        let a = IrUsage::from_io(1, 2);
        let b = IrUsage::from_io(3, 4);
        let c = IrUsage::from_io(5, 6);
        let ab_c = a.merge(b).merge(c);
        let a_bc = a.merge(b.merge(c));
        assert_eq!(ab_c, a_bc);
    }

    #[test]
    fn merge_commutativity() {
        let a = IrUsage::from_io(10, 20);
        let b = IrUsage::from_io(30, 40);
        assert_eq!(a.merge(b), b.merge(a));
    }

    #[test]
    fn copy_semantics() {
        let a = IrUsage::from_io(1, 2);
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn serde_roundtrip() {
        super::serde_roundtrip(&IrUsage::from_io(1000, 500));
    }

    #[test]
    fn serde_roundtrip_with_cache() {
        super::serde_roundtrip(&IrUsage::with_cache(1000, 500, 100, 50));
    }

    #[test]
    fn serde_roundtrip_default() {
        super::serde_roundtrip(&IrUsage::default());
    }

    #[test]
    fn json_field_names() {
        let u = IrUsage::from_io(1, 2);
        let v = serde_json::to_value(u).unwrap();
        assert!(v.get("input_tokens").is_some());
        assert!(v.get("output_tokens").is_some());
        assert!(v.get("total_tokens").is_some());
        assert!(v.get("cache_read_tokens").is_some());
        assert!(v.get("cache_write_tokens").is_some());
    }

    #[test]
    fn large_values() {
        let u = IrUsage::from_io(u64::MAX / 2, u64::MAX / 2);
        assert_eq!(u.total_tokens, (u64::MAX / 2) * 2);
    }

    #[test]
    fn merge_multiple() {
        let usages = vec![
            IrUsage::from_io(10, 5),
            IrUsage::from_io(20, 10),
            IrUsage::from_io(30, 15),
        ];
        let total = usages
            .into_iter()
            .fold(IrUsage::default(), |acc, u| acc.merge(u));
        assert_eq!(total.input_tokens, 60);
        assert_eq!(total.output_tokens, 30);
        assert_eq!(total.total_tokens, 90);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Serialization roundtrip (JSON fidelity)
// ═══════════════════════════════════════════════════════════════════════

mod serialization {
    use super::*;

    #[test]
    fn conversation_json_double_roundtrip() {
        let c = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "q"))
            .push(IrMessage::text(IrRole::Assistant, "a"));
        let json1 = serde_json::to_string(&c).unwrap();
        let c2: IrConversation = serde_json::from_str(&json1).unwrap();
        let json2 = serde_json::to_string(&c2).unwrap();
        assert_eq!(json1, json2);
    }

    #[test]
    fn pretty_json_roundtrip() {
        let c = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let pretty = serde_json::to_string_pretty(&c).unwrap();
        let back: IrConversation = serde_json::from_str(&pretty).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn value_roundtrip() {
        let c = IrConversation::new().push(IrMessage::text(IrRole::User, "x"));
        let v = serde_json::to_value(&c).unwrap();
        let back: IrConversation = serde_json::from_value(v).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn deterministic_serialization() {
        let mut m = IrMessage::text(IrRole::User, "test");
        m.metadata.insert("a".to_string(), json!(1));
        m.metadata.insert("b".to_string(), json!(2));
        m.metadata.insert("z".to_string(), json!(26));
        // BTreeMap ensures deterministic order
        let s1 = serde_json::to_string(&m).unwrap();
        let s2 = serde_json::to_string(&m).unwrap();
        assert_eq!(s1, s2);
    }

    #[test]
    fn btreemap_metadata_ordering() {
        let mut m = IrMessage::text(IrRole::User, "test");
        m.metadata.insert("zebra".to_string(), json!(1));
        m.metadata.insert("alpha".to_string(), json!(2));
        let json = serde_json::to_string(&m).unwrap();
        let alpha_pos = json.find("alpha").unwrap();
        let zebra_pos = json.find("zebra").unwrap();
        assert!(
            alpha_pos < zebra_pos,
            "BTreeMap should order keys alphabetically"
        );
    }

    #[test]
    fn tool_definition_list_roundtrip() {
        let tools = vec![
            IrToolDefinition {
                name: "read".into(),
                description: "Read file".into(),
                parameters: simple_tool_schema(),
            },
            IrToolDefinition {
                name: "write".into(),
                description: "Write file".into(),
                parameters: json!({"type": "object"}),
            },
        ];
        let json = serde_json::to_string(&tools).unwrap();
        let back: Vec<IrToolDefinition> = serde_json::from_str(&json).unwrap();
        assert_eq!(tools, back);
    }

    #[test]
    fn usage_json_has_no_extra_fields() {
        let u = IrUsage::from_io(10, 20);
        let v = serde_json::to_value(u).unwrap();
        let obj = v.as_object().unwrap();
        assert_eq!(obj.len(), 5); // input, output, total, cache_read, cache_write
    }

    #[test]
    fn deserialize_message_missing_metadata() {
        let json = r#"{"role":"user","content":[{"type":"text","text":"hi"}]}"#;
        let m: IrMessage = serde_json::from_str(json).unwrap();
        assert!(m.metadata.is_empty());
    }

    #[test]
    fn deserialize_conversation_from_messages_array() {
        let json = r#"{"messages":[{"role":"user","content":[{"type":"text","text":"hi"}]}]}"#;
        let c: IrConversation = serde_json::from_str(json).unwrap();
        assert_eq!(c.len(), 1);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Cross-dialect IR representation
// ═══════════════════════════════════════════════════════════════════════

mod cross_dialect {
    use super::*;

    /// Represents how an OpenAI-style conversation would look in IR.
    fn openai_style_conversation() -> IrConversation {
        IrConversation::new()
            .push(IrMessage::text(IrRole::System, "You are helpful."))
            .push(IrMessage::text(IrRole::User, "What is 2+2?"))
            .push(IrMessage::text(IrRole::Assistant, "2+2 is 4."))
    }

    /// Represents how an Anthropic-style conversation would look in IR.
    fn anthropic_style_conversation() -> IrConversation {
        // Anthropic uses system as a separate field, but in IR it's just a message
        IrConversation::new()
            .push(IrMessage::text(IrRole::System, "You are helpful."))
            .push(IrMessage::text(IrRole::User, "What is 2+2?"))
            .push(IrMessage::text(IrRole::Assistant, "2+2 is 4."))
    }

    #[test]
    fn same_conversation_from_different_dialects() {
        // The IR representation is canonical regardless of source dialect.
        assert_eq!(openai_style_conversation(), anthropic_style_conversation());
    }

    #[test]
    fn openai_tool_call_pattern() {
        // OpenAI: assistant sends function_call, then a function role message returns
        let c = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "What's the weather?"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![tool_use_block(
                    "call_1",
                    "get_weather",
                    json!({"city": "NYC"}),
                )],
            ))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![tool_result_block("call_1", "72°F and sunny", false)],
            ))
            .push(IrMessage::text(
                IrRole::Assistant,
                "It's 72°F and sunny in NYC.",
            ));

        assert_eq!(c.tool_calls().len(), 1);
        assert_eq!(c.messages_by_role(IrRole::Tool).len(), 1);
    }

    #[test]
    fn anthropic_tool_call_pattern() {
        // Anthropic: assistant content has text + tool_use blocks; tool result is user msg
        // In IR, we normalize tool results into Tool role
        let c = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "What's the weather?"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "Let me check.".into(),
                    },
                    tool_use_block("tu_1", "get_weather", json!({"city": "NYC"})),
                ],
            ))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![tool_result_block("tu_1", "72°F and sunny", false)],
            ))
            .push(IrMessage::text(
                IrRole::Assistant,
                "It's 72°F and sunny in NYC.",
            ));

        assert_eq!(c.tool_calls().len(), 1);
        assert_eq!(c.len(), 4);
    }

    #[test]
    fn multi_tool_call_pattern() {
        // Both OpenAI and Anthropic support parallel tool calls
        let c = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "Compare weather"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    tool_use_block("t1", "weather", json!({"city": "NYC"})),
                    tool_use_block("t2", "weather", json!({"city": "LA"})),
                ],
            ))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![tool_result_block("t1", "72°F", false)],
            ))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![tool_result_block("t2", "85°F", false)],
            ))
            .push(IrMessage::text(IrRole::Assistant, "NYC=72, LA=85"));

        assert_eq!(c.tool_calls().len(), 2);
    }

    #[test]
    fn thinking_block_dialect() {
        // Claude extended thinking -> IR Thinking block
        let c = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "Hard math problem"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Thinking {
                        text: "Let me work through this step by step...".into(),
                    },
                    IrContentBlock::Text {
                        text: "The answer is 42.".into(),
                    },
                ],
            ));

        let last = c.last_assistant().unwrap();
        assert!(!last.is_text_only());
        assert_eq!(last.text_content(), "The answer is 42.");
    }

    #[test]
    fn image_input_pattern() {
        // Both OpenAI and Anthropic support image content
        let c = IrConversation::new()
            .push(IrMessage::new(
                IrRole::User,
                vec![
                    IrContentBlock::Text {
                        text: "What's in this image?".into(),
                    },
                    sample_image_block(),
                ],
            ))
            .push(IrMessage::text(IrRole::Assistant, "I see a cat."));

        let user_msg = &c.messages[0];
        assert!(!user_msg.is_text_only());
        assert_eq!(user_msg.text_content(), "What's in this image?");
    }

    #[test]
    fn ir_preserves_tool_error_status() {
        // All dialects distinguish success/error tool results
        let c = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![tool_result_block("t1", "Permission denied", true)],
        ));
        if let IrContentBlock::ToolResult { is_error, .. } = &c.messages[0].content[0] {
            assert!(is_error);
        } else {
            panic!("expected ToolResult");
        }
    }

    #[test]
    fn multi_turn_with_metadata() {
        // Metadata can carry dialect-specific info through the IR
        let mut sys = IrMessage::text(IrRole::System, "sys");
        sys.metadata
            .insert("source_dialect".to_string(), json!("openai"));
        let mut sys2 = IrMessage::text(IrRole::System, "sys");
        sys2.metadata
            .insert("source_dialect".to_string(), json!("anthropic"));

        // Same semantic content, different metadata
        assert_eq!(sys.text_content(), sys2.text_content());
        assert_ne!(sys.metadata, sys2.metadata);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Edge cases
// ═══════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;

    #[test]
    fn empty_conversation_accessors() {
        let c = IrConversation::new();
        assert!(c.system_message().is_none());
        assert!(c.last_assistant().is_none());
        assert!(c.last_message().is_none());
        assert!(c.tool_calls().is_empty());
        assert!(c.messages_by_role(IrRole::User).is_empty());
    }

    #[test]
    fn message_with_no_content() {
        let m = IrMessage::new(IrRole::User, vec![]);
        assert!(m.is_text_only());
        assert_eq!(m.text_content(), "");
        assert!(m.tool_use_blocks().is_empty());
        serde_roundtrip(&m);
    }

    #[test]
    fn deeply_nested_tool_result() {
        let inner = IrContentBlock::ToolResult {
            tool_use_id: "inner".into(),
            content: vec![
                IrContentBlock::Text {
                    text: "nested1".into(),
                },
                IrContentBlock::ToolResult {
                    tool_use_id: "deeper".into(),
                    content: vec![IrContentBlock::Text {
                        text: "level3".into(),
                    }],
                    is_error: false,
                },
            ],
            is_error: false,
        };
        serde_roundtrip(&inner);
    }

    #[test]
    fn very_long_text() {
        let long = "x".repeat(100_000);
        let m = IrMessage::text(IrRole::User, &long);
        assert_eq!(m.text_content().len(), 100_000);
        serde_roundtrip(&m);
    }

    #[test]
    fn unicode_content() {
        let m = IrMessage::text(IrRole::User, "こんにちは世界 🌍 مرحبا");
        assert_eq!(m.text_content(), "こんにちは世界 🌍 مرحبا");
        serde_roundtrip(&m);
    }

    #[test]
    fn special_json_characters() {
        let m = IrMessage::text(IrRole::User, r#"He said "hello\nworld""#);
        serde_roundtrip(&m);
    }

    #[test]
    fn null_json_in_tool_input() {
        let b = tool_use_block("1", "tool", serde_json::Value::Null);
        serde_roundtrip(&b);
    }

    #[test]
    fn array_json_in_tool_input() {
        let b = tool_use_block("1", "tool", json!([1, 2, 3]));
        serde_roundtrip(&b);
    }

    #[test]
    fn large_metadata_map() {
        let mut m = IrMessage::text(IrRole::User, "test");
        for i in 0..100 {
            m.metadata.insert(format!("key_{i}"), json!(i));
        }
        assert_eq!(m.metadata.len(), 100);
        serde_roundtrip(&m);
    }

    #[test]
    fn conversation_many_messages() {
        let mut c = IrConversation::new();
        for i in 0..500 {
            let role = if i % 2 == 0 {
                IrRole::User
            } else {
                IrRole::Assistant
            };
            c = c.push(IrMessage::text(role, format!("msg {i}")));
        }
        assert_eq!(c.len(), 500);
        assert_eq!(c.messages_by_role(IrRole::User).len(), 250);
        assert_eq!(c.messages_by_role(IrRole::Assistant).len(), 250);
    }

    #[test]
    fn many_tool_calls_in_single_message() {
        let blocks: Vec<IrContentBlock> = (0..50)
            .map(|i| tool_use_block(&format!("t{i}"), &format!("tool_{i}"), json!({"i": i})))
            .collect();
        let m = IrMessage::new(IrRole::Assistant, blocks);
        assert_eq!(m.tool_use_blocks().len(), 50);
    }

    #[test]
    fn empty_tool_result_content() {
        let b = IrContentBlock::ToolResult {
            tool_use_id: "t1".into(),
            content: vec![],
            is_error: false,
        };
        serde_roundtrip(&b);
    }

    #[test]
    fn image_with_large_data() {
        let data = "A".repeat(10_000);
        let b = IrContentBlock::Image {
            media_type: "image/jpeg".into(),
            data,
        };
        serde_roundtrip(&b);
    }

    #[test]
    fn image_various_media_types() {
        for mime in [
            "image/png",
            "image/jpeg",
            "image/gif",
            "image/webp",
            "image/svg+xml",
        ] {
            let b = IrContentBlock::Image {
                media_type: mime.into(),
                data: "base64data".into(),
            };
            serde_roundtrip(&b);
        }
    }

    #[test]
    fn mixed_content_block_types() {
        let m = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "thinking".into(),
                },
                IrContentBlock::Text {
                    text: "here's what I found:".into(),
                },
                tool_use_block("t1", "search", json!({"q": "test"})),
                IrContentBlock::Text {
                    text: "and also".into(),
                },
                sample_image_block(),
            ],
        );
        assert!(!m.is_text_only());
        assert_eq!(m.tool_use_blocks().len(), 1);
        assert_eq!(m.text_content(), "here's what I found:and also");
    }

    #[test]
    fn conversation_with_all_roles() {
        let c = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "u"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![tool_use_block("t1", "tool", json!({}))],
            ))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![tool_result_block("t1", "result", false)],
            ));
        assert_eq!(c.len(), 4);
        for role in [
            IrRole::System,
            IrRole::User,
            IrRole::Assistant,
            IrRole::Tool,
        ] {
            assert_eq!(c.messages_by_role(role).len(), 1);
        }
    }

    #[test]
    fn metadata_with_nested_json() {
        let mut m = IrMessage::text(IrRole::User, "test");
        m.metadata.insert(
            "complex".to_string(),
            json!({
                "nested": {
                    "array": [1, 2, {"deep": true}],
                    "null_val": null
                }
            }),
        );
        serde_roundtrip(&m);
    }

    #[test]
    fn empty_string_tool_name() {
        let b = tool_use_block("id", "", json!({}));
        serde_roundtrip(&b);
    }

    #[test]
    fn tool_definition_empty_description() {
        let td = IrToolDefinition {
            name: "tool".into(),
            description: String::new(),
            parameters: json!({}),
        };
        serde_roundtrip(&td);
    }

    #[test]
    fn conversation_clone_independence() {
        let c1 = IrConversation::new().push(IrMessage::text(IrRole::User, "original"));
        let c2 = c1.clone();
        let c1_extended = c1.push(IrMessage::text(IrRole::Assistant, "reply"));
        assert_eq!(c2.len(), 1);
        assert_eq!(c1_extended.len(), 2);
    }

    #[test]
    fn usage_merge_identity() {
        let u = IrUsage::from_io(42, 58);
        let zero = IrUsage::default();
        assert_eq!(u.merge(zero), u);
        assert_eq!(zero.merge(u), u);
    }

    #[test]
    fn conversation_json_stability() {
        // Verify that serializing, deserializing, and re-serializing
        // produces byte-identical JSON (determinism).
        let c = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "hello"));
        let j1 = serde_json::to_string(&c).unwrap();
        let c2: IrConversation = serde_json::from_str(&j1).unwrap();
        let j2 = serde_json::to_string(&c2).unwrap();
        let c3: IrConversation = serde_json::from_str(&j2).unwrap();
        let j3 = serde_json::to_string(&c3).unwrap();
        assert_eq!(j1, j2);
        assert_eq!(j2, j3);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Integration: full conversation flows
// ═══════════════════════════════════════════════════════════════════════

mod integration {
    use super::*;

    #[test]
    fn full_agentic_loop() {
        let c = IrConversation::new()
            .push(IrMessage::text(
                IrRole::System,
                "You are a coding assistant.",
            ))
            .push(IrMessage::text(
                IrRole::User,
                "Read main.rs and fix the bug",
            ))
            // Turn 1: assistant reads file
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Thinking {
                        text: "I need to read the file first.".into(),
                    },
                    tool_use_block("tu1", "read_file", json!({"path": "main.rs"})),
                ],
            ))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![tool_result_block("tu1", "fn main() { bug() }", false)],
            ))
            // Turn 2: assistant writes fix
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "I found the bug. Let me fix it.".into(),
                    },
                    tool_use_block(
                        "tu2",
                        "write_file",
                        json!({"path": "main.rs", "content": "fn main() { fixed() }"}),
                    ),
                ],
            ))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![tool_result_block("tu2", "File written.", false)],
            ))
            // Turn 3: assistant confirms
            .push(IrMessage::text(IrRole::Assistant, "Done! I fixed the bug."));

        assert_eq!(c.len(), 7);
        assert_eq!(c.tool_calls().len(), 2);
        assert_eq!(
            c.system_message().unwrap().text_content(),
            "You are a coding assistant."
        );
        assert_eq!(
            c.last_assistant().unwrap().text_content(),
            "Done! I fixed the bug."
        );
        assert_eq!(c.messages_by_role(IrRole::Tool).len(), 2);
    }

    #[test]
    fn tool_error_recovery_flow() {
        let c = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "Read /etc/shadow"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![tool_use_block(
                    "tu1",
                    "read_file",
                    json!({"path": "/etc/shadow"}),
                )],
            ))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![tool_result_block("tu1", "Permission denied", true)],
            ))
            .push(IrMessage::text(
                IrRole::Assistant,
                "I don't have permission to read that file.",
            ));

        // Verify error is tracked
        if let IrContentBlock::ToolResult { is_error, .. } = &c.messages[2].content[0] {
            assert!(is_error);
        }
        assert_eq!(c.tool_calls().len(), 1);
    }

    #[test]
    fn usage_tracking_across_turns() {
        let turn1 = IrUsage::from_io(500, 200);
        let turn2 = IrUsage::with_cache(400, 300, 100, 0);
        let turn3 = IrUsage::from_io(600, 150);

        let total = turn1.merge(turn2).merge(turn3);
        assert_eq!(total.input_tokens, 1500);
        assert_eq!(total.output_tokens, 650);
        assert_eq!(total.total_tokens, 2150);
        assert_eq!(total.cache_read_tokens, 100);
    }

    #[test]
    fn conversation_and_tools_together() {
        let tools = vec![
            IrToolDefinition {
                name: "search".into(),
                description: "Search files".into(),
                parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
            },
            IrToolDefinition {
                name: "edit".into(),
                description: "Edit file".into(),
                parameters: json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}),
            },
        ];

        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "Find and edit"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![tool_use_block("s1", "search", json!({"q": "bug"}))],
            ));

        // Tools and conversation serialize independently
        let tools_json = serde_json::to_string(&tools).unwrap();
        let conv_json = serde_json::to_string(&conv).unwrap();

        let tools_back: Vec<IrToolDefinition> = serde_json::from_str(&tools_json).unwrap();
        let conv_back: IrConversation = serde_json::from_str(&conv_json).unwrap();

        assert_eq!(tools, tools_back);
        assert_eq!(conv, conv_back);
    }

    #[test]
    fn multimodal_conversation() {
        let c = IrConversation::new()
            .push(IrMessage::new(
                IrRole::User,
                vec![
                    IrContentBlock::Text {
                        text: "Describe these images:".into(),
                    },
                    IrContentBlock::Image {
                        media_type: "image/png".into(),
                        data: "img1base64".into(),
                    },
                    IrContentBlock::Image {
                        media_type: "image/jpeg".into(),
                        data: "img2base64".into(),
                    },
                ],
            ))
            .push(IrMessage::text(
                IrRole::Assistant,
                "The first image shows a cat, the second a dog.",
            ));

        let user_msg = &c.messages[0];
        assert!(!user_msg.is_text_only());
        assert_eq!(user_msg.content.len(), 3);
        serde_roundtrip(&c);
    }
}
