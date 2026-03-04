#![allow(clippy::all)]
#![allow(unknown_lints)]

use abp_ir::lower::{
    ir_role_to_dialect, lower_for_dialect, lower_to_claude, lower_to_codex, lower_to_copilot,
    lower_to_gemini, lower_to_kimi, lower_to_openai,
};
use abp_ir::normalize::{
    dedup_system, extract_system, merge_adjacent_text, normalize, normalize_role,
    normalize_tool_schemas, sort_tools, strip_empty, strip_metadata, trim_text,
};
use abp_ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_sdk_types::Dialect;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn sys(text: &str) -> IrMessage {
    IrMessage::text(IrRole::System, text)
}

fn user(text: &str) -> IrMessage {
    IrMessage::text(IrRole::User, text)
}

fn asst(text: &str) -> IrMessage {
    IrMessage::text(IrRole::Assistant, text)
}

fn tool_result_msg(tool_use_id: &str, content_text: &str, is_error: bool) -> IrMessage {
    IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: vec![IrContentBlock::Text {
                text: content_text.into(),
            }],
            is_error,
        }],
    )
}

fn tool_use_block(id: &str, name: &str, input: serde_json::Value) -> IrContentBlock {
    IrContentBlock::ToolUse {
        id: id.into(),
        name: name.into(),
        input,
    }
}

fn sample_tool(name: &str) -> IrToolDefinition {
    IrToolDefinition {
        name: name.into(),
        description: format!("{name} tool"),
        parameters: json!({"type": "object", "properties": {}}),
    }
}

fn basic_conv() -> IrConversation {
    IrConversation::new()
        .push(sys("You are helpful."))
        .push(user("Hi"))
        .push(asst("Hello!"))
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. IrRole — enum variants & serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_role_serde_system() {
    let json = serde_json::to_string(&IrRole::System).unwrap();
    assert_eq!(json, "\"system\"");
    let back: IrRole = serde_json::from_str(&json).unwrap();
    assert_eq!(back, IrRole::System);
}

#[test]
fn ir_role_serde_user() {
    let json = serde_json::to_string(&IrRole::User).unwrap();
    assert_eq!(json, "\"user\"");
}

#[test]
fn ir_role_serde_assistant() {
    let json = serde_json::to_string(&IrRole::Assistant).unwrap();
    assert_eq!(json, "\"assistant\"");
}

#[test]
fn ir_role_serde_tool() {
    let json = serde_json::to_string(&IrRole::Tool).unwrap();
    assert_eq!(json, "\"tool\"");
}

#[test]
fn ir_role_clone_and_copy() {
    let r = IrRole::User;
    let c = r;
    let cl = r.clone();
    assert_eq!(c, cl);
}

#[test]
fn ir_role_eq_and_hash() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(IrRole::System);
    set.insert(IrRole::System);
    assert_eq!(set.len(), 1);
    set.insert(IrRole::User);
    assert_eq!(set.len(), 2);
}

#[test]
fn ir_role_debug_format() {
    let dbg = format!("{:?}", IrRole::Assistant);
    assert!(dbg.contains("Assistant"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. IrContentBlock — construction & serde roundtrips
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn content_block_text_roundtrip() {
    let block = IrContentBlock::Text {
        text: "hello".into(),
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "text");
    assert_eq!(json["text"], "hello");
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn content_block_image_roundtrip() {
    let block = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "iVBOR...".into(),
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "image");
    assert_eq!(json["media_type"], "image/png");
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn content_block_tool_use_roundtrip() {
    let block = IrContentBlock::ToolUse {
        id: "call_1".into(),
        name: "search".into(),
        input: json!({"q": "rust"}),
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "tool_use");
    assert_eq!(json["name"], "search");
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn content_block_tool_result_roundtrip() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "call_1".into(),
        content: vec![IrContentBlock::Text { text: "42".into() }],
        is_error: false,
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "tool_result");
    assert_eq!(json["is_error"], false);
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn content_block_tool_result_error_flag() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "c1".into(),
        content: vec![IrContentBlock::Text {
            text: "fail".into(),
        }],
        is_error: true,
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["is_error"], true);
}

#[test]
fn content_block_thinking_roundtrip() {
    let block = IrContentBlock::Thinking {
        text: "let me think".into(),
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "thinking");
    assert_eq!(json["text"], "let me think");
    let back: IrContentBlock = serde_json::from_value(json).unwrap();
    assert_eq!(back, block);
}

#[test]
fn content_block_empty_text() {
    let block = IrContentBlock::Text {
        text: String::new(),
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["text"], "");
}

#[test]
fn content_block_tool_result_empty_content() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "c".into(),
        content: vec![],
        is_error: false,
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["content"].as_array().unwrap().len(), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. IrMessage — constructors & helpers
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_message_text_constructor() {
    let m = IrMessage::text(IrRole::User, "hello");
    assert_eq!(m.role, IrRole::User);
    assert_eq!(m.content.len(), 1);
    assert!(m.metadata.is_empty());
}

#[test]
fn ir_message_new_constructor() {
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
fn ir_message_text_content_concatenation() {
    let m = IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "hello ".into(),
            },
            IrContentBlock::ToolUse {
                id: "c".into(),
                name: "t".into(),
                input: json!({}),
            },
            IrContentBlock::Text {
                text: "world".into(),
            },
        ],
    );
    assert_eq!(m.text_content(), "hello world");
}

