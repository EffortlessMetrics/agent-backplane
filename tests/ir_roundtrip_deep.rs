// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the IR (Intermediate Representation) system.
//!
//! Covers construction, serde roundtrip, vendor lowering/raising, validation,
//! metadata, tool-call pairing, streaming, edge cases, and cross-dialect
//! semantic equivalence.

use std::collections::BTreeMap;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use serde_json::json;

// ── SDK imports ─────────────────────────────────────────────────────────

use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};
use abp_openai_sdk::lowering as openai_ir;

use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeImageSource, ClaudeMessage};
use abp_claude_sdk::lowering as claude_ir;

use abp_gemini_sdk::dialect::{GeminiContent, GeminiInlineData, GeminiPart};
use abp_gemini_sdk::lowering as gemini_ir;

use abp_codex_sdk::dialect::{
    CodexContentPart, CodexInputItem, CodexResponseItem, CodexUsage, ReasoningSummary,
};
use abp_codex_sdk::lowering as codex_ir;

use abp_kimi_sdk::dialect::{KimiFunctionCall, KimiMessage, KimiToolCall, KimiUsage};
use abp_kimi_sdk::lowering as kimi_ir;

use abp_copilot_sdk::dialect::{CopilotMessage, CopilotReference, CopilotReferenceType};
use abp_copilot_sdk::lowering as copilot_ir;

// ═══════════════════════════════════════════════════════════════════════════
// §1  IR Type Construction (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn construct_text_message_user() {
    let msg = IrMessage::text(IrRole::User, "hello world");
    assert_eq!(msg.role, IrRole::User);
    assert_eq!(msg.text_content(), "hello world");
    assert!(msg.is_text_only());
    assert!(msg.metadata.is_empty());
}

#[test]
fn construct_text_message_system() {
    let msg = IrMessage::text(IrRole::System, "You are a helpful assistant.");
    assert_eq!(msg.role, IrRole::System);
    assert_eq!(msg.text_content(), "You are a helpful assistant.");
}

#[test]
fn construct_text_message_assistant() {
    let msg = IrMessage::text(IrRole::Assistant, "Sure, I can help.");
    assert_eq!(msg.role, IrRole::Assistant);
    assert!(msg.is_text_only());
}

#[test]
fn construct_text_message_tool() {
    let msg = IrMessage::text(IrRole::Tool, "result data");
    assert_eq!(msg.role, IrRole::Tool);
    assert_eq!(msg.text_content(), "result data");
}

#[test]
fn construct_message_with_tool_use() {
    let blocks = vec![
        IrContentBlock::Text {
            text: "Let me search.".into(),
        },
        IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "web_search".into(),
            input: json!({"query": "rust language"}),
        },
    ];
    let msg = IrMessage::new(IrRole::Assistant, blocks);
    assert!(!msg.is_text_only());
    assert_eq!(msg.text_content(), "Let me search.");
    assert_eq!(msg.tool_use_blocks().len(), 1);
}

#[test]
fn construct_message_with_tool_result() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "t1".into(),
        content: vec![IrContentBlock::Text { text: "42".into() }],
        is_error: false,
    };
    let msg = IrMessage::new(IrRole::Tool, vec![block]);
    assert!(!msg.is_text_only());
    assert!(msg.tool_use_blocks().is_empty());
}

#[test]
fn construct_message_with_image() {
    let block = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "iVBORw0KGgo=".into(),
    };
    let msg = IrMessage::new(IrRole::User, vec![block]);
    assert!(!msg.is_text_only());
    assert!(msg.text_content().is_empty());
}

#[test]
fn construct_message_with_thinking() {
    let block = IrContentBlock::Thinking {
        text: "Let me reason step by step.".into(),
    };
    let msg = IrMessage::new(IrRole::Assistant, vec![block]);
    assert!(!msg.is_text_only());
}

#[test]
fn construct_message_with_metadata() {
    let mut metadata = BTreeMap::new();
    metadata.insert("vendor".into(), json!("openai"));
    metadata.insert("model".into(), json!("gpt-4"));
    let msg = IrMessage {
        role: IrRole::Assistant,
        content: vec![IrContentBlock::Text { text: "hi".into() }],
        metadata,
    };
    assert_eq!(msg.metadata.len(), 2);
    assert_eq!(msg.metadata["vendor"], json!("openai"));
}

#[test]
fn construct_tool_definition() {
    let def = IrToolDefinition {
        name: "read_file".into(),
        description: "Read contents of a file".into(),
        parameters: json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        }),
    };
    assert_eq!(def.name, "read_file");
}

// ═══════════════════════════════════════════════════════════════════════════
// §2  IrConversation Accessors (10 tests)
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
        .push(IrMessage::text(IrRole::System, "sys"))
        .push(IrMessage::text(IrRole::User, "hi"))
        .push(IrMessage::text(IrRole::Assistant, "hello"));
    assert_eq!(conv.len(), 3);
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
fn conversation_system_message_accessor() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Be concise."))
        .push(IrMessage::text(IrRole::User, "Hi"));
    assert_eq!(conv.system_message().unwrap().text_content(), "Be concise.");
}

#[test]
fn conversation_no_system_message_returns_none() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));
    assert!(conv.system_message().is_none());
}

#[test]
fn conversation_last_assistant() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::User, "q1"))
        .push(IrMessage::text(IrRole::Assistant, "a1"))
        .push(IrMessage::text(IrRole::User, "q2"))
        .push(IrMessage::text(IrRole::Assistant, "a2"));
    assert_eq!(conv.last_assistant().unwrap().text_content(), "a2");
}

#[test]
fn conversation_last_message() {
    let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "final"));
    assert_eq!(conv.last_message().unwrap().text_content(), "final");
}

#[test]
fn conversation_messages_by_role() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "a"),
        IrMessage::text(IrRole::Assistant, "b"),
        IrMessage::text(IrRole::User, "c"),
        IrMessage::text(IrRole::User, "d"),
    ]);
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 3);
    assert_eq!(conv.messages_by_role(IrRole::Assistant).len(), 1);
    assert!(conv.messages_by_role(IrRole::System).is_empty());
}

