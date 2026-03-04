// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for IR normalization and conversation handling.
//!
//! Covers conversation construction, message normalization, role handling,
//! content types, serialization roundtrips, and conversation operations.

use abp_ir::normalize::*;
use abp_ir::*;
use serde_json::json;
use std::collections::BTreeMap;

// ═══════════════════════════════════════════════════════════════════════
// 1. IrConversation construction
// ═══════════════════════════════════════════════════════════════════════

mod conversation_construction {
    use super::*;

    #[test]
    fn empty_conversation() {
        let conv = IrConversation::new();
        assert!(conv.is_empty());
        assert_eq!(conv.len(), 0);
        assert!(conv.messages.is_empty());
    }

    #[test]
    fn empty_conversation_default() {
        let conv = IrConversation::default();
        assert!(conv.is_empty());
        assert_eq!(conv.len(), 0);
    }

    #[test]
    fn single_message() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hello"));
        assert_eq!(conv.len(), 1);
        assert!(!conv.is_empty());
    }

    #[test]
    fn multi_message_conversation() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Be helpful"))
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::Assistant, "hello"));
        assert_eq!(conv.len(), 3);
    }

    #[test]
    fn builder_pattern_chaining() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "a"))
            .push(IrMessage::text(IrRole::User, "b"))
            .push(IrMessage::text(IrRole::User, "c"))
            .push(IrMessage::text(IrRole::User, "d"));
        assert_eq!(conv.len(), 4);
        assert_eq!(conv.messages[3].text_content(), "d");
    }

    #[test]
    fn from_messages_vec() {
        let msgs = vec![
            IrMessage::text(IrRole::User, "one"),
            IrMessage::text(IrRole::Assistant, "two"),
        ];
        let conv = IrConversation::from_messages(msgs);
        assert_eq!(conv.len(), 2);
    }

    #[test]
    fn from_messages_empty_vec() {
        let conv = IrConversation::from_messages(vec![]);
        assert!(conv.is_empty());
    }

    #[test]
    fn messages_with_different_roles() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "usr"))
            .push(IrMessage::text(IrRole::Assistant, "asst"))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "result".into(),
                    }],
                    is_error: false,
                }],
            ));
        assert_eq!(conv.len(), 4);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[1].role, IrRole::User);
        assert_eq!(conv.messages[2].role, IrRole::Assistant);
        assert_eq!(conv.messages[3].role, IrRole::Tool);
    }

    #[test]
    fn push_preserves_order() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "first"))
            .push(IrMessage::text(IrRole::User, "second"))
            .push(IrMessage::text(IrRole::User, "third"));
        assert_eq!(conv.messages[0].text_content(), "first");
        assert_eq!(conv.messages[1].text_content(), "second");
        assert_eq!(conv.messages[2].text_content(), "third");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. IrMessage normalization
// ═══════════════════════════════════════════════════════════════════════

mod message_normalization {
    use super::*;

    // ── trim_text pass ─────────────────────────────────────────────────