#[test]
fn ir_message_text_content_empty_when_no_text() {
    let m = IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "c".into(),
            name: "t".into(),
            input: json!({}),
        }],
    );
    assert_eq!(m.text_content(), "");
}

#[test]
fn ir_message_is_text_only_true() {
    let m = IrMessage::text(IrRole::User, "only text");
    assert!(m.is_text_only());
}

#[test]
fn ir_message_is_text_only_false_with_tool_use() {
    let m = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text { text: "hi".into() },
            tool_use_block("c1", "search", json!({})),
        ],
    );
    assert!(!m.is_text_only());
}

#[test]
fn ir_message_is_text_only_empty_content() {
    let m = IrMessage::new(IrRole::User, vec![]);
    assert!(m.is_text_only()); // vacuously true
}

#[test]
fn ir_message_tool_use_blocks() {
    let m = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text { text: "hi".into() },
            tool_use_block("c1", "search", json!({})),
            tool_use_block("c2", "read", json!({})),
        ],
    );
    let blocks = m.tool_use_blocks();
    assert_eq!(blocks.len(), 2);
}

#[test]
fn ir_message_tool_use_blocks_empty_when_no_tools() {
    let m = user("no tools");
    assert!(m.tool_use_blocks().is_empty());
}

#[test]
fn ir_message_serde_roundtrip() {
    let m = IrMessage::text(IrRole::User, "test");
    let json = serde_json::to_value(&m).unwrap();
    let back: IrMessage = serde_json::from_value(json).unwrap();
    assert_eq!(back, m);
}

#[test]
fn ir_message_metadata_skipped_when_empty() {
    let m = IrMessage::text(IrRole::User, "hi");
    let json = serde_json::to_string(&m).unwrap();
    assert!(!json.contains("metadata"));
}

#[test]
fn ir_message_metadata_serialized_when_present() {
    let mut m = IrMessage::text(IrRole::User, "hi");
    m.metadata.insert("key".into(), json!("val"));
    let json = serde_json::to_string(&m).unwrap();
    assert!(json.contains("metadata"));
}