#[test]
fn conversation_tool_calls_aggregation() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "a".into(),
                input: json!({}),
            }],
        ),
        IrMessage::text(IrRole::User, "ok"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::ToolUse {
                    id: "t2".into(),
                    name: "b".into(),
                    input: json!({}),
                },
                IrContentBlock::ToolUse {
                    id: "t3".into(),
                    name: "c".into(),
                    input: json!({}),
                },
            ],
        ),
    ]);
    assert_eq!(conv.tool_calls().len(), 3);
}

#[test]
fn conversation_empty_tool_calls() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hello")]);
    assert!(conv.tool_calls().is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  IrUsage (8 tests)
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
fn usage_default_is_zero() {
    let u = IrUsage::default();
    assert_eq!(u.input_tokens, 0);
    assert_eq!(u.output_tokens, 0);
    assert_eq!(u.total_tokens, 0);
}

#[test]
fn usage_merge() {
    let a = IrUsage::from_io(100, 50);
    let b = IrUsage::with_cache(200, 100, 30, 15);
    let merged = a.merge(b);
    assert_eq!(merged.input_tokens, 300);
    assert_eq!(merged.output_tokens, 150);
    assert_eq!(merged.total_tokens, 450);
    assert_eq!(merged.cache_read_tokens, 30);
    assert_eq!(merged.cache_write_tokens, 15);
}

#[test]
fn usage_merge_identity() {
    let u = IrUsage::from_io(42, 58);
    let merged = u.merge(IrUsage::default());
    assert_eq!(merged.input_tokens, 42);
    assert_eq!(merged.output_tokens, 58);
    assert_eq!(merged.total_tokens, 100);
}

#[test]
fn usage_serde_roundtrip() {
    let u = IrUsage::with_cache(500, 200, 50, 25);
    let json = serde_json::to_string(&u).unwrap();
    let u2: IrUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(u, u2);
}

#[test]
fn usage_copy_semantics() {
    let u = IrUsage::from_io(10, 20);
    let u2 = u;
    assert_eq!(u.input_tokens, u2.input_tokens);
}

#[test]
fn usage_merge_associative() {
    let a = IrUsage::from_io(10, 20);
    let b = IrUsage::from_io(30, 40);
    let c = IrUsage::from_io(50, 60);
    let ab_c = a.merge(b).merge(c);
    let a_bc = a.merge(b.merge(c));
    assert_eq!(ab_c, a_bc);
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  IR Serde Roundtrip — every type (12 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn serde_roundtrip_ir_role_all_variants() {
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
fn serde_roundtrip_text_block() {
    let block = IrContentBlock::Text {
        text: "hello".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
    assert!(json.contains(r#""type":"text"#));
}

#[test]
fn serde_roundtrip_image_block() {
    let block = IrContentBlock::Image {
        media_type: "image/jpeg".into(),
        data: "base64data==".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_roundtrip_tool_use_block() {
    let block = IrContentBlock::ToolUse {
        id: "call_123".into(),
        name: "search".into(),
        input: json!({"query": "test", "limit": 10}),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_roundtrip_tool_result_block() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "call_123".into(),
        content: vec![IrContentBlock::Text {
            text: "found it".into(),
        }],
        is_error: false,
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_roundtrip_tool_result_error() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "call_err".into(),
        content: vec![IrContentBlock::Text {
            text: "not found".into(),
        }],
        is_error: true,
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_roundtrip_thinking_block() {
    let block = IrContentBlock::Thinking {
        text: "Reasoning step by step...".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_roundtrip_ir_message() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Here you go:".into(),
            },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "ls".into(),
                input: json!({"dir": "/"}),
            },
        ],
    );
    let json = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn serde_roundtrip_ir_message_with_metadata() {
    let mut msg = IrMessage::text(IrRole::User, "hi");
    msg.metadata.insert("source".into(), json!("cli"));
    msg.metadata.insert("version".into(), json!(2));
    let json = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn serde_roundtrip_ir_conversation() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "Be helpful"))
        .push(IrMessage::text(IrRole::User, "What is Rust?"))
        .push(IrMessage::text(IrRole::Assistant, "A systems language."));
    let json = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(conv, back);
}

#[test]
fn serde_roundtrip_ir_tool_definition() {
    let def = IrToolDefinition {
        name: "read_file".into(),
        description: "Read a file by path".into(),
        parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    };
    let json = serde_json::to_string(&def).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(def, back);
}

#[test]
fn serde_metadata_omitted_when_empty() {
    let msg = IrMessage::text(IrRole::User, "hi");
    let json = serde_json::to_string(&msg).unwrap();
    assert!(!json.contains("metadata"));
}

// ═══════════════════════════════════════════════════════════════════════════
// §5  OpenAI ↔ IR Roundtrip (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_user_text_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "user".into(),
        content: Some("Hello".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let conv = openai_ir::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::User);
    let back = openai_ir::from_ir(&conv);
    assert_eq!(back[0].content.as_deref(), Some("Hello"));
}

#[test]
fn openai_system_assistant_roundtrip() {
    let msgs = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("instructions".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "assistant".into(),
            content: Some("ok".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let conv = openai_ir::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::Assistant);
    let back = openai_ir::from_ir(&conv);
    assert_eq!(back.len(), 2);
}

#[test]
fn openai_tool_call_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "call_42".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "search".into(),
                arguments: r#"{"q":"rust"}"#.into(),
            },
        }]),
        tool_call_id: None,
    }];
    let conv = openai_ir::to_ir(&msgs);
    let back = openai_ir::from_ir(&conv);
    let tc = &back[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "call_42");
    assert_eq!(tc.function.name, "search");
}

#[test]
fn openai_tool_result_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "tool".into(),
        content: Some("data here".into()),
        tool_calls: None,
        tool_call_id: Some("call_42".into()),
    }];
    let conv = openai_ir::to_ir(&msgs);
    let back = openai_ir::from_ir(&conv);
    assert_eq!(back[0].tool_call_id.as_deref(), Some("call_42"));
    assert_eq!(back[0].content.as_deref(), Some("data here"));
}