    #[test]
    fn trim_text_strips_leading_trailing_whitespace() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "  hello world  "));
        let trimmed = trim_text(&conv);
        assert_eq!(trimmed.messages[0].text_content(), "hello world");
    }

    #[test]
    fn trim_text_handles_newlines_and_tabs() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "\n\thello\t\n"));
        let trimmed = trim_text(&conv);
        assert_eq!(trimmed.messages[0].text_content(), "hello");
    }

    #[test]
    fn trim_text_preserves_interior_whitespace() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "  hello   world  "));
        let trimmed = trim_text(&conv);
        assert_eq!(trimmed.messages[0].text_content(), "hello   world");
    }

    #[test]
    fn trim_text_all_whitespace_becomes_empty() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "   "));
        let trimmed = trim_text(&conv);
        assert_eq!(trimmed.messages[0].text_content(), "");
    }

    #[test]
    fn trim_text_leaves_tool_use_untouched() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "  search  ".into(),
                input: json!({"q": "  spaces  "}),
            }],
        ));
        let trimmed = trim_text(&conv);
        if let IrContentBlock::ToolUse { name, input, .. } = &trimmed.messages[0].content[0] {
            assert_eq!(name, "  search  ");
            assert_eq!(input["q"], "  spaces  ");
        } else {
            panic!("expected ToolUse block");
        }
    }

    #[test]
    fn trim_text_leaves_image_untouched() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "  base64data  ".into(),
            }],
        ));
        let trimmed = trim_text(&conv);
        if let IrContentBlock::Image { data, .. } = &trimmed.messages[0].content[0] {
            assert_eq!(data, "  base64data  ");
        } else {
            panic!("expected Image block");
        }
    }

    #[test]
    fn trim_text_leaves_thinking_untouched() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "  thinking  ".into(),
            }],
        ));
        let trimmed = trim_text(&conv);
        if let IrContentBlock::Thinking { text } = &trimmed.messages[0].content[0] {
            assert_eq!(text, "  thinking  ");
        } else {
            panic!("expected Thinking block");
        }
    }

    // ── Tool use messages ──────────────────────────────────────────────

    #[test]
    fn tool_use_with_complex_parameters() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_42".into(),
                name: "write_file".into(),
                input: json!({
                    "path": "/tmp/test.rs",
                    "content": "fn main() { println!(\"hello\"); }",
                    "options": {"overwrite": true, "mode": 0o644}
                }),
            }],
        );
        let blocks = msg.tool_use_blocks();
        assert_eq!(blocks.len(), 1);
        if let IrContentBlock::ToolUse { input, .. } = blocks[0] {
            assert_eq!(input["path"], "/tmp/test.rs");
            assert!(input["options"]["overwrite"].as_bool().unwrap());
        }
    }

    #[test]
    fn tool_result_success() {
        let msg = IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_42".into(),
                content: vec![IrContentBlock::Text {
                    text: "File written successfully".into(),
                }],
                is_error: false,
            }],
        );
        if let IrContentBlock::ToolResult { is_error, .. } = &msg.content[0] {
            assert!(!is_error);
        }
    }

    #[test]
    fn tool_result_error() {
        let msg = IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_42".into(),
                content: vec![IrContentBlock::Text {
                    text: "Permission denied".into(),
                }],
                is_error: true,
            }],
        );
        if let IrContentBlock::ToolResult {
            is_error, content, ..
        } = &msg.content[0]
        {
            assert!(is_error);
            if let IrContentBlock::Text { text } = &content[0] {
                assert_eq!(text, "Permission denied");
            }
        }
    }

    #[test]
    fn tool_result_with_multiple_content_blocks() {
        let msg = IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: vec![
                    IrContentBlock::Text {
                        text: "Line 1\n".into(),
                    },
                    IrContentBlock::Text {
                        text: "Line 2\n".into(),
                    },
                ],
                is_error: false,
            }],
        );
        if let IrContentBlock::ToolResult { content, .. } = &msg.content[0] {
            assert_eq!(content.len(), 2);
        }
    }

    // ── System messages ────────────────────────────────────────────────

    #[test]
    fn system_message_text_content() {
        let msg = IrMessage::text(IrRole::System, "You are a helpful assistant.");
        assert_eq!(msg.role, IrRole::System);
        assert_eq!(msg.text_content(), "You are a helpful assistant.");
        assert!(msg.is_text_only());
    }

    #[test]
    fn system_message_empty_text() {
        let msg = IrMessage::text(IrRole::System, "");
        assert_eq!(msg.text_content(), "");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Role handling
// ═══════════════════════════════════════════════════════════════════════

mod role_handling {
    use super::*;

    #[test]
    fn all_ir_role_variants_exist() {
        let roles = [
            IrRole::System,
            IrRole::User,
            IrRole::Assistant,
            IrRole::Tool,
        ];
        assert_eq!(roles.len(), 4);
    }

    #[test]
    fn normalize_role_canonical_names() {
        assert_eq!(normalize_role("system"), Some(IrRole::System));
        assert_eq!(normalize_role("user"), Some(IrRole::User));
        assert_eq!(normalize_role("assistant"), Some(IrRole::Assistant));
        assert_eq!(normalize_role("tool"), Some(IrRole::Tool));
    }

    #[test]
    fn normalize_role_gemini_model_alias() {
        assert_eq!(normalize_role("model"), Some(IrRole::Assistant));
    }

    #[test]
    fn normalize_role_openai_function_alias() {
        assert_eq!(normalize_role("function"), Some(IrRole::Tool));
    }

    #[test]
    fn normalize_role_openai_developer_alias() {
        assert_eq!(normalize_role("developer"), Some(IrRole::System));
    }

    #[test]
    fn normalize_role_anthropic_human_alias() {
        assert_eq!(normalize_role("human"), Some(IrRole::User));
    }

    #[test]
    fn normalize_role_bot_alias() {
        assert_eq!(normalize_role("bot"), Some(IrRole::Assistant));
    }

    #[test]
    fn normalize_role_unknown_returns_none() {
        assert!(normalize_role("narrator").is_none());
        assert!(normalize_role("moderator").is_none());
        assert!(normalize_role("").is_none());
    }

    #[test]
    fn normalize_role_case_sensitive() {
        assert!(normalize_role("System").is_none());
        assert!(normalize_role("USER").is_none());
        assert!(normalize_role("ASSISTANT").is_none());
    }

    #[test]
    fn role_transitions_in_conversation() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "q1"))
            .push(IrMessage::text(IrRole::Assistant, "a1"))
            .push(IrMessage::text(IrRole::User, "q2"))
            .push(IrMessage::text(IrRole::Assistant, "a2"));
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[1].role, IrRole::User);
        assert_eq!(conv.messages[2].role, IrRole::Assistant);
        assert_eq!(conv.messages[3].role, IrRole::User);
        assert_eq!(conv.messages[4].role, IrRole::Assistant);
    }

    #[test]
    fn consecutive_same_role_allowed() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "part1"))
            .push(IrMessage::text(IrRole::User, "part2"));
        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[1].role, IrRole::User);
    }

    #[test]
    fn ir_role_equality() {
        assert_eq!(IrRole::System, IrRole::System);
        assert_eq!(IrRole::User, IrRole::User);
        assert_ne!(IrRole::User, IrRole::Assistant);
        assert_ne!(IrRole::Tool, IrRole::System);
    }

    #[test]
    fn ir_role_copy_semantics() {
        let role = IrRole::User;
        let copied = role;
        assert_eq!(role, copied);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Content types
// ═══════════════════════════════════════════════════════════════════════

mod content_types {
    use super::*;

    #[test]
    fn plain_text_block() {
        let block = IrContentBlock::Text {
            text: "hello world".into(),
        };
        if let IrContentBlock::Text { text } = &block {
            assert_eq!(text, "hello world");
        }
    }

    #[test]
    fn text_only_message() {
        let msg = IrMessage::text(IrRole::User, "simple text");
        assert!(msg.is_text_only());
        assert_eq!(msg.content.len(), 1);
    }

    #[test]
    fn empty_text_block() {
        let msg = IrMessage::text(IrRole::User, "");
        assert!(msg.is_text_only());
        assert_eq!(msg.text_content(), "");
    }

    #[test]
    fn tool_use_block_structure() {
        let block = IrContentBlock::ToolUse {
            id: "call_123".into(),
            name: "search".into(),
            input: json!({"query": "rust", "limit": 10}),
        };
        if let IrContentBlock::ToolUse { id, name, input } = &block {
            assert_eq!(id, "call_123");
            assert_eq!(name, "search");
            assert_eq!(input["query"], "rust");
            assert_eq!(input["limit"], 10);
        }
    }

    #[test]
    fn tool_result_block_structure() {
        let block = IrContentBlock::ToolResult {
            tool_use_id: "call_123".into(),
            content: vec![IrContentBlock::Text {
                text: "found 5 results".into(),
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
            assert_eq!(content.len(), 1);
            assert!(!is_error);
        }
    }

    #[test]
    fn image_block_structure() {
        let block = IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "iVBORw0KGgo=".into(),
        };
        if let IrContentBlock::Image { media_type, data } = &block {
            assert_eq!(media_type, "image/png");
            assert_eq!(data, "iVBORw0KGgo=");
        }
    }

    #[test]
    fn thinking_block_structure() {
        let block = IrContentBlock::Thinking {
            text: "Let me think about this...".into(),
        };
        if let IrContentBlock::Thinking { text } = &block {
            assert_eq!(text, "Let me think about this...");
        }
    }

    #[test]
    fn mixed_content_in_single_message() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "reasoning".into(),
                },
                IrContentBlock::Text {
                    text: "Here is the result.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "read_file".into(),
                    input: json!({"path": "/tmp/f.txt"}),
                },
            ],
        );
        assert_eq!(msg.content.len(), 3);
        assert!(!msg.is_text_only());
        assert_eq!(msg.text_content(), "Here is the result.");
        assert_eq!(msg.tool_use_blocks().len(), 1);
    }

    #[test]
    fn is_text_only_false_with_tool_use() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text { text: "ok".into() },
                IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "calc".into(),
                    input: json!({}),
                },
            ],
        );
        assert!(!msg.is_text_only());
    }

    #[test]
    fn is_text_only_true_with_multiple_text_blocks() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "part1".into(),
                },
                IrContentBlock::Text {
                    text: "part2".into(),
                },
            ],
        );
        assert!(msg.is_text_only());
    }

    #[test]
    fn text_content_concatenates_all_text_blocks() {
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
    fn text_content_skips_non_text_blocks() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "before".into(),
                },
                IrContentBlock::ToolUse {
                    id: "c".into(),
                    name: "f".into(),
                    input: json!({}),
                },
                IrContentBlock::Text {
                    text: "after".into(),
                },
            ],
        );
        assert_eq!(msg.text_content(), "beforeafter");
    }

    #[test]
    fn tool_use_blocks_returns_only_tool_use() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text { text: "ok".into() },
                IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "search".into(),
                    input: json!({}),
                },
                IrContentBlock::ToolUse {
                    id: "c2".into(),
                    name: "calc".into(),
                    input: json!({}),
                },
            ],
        );
        assert_eq!(msg.tool_use_blocks().len(), 2);
    }

    #[test]
    fn tool_use_blocks_empty_for_text_only() {
        let msg = IrMessage::text(IrRole::User, "no tools here");
        assert!(msg.tool_use_blocks().is_empty());
    }

    #[test]
    fn empty_content_message() {
        let msg = IrMessage::new(IrRole::User, vec![]);
        assert!(msg.content.is_empty());
        assert!(msg.is_text_only());
        assert_eq!(msg.text_content(), "");
        assert!(msg.tool_use_blocks().is_empty());
    }

    #[test]
    fn message_with_metadata() {
        let mut meta = BTreeMap::new();
        meta.insert("source".into(), json!("test"));
        meta.insert("timestamp".into(), json!(1234567890));
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text {
                text: "hello".into(),
            }],
            metadata: meta,
        };
        assert_eq!(msg.metadata.len(), 2);
        assert_eq!(msg.metadata["source"], json!("test"));
    }

    #[test]
    fn new_message_has_empty_metadata() {
        let msg = IrMessage::new(IrRole::User, vec![]);
        assert!(msg.metadata.is_empty());
    }

    #[test]
    fn text_message_has_empty_metadata() {
        let msg = IrMessage::text(IrRole::User, "hi");
        assert!(msg.metadata.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Serialization
// ═══════════════════════════════════════════════════════════════════════

mod serialization {
    use super::*;

    #[test]
    fn ir_role_roundtrip_json() {
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
    fn ir_role_serialization_format() {
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
    fn text_block_roundtrip() {
        let block = IrContentBlock::Text {
            text: "hello world".into(),
        };
        let json_str = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json_str).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn tool_use_block_roundtrip() {
        let block = IrContentBlock::ToolUse {
            id: "call_1".into(),
            name: "search".into(),
            input: json!({"q": "rust", "limit": 5}),
        };
        let json_str = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json_str).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn tool_result_block_roundtrip() {
        let block = IrContentBlock::ToolResult {
            tool_use_id: "call_1".into(),
            content: vec![IrContentBlock::Text {
                text: "result data".into(),
            }],
            is_error: true,
        };
        let json_str = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json_str).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn image_block_roundtrip() {
        let block = IrContentBlock::Image {
            media_type: "image/jpeg".into(),
            data: "base64data==".into(),
        };
        let json_str = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json_str).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn thinking_block_roundtrip() {
        let block = IrContentBlock::Thinking {
            text: "Let me think...".into(),
        };
        let json_str = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json_str).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn ir_message_roundtrip() {
        let msg = IrMessage::text(IrRole::User, "hello");
        let json_str = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json_str).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn ir_message_with_metadata_roundtrip() {
        let mut meta = BTreeMap::new();
        meta.insert("key".into(), json!("value"));
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text {
                text: "hello".into(),
            }],
            metadata: meta,
        };
        let json_str = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json_str).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn ir_conversation_roundtrip() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::Assistant, "hello"));
        let json_str = serde_json::to_string(&conv).unwrap();
        let back: IrConversation = serde_json::from_str(&json_str).unwrap();
        assert_eq!(conv, back);
    }

    #[test]
    fn empty_conversation_roundtrip() {
        let conv = IrConversation::new();
        let json_str = serde_json::to_string(&conv).unwrap();
        let back: IrConversation = serde_json::from_str(&json_str).unwrap();
        assert_eq!(conv, back);
    }

    #[test]
    fn metadata_btreemap_deterministic_order() {
        let mut meta = BTreeMap::new();
        meta.insert("zebra".into(), json!(1));
        meta.insert("apple".into(), json!(2));
        meta.insert("mango".into(), json!(3));
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text { text: "hi".into() }],
            metadata: meta,
        };
        let json_str = serde_json::to_string(&msg).unwrap();
        // BTreeMap should serialize keys in alphabetical order
        let apple_pos = json_str.find("apple").unwrap();
        let mango_pos = json_str.find("mango").unwrap();
        let zebra_pos = json_str.find("zebra").unwrap();
        assert!(apple_pos < mango_pos);
        assert!(mango_pos < zebra_pos);
    }

    #[test]
    fn empty_metadata_omitted_in_json() {
        let msg = IrMessage::text(IrRole::User, "hi");
        let json_str = serde_json::to_string(&msg).unwrap();
        assert!(!json_str.contains("metadata"));
    }

    #[test]
    fn non_empty_metadata_included_in_json() {
        let mut meta = BTreeMap::new();
        meta.insert("key".into(), json!("val"));
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text { text: "hi".into() }],
            metadata: meta,
        };
        let json_str = serde_json::to_string(&msg).unwrap();
        assert!(json_str.contains("metadata"));
    }

    #[test]
    fn content_block_tagged_serialization() {
        let text_json = serde_json::to_value(IrContentBlock::Text { text: "hi".into() }).unwrap();
        assert_eq!(text_json["type"], "text");

        let tool_json = serde_json::to_value(IrContentBlock::ToolUse {
            id: "c".into(),
            name: "f".into(),
            input: json!({}),
        })
        .unwrap();
        assert_eq!(tool_json["type"], "tool_use");

        let result_json = serde_json::to_value(IrContentBlock::ToolResult {
            tool_use_id: "c".into(),
            content: vec![],
            is_error: false,
        })
        .unwrap();
        assert_eq!(result_json["type"], "tool_result");

        let img_json = serde_json::to_value(IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "abc".into(),
        })
        .unwrap();
        assert_eq!(img_json["type"], "image");

        let think_json =
            serde_json::to_value(IrContentBlock::Thinking { text: "hmm".into() }).unwrap();
        assert_eq!(think_json["type"], "thinking");
    }

    #[test]
    fn tool_definition_roundtrip() {
        let tool = IrToolDefinition {
            name: "calculator".into(),
            description: "Evaluate math".into(),
            parameters: json!({"type": "object", "properties": {"expr": {"type": "string"}}}),
        };
        let json_str = serde_json::to_string(&tool).unwrap();
        let back: IrToolDefinition = serde_json::from_str(&json_str).unwrap();
        assert_eq!(tool, back);
    }

    #[test]
    fn complex_conversation_roundtrip() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "You are helpful."))
            .push(IrMessage::text(IrRole::User, "Read the file"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "I'll read it.".into(),
                    },
                    IrContentBlock::ToolUse {
                        id: "c1".into(),
                        name: "read_file".into(),
                        input: json!({"path": "/tmp/a.txt"}),
                    },
                ],
            ))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "c1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "file contents here".into(),
                    }],
                    is_error: false,
                }],
            ))
            .push(IrMessage::text(
                IrRole::Assistant,
                "The file contains: file contents here",
            ));
        let json_str = serde_json::to_string(&conv).unwrap();
        let back: IrConversation = serde_json::from_str(&json_str).unwrap();
        assert_eq!(conv, back);
    }

    #[test]
    fn deserialize_message_without_metadata_field() {
        let json_str = r#"{"role":"user","content":[{"type":"text","text":"hi"}]}"#;
        let msg: IrMessage = serde_json::from_str(json_str).unwrap();
        assert_eq!(msg.role, IrRole::User);
        assert!(msg.metadata.is_empty());
    }

    #[test]
    fn ir_usage_roundtrip() {
        let usage = IrUsage::from_io(100, 200);
        let json_str = serde_json::to_string(&usage).unwrap();
        let back: IrUsage = serde_json::from_str(&json_str).unwrap();
        assert_eq!(usage, back);
    }

    #[test]
    fn ir_usage_with_cache_roundtrip() {
        let usage = IrUsage::with_cache(100, 200, 50, 30);
        let json_str = serde_json::to_string(&usage).unwrap();
        let back: IrUsage = serde_json::from_str(&json_str).unwrap();
        assert_eq!(usage, back);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Conversation operations
// ═══════════════════════════════════════════════════════════════════════

mod conversation_operations {
    use super::*;

    #[test]
    fn last_message_on_empty() {
        let conv = IrConversation::new();
        assert!(conv.last_message().is_none());
    }

    #[test]
    fn last_message_returns_final() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "first"))
            .push(IrMessage::text(IrRole::Assistant, "last"));
        let last = conv.last_message().unwrap();
        assert_eq!(last.role, IrRole::Assistant);
        assert_eq!(last.text_content(), "last");
    }

    #[test]
    fn system_message_found() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "hi"));
        let sys = conv.system_message().unwrap();
        assert_eq!(sys.role, IrRole::System);
        assert_eq!(sys.text_content(), "sys");
    }

    #[test]
    fn system_message_none_when_absent() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        assert!(conv.system_message().is_none());
    }

    #[test]
    fn system_message_returns_first() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "first sys"))
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::System, "second sys"));
        let sys = conv.system_message().unwrap();
        assert_eq!(sys.text_content(), "first sys");
    }

    #[test]
    fn last_assistant_found() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::Assistant, "first"))
            .push(IrMessage::text(IrRole::User, "q"))
            .push(IrMessage::text(IrRole::Assistant, "second"));
        let last = conv.last_assistant().unwrap();
        assert_eq!(last.text_content(), "second");
    }

    #[test]
    fn last_assistant_none_when_absent() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        assert!(conv.last_assistant().is_none());
    }

    #[test]
    fn messages_by_role_filters_correctly() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "q1"))
            .push(IrMessage::text(IrRole::Assistant, "a1"))
            .push(IrMessage::text(IrRole::User, "q2"))
            .push(IrMessage::text(IrRole::Assistant, "a2"))
            .push(IrMessage::text(IrRole::User, "q3"));
        let users = conv.messages_by_role(IrRole::User);
        assert_eq!(users.len(), 3);
        let assistants = conv.messages_by_role(IrRole::Assistant);
        assert_eq!(assistants.len(), 2);
    }

    #[test]
    fn messages_by_role_empty_result() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let tools = conv.messages_by_role(IrRole::Tool);
        assert!(tools.is_empty());
    }

    #[test]
    fn tool_calls_across_conversation() {
        let conv = IrConversation::new()
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "search".into(),
                    input: json!({}),
                }],
            ))
            .push(IrMessage::text(IrRole::User, "interlude"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "c2".into(),
                    name: "calc".into(),
                    input: json!({}),
                }],
            ));
        let calls = conv.tool_calls();
        assert_eq!(calls.len(), 2);
    }

    #[test]
    fn tool_calls_empty_when_none() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::Assistant, "hello"));
        assert!(conv.tool_calls().is_empty());
    }

    #[test]
    fn clone_and_modify() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::Assistant, "hello"));
        let mut cloned = conv.clone();
        cloned
            .messages
            .push(IrMessage::text(IrRole::User, "follow up"));
        assert_eq!(conv.len(), 2);
        assert_eq!(cloned.len(), 3);
    }

    #[test]
    fn conversation_equality() {
        let a = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::Assistant, "hello"));
        let b = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::Assistant, "hello"));
        assert_eq!(a, b);
    }

    #[test]
    fn conversation_inequality_different_content() {
        let a = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let b = IrConversation::new().push(IrMessage::text(IrRole::User, "bye"));
        assert_ne!(a, b);
    }

    #[test]
    fn conversation_inequality_different_order() {
        let a = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::Assistant, "hello"));
        let b = IrConversation::new()
            .push(IrMessage::text(IrRole::Assistant, "hello"))
            .push(IrMessage::text(IrRole::User, "hi"));
        assert_ne!(a, b);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Normalization passes