#[test]
fn ir_message_metadata_roundtrip() {
    let mut m = IrMessage::text(IrRole::User, "hi");
    m.metadata.insert("source".into(), json!("test"));
    m.metadata.insert("priority".into(), json!(1));
    let json = serde_json::to_value(&m).unwrap();
    let back: IrMessage = serde_json::from_value(json).unwrap();
    assert_eq!(back.metadata.len(), 2);
    assert_eq!(back.metadata["source"], json!("test"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. IrToolDefinition — serde
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_tool_definition_serde_roundtrip() {
    let t = IrToolDefinition {
        name: "calc".into(),
        description: "Math evaluator".into(),
        parameters: json!({"type": "object", "properties": {"expr": {"type": "string"}}}),
    };
    let json = serde_json::to_value(&t).unwrap();
    let back: IrToolDefinition = serde_json::from_value(json).unwrap();
    assert_eq!(back, t);
}

#[test]
fn ir_tool_definition_empty_parameters() {
    let t = IrToolDefinition {
        name: "noop".into(),
        description: "Does nothing".into(),
        parameters: json!({}),
    };
    let json = serde_json::to_value(&t).unwrap();
    assert!(json["parameters"].as_object().unwrap().is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. IrConversation — constructors & accessors
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn conversation_new_is_empty() {
    let c = IrConversation::new();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
}

#[test]
fn conversation_default_is_empty() {
    let c = IrConversation::default();
    assert!(c.is_empty());
}

#[test]
fn conversation_push_chaining() {
    let c = IrConversation::new()
        .push(user("a"))
        .push(asst("b"))
        .push(user("c"));
    assert_eq!(c.len(), 3);
    assert!(!c.is_empty());
}

#[test]
fn conversation_from_messages() {
    let c = IrConversation::from_messages(vec![user("a"), asst("b")]);
    assert_eq!(c.len(), 2);
}

#[test]
fn conversation_system_message_found() {
    let c = basic_conv();
    let sm = c.system_message().unwrap();
    assert_eq!(sm.role, IrRole::System);
    assert_eq!(sm.text_content(), "You are helpful.");
}

#[test]
fn conversation_system_message_none() {
    let c = IrConversation::new().push(user("hi"));
    assert!(c.system_message().is_none());
}

#[test]
fn conversation_last_assistant() {
    let c = IrConversation::new()
        .push(user("a"))
        .push(asst("first"))
        .push(user("b"))
        .push(asst("second"));
    assert_eq!(c.last_assistant().unwrap().text_content(), "second");
}

#[test]
fn conversation_last_assistant_none() {
    let c = IrConversation::new().push(user("hello"));
    assert!(c.last_assistant().is_none());
}

#[test]
fn conversation_tool_calls_across_messages() {
    let c = IrConversation::new()
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![tool_use_block("c1", "search", json!({}))],
        ))
        .push(user("ok"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![tool_use_block("c2", "read", json!({}))],
        ));
    assert_eq!(c.tool_calls().len(), 2);
}

#[test]
fn conversation_tool_calls_empty() {
    let c = IrConversation::new().push(user("hi"));
    assert!(c.tool_calls().is_empty());
}

#[test]
fn conversation_messages_by_role() {
    let c = basic_conv();
    assert_eq!(c.messages_by_role(IrRole::System).len(), 1);
    assert_eq!(c.messages_by_role(IrRole::User).len(), 1);
    assert_eq!(c.messages_by_role(IrRole::Assistant).len(), 1);
    assert_eq!(c.messages_by_role(IrRole::Tool).len(), 0);
}

#[test]
fn conversation_last_message() {
    let c = basic_conv();
    assert_eq!(c.last_message().unwrap().text_content(), "Hello!");
}

#[test]
fn conversation_last_message_empty() {
    let c = IrConversation::new();
    assert!(c.last_message().is_none());
}

#[test]
fn conversation_serde_roundtrip() {
    let c = basic_conv();
    let json = serde_json::to_value(&c).unwrap();
    let back: IrConversation = serde_json::from_value(json).unwrap();
    assert_eq!(back, c);
}

#[test]
fn conversation_serde_roundtrip_complex() {
    let c = IrConversation::new()
        .push(sys("Be brief"))
        .push(user("hello"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking { text: "hmm".into() },
                IrContentBlock::Text { text: "Hi".into() },
                tool_use_block("c1", "search", json!({"q": "rust"})),
            ],
        ))
        .push(tool_result_msg("c1", "result", false));
    let json = serde_json::to_string(&c).unwrap();
    let back: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(back, c);
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. IrUsage — constructors & merge
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn usage_default_all_zero() {
    let u = IrUsage::default();
    assert_eq!(u.input_tokens, 0);
    assert_eq!(u.output_tokens, 0);
    assert_eq!(u.total_tokens, 0);
    assert_eq!(u.cache_read_tokens, 0);
    assert_eq!(u.cache_write_tokens, 0);
}

#[test]
fn usage_from_io_computes_total() {
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
fn usage_merge_sums_all_fields() {
    let a = IrUsage::with_cache(100, 50, 10, 5);
    let b = IrUsage::with_cache(200, 100, 20, 10);
    let m = a.merge(b);
    assert_eq!(m.input_tokens, 300);
    assert_eq!(m.output_tokens, 150);
    assert_eq!(m.total_tokens, 450);
    assert_eq!(m.cache_read_tokens, 30);
    assert_eq!(m.cache_write_tokens, 15);
}

#[test]
fn usage_merge_with_default_is_identity() {
    let a = IrUsage::from_io(42, 17);
    let m = a.merge(IrUsage::default());
    assert_eq!(m, a);
}

#[test]
fn usage_serde_roundtrip() {
    let u = IrUsage::with_cache(100, 200, 30, 40);
    let json = serde_json::to_value(&u).unwrap();
    let back: IrUsage = serde_json::from_value(json).unwrap();
    assert_eq!(back, u);
}

#[test]
fn usage_clone_and_copy() {
    let u = IrUsage::from_io(1, 2);
    let c = u;
    let cl = u.clone();
    assert_eq!(c, cl);
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. normalize — dedup_system
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn dedup_system_merges_multiple() {
    let c = IrConversation::new()
        .push(sys("A"))
        .push(user("hi"))
        .push(sys("B"));
    let d = dedup_system(&c);
    assert_eq!(d.messages_by_role(IrRole::System).len(), 1);
    assert_eq!(d.system_message().unwrap().text_content(), "A\nB");
}

#[test]
fn dedup_system_preserves_non_system_order() {
    let c = IrConversation::new()
        .push(sys("sys"))
        .push(user("u1"))
        .push(asst("a1"))
        .push(user("u2"));
    let d = dedup_system(&c);
    assert_eq!(d.len(), 4);
    assert_eq!(d.messages[1].text_content(), "u1");
    assert_eq!(d.messages[2].text_content(), "a1");
    assert_eq!(d.messages[3].text_content(), "u2");
}

#[test]
fn dedup_system_no_system_messages() {
    let c = IrConversation::new().push(user("hi"));
    let d = dedup_system(&c);
    assert_eq!(d.len(), 1);
    assert!(d.system_message().is_none());
}

#[test]
fn dedup_system_single_system_unchanged() {
    let c = IrConversation::new().push(sys("only one"));
    let d = dedup_system(&c);
    assert_eq!(d.system_message().unwrap().text_content(), "only one");
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. normalize — trim_text
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn trim_text_strips_whitespace() {
    let c = IrConversation::new().push(IrMessage::text(IrRole::User, "  hello  "));
    let t = trim_text(&c);
    assert_eq!(t.messages[0].text_content(), "hello");
}

#[test]
fn trim_text_leaves_non_text_blocks() {
    let c = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "  hmm  ".into(),
        }],
    ));
    let t = trim_text(&c);
    // Thinking is not trimmed
    if let IrContentBlock::Thinking { text } = &t.messages[0].content[0] {
        assert_eq!(text, "  hmm  ");
    } else {
        panic!("expected thinking block");
    }
}

#[test]
fn trim_text_handles_empty_text() {
    let c = IrConversation::new().push(IrMessage::text(IrRole::User, "   "));
    let t = trim_text(&c);
    assert_eq!(t.messages[0].text_content(), "");
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. normalize — strip_empty
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn strip_empty_removes_no_content_messages() {
    let c = IrConversation::new()
        .push(user("hi"))
        .push(IrMessage::new(IrRole::Assistant, vec![]))
        .push(asst("ok"));
    let s = strip_empty(&c);
    assert_eq!(s.len(), 2);
}

#[test]
fn strip_empty_keeps_all_when_nonempty() {
    let c = basic_conv();
    let s = strip_empty(&c);
    assert_eq!(s.len(), 3);
}

#[test]
fn strip_empty_all_empty() {
    let c = IrConversation::new()
        .push(IrMessage::new(IrRole::User, vec![]))
        .push(IrMessage::new(IrRole::Assistant, vec![]));
    let s = strip_empty(&c);
    assert!(s.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. normalize — merge_adjacent_text
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn merge_adjacent_text_combines_consecutive() {
    let c = IrConversation::new().push(IrMessage::new(
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
    let m = merge_adjacent_text(&c);
    assert_eq!(m.messages[0].content.len(), 1);
    assert_eq!(m.messages[0].text_content(), "hello world");
}

#[test]
fn merge_adjacent_text_does_not_merge_across_non_text() {
    let c = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text { text: "a".into() },
            tool_use_block("c1", "t", json!({})),
            IrContentBlock::Text { text: "b".into() },
        ],
    ));
    let m = merge_adjacent_text(&c);
    assert_eq!(m.messages[0].content.len(), 3);
}

#[test]
fn merge_adjacent_text_three_consecutive() {
    let c = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text { text: "a".into() },
            IrContentBlock::Text { text: "b".into() },
            IrContentBlock::Text { text: "c".into() },
        ],
    ));
    let m = merge_adjacent_text(&c);
    assert_eq!(m.messages[0].content.len(), 1);
    assert_eq!(m.messages[0].text_content(), "abc");
}

#[test]
fn merge_adjacent_text_no_text_blocks() {
    let c = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![
            tool_use_block("c1", "a", json!({})),
            tool_use_block("c2", "b", json!({})),
        ],
    ));
    let m = merge_adjacent_text(&c);
    assert_eq!(m.messages[0].content.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. normalize — strip_metadata
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn strip_metadata_removes_all() {
    let mut m = user("hi");
    m.metadata.insert("a".into(), json!(1));
    m.metadata.insert("b".into(), json!(2));
    let c = IrConversation::from_messages(vec![m]);
    let s = strip_metadata(&c, &[]);
    assert!(s.messages[0].metadata.is_empty());
}

#[test]
fn strip_metadata_keeps_specified() {
    let mut m = user("hi");
    m.metadata.insert("keep".into(), json!(1));
    m.metadata.insert("drop".into(), json!(2));
    let c = IrConversation::from_messages(vec![m]);
    let s = strip_metadata(&c, &["keep"]);
    assert_eq!(s.messages[0].metadata.len(), 1);
    assert!(s.messages[0].metadata.contains_key("keep"));
}

#[test]
fn strip_metadata_no_metadata_noop() {
    let c = IrConversation::new().push(user("hi"));
    let s = strip_metadata(&c, &[]);
    assert!(s.messages[0].metadata.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. normalize — extract_system
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn extract_system_basic() {
    let c = IrConversation::new()
        .push(sys("instructions"))
        .push(user("hi"));
    let (s, rest) = extract_system(&c);
    assert_eq!(s.unwrap(), "instructions");
    assert_eq!(rest.len(), 1);
}

#[test]
fn extract_system_merges_multiple() {
    let c = IrConversation::new()
        .push(sys("A"))
        .push(user("x"))
        .push(sys("B"));
    let (s, rest) = extract_system(&c);
    assert_eq!(s.unwrap(), "A\nB");
    assert_eq!(rest.len(), 1);
}

#[test]
fn extract_system_none_when_absent() {
    let c = IrConversation::new().push(user("hi"));
    let (s, _) = extract_system(&c);
    assert!(s.is_none());
}

#[test]
fn extract_system_skips_empty_system_text() {
    let c = IrConversation::new()
        .push(IrMessage::new(IrRole::System, vec![]))
        .push(user("hi"));
    let (s, _) = extract_system(&c);
    assert!(s.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. normalize — normalize_role
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn normalize_role_standard() {
    assert_eq!(normalize_role("system"), Some(IrRole::System));
    assert_eq!(normalize_role("user"), Some(IrRole::User));
    assert_eq!(normalize_role("assistant"), Some(IrRole::Assistant));
    assert_eq!(normalize_role("tool"), Some(IrRole::Tool));
}

#[test]
fn normalize_role_vendor_aliases() {
    assert_eq!(normalize_role("developer"), Some(IrRole::System));
    assert_eq!(normalize_role("human"), Some(IrRole::User));
    assert_eq!(normalize_role("model"), Some(IrRole::Assistant));
    assert_eq!(normalize_role("bot"), Some(IrRole::Assistant));
    assert_eq!(normalize_role("function"), Some(IrRole::Tool));
}

#[test]
fn normalize_role_unknown_returns_none() {
    assert_eq!(normalize_role("narrator"), None);
    assert_eq!(normalize_role(""), None);
    assert_eq!(normalize_role("SYSTEM"), None);
    assert_eq!(normalize_role("Admin"), None);
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. normalize — sort_tools
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sort_tools_alphabetical() {
    let mut tools = vec![
        sample_tool("zebra"),
        sample_tool("apple"),
        sample_tool("mango"),
    ];
    sort_tools(&mut tools);
    assert_eq!(tools[0].name, "apple");
    assert_eq!(tools[1].name, "mango");
    assert_eq!(tools[2].name, "zebra");
}

#[test]
fn sort_tools_already_sorted() {
    let mut tools = vec![sample_tool("a"), sample_tool("b")];
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
fn sort_tools_single() {
    let mut tools = vec![sample_tool("only")];
    sort_tools(&mut tools);
    assert_eq!(tools[0].name, "only");
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. normalize — normalize_tool_schemas
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn normalize_tool_schemas_adds_type() {
    let tools = vec![IrToolDefinition {
        name: "t".into(),
        description: "d".into(),
        parameters: json!({"properties": {"x": {"type": "string"}}}),
    }];
    let n = normalize_tool_schemas(&tools);
    assert_eq!(n[0].parameters["type"], "object");
}

#[test]
fn normalize_tool_schemas_preserves_existing() {
    let tools = vec![IrToolDefinition {
        name: "t".into(),
        description: "d".into(),
        parameters: json!({"type": "object", "properties": {}}),
    }];
    let n = normalize_tool_schemas(&tools);
    assert_eq!(n[0].parameters["type"], "object");
}

#[test]
fn normalize_tool_schemas_non_object_params() {
    let tools = vec![IrToolDefinition {
        name: "t".into(),
        description: "d".into(),
        parameters: json!("not an object"),
    }];
    let n = normalize_tool_schemas(&tools);
    assert_eq!(n[0].parameters, json!("not an object"));
}

#[test]
fn normalize_tool_schemas_empty() {
    let n = normalize_tool_schemas(&[]);
    assert!(n.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. normalize — full pipeline
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn full_pipeline_basic() {
    let c = IrConversation::new()
        .push(sys("  sys1  "))
        .push(user("  hello  "))
        .push(sys("  sys2  "));
    let n = normalize(&c);
    assert_eq!(n.messages_by_role(IrRole::System).len(), 1);
    // dedup joins raw text_content() with \n, then trim only trims outer edges
    assert_eq!(n.system_message().unwrap().text_content(), "sys1  \n  sys2");
    assert_eq!(n.messages_by_role(IrRole::User)[0].text_content(), "hello");
}

#[test]
fn full_pipeline_idempotent() {
    let c = IrConversation::new()
        .push(sys("  sys  "))
        .push(user(" hi "))
        .push(sys(" extra "));
    let once = normalize(&c);
    let twice = normalize(&once);
    assert_eq!(once, twice);
}

#[test]
fn full_pipeline_strips_empty_after_trim() {
    let c = IrConversation::new().push(user("   ")).push(user("hello"));
    let n = normalize(&c);
    // The first message has trimmed empty text but content is not empty (1 block)
    // strip_empty only removes messages with zero content blocks
    assert_eq!(n.len(), 2);
}

#[test]
fn full_pipeline_merges_adjacent_text() {
    let c = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text { text: " a ".into() },
            IrContentBlock::Text { text: " b ".into() },
        ],
    ));
    let n = normalize(&c);
    assert_eq!(n.messages[0].content.len(), 1);
    assert_eq!(n.messages[0].text_content(), "ab");
}

#[test]
fn full_pipeline_empty_conversation() {
    let c = IrConversation::new();
    let n = normalize(&c);
    assert!(n.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. lower — ir_role_to_dialect
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn role_mapping_openai_all_roles() {
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
fn role_mapping_claude_tool_is_user() {
    assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::Claude), "user");
}

#[test]
fn role_mapping_gemini_assistant_is_model() {
    assert_eq!(
        ir_role_to_dialect(IrRole::Assistant, Dialect::Gemini),
        "model"
    );
}

#[test]
fn role_mapping_gemini_tool_is_user() {
    assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::Gemini), "user");
}

#[test]
fn role_mapping_kimi_matches_openai() {
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        assert_eq!(
            ir_role_to_dialect(role, Dialect::Kimi),
            ir_role_to_dialect(role, Dialect::OpenAi),
        );
    }
}

#[test]
fn role_mapping_codex_matches_openai() {
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        assert_eq!(
            ir_role_to_dialect(role, Dialect::Codex),
            ir_role_to_dialect(role, Dialect::OpenAi),
        );
    }
}

#[test]
fn role_mapping_copilot_matches_openai() {
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        assert_eq!(
            ir_role_to_dialect(role, Dialect::Copilot),
            ir_role_to_dialect(role, Dialect::OpenAi),
        );
    }
}

#[test]
fn role_mapping_all_dialects_have_user() {
    for d in Dialect::all() {
        assert_eq!(ir_role_to_dialect(IrRole::User, *d), "user");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. lower — lower_to_openai
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_basic_structure() {
    let v = lower_to_openai(&basic_conv(), &[]);
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[1]["role"], "user");
    assert_eq!(msgs[2]["role"], "assistant");
}

#[test]
fn openai_with_tools() {
    let tools = vec![sample_tool("calc")];
    let v = lower_to_openai(&basic_conv(), &tools);
    let t = v["tools"].as_array().unwrap();
    assert_eq!(t.len(), 1);
    assert_eq!(t[0]["type"], "function");
    assert_eq!(t[0]["function"]["name"], "calc");
}

#[test]
fn openai_no_tools_omits_key() {
    let v = lower_to_openai(&basic_conv(), &[]);
    assert!(v.get("tools").is_none());
}

#[test]
fn openai_tool_use_in_assistant() {
    let c = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Let me check.".into(),
            },
            tool_use_block("call_1", "read_file", json!({"path": "/a.rs"})),
        ],
    ));
    let v = lower_to_openai(&c, &[]);
    let msg = &v["messages"][0];
    assert_eq!(msg["content"], "Let me check.");
    assert_eq!(msg["tool_calls"][0]["id"], "call_1");
    assert_eq!(msg["tool_calls"][0]["type"], "function");
    assert_eq!(msg["tool_calls"][0]["function"]["name"], "read_file");
}