#[test]
fn openai_multi_tool_call_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![
            OpenAIToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "a".into(),
                    arguments: "{}".into(),
                },
            },
            OpenAIToolCall {
                id: "c2".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "b".into(),
                    arguments: "{}".into(),
                },
            },
        ]),
        tool_call_id: None,
    }];
    let conv = openai_ir::to_ir(&msgs);
    assert_eq!(conv.messages[0].content.len(), 2);
    let back = openai_ir::from_ir(&conv);
    assert_eq!(back[0].tool_calls.as_ref().unwrap().len(), 2);
}

#[test]
fn openai_text_plus_tool_call_roundtrip() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: Some("Let me check.".into()),
        tool_calls: Some(vec![OpenAIToolCall {
            id: "c1".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "ls".into(),
                arguments: "{}".into(),
            },
        }]),
        tool_call_id: None,
    }];
    let conv = openai_ir::to_ir(&msgs);
    assert_eq!(conv.messages[0].content.len(), 2);
    let back = openai_ir::from_ir(&conv);
    assert_eq!(back[0].content.as_deref(), Some("Let me check."));
    assert!(back[0].tool_calls.is_some());
}

#[test]
fn openai_unknown_role_defaults_to_user() {
    let msgs = vec![OpenAIMessage {
        role: "developer".into(),
        content: Some("x".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let conv = openai_ir::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

#[test]
fn openai_malformed_args_kept_as_string() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "c1".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "f".into(),
                arguments: "not-json".into(),
            },
        }]),
        tool_call_id: None,
    }];
    let conv = openai_ir::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => {
            assert_eq!(input, &serde_json::Value::String("not-json".into()));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §6  Claude ↔ IR Roundtrip (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn claude_user_text_roundtrip() {
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: "Hello".into(),
    }];
    let conv = claude_ir::to_ir(&msgs, None);
    assert_eq!(conv.messages[0].role, IrRole::User);
    let back = claude_ir::from_ir(&conv);
    assert_eq!(back[0].content, "Hello");
}

#[test]
fn claude_system_prompt_roundtrip() {
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: "Hi".into(),
    }];
    let conv = claude_ir::to_ir(&msgs, Some("Be helpful"));
    assert_eq!(conv.messages[0].role, IrRole::System);
    let sys = claude_ir::extract_system_prompt(&conv);
    assert_eq!(sys.as_deref(), Some("Be helpful"));
}

#[test]
fn claude_tool_use_roundtrip() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "read_file".into(),
        input: json!({"path": "main.rs"}),
    }];
    let msgs = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = claude_ir::to_ir(&msgs, None);
    let back = claude_ir::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::ToolUse { id, name, .. } => {
            assert_eq!(id, "tu_1");
            assert_eq!(name, "read_file");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn claude_tool_result_roundtrip() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: Some("file data".into()),
        is_error: None,
    }];
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = claude_ir::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult {
            tool_use_id,
            is_error,
            ..
        } => {
            assert_eq!(tool_use_id, "tu_1");
            assert!(!is_error);
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn claude_tool_result_error_flag() {
    let blocks = vec![ClaudeContentBlock::ToolResult {
        tool_use_id: "tu_err".into(),
        content: Some("not found".into()),
        is_error: Some(true),
    }];
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = claude_ir::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolResult { is_error, .. } => assert!(is_error),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn claude_thinking_roundtrip() {
    let blocks = vec![ClaudeContentBlock::Thinking {
        thinking: "Step 1: analyze the problem".into(),
        signature: Some("sig_abc".into()),
    }];
    let msgs = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = claude_ir::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::Thinking { text } => assert!(text.contains("Step 1")),
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn claude_image_base64_roundtrip() {
    let blocks = vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Base64 {
            media_type: "image/png".into(),
            data: "abc123".into(),
        },
    }];
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = claude_ir::to_ir(&msgs, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/png");
            assert_eq!(data, "abc123");
        }
        other => panic!("expected Image, got {other:?}"),
    }
}

#[test]
fn claude_system_skipped_in_from_ir() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    let back = claude_ir::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
}

// ═══════════════════════════════════════════════════════════════════════════
// §7  Gemini ↔ IR Roundtrip (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn gemini_user_text_roundtrip() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Hello".into())],
    }];
    let conv = gemini_ir::to_ir(&contents, None);
    assert_eq!(conv.messages[0].role, IrRole::User);
    let back = gemini_ir::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Hello"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn gemini_model_text_roundtrip() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::Text("Hi there".into())],
    }];
    let conv = gemini_ir::to_ir(&contents, None);
    assert_eq!(conv.messages[0].role, IrRole::Assistant);
    let back = gemini_ir::from_ir(&conv);
    assert_eq!(back[0].role, "model");
}

#[test]
fn gemini_system_instruction_roundtrip() {
    let sys = GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Be concise.".into())],
    };
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Hi".into())],
    }];
    let conv = gemini_ir::to_ir(&contents, Some(&sys));
    assert_eq!(conv.messages[0].role, IrRole::System);
    let extracted = gemini_ir::extract_system_instruction(&conv).unwrap();
    match &extracted.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Be concise."),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn gemini_function_call_roundtrip() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "search".into(),
            args: json!({"query": "rust"}),
        }],
    }];
    let conv = gemini_ir::to_ir(&contents, None);
    let back = gemini_ir::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args, &json!({"query": "rust"}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn gemini_function_response_roundtrip() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "search".into(),
            response: json!("results"),
        }],
    }];
    let conv = gemini_ir::to_ir(&contents, None);
    let back = gemini_ir::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::FunctionResponse { name, response } => {
            assert_eq!(name, "search");
            assert_eq!(response, &json!("results"));
        }
        other => panic!("expected FunctionResponse, got {other:?}"),
    }
}

#[test]
fn gemini_inline_data_roundtrip() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/jpeg".into(),
            data: "base64data".into(),
        })],
    }];
    let conv = gemini_ir::to_ir(&contents, None);
    let back = gemini_ir::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::InlineData(d) => {
            assert_eq!(d.mime_type, "image/jpeg");
            assert_eq!(d.data, "base64data");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

#[test]
fn gemini_system_skipped_in_from_ir() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    let back = gemini_ir::from_ir(&conv);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].role, "user");
}