// ═══════════════════════════════════════════════════════════════════════

mod normalization_passes {
    use super::*;

    // ── dedup_system ───────────────────────────────────────────────────

    #[test]
    fn dedup_system_merges_scattered_systems() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Be concise."))
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::System, "Be helpful."));
        let deduped = dedup_system(&conv);
        assert_eq!(deduped.messages_by_role(IrRole::System).len(), 1);
        assert_eq!(
            deduped.messages[0].text_content(),
            "Be concise.\nBe helpful."
        );
    }

    #[test]
    fn dedup_system_preserves_non_system_order() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "q1"))
            .push(IrMessage::text(IrRole::System, "sys2"))
            .push(IrMessage::text(IrRole::Assistant, "a1"))
            .push(IrMessage::text(IrRole::User, "q2"));
        let deduped = dedup_system(&conv);
        assert_eq!(deduped.messages[0].role, IrRole::System);
        assert_eq!(deduped.messages[1].role, IrRole::User);
        assert_eq!(deduped.messages[1].text_content(), "q1");
        assert_eq!(deduped.messages[2].role, IrRole::Assistant);
        assert_eq!(deduped.messages[3].role, IrRole::User);
        assert_eq!(deduped.messages[3].text_content(), "q2");
    }

    #[test]
    fn dedup_system_no_system_messages() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::Assistant, "hello"));
        let deduped = dedup_system(&conv);
        assert_eq!(deduped.len(), 2);
        assert!(deduped.system_message().is_none());
    }

    #[test]
    fn dedup_system_single_system_preserved() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Be nice."))
            .push(IrMessage::text(IrRole::User, "hi"));
        let deduped = dedup_system(&conv);
        assert_eq!(deduped.system_message().unwrap().text_content(), "Be nice.");
    }

    // ── strip_empty ────────────────────────────────────────────────────

    #[test]
    fn strip_empty_removes_no_content_messages() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "hi"),
            IrMessage::new(IrRole::Assistant, vec![]),
            IrMessage::text(IrRole::Assistant, "hello"),
        ]);
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
    fn strip_empty_all_empty_yields_empty() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::new(IrRole::User, vec![]),
            IrMessage::new(IrRole::Assistant, vec![]),
        ]);
        let stripped = strip_empty(&conv);
        assert!(stripped.is_empty());
    }

    // ── merge_adjacent_text ────────────────────────────────────────────

    #[test]
    fn merge_adjacent_text_coalesces_sequential_text() {
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

    // ── strip_metadata ─────────────────────────────────────────────────

    #[test]
    fn strip_metadata_selective_keep() {
        let mut meta = BTreeMap::new();
        meta.insert("keep_me".into(), json!("yes"));
        meta.insert("drop_me".into(), json!("no"));
        meta.insert("also_drop".into(), json!("nope"));
        let conv = IrConversation::from_messages(vec![IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text { text: "hi".into() }],
            metadata: meta,
        }]);
        let stripped = strip_metadata(&conv, &["keep_me"]);
        assert_eq!(stripped.messages[0].metadata.len(), 1);
        assert!(stripped.messages[0].metadata.contains_key("keep_me"));
    }

    #[test]
    fn strip_metadata_removes_all() {
        let mut meta = BTreeMap::new();
        meta.insert("a".into(), json!(1));
        meta.insert("b".into(), json!(2));
        let conv = IrConversation::from_messages(vec![IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text { text: "hi".into() }],
            metadata: meta,
        }]);
        let stripped = strip_metadata(&conv, &[]);
        assert!(stripped.messages[0].metadata.is_empty());
    }

    #[test]
    fn strip_metadata_no_metadata_is_noop() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let stripped = strip_metadata(&conv, &["anything"]);
        assert!(stripped.messages[0].metadata.is_empty());
    }

    // ── extract_system ─────────────────────────────────────────────────

    #[test]
    fn extract_system_returns_merged() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Rule 1."))
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::System, "Rule 2."));
        let (sys, rest) = extract_system(&conv);
        assert_eq!(sys.unwrap(), "Rule 1.\nRule 2.");
        assert_eq!(rest.len(), 1);
        assert!(rest.messages.iter().all(|m| m.role != IrRole::System));
    }

    #[test]
    fn extract_system_none_when_no_system() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let (sys, rest) = extract_system(&conv);
        assert!(sys.is_none());
        assert_eq!(rest.len(), 1);
    }

    #[test]
    fn extract_system_skips_empty_system_text() {
        let conv = IrConversation::new()
            .push(IrMessage::new(IrRole::System, vec![]))
            .push(IrMessage::text(IrRole::User, "hi"));
        let (sys, _rest) = extract_system(&conv);
        assert!(sys.is_none());
    }

    // ── normalize_tool_schemas ─────────────────────────────────────────

    #[test]
    fn normalize_tool_schemas_injects_type_object() {
        let tools = vec![IrToolDefinition {
            name: "search".into(),
            description: "Search things".into(),
            parameters: json!({"properties": {"q": {"type": "string"}}}),
        }];
        let normalized = normalize_tool_schemas(&tools);
        assert_eq!(normalized[0].parameters["type"], "object");
    }

    #[test]
    fn normalize_tool_schemas_preserves_existing() {
        let tools = vec![IrToolDefinition {
            name: "calc".into(),
            description: "Calculate".into(),
            parameters: json!({"type": "object", "properties": {}}),
        }];
        let normalized = normalize_tool_schemas(&tools);
        assert_eq!(normalized[0].parameters["type"], "object");
    }

    #[test]
    fn normalize_tool_schemas_empty_tools() {
        let normalized = normalize_tool_schemas(&[]);
        assert!(normalized.is_empty());
    }

    // ── sort_tools ─────────────────────────────────────────────────────

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
        assert_eq!(tools[1].name, "b");
    }

    #[test]
    fn sort_tools_single() {
        let mut tools = vec![IrToolDefinition {
            name: "only".into(),
            description: "".into(),
            parameters: json!({}),
        }];
        sort_tools(&mut tools);
        assert_eq!(tools[0].name, "only");
    }

    // ── Full pipeline ──────────────────────────────────────────────────

    #[test]
    fn full_pipeline_applies_all_passes() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "  rule 1  "))
            .push(IrMessage::text(IrRole::User, "  hello  "))
            .push(IrMessage::new(IrRole::Assistant, vec![]))
            .push(IrMessage::text(IrRole::System, "  rule 2  "));
        let normalized = normalize(&conv);
        // dedup_system merges system messages
        assert_eq!(normalized.messages_by_role(IrRole::System).len(), 1);
        // dedup_system joins with \n before trim_text runs on the merged block;
        // trim_text only strips leading/trailing whitespace on each Text block,
        // so inner padding around the newline remains.
        let sys_text = normalized.system_message().unwrap().text_content();
        assert!(sys_text.contains("rule 1"));
        assert!(sys_text.contains("rule 2"));
        // strip_empty removes empty assistant message
        assert!(!normalized.messages.iter().any(|m| m.content.is_empty()));
        // User text is trimmed
        let user = &normalized.messages_by_role(IrRole::User)[0];
        assert_eq!(user.text_content(), "hello");
    }

    #[test]
    fn full_pipeline_idempotent() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "  sys  "))
            .push(IrMessage::text(IrRole::User, " hi "))
            .push(IrMessage::text(IrRole::System, " extra "));
        let once = normalize(&conv);
        let twice = normalize(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn full_pipeline_empty_conversation() {
        let conv = IrConversation::new();
        let normalized = normalize(&conv);
        assert!(normalized.is_empty());
    }

    #[test]
    fn full_pipeline_preserves_tool_content() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "  do stuff  "))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "  I'll help  ".into(),
                    },
                    IrContentBlock::ToolUse {
                        id: "c1".into(),
                        name: "search".into(),
                        input: json!({"q": "  hello  "}),
                    },
                ],
            ));
        let normalized = normalize(&conv);
        // Text is trimmed
        assert_eq!(
            normalized.messages_by_role(IrRole::User)[0].text_content(),
            "do stuff"
        );
        // Tool use preserved
        assert_eq!(normalized.messages[1].tool_use_blocks().len(), 1);
        if let IrContentBlock::ToolUse { input, .. } = normalized.messages[1].tool_use_blocks()[0] {
            assert_eq!(input["q"], "  hello  ");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. IrUsage
// ═══════════════════════════════════════════════════════════════════════

mod usage_tests {
    use super::*;

    #[test]
    fn from_io_computes_total() {
        let usage = IrUsage::from_io(100, 200);
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 200);
        assert_eq!(usage.total_tokens, 300);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.cache_write_tokens, 0);
    }

    #[test]
    fn with_cache_computes_total() {
        let usage = IrUsage::with_cache(100, 200, 50, 30);
        assert_eq!(usage.total_tokens, 300);
        assert_eq!(usage.cache_read_tokens, 50);
        assert_eq!(usage.cache_write_tokens, 30);
    }

    #[test]
    fn merge_sums_all_fields() {
        let a = IrUsage::with_cache(10, 20, 5, 3);
        let b = IrUsage::with_cache(100, 200, 50, 30);
        let merged = a.merge(b);
        assert_eq!(merged.input_tokens, 110);
        assert_eq!(merged.output_tokens, 220);
        assert_eq!(merged.total_tokens, 330);
        assert_eq!(merged.cache_read_tokens, 55);
        assert_eq!(merged.cache_write_tokens, 33);
    }

    #[test]
    fn default_usage_is_zero() {
        let usage = IrUsage::default();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.cache_write_tokens, 0);
    }

    #[test]
    fn merge_with_zero_is_identity() {
        let usage = IrUsage::from_io(10, 20);
        let merged = usage.merge(IrUsage::default());
        assert_eq!(usage, merged);
    }
}