#[test]
fn openai_tool_result_becomes_tool_role() {
    let c = IrConversation::new().push(tool_result_msg("call_1", "result text", false));
    let v = lower_to_openai(&c, &[]);
    let msg = &v["messages"][0];
    assert_eq!(msg["role"], "tool");
    assert_eq!(msg["tool_call_id"], "call_1");
    assert_eq!(msg["content"], "result text");
}

#[test]
fn openai_multiple_tool_results_expand() {
    let c = IrConversation::new().push(IrMessage::new(
        IrRole::Tool,
        vec![
            IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![IrContentBlock::Text { text: "r1".into() }],
                is_error: false,
            },
            IrContentBlock::ToolResult {
                tool_use_id: "c2".into(),
                content: vec![IrContentBlock::Text { text: "r2".into() }],
                is_error: false,
            },
        ],
    ));
    let v = lower_to_openai(&c, &[]);
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0]["tool_call_id"], "c1");
    assert_eq!(msgs[1]["tool_call_id"], "c2");
}

#[test]
fn openai_only_tool_calls_no_content() {
    let c = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![tool_use_block("c1", "search", json!({}))],
    ));
    let v = lower_to_openai(&c, &[]);
    let msg = &v["messages"][0];
    assert!(msg.get("content").is_none());
    assert!(msg["tool_calls"].as_array().unwrap().len() == 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. lower — lower_to_claude
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_extracts_system() {
    let v = lower_to_claude(&basic_conv(), &[]);
    assert_eq!(v["system"], "You are helpful.");
    let msgs = v["messages"].as_array().unwrap();
    assert!(msgs.iter().all(|m| m["role"] != "system"));
}

#[test]
fn claude_no_system_omits_key() {
    let c = IrConversation::new().push(user("hi"));
    let v = lower_to_claude(&c, &[]);
    assert!(v.get("system").is_none());
}

#[test]
fn claude_content_blocks_typed() {
    let c = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text { text: "hi".into() },
            tool_use_block("c1", "search", json!({"q": "r"})),
        ],
    ));
    let v = lower_to_claude(&c, &[]);
    let content = v["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "tool_use");
    assert_eq!(content[1]["id"], "c1");
}