#[test]
fn gemini_synthesized_id_for_function_call() {
    let contents = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "my_tool".into(),
            args: json!({}),
        }],
    }];
    let conv = gemini_ir::to_ir(&contents, None);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { id, .. } => assert_eq!(id, "gemini_my_tool"),
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §8  Codex ↔ IR Roundtrip (8 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn codex_input_to_ir() {
    let items = vec![
        CodexInputItem::Message {
            role: "system".into(),
            content: "Be helpful".into(),
        },
        CodexInputItem::Message {
            role: "user".into(),
            content: "Hello".into(),
        },
    ];
    let conv = codex_ir::input_to_ir(&items);
    assert_eq!(conv.len(), 2);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].role, IrRole::User);
}

#[test]
fn codex_response_message_roundtrip() {
    let items = vec![CodexResponseItem::Message {
        role: "assistant".into(),
        content: vec![CodexContentPart::OutputText {
            text: "Done!".into(),
        }],
    }];
    let conv = codex_ir::to_ir(&items);
    let back = codex_ir::from_ir(&conv);
    assert_eq!(back.len(), 1);
    match &back[0] {
        CodexResponseItem::Message { content, .. } => match &content[0] {
            CodexContentPart::OutputText { text } => assert_eq!(text, "Done!"),
        },
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn codex_function_call_roundtrip() {
    let items = vec![CodexResponseItem::FunctionCall {
        id: "fc_1".into(),
        call_id: None,
        name: "shell".into(),
        arguments: r#"{"cmd":"ls"}"#.into(),
    }];
    let conv = codex_ir::to_ir(&items);
    let back = codex_ir::from_ir(&conv);
    match &back[0] {
        CodexResponseItem::FunctionCall { id, name, .. } => {
            assert_eq!(id, "fc_1");
            assert_eq!(name, "shell");
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn codex_function_call_output_roundtrip() {
    let items = vec![CodexResponseItem::FunctionCallOutput {
        call_id: "fc_1".into(),
        output: "file.txt".into(),
    }];
    let conv = codex_ir::to_ir(&items);
    let back = codex_ir::from_ir(&conv);
    match &back[0] {
        CodexResponseItem::FunctionCallOutput { call_id, output } => {
            assert_eq!(call_id, "fc_1");
            assert_eq!(output, "file.txt");
        }
        other => panic!("expected FunctionCallOutput, got {other:?}"),
    }
}

#[test]
fn codex_reasoning_roundtrip() {
    let items = vec![CodexResponseItem::Reasoning {
        summary: vec![ReasoningSummary {
            text: "thinking...".into(),
        }],
    }];
    let conv = codex_ir::to_ir(&items);
    let back = codex_ir::from_ir(&conv);
    match &back[0] {
        CodexResponseItem::Reasoning { summary } => {
            assert_eq!(summary[0].text, "thinking...");
        }
        other => panic!("expected Reasoning, got {other:?}"),
    }
}

#[test]
fn codex_usage_to_ir() {
    let usage = CodexUsage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
    };
    let ir = codex_ir::usage_to_ir(&usage);
    assert_eq!(ir.input_tokens, 100);
    assert_eq!(ir.output_tokens, 50);
    assert_eq!(ir.total_tokens, 150);
}

#[test]
fn codex_system_user_skipped_in_from_ir() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "user"),
        IrMessage::text(IrRole::Assistant, "asst"),
    ]);
    let back = codex_ir::from_ir(&conv);
    assert_eq!(back.len(), 1);
}

#[test]
fn codex_text_and_tool_use_splits() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Checking...".into(),
            },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "search".into(),
                input: json!({"q": "rust"}),
            },
        ],
    )]);
    let items = codex_ir::from_ir(&conv);
    assert_eq!(items.len(), 2);
    assert!(matches!(&items[0], CodexResponseItem::Message { .. }));
    assert!(matches!(&items[1], CodexResponseItem::FunctionCall { .. }));
}

// ═══════════════════════════════════════════════════════════════════════════
// §9  Kimi ↔ IR Roundtrip (6 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn kimi_user_text_roundtrip() {
    let msgs = vec![KimiMessage {
        role: "user".into(),
        content: Some("Hello".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let conv = kimi_ir::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::User);
    let back = kimi_ir::from_ir(&conv);
    assert_eq!(back[0].content.as_deref(), Some("Hello"));
}

#[test]
fn kimi_tool_call_roundtrip() {
    let msgs = vec![KimiMessage {
        role: "assistant".into(),
        content: None,
        tool_call_id: None,
        tool_calls: Some(vec![KimiToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: KimiFunctionCall {
                name: "web_search".into(),
                arguments: r#"{"q":"test"}"#.into(),
            },
        }]),
    }];
    let conv = kimi_ir::to_ir(&msgs);
    let back = kimi_ir::from_ir(&conv);
    let tc = &back[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.function.name, "web_search");
}

#[test]
fn kimi_tool_result_roundtrip() {
    let msgs = vec![KimiMessage {
        role: "tool".into(),
        content: Some("result data".into()),
        tool_call_id: Some("call_1".into()),
        tool_calls: None,
    }];
    let conv = kimi_ir::to_ir(&msgs);
    let back = kimi_ir::from_ir(&conv);
    assert_eq!(back[0].tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(back[0].content.as_deref(), Some("result data"));
}

#[test]
fn kimi_usage_to_ir() {
    let usage = KimiUsage {
        prompt_tokens: 200,
        completion_tokens: 80,
        total_tokens: 280,
    };
    let ir = kimi_ir::usage_to_ir(&usage);
    assert_eq!(ir.input_tokens, 200);
    assert_eq!(ir.output_tokens, 80);
    assert_eq!(ir.total_tokens, 280);
}

#[test]
fn kimi_system_message_roundtrip() {
    let msgs = vec![KimiMessage {
        role: "system".into(),
        content: Some("Be helpful.".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let conv = kimi_ir::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::System);
    let back = kimi_ir::from_ir(&conv);
    assert_eq!(back[0].role, "system");
}

#[test]
fn kimi_unknown_role_defaults_to_user() {
    let msgs = vec![KimiMessage {
        role: "custom".into(),
        content: Some("x".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let conv = kimi_ir::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::User);
}

// ═══════════════════════════════════════════════════════════════════════════
// §10  Copilot ↔ IR Roundtrip (6 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn copilot_user_text_roundtrip() {
    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "Hello".into(),
        name: None,
        copilot_references: vec![],
    }];
    let conv = copilot_ir::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::User);
    let back = copilot_ir::from_ir(&conv);
    assert_eq!(back[0].content, "Hello");
}

#[test]
fn copilot_references_roundtrip() {
    let refs = vec![CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: "file-0".into(),
        data: json!({"path": "src/main.rs"}),
        metadata: None,
    }];
    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "Read this file".into(),
        name: None,
        copilot_references: refs,
    }];
    let conv = copilot_ir::to_ir(&msgs);
    let back = copilot_ir::from_ir(&conv);
    assert_eq!(back[0].copilot_references.len(), 1);
    assert_eq!(back[0].copilot_references[0].id, "file-0");
}