#[test]
fn claude_tool_result_block() {
    let c = IrConversation::new().push(tool_result_msg("c1", "result", true));
    let v = lower_to_claude(&c, &[]);
    let content = v["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "tool_result");
    assert_eq!(content[0]["tool_use_id"], "c1");
    assert_eq!(content[0]["is_error"], true);
}

#[test]
fn claude_image_block() {
    let c = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "abc123".into(),
        }],
    ));
    let v = lower_to_claude(&c, &[]);
    let block = &v["messages"][0]["content"][0];
    assert_eq!(block["type"], "image");
    assert_eq!(block["source"]["type"], "base64");
    assert_eq!(block["source"]["media_type"], "image/png");
}

#[test]
fn claude_thinking_block() {
    let c = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking { text: "hmm".into() }],
    ));
    let v = lower_to_claude(&c, &[]);
    let block = &v["messages"][0]["content"][0];
    assert_eq!(block["type"], "thinking");
    assert_eq!(block["thinking"], "hmm");
}

#[test]
fn claude_tools_use_input_schema() {
    let tools = vec![sample_tool("calc")];
    let v = lower_to_claude(&basic_conv(), &tools);
    let t = &v["tools"][0];
    assert!(t.get("input_schema").is_some());
    assert!(t.get("parameters").is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// 20. lower — lower_to_gemini
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn gemini_uses_contents_key() {
    let v = lower_to_gemini(&basic_conv(), &[]);
    assert!(v.get("contents").is_some());
    assert!(v.get("messages").is_none());
}

#[test]
fn gemini_system_instruction() {
    let v = lower_to_gemini(&basic_conv(), &[]);
    assert_eq!(
        v["system_instruction"]["parts"][0]["text"],
        "You are helpful."
    );
}

#[test]
fn gemini_no_system_omits_instruction() {
    let c = IrConversation::new().push(user("hi"));
    let v = lower_to_gemini(&c, &[]);
    assert!(v.get("system_instruction").is_none());
}

#[test]
fn gemini_assistant_role_is_model() {
    let v = lower_to_gemini(&basic_conv(), &[]);
    let contents = v["contents"].as_array().unwrap();
    // contents[0] = user, contents[1] = assistant
    assert_eq!(contents[1]["role"], "model");
}

#[test]
fn gemini_thinking_block_skipped() {
    let c = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Thinking { text: "hmm".into() },
            IrContentBlock::Text {
                text: "answer".into(),
            },
        ],
    ));
    let v = lower_to_gemini(&c, &[]);
    let parts = v["contents"][0]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0]["text"], "answer");
}

#[test]
fn gemini_function_call() {
    let c = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![tool_use_block("c1", "search", json!({"q": "r"}))],
    ));
    let v = lower_to_gemini(&c, &[]);
    let part = &v["contents"][0]["parts"][0];
    assert_eq!(part["functionCall"]["name"], "search");
    assert_eq!(part["functionCall"]["args"], json!({"q": "r"}));
}

#[test]
fn gemini_function_response() {
    let c = IrConversation::new().push(tool_result_msg("call_1", "42", false));
    let v = lower_to_gemini(&c, &[]);
    let part = &v["contents"][0]["parts"][0];
    assert_eq!(part["functionResponse"]["name"], "call_1");
    assert_eq!(part["functionResponse"]["response"]["result"], "42");
}

#[test]
fn gemini_image_inline_data() {
    let c = IrConversation::new().push(IrMessage::new(
        IrRole::User,
        vec![IrContentBlock::Image {
            media_type: "image/jpeg".into(),
            data: "base64data".into(),
        }],
    ));
    let v = lower_to_gemini(&c, &[]);
    let part = &v["contents"][0]["parts"][0];
    assert_eq!(part["inline_data"]["mime_type"], "image/jpeg");
    assert_eq!(part["inline_data"]["data"], "base64data");
}

#[test]
fn gemini_tools_function_declarations() {
    let tools = vec![sample_tool("calc"), sample_tool("search")];
    let v = lower_to_gemini(&basic_conv(), &tools);
    let decls = v["tools"][0]["function_declarations"].as_array().unwrap();
    assert_eq!(decls.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// 21. lower — convenience aliases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn kimi_equals_openai() {
    let c = basic_conv();
    let tools = vec![sample_tool("t")];
    assert_eq!(lower_to_kimi(&c, &tools), lower_to_openai(&c, &tools));
}

#[test]
fn codex_equals_openai() {
    let c = basic_conv();
    let tools = vec![sample_tool("t")];
    assert_eq!(lower_to_codex(&c, &tools), lower_to_openai(&c, &tools));
}

#[test]
fn copilot_equals_openai() {
    let c = basic_conv();
    let tools = vec![sample_tool("t")];
    assert_eq!(lower_to_copilot(&c, &tools), lower_to_openai(&c, &tools));
}

// ═══════════════════════════════════════════════════════════════════════════
// 22. lower — lower_for_dialect dispatch
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lower_for_dialect_openai() {
    let c = basic_conv();
    let t = vec![sample_tool("t")];
    assert_eq!(
        lower_for_dialect(Dialect::OpenAi, &c, &t),
        lower_to_openai(&c, &t)
    );
}

#[test]
fn lower_for_dialect_claude() {
    let c = basic_conv();
    let t = vec![sample_tool("t")];
    assert_eq!(
        lower_for_dialect(Dialect::Claude, &c, &t),
        lower_to_claude(&c, &t)
    );
}

#[test]
fn lower_for_dialect_gemini() {
    let c = basic_conv();
    let t = vec![sample_tool("t")];
    assert_eq!(
        lower_for_dialect(Dialect::Gemini, &c, &t),
        lower_to_gemini(&c, &t)
    );
}

#[test]
fn lower_for_dialect_all_produce_objects() {
    let c = basic_conv();
    let tools = vec![sample_tool("t")];
    for d in Dialect::all() {
        let v = lower_for_dialect(*d, &c, &tools);
        assert!(v.is_object(), "{d}: expected object");
    }
}

#[test]
fn lower_for_dialect_no_tools_omits_tools_key() {
    let c = IrConversation::new().push(user("hi"));
    for d in Dialect::all() {
        let v = lower_for_dialect(*d, &c, &[]);
        assert!(v.get("tools").is_none(), "{d}: tools should be absent");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 23. Edge cases — empty conversations
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lower_openai_empty_conv() {
    let v = lower_to_openai(&IrConversation::new(), &[]);
    assert_eq!(v["messages"].as_array().unwrap().len(), 0);
}

#[test]
fn lower_claude_empty_conv() {
    let v = lower_to_claude(&IrConversation::new(), &[]);
    assert_eq!(v["messages"].as_array().unwrap().len(), 0);
    assert!(v.get("system").is_none());
}

#[test]
fn lower_gemini_empty_conv() {
    let v = lower_to_gemini(&IrConversation::new(), &[]);
    assert_eq!(v["contents"].as_array().unwrap().len(), 0);
    assert!(v.get("system_instruction").is_none());
}

#[test]
fn normalize_empty_conv() {
    let n = normalize(&IrConversation::new());
    assert!(n.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 24. Edge cases — unicode & special content
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn unicode_text_preserved() {
    let c = IrConversation::new().push(user("你好世界 🌍"));
    let v = lower_to_openai(&c, &[]);
    assert_eq!(v["messages"][0]["content"], "你好世界 🌍");
}

#[test]
fn unicode_serde_roundtrip() {
    let m = IrMessage::text(IrRole::User, "日本語テスト 🎌");
    let json = serde_json::to_value(&m).unwrap();
    let back: IrMessage = serde_json::from_value(json).unwrap();
    assert_eq!(back.text_content(), "日本語テスト 🎌");
}

#[test]
fn newline_in_text_preserved() {
    let c = IrConversation::new().push(user("line1\nline2\nline3"));
    let v = lower_to_openai(&c, &[]);
    assert_eq!(v["messages"][0]["content"], "line1\nline2\nline3");
}

#[test]
fn empty_string_text() {
    let c = IrConversation::new().push(IrMessage::text(IrRole::User, ""));
    let v = lower_to_openai(&c, &[]);
    assert_eq!(v["messages"][0]["content"], "");
}

// ═══════════════════════════════════════════════════════════════════════════
// 25. Cross-cutting — tool_calls arguments serialized as string
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_tool_call_arguments_is_string() {
    let c = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![tool_use_block("c1", "calc", json!({"expr": "2+2"}))],
    ));
    let v = lower_to_openai(&c, &[]);
    let args = &v["messages"][0]["tool_calls"][0]["function"]["arguments"];
    assert!(args.is_string());
}

// ═══════════════════════════════════════════════════════════════════════════
// 26. Conversation cloning and equality
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn conversation_clone_equals_original() {
    let c = basic_conv();
    let c2 = c.clone();
    assert_eq!(c, c2);
}

#[test]
fn conversation_different_messages_not_equal() {
    let a = IrConversation::new().push(user("a"));
    let b = IrConversation::new().push(user("b"));
    assert_ne!(a, b);
}

// ═══════════════════════════════════════════════════════════════════════════
// 27. Multi-tool workflow roundtrip through lowering
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn multi_turn_tool_workflow_openai() {
    let c = IrConversation::new()
        .push(sys("You are a calculator"))
        .push(user("What is 2+2?"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me calculate.".into(),
                },
                tool_use_block("c1", "calc", json!({"expr": "2+2"})),
            ],
        ))
        .push(tool_result_msg("c1", "4", false))
        .push(asst("The answer is 4."));

    let v = lower_to_openai(&c, &[sample_tool("calc")]);
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 5);
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[2]["tool_calls"][0]["function"]["name"], "calc");
    assert_eq!(msgs[3]["role"], "tool");
    assert_eq!(msgs[4]["content"], "The answer is 4.");
}