#[test]
fn copilot_name_roundtrip() {
    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "Hi".into(),
        name: Some("alice".into()),
        copilot_references: vec![],
    }];
    let conv = copilot_ir::to_ir(&msgs);
    let back = copilot_ir::from_ir(&conv);
    assert_eq!(back[0].name.as_deref(), Some("alice"));
}

#[test]
fn copilot_tool_role_mapped_to_user() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Tool,
        vec![IrContentBlock::ToolResult {
            tool_use_id: "c1".into(),
            content: vec![IrContentBlock::Text {
                text: "result".into(),
            }],
            is_error: false,
        }],
    )]);
    let back = copilot_ir::from_ir(&conv);
    assert_eq!(back[0].role, "user");
}

#[test]
fn copilot_extract_references_across_messages() {
    let msgs = vec![
        CopilotMessage {
            role: "user".into(),
            content: "msg1".into(),
            name: None,
            copilot_references: vec![CopilotReference {
                ref_type: CopilotReferenceType::File,
                id: "f1".into(),
                data: json!({}),
                metadata: None,
            }],
        },
        CopilotMessage {
            role: "user".into(),
            content: "msg2".into(),
            name: None,
            copilot_references: vec![CopilotReference {
                ref_type: CopilotReferenceType::Snippet,
                id: "s1".into(),
                data: json!({}),
                metadata: None,
            }],
        },
    ];
    let conv = copilot_ir::to_ir(&msgs);
    let all_refs = copilot_ir::extract_references(&conv);
    assert_eq!(all_refs.len(), 2);
}

#[test]
fn copilot_thinking_becomes_text() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "reasoning...".into(),
        }],
    )]);
    let back = copilot_ir::from_ir(&conv);
    assert_eq!(back[0].content, "reasoning...");
}

// ═══════════════════════════════════════════════════════════════════════════
// §11  Tool Call / Tool Result Pairing (6 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn tool_call_and_result_ids_match() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_abc".into(),
                name: "read_file".into(),
                input: json!({"path": "main.rs"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_abc".into(),
                content: vec![IrContentBlock::Text {
                    text: "fn main() {}".into(),
                }],
                is_error: false,
            }],
        ),
    ]);
    let calls = conv.tool_calls();
    assert_eq!(calls.len(), 1);
    match calls[0] {
        IrContentBlock::ToolUse { id, .. } => match &conv.messages[1].content[0] {
            IrContentBlock::ToolResult { tool_use_id, .. } => assert_eq!(id, tool_use_id),
            other => panic!("expected ToolResult, got {other:?}"),
        },
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn multiple_tool_calls_with_matching_results() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::Assistant,
            vec![
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
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![IrContentBlock::Text {
                    text: "data1".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c2".into(),
                content: vec![IrContentBlock::Text { text: "ok".into() }],
                is_error: false,
            }],
        ),
    ]);
    assert_eq!(conv.tool_calls().len(), 2);
    assert_eq!(conv.messages_by_role(IrRole::Tool).len(), 2);
}