#[test]
fn multi_turn_tool_workflow_claude() {
    let c = IrConversation::new()
        .push(sys("You are a calculator"))
        .push(user("What is 2+2?"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![tool_use_block("c1", "calc", json!({"expr": "2+2"}))],
        ))
        .push(tool_result_msg("c1", "4", false));

    let v = lower_to_claude(&c, &[]);
    assert_eq!(v["system"], "You are a calculator");
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 3); // user, assistant, tool(user)
}

// ═══════════════════════════════════════════════════════════════════════════
// 28. Normalize pipeline composition
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn pipeline_then_lower_produces_clean_output() {
    let c = IrConversation::new()
        .push(sys("  Be nice.  "))
        .push(user("  hi  "))
        .push(sys("  Be brief.  "))
        .push(IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text {
                    text: " hello ".into(),
                },
                IrContentBlock::Text {
                    text: " world ".into(),
                },
            ],
        ));

    let n = normalize(&c);
    let v = lower_to_openai(&n, &[]);
    let msgs = v["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["content"], "Be nice.  \n  Be brief.");
    assert_eq!(msgs[1]["content"], "hi");
    assert_eq!(msgs[2]["content"], "helloworld");
}

// ═══════════════════════════════════════════════════════════════════════════
// 29. Additional IrUsage edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn usage_from_io_zero() {
    let u = IrUsage::from_io(0, 0);
    assert_eq!(u, IrUsage::default());
}