#[test]
fn tool_result_with_error() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "read_file".into(),
                input: json!({"path": "nonexistent.txt"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![IrContentBlock::Text {
                    text: "File not found".into(),
                }],
                is_error: true,
            }],
        ),
    ]);
    match &conv.messages[1].content[0] {
        IrContentBlock::ToolResult { is_error, .. } => assert!(is_error),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn tool_result_with_empty_content() {
    let result = IrContentBlock::ToolResult {
        tool_use_id: "c1".into(),
        content: vec![],
        is_error: false,
    };
    let msg = IrMessage::new(IrRole::Tool, vec![result]);
    let json = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json).unwrap();
    match &back.content[0] {
        IrContentBlock::ToolResult { content, .. } => assert!(content.is_empty()),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn tool_result_nested_content_blocks() {
    let result = IrContentBlock::ToolResult {
        tool_use_id: "c1".into(),
        content: vec![
            IrContentBlock::Text {
                text: "Line 1\n".into(),
            },
            IrContentBlock::Text {
                text: "Line 2\n".into(),
            },
        ],
        is_error: false,
    };
    let json = serde_json::to_string(&result).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    match back {
        IrContentBlock::ToolResult { content, .. } => assert_eq!(content.len(), 2),
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn tool_use_with_complex_input() {
    let input = json!({
        "command": "grep",
        "args": ["-r", "fn main", "src/"],
        "options": {"case_sensitive": false, "max_depth": 5}
    });
    let block = IrContentBlock::ToolUse {
        id: "c1".into(),
        name: "shell".into(),
        input: input.clone(),
    };
    let json_str = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json_str).unwrap();
    match back {
        IrContentBlock::ToolUse {
            input: back_input, ..
        } => assert_eq!(back_input, input),
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §12  Edge Cases (12 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_conversation_serde() {
    let conv = IrConversation::new();
    let json = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json).unwrap();
    assert!(back.is_empty());
}

#[test]
fn empty_content_text_message() {
    let msg = IrMessage::text(IrRole::User, "");
    assert_eq!(msg.text_content(), "");
    assert!(msg.is_text_only());
}

#[test]
fn empty_blocks_message() {
    let msg = IrMessage::new(IrRole::User, vec![]);
    assert!(msg.content.is_empty());
    assert!(msg.is_text_only()); // vacuously true
    assert!(msg.text_content().is_empty());
}

#[test]
fn unicode_content_roundtrip() {
    let unicode_text = "Hello 🌍! Привет мир. こんにちは世界. مرحبا بالعالم";
    let msg = IrMessage::text(IrRole::User, unicode_text);
    let json = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.text_content(), unicode_text);
}

#[test]
fn emoji_heavy_content_roundtrip() {
    let emoji_text = "🔥🚀💻🎯✅❌🔧⚡🎉🌟🦀";
    let msg = IrMessage::text(IrRole::Assistant, emoji_text);
    let json = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.text_content(), emoji_text);
}

#[test]
fn very_long_content() {
    let long_text: String = "x".repeat(100_000);
    let msg = IrMessage::text(IrRole::User, &long_text);
    let json = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.text_content().len(), 100_000);
}

#[test]
fn special_characters_in_text() {
    let text = "Line 1\nLine 2\tTabbed\r\nWindows newline\\ backslash \"quoted\"";
    let msg = IrMessage::text(IrRole::User, text);
    let json = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.text_content(), text);
}

#[test]
fn null_json_in_tool_input() {
    let block = IrContentBlock::ToolUse {
        id: "c1".into(),
        name: "noop".into(),
        input: serde_json::Value::Null,
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    match back {
        IrContentBlock::ToolUse { input, .. } => assert!(input.is_null()),
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn deeply_nested_json_in_tool_input() {
    let input = json!({"a": {"b": {"c": {"d": {"e": "deep"}}}}});
    let block = IrContentBlock::ToolUse {
        id: "c1".into(),
        name: "nested".into(),
        input: input.clone(),
    };
    let json_str = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json_str).unwrap();
    assert_eq!(block, back);
}

#[test]
fn many_content_blocks_in_one_message() {
    let blocks: Vec<IrContentBlock> = (0..50)
        .map(|i| IrContentBlock::Text {
            text: format!("Block {i}"),
        })
        .collect();
    let msg = IrMessage::new(IrRole::Assistant, blocks);
    assert_eq!(msg.content.len(), 50);
    assert!(msg.is_text_only());
    let json = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.content.len(), 50);
}

#[test]
fn large_conversation() {
    let msgs: Vec<IrMessage> = (0..200)
        .map(|i| {
            let role = if i % 2 == 0 {
                IrRole::User
            } else {
                IrRole::Assistant
            };
            IrMessage::text(role, format!("Message {i}"))
        })
        .collect();
    let conv = IrConversation::from_messages(msgs);
    assert_eq!(conv.len(), 200);
    assert_eq!(conv.messages_by_role(IrRole::User).len(), 100);
    let json = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(back.len(), 200);
}

#[test]
fn metadata_with_complex_values() {
    let mut msg = IrMessage::text(IrRole::User, "test");
    msg.metadata.insert("array".into(), json!([1, 2, 3]));
    msg.metadata
        .insert("nested".into(), json!({"a": {"b": true}}));
    msg.metadata
        .insert("null_val".into(), serde_json::Value::Null);
    let json = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg.metadata, back.metadata);
}

// ═══════════════════════════════════════════════════════════════════════════
// §13  Property: JSON → back gives same type (6 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn property_text_block_roundtrip_identity() {
    let texts = ["", "hello", "a\nb\nc", "🦀", "x".repeat(10_000).as_str()].map(String::from);
    for t in texts {
        let block = IrContentBlock::Text { text: t.clone() };
        let json = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, back, "failed for text: {t:?}");
    }
}

#[test]
fn property_all_roles_roundtrip() {
    for role in [
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        let msg = IrMessage::text(role, "test");
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back, "failed for role: {role:?}");
    }
}

#[test]
fn property_tool_use_various_inputs() {
    let inputs = vec![
        json!(null),
        json!({}),
        json!({"key": "value"}),
        json!([1, 2, 3]),
        json!({"nested": {"deep": true}}),
        json!("string_input"),
        json!(42),
    ];
    for input in inputs {
        let block = IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "test".into(),
            input: input.clone(),
        };
        let json_str = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json_str).unwrap();
        assert_eq!(block, back, "failed for input: {input:?}");
    }
}

#[test]
fn property_conversation_json_roundtrip() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "sys"))
        .push(IrMessage::text(IrRole::User, "hello"))
        .push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me help.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "search".into(),
                    input: json!({"q": "rust"}),
                },
            ],
        ))
        .push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "results".into(),
                }],
                is_error: false,
            }],
        ))
        .push(IrMessage::text(IrRole::Assistant, "Found it."));
    let json = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(conv, back);
}

#[test]
fn property_tool_definition_roundtrip_various_schemas() {
    let schemas = vec![
        json!({"type": "object"}),
        json!({"type": "object", "properties": {"x": {"type": "number"}}}),
        json!({"type": "object", "properties": {}, "additionalProperties": false}),
    ];
    for schema in schemas {
        let def = IrToolDefinition {
            name: "tool".into(),
            description: "desc".into(),
            parameters: schema.clone(),
        };
        let json_str = serde_json::to_string(&def).unwrap();
        let back: IrToolDefinition = serde_json::from_str(&json_str).unwrap();
        assert_eq!(def, back, "failed for schema: {schema:?}");
    }
}

#[test]
fn property_image_block_roundtrip() {
    let media_types = ["image/png", "image/jpeg", "image/gif", "image/webp"];
    for mt in media_types {
        let block = IrContentBlock::Image {
            media_type: mt.into(),
            data: "dGVzdA==".into(),
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, back, "failed for media_type: {mt}");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §14  Cross-Dialect: same semantic meaning (10 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn cross_dialect_user_text_same_ir() {
    let openai_msg = vec![OpenAIMessage {
        role: "user".into(),
        content: Some("Hello".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let claude_msg = vec![ClaudeMessage {
        role: "user".into(),
        content: "Hello".into(),
    }];
    let gemini_msg = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Hello".into())],
    }];
    let kimi_msg = vec![KimiMessage {
        role: "user".into(),
        content: Some("Hello".into()),
        tool_call_id: None,
        tool_calls: None,
    }];

    let ir_openai = openai_ir::to_ir(&openai_msg);
    let ir_claude = claude_ir::to_ir(&claude_msg, None);
    let ir_gemini = gemini_ir::to_ir(&gemini_msg, None);
    let ir_kimi = kimi_ir::to_ir(&kimi_msg);

    // All produce the same IR
    assert_eq!(ir_openai.messages[0].role, IrRole::User);
    assert_eq!(ir_claude.messages[0].role, IrRole::User);
    assert_eq!(ir_gemini.messages[0].role, IrRole::User);
    assert_eq!(ir_kimi.messages[0].role, IrRole::User);

    assert_eq!(ir_openai.messages[0].text_content(), "Hello");
    assert_eq!(ir_claude.messages[0].text_content(), "Hello");
    assert_eq!(ir_gemini.messages[0].text_content(), "Hello");
    assert_eq!(ir_kimi.messages[0].text_content(), "Hello");
}

#[test]
fn cross_dialect_assistant_text_same_ir() {
    let openai_msg = vec![OpenAIMessage {
        role: "assistant".into(),
        content: Some("Sure!".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let claude_msg = vec![ClaudeMessage {
        role: "assistant".into(),
        content: "Sure!".into(),
    }];
    let gemini_msg = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::Text("Sure!".into())],
    }];

    let ir_openai = openai_ir::to_ir(&openai_msg);
    let ir_claude = claude_ir::to_ir(&claude_msg, None);
    let ir_gemini = gemini_ir::to_ir(&gemini_msg, None);

    assert_eq!(ir_openai.messages[0].role, IrRole::Assistant);
    assert_eq!(ir_claude.messages[0].role, IrRole::Assistant);
    assert_eq!(ir_gemini.messages[0].role, IrRole::Assistant);
}

#[test]
fn cross_dialect_system_prompt_same_ir() {
    let openai_msgs = vec![OpenAIMessage {
        role: "system".into(),
        content: Some("Be helpful".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let kimi_msgs = vec![KimiMessage {
        role: "system".into(),
        content: Some("Be helpful".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let copilot_msgs = vec![CopilotMessage {
        role: "system".into(),
        content: "Be helpful".into(),
        name: None,
        copilot_references: vec![],
    }];

    let ir_openai = openai_ir::to_ir(&openai_msgs);
    let ir_kimi = kimi_ir::to_ir(&kimi_msgs);
    let ir_copilot = copilot_ir::to_ir(&copilot_msgs);

    // Claude and Gemini handle system differently (as param), so we test separately
    let ir_claude = claude_ir::to_ir(&[], Some("Be helpful"));

    assert_eq!(ir_openai.messages[0].text_content(), "Be helpful");
    assert_eq!(ir_kimi.messages[0].text_content(), "Be helpful");
    assert_eq!(ir_copilot.messages[0].text_content(), "Be helpful");
    assert_eq!(ir_claude.messages[0].text_content(), "Be helpful");

    for ir in [&ir_openai, &ir_kimi, &ir_copilot, &ir_claude] {
        assert_eq!(ir.messages[0].role, IrRole::System);
    }
}

#[test]
fn cross_dialect_tool_call_same_ir_semantics() {
    // OpenAI
    let openai_msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"main.rs"}"#.into(),
            },
        }]),
        tool_call_id: None,
    }];

    // Kimi (same format as OpenAI)
    let kimi_msgs = vec![KimiMessage {
        role: "assistant".into(),
        content: None,
        tool_call_id: None,
        tool_calls: Some(vec![KimiToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: KimiFunctionCall {
                name: "read_file".into(),
                arguments: r#"{"path":"main.rs"}"#.into(),
            },
        }]),
    }];

    let ir_openai = openai_ir::to_ir(&openai_msgs);
    let ir_kimi = kimi_ir::to_ir(&kimi_msgs);

    // Both produce ToolUse with same name and input
    for ir in [&ir_openai, &ir_kimi] {
        match &ir.messages[0].content[0] {
            IrContentBlock::ToolUse { name, input, .. } => {
                assert_eq!(name, "read_file");
                assert_eq!(input, &json!({"path": "main.rs"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }
}

#[test]
fn cross_dialect_tool_result_same_ir_semantics() {
    // OpenAI
    let openai_msgs = vec![OpenAIMessage {
        role: "tool".into(),
        content: Some("fn main() {}".into()),
        tool_calls: None,
        tool_call_id: Some("call_1".into()),
    }];

    // Kimi
    let kimi_msgs = vec![KimiMessage {
        role: "tool".into(),
        content: Some("fn main() {}".into()),
        tool_call_id: Some("call_1".into()),
        tool_calls: None,
    }];

    let ir_openai = openai_ir::to_ir(&openai_msgs);
    let ir_kimi = kimi_ir::to_ir(&kimi_msgs);

    for ir in [&ir_openai, &ir_kimi] {
        assert_eq!(ir.messages[0].role, IrRole::Tool);
        match &ir.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                is_error,
                ..
            } => {
                assert_eq!(tool_use_id, "call_1");
                assert!(!is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }
}

#[test]
fn cross_dialect_image_same_ir() {
    // Claude
    let claude_blocks = vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Base64 {
            media_type: "image/png".into(),
            data: "abc123".into(),
        },
    }];
    let claude_msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&claude_blocks).unwrap(),
    }];

    // Gemini
    let gemini_contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/png".into(),
            data: "abc123".into(),
        })],
    }];

    let ir_claude = claude_ir::to_ir(&claude_msgs, None);
    let ir_gemini = gemini_ir::to_ir(&gemini_contents, None);

    for ir in [&ir_claude, &ir_gemini] {
        match &ir.messages[0].content[0] {
            IrContentBlock::Image { media_type, data } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "abc123");
            }
            other => panic!("expected Image, got {other:?}"),
        }
    }
}