#[test]
fn usage_merge_commutative() {
    let a = IrUsage::from_io(10, 20);
    let b = IrUsage::from_io(30, 40);
    assert_eq!(a.merge(b), b.merge(a));
}

#[test]
fn usage_merge_associative() {
    let a = IrUsage::from_io(1, 2);
    let b = IrUsage::from_io(3, 4);
    let c = IrUsage::from_io(5, 6);
    assert_eq!(a.merge(b).merge(c), a.merge(b.merge(c)));
}

#[test]
fn usage_debug_format() {
    let u = IrUsage::from_io(100, 200);
    let dbg = format!("{:?}", u);
    assert!(dbg.contains("100"));
    assert!(dbg.contains("200"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 30. Claude tool_result with non-text nested blocks
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_tool_result_unknown_nested_block() {
    let c = IrConversation::new().push(IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![
                IrContentBlock::Text {
                    text: "text part".into(),
                },
                IrContentBlock::Image {
                    media_type: "image/png".into(),
                    data: "abc".into(),
                },
            ],
            is_error: false,
        }],
    ));
    let v = lower_to_claude(&c, &[]);
    let inner = v["messages"][0]["content"][0]["content"]
        .as_array()
        .unwrap();
    assert_eq!(inner[0]["type"], "text");
    assert_eq!(inner[1]["type"], "unknown");
}

// ═══════════════════════════════════════════════════════════════════════════
// 31. Gemini only-thinking message produces empty parts
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn gemini_only_thinking_empty_parts() {
    let c = IrConversation::new().push(IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "thinking...".into(),
        }],
    ));
    let v = lower_to_gemini(&c, &[]);
    let parts = v["contents"][0]["parts"].as_array().unwrap();
    assert!(parts.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// 32. OpenAI text-only assistant no tool_calls key
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_text_only_assistant_no_tool_calls_key() {
    let c = IrConversation::new().push(asst("just text"));
    let v = lower_to_openai(&c, &[]);
    let msg = &v["messages"][0];
    assert!(msg.get("tool_calls").is_none());
    assert_eq!(msg["content"], "just text");
}