#[test]
fn cross_dialect_openai_to_claude_via_ir() {
    let openai_msgs = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("Be helpful".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "assistant".into(),
            content: Some("Hi!".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let ir = openai_ir::to_ir(&openai_msgs);

    // Claude skips system in from_ir
    let claude_msgs = claude_ir::from_ir(&ir);
    assert_eq!(claude_msgs.len(), 2);
    assert_eq!(claude_msgs[0].role, "user");
    assert_eq!(claude_msgs[0].content, "Hello");

    // System prompt extracted separately
    let sys = claude_ir::extract_system_prompt(&ir);
    assert_eq!(sys.as_deref(), Some("Be helpful"));
}

#[test]
fn cross_dialect_claude_to_gemini_via_ir() {
    let claude_msgs = vec![
        ClaudeMessage {
            role: "user".into(),
            content: "What is Rust?".into(),
        },
        ClaudeMessage {
            role: "assistant".into(),
            content: "A systems language.".into(),
        },
    ];
    let ir = claude_ir::to_ir(&claude_msgs, Some("Be concise"));

    let gemini_contents = gemini_ir::from_ir(&ir);
    assert_eq!(gemini_contents.len(), 2); // system skipped
    assert_eq!(gemini_contents[0].role, "user");
    assert_eq!(gemini_contents[1].role, "model");

    let sys = gemini_ir::extract_system_instruction(&ir).unwrap();
    match &sys.parts[0] {
        GeminiPart::Text(t) => assert_eq!(t, "Be concise"),
        other => panic!("expected Text, got {other:?}"),
    }
}

#[test]
fn cross_dialect_gemini_to_openai_via_ir() {
    let gemini_contents = vec![
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Hi".into())],
        },
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::Text("Hello!".into())],
        },
    ];
    let ir = gemini_ir::to_ir(&gemini_contents, None);
    let openai_msgs = openai_ir::from_ir(&ir);
    assert_eq!(openai_msgs.len(), 2);
    assert_eq!(openai_msgs[0].role, "user");
    assert_eq!(openai_msgs[0].content.as_deref(), Some("Hi"));
    assert_eq!(openai_msgs[1].role, "assistant");
    assert_eq!(openai_msgs[1].content.as_deref(), Some("Hello!"));
}

#[test]
fn cross_dialect_codex_to_kimi_via_ir() {
    let codex_items = vec![
        CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "Here you go.".into(),
            }],
        },
        CodexResponseItem::FunctionCall {
            id: "fc_1".into(),
            call_id: None,
            name: "shell".into(),
            arguments: r#"{"cmd":"ls"}"#.into(),
        },
    ];
    let ir = codex_ir::to_ir(&codex_items);
    let kimi_msgs = kimi_ir::from_ir(&ir);

    // First is assistant text message, second is assistant tool call
    assert_eq!(kimi_msgs[0].role, "assistant");
    assert_eq!(kimi_msgs[0].content.as_deref(), Some("Here you go."));
    assert_eq!(kimi_msgs[1].role, "assistant");
    assert!(kimi_msgs[1].tool_calls.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// §15  Rich Metadata (4 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn metadata_preserved_through_serde() {
    let mut msg = IrMessage::text(IrRole::User, "test");
    msg.metadata.insert("request_id".into(), json!("req-123"));
    msg.metadata.insert("latency_ms".into(), json!(42));
    msg.metadata
        .insert("tags".into(), json!(["fast", "cached"]));
    let json = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(back.metadata["request_id"], json!("req-123"));
    assert_eq!(back.metadata["latency_ms"], json!(42));
}

#[test]
fn metadata_btreemap_deterministic_order() {
    let mut msg = IrMessage::text(IrRole::User, "test");
    msg.metadata.insert("z_key".into(), json!(1));
    msg.metadata.insert("a_key".into(), json!(2));
    msg.metadata.insert("m_key".into(), json!(3));
    let json = serde_json::to_string(&msg).unwrap();
    // BTreeMap ensures alphabetical ordering
    let a_pos = json.find("a_key").unwrap();
    let m_pos = json.find("m_key").unwrap();
    let z_pos = json.find("z_key").unwrap();
    assert!(a_pos < m_pos);
    assert!(m_pos < z_pos);
}

#[test]
fn metadata_in_conversation_preserved() {
    let mut msg = IrMessage::text(IrRole::User, "with meta");
    msg.metadata.insert("source".into(), json!("web"));
    let conv = IrConversation::new().push(msg);
    let json = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(back.messages[0].metadata["source"], json!("web"));
}

#[test]
fn copilot_references_as_metadata() {
    let refs = vec![CopilotReference {
        ref_type: CopilotReferenceType::Repository,
        id: "repo-0".into(),
        data: json!({"owner": "octocat", "name": "hello"}),
        metadata: None,
    }];
    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "Check this repo".into(),
        name: None,
        copilot_references: refs,
    }];
    let conv = copilot_ir::to_ir(&msgs);
    assert!(conv.messages[0].metadata.contains_key("copilot_references"));

    // Verify the metadata survives JSON roundtrip
    let json = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json).unwrap();
    assert!(back.messages[0].metadata.contains_key("copilot_references"));
}

// ═══════════════════════════════════════════════════════════════════════════
// §16  Empty input handling across dialects (6 tests)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn openai_empty_messages() {
    let conv = openai_ir::to_ir(&[]);
    assert!(conv.is_empty());
    let back = openai_ir::from_ir(&conv);
    assert!(back.is_empty());
}

#[test]
fn claude_empty_messages() {
    let conv = claude_ir::to_ir(&[], None);
    assert!(conv.is_empty());
    let back = claude_ir::from_ir(&conv);
    assert!(back.is_empty());
}

#[test]
fn gemini_empty_contents() {
    let conv = gemini_ir::to_ir(&[], None);
    assert!(conv.is_empty());
    let back = gemini_ir::from_ir(&conv);
    assert!(back.is_empty());
}

#[test]
fn codex_empty_items() {
    let conv = codex_ir::to_ir(&[]);
    assert!(conv.is_empty());
    let back = codex_ir::from_ir(&conv);
    assert!(back.is_empty());
}

#[test]
fn kimi_empty_messages() {
    let conv = kimi_ir::to_ir(&[]);
    assert!(conv.is_empty());
    let back = kimi_ir::from_ir(&conv);
    assert!(back.is_empty());
}

#[test]
fn copilot_empty_messages() {
    let conv = copilot_ir::to_ir(&[]);
    assert!(conv.is_empty());
    let back = copilot_ir::from_ir(&conv);
    assert!(back.is_empty());
}
