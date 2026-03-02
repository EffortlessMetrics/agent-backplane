// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the IR (Intermediate Representation) layer.
//!
//! Tests cover IR type construction, dialect lifting/lowering, roundtrip
//! fidelity, cross-dialect mapping, serde, and edge cases.

use std::collections::BTreeMap;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_dialect::Dialect;
use abp_mapping::{Fidelity, MappingError, MappingRegistry, MappingRule};
use serde_json::json;

// ── SDK imports ─────────────────────────────────────────────────────────

use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeImageSource, ClaudeMessage};
use abp_claude_sdk::lowering as claude_low;

use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};
use abp_openai_sdk::lowering as openai_low;

use abp_gemini_sdk::dialect::{GeminiContent, GeminiInlineData, GeminiPart};
use abp_gemini_sdk::lowering as gemini_low;

use abp_codex_sdk::dialect::{
    CodexContentPart, CodexInputItem, CodexResponseItem, CodexUsage, ReasoningSummary,
};
use abp_codex_sdk::lowering as codex_low;

use abp_kimi_sdk::dialect::{KimiFunctionCall, KimiMessage, KimiToolCall, KimiUsage};
use abp_kimi_sdk::lowering as kimi_low;

use abp_copilot_sdk::dialect::{CopilotMessage, CopilotReference, CopilotReferenceType};
use abp_copilot_sdk::lowering as copilot_low;

// ═══════════════════════════════════════════════════════════════════════════
// §1  IR Type Construction
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_message_text_construction() {
    let msg = IrMessage::text(IrRole::User, "hello");
    assert_eq!(msg.role, IrRole::User);
    assert_eq!(msg.text_content(), "hello");
    assert!(msg.is_text_only());
    assert!(msg.metadata.is_empty());
}

#[test]
fn ir_message_new_construction() {
    let blocks = vec![
        IrContentBlock::Text { text: "hi".into() },
        IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "search".into(),
            input: json!({"q": "rust"}),
        },
    ];
    let msg = IrMessage::new(IrRole::Assistant, blocks);
    assert!(!msg.is_text_only());
    assert_eq!(msg.text_content(), "hi");
    assert_eq!(msg.tool_use_blocks().len(), 1);
}

#[test]
fn ir_conversation_builder_pattern() {
    let conv = IrConversation::new()
        .push(IrMessage::text(IrRole::System, "instructions"))
        .push(IrMessage::text(IrRole::User, "hello"))
        .push(IrMessage::text(IrRole::Assistant, "hi"));
    assert_eq!(conv.len(), 3);
    assert!(!conv.is_empty());
    assert_eq!(
        conv.system_message().unwrap().text_content(),
        "instructions"
    );
    assert_eq!(conv.last_assistant().unwrap().text_content(), "hi");
    assert_eq!(conv.last_message().unwrap().role, IrRole::Assistant);
}

#[test]
fn ir_conversation_messages_by_role() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "a"),
        IrMessage::text(IrRole::Assistant, "b"),
        IrMessage::text(IrRole::User, "c"),
    ]);
    let users = conv.messages_by_role(IrRole::User);
    assert_eq!(users.len(), 2);
}

#[test]
fn ir_conversation_tool_calls_aggregation() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "read".into(),
                input: json!({}),
            }],
        ),
        IrMessage::text(IrRole::User, "gap"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t2".into(),
                name: "write".into(),
                input: json!({}),
            }],
        ),
    ]);
    assert_eq!(conv.tool_calls().len(), 2);
}

#[test]
fn ir_conversation_empty() {
    let conv = IrConversation::new();
    assert!(conv.is_empty());
    assert_eq!(conv.len(), 0);
    assert!(conv.system_message().is_none());
    assert!(conv.last_assistant().is_none());
    assert!(conv.last_message().is_none());
    assert!(conv.tool_calls().is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// §2  IR ← each dialect (lifting)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lift_openai_user_text() {
    let msgs = vec![OpenAIMessage {
        role: "user".into(),
        content: Some("Hello from OpenAI".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let conv = openai_low::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "Hello from OpenAI");
}

#[test]
fn lift_claude_user_text_with_system() {
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: "Hello from Claude".into(),
    }];
    let conv = claude_low::to_ir(&msgs, Some("Be helpful"));
    assert_eq!(conv.len(), 2);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].text_content(), "Hello from Claude");
}

#[test]
fn lift_gemini_user_text() {
    let contents = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::Text("Hello from Gemini".into())],
    }];
    let conv = gemini_low::to_ir(&contents, None);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "Hello from Gemini");
}

#[test]
fn lift_codex_input_items() {
    let items = vec![
        CodexInputItem::Message {
            role: "system".into(),
            content: "instructions".into(),
        },
        CodexInputItem::Message {
            role: "user".into(),
            content: "Hello from Codex".into(),
        },
    ];
    let conv = codex_low::input_to_ir(&items);
    assert_eq!(conv.messages[0].role, IrRole::System);
    assert_eq!(conv.messages[1].text_content(), "Hello from Codex");
}

#[test]
fn lift_kimi_user_text() {
    let msgs = vec![KimiMessage {
        role: "user".into(),
        content: Some("Hello from Kimi".into()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let conv = kimi_low::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "Hello from Kimi");
}

#[test]
fn lift_copilot_user_text() {
    let msgs = vec![CopilotMessage {
        role: "user".into(),
        content: "Hello from Copilot".into(),
        name: None,
        copilot_references: vec![],
    }];
    let conv = copilot_low::to_ir(&msgs);
    assert_eq!(conv.messages[0].role, IrRole::User);
    assert_eq!(conv.messages[0].text_content(), "Hello from Copilot");
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  IR → each dialect (lowering)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn lower_to_openai() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be helpful"),
        IrMessage::text(IrRole::User, "Hi"),
    ]);
    let msgs = openai_low::from_ir(&conv);
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, "system");
    assert_eq!(msgs[1].role, "user");
}

#[test]
fn lower_to_claude() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be helpful"),
        IrMessage::text(IrRole::User, "Hi"),
    ]);
    let msgs = claude_low::from_ir(&conv);
    // Claude skips system messages
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].role, "user");

    let sys = claude_low::extract_system_prompt(&conv);
    assert_eq!(sys.as_deref(), Some("Be helpful"));
}

#[test]
fn lower_to_gemini() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be helpful"),
        IrMessage::text(IrRole::User, "Hi"),
    ]);
    let contents = gemini_low::from_ir(&conv);
    // Gemini skips system messages
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0].role, "user");

    let sys = gemini_low::extract_system_instruction(&conv);
    assert!(sys.is_some());
}

#[test]
fn lower_to_codex() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "inst"),
        IrMessage::text(IrRole::User, "hi"),
        IrMessage::text(IrRole::Assistant, "hello"),
    ]);
    let items = codex_low::from_ir(&conv);
    // Codex skips system/user
    assert_eq!(items.len(), 1);
    match &items[0] {
        CodexResponseItem::Message { role, .. } => assert_eq!(role, "assistant"),
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn lower_to_kimi() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "Hi"),
    ]);
    let msgs = kimi_low::from_ir(&conv);
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, "system");
    assert_eq!(msgs[1].role, "user");
}

#[test]
fn lower_to_copilot() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "Hi"),
    ]);
    let msgs = copilot_low::from_ir(&conv);
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, "system");
    assert_eq!(msgs[1].role, "user");
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  Roundtrip fidelity: dialect → IR → same dialect
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_openai_text() {
    let orig = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("sys".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("hi".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "assistant".into(),
            content: Some("hello".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let conv = openai_low::to_ir(&orig);
    let back = openai_low::from_ir(&conv);
    assert_eq!(back.len(), 3);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[0].content.as_deref(), Some("sys"));
    assert_eq!(back[1].content.as_deref(), Some("hi"));
    assert_eq!(back[2].content.as_deref(), Some("hello"));
}

#[test]
fn roundtrip_openai_tool_call() {
    let orig = vec![OpenAIMessage {
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
    let conv = openai_low::to_ir(&orig);
    let back = openai_low::from_ir(&conv);
    let tc = &back[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "call_1");
    assert_eq!(tc.function.name, "read_file");
}

#[test]
fn roundtrip_openai_tool_result() {
    let orig = vec![OpenAIMessage {
        role: "tool".into(),
        content: Some("file data".into()),
        tool_calls: None,
        tool_call_id: Some("call_1".into()),
    }];
    let conv = openai_low::to_ir(&orig);
    let back = openai_low::from_ir(&conv);
    assert_eq!(back[0].role, "tool");
    assert_eq!(back[0].content.as_deref(), Some("file data"));
    assert_eq!(back[0].tool_call_id.as_deref(), Some("call_1"));
}

#[test]
fn roundtrip_claude_text() {
    let orig = vec![
        ClaudeMessage {
            role: "user".into(),
            content: "hi".into(),
        },
        ClaudeMessage {
            role: "assistant".into(),
            content: "hello".into(),
        },
    ];
    let conv = claude_low::to_ir(&orig, None);
    let back = claude_low::from_ir(&conv);
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].content, "hi");
    assert_eq!(back[1].content, "hello");
}

#[test]
fn roundtrip_claude_tool_use() {
    let blocks = vec![ClaudeContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "search".into(),
        input: json!({"q": "test"}),
    }];
    let orig = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = claude_low::to_ir(&orig, None);
    let back = claude_low::from_ir(&conv);
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::ToolUse { id, name, .. } => {
            assert_eq!(id, "tu_1");
            assert_eq!(name, "search");
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn roundtrip_gemini_text() {
    let orig = vec![
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("hi".into())],
        },
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::Text("hello".into())],
        },
    ];
    let conv = gemini_low::to_ir(&orig, None);
    let back = gemini_low::from_ir(&conv);
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].role, "user");
    assert_eq!(back[1].role, "model");
}

#[test]
fn roundtrip_gemini_function_call() {
    let orig = vec![GeminiContent {
        role: "model".into(),
        parts: vec![GeminiPart::FunctionCall {
            name: "search".into(),
            args: json!({"q": "rust"}),
        }],
    }];
    let conv = gemini_low::to_ir(&orig, None);
    let back = gemini_low::from_ir(&conv);
    match &back[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args, &json!({"q": "rust"}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn roundtrip_kimi_text() {
    let orig = vec![
        KimiMessage {
            role: "system".into(),
            content: Some("sys".into()),
            tool_call_id: None,
            tool_calls: None,
        },
        KimiMessage {
            role: "user".into(),
            content: Some("hi".into()),
            tool_call_id: None,
            tool_calls: None,
        },
    ];
    let conv = kimi_low::to_ir(&orig);
    let back = kimi_low::from_ir(&conv);
    assert_eq!(back[0].role, "system");
    assert_eq!(back[0].content.as_deref(), Some("sys"));
    assert_eq!(back[1].content.as_deref(), Some("hi"));
}

#[test]
fn roundtrip_kimi_tool_call() {
    let orig = vec![KimiMessage {
        role: "assistant".into(),
        content: None,
        tool_call_id: None,
        tool_calls: Some(vec![KimiToolCall {
            id: "k_c1".into(),
            call_type: "function".into(),
            function: KimiFunctionCall {
                name: "web_search".into(),
                arguments: r#"{"query":"rust"}"#.into(),
            },
        }]),
    }];
    let conv = kimi_low::to_ir(&orig);
    let back = kimi_low::from_ir(&conv);
    let tc = &back[0].tool_calls.as_ref().unwrap()[0];
    assert_eq!(tc.id, "k_c1");
    assert_eq!(tc.function.name, "web_search");
}

#[test]
fn roundtrip_copilot_text() {
    let orig = vec![CopilotMessage {
        role: "user".into(),
        content: "hi there".into(),
        name: Some("alice".into()),
        copilot_references: vec![],
    }];
    let conv = copilot_low::to_ir(&orig);
    let back = copilot_low::from_ir(&conv);
    assert_eq!(back[0].content, "hi there");
    assert_eq!(back[0].name.as_deref(), Some("alice"));
}

#[test]
fn roundtrip_copilot_references() {
    let refs = vec![CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: "f1".into(),
        data: json!({"path": "main.rs"}),
        metadata: None,
    }];
    let orig = vec![CopilotMessage {
        role: "user".into(),
        content: "check file".into(),
        name: None,
        copilot_references: refs,
    }];
    let conv = copilot_low::to_ir(&orig);
    let back = copilot_low::from_ir(&conv);
    assert_eq!(back[0].copilot_references.len(), 1);
    assert_eq!(back[0].copilot_references[0].id, "f1");
}

#[test]
fn roundtrip_codex_response_items() {
    let items = vec![
        CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "hello".into(),
            }],
        },
        CodexResponseItem::FunctionCall {
            id: "fc_1".into(),
            call_id: None,
            name: "shell".into(),
            arguments: r#"{"cmd":"ls"}"#.into(),
        },
        CodexResponseItem::FunctionCallOutput {
            call_id: "fc_1".into(),
            output: "file.txt".into(),
        },
    ];
    let conv = codex_low::to_ir(&items);
    let back = codex_low::from_ir(&conv);
    assert_eq!(back.len(), 3);
    assert!(matches!(&back[0], CodexResponseItem::Message { .. }));
    assert!(matches!(&back[1], CodexResponseItem::FunctionCall { .. }));
    assert!(matches!(
        &back[2],
        CodexResponseItem::FunctionCallOutput { .. }
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// §5  Cross-dialect mapping: A → IR → B
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn cross_openai_to_kimi_text() {
    let openai_msgs = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("sys".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("hi".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let ir = openai_low::to_ir(&openai_msgs);
    let kimi_msgs = kimi_low::from_ir(&ir);
    assert_eq!(kimi_msgs.len(), 2);
    assert_eq!(kimi_msgs[0].role, "system");
    assert_eq!(kimi_msgs[0].content.as_deref(), Some("sys"));
    assert_eq!(kimi_msgs[1].content.as_deref(), Some("hi"));
}

#[test]
fn cross_openai_to_claude() {
    let openai_msgs = vec![
        OpenAIMessage {
            role: "system".into(),
            content: Some("instructions".into()),
            tool_calls: None,
            tool_call_id: None,
        },
        OpenAIMessage {
            role: "user".into(),
            content: Some("question".into()),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let ir = openai_low::to_ir(&openai_msgs);
    let claude_msgs = claude_low::from_ir(&ir);
    // Claude drops system from messages, extracts separately
    assert_eq!(claude_msgs.len(), 1);
    assert_eq!(claude_msgs[0].role, "user");

    let sys = claude_low::extract_system_prompt(&ir);
    assert_eq!(sys.as_deref(), Some("instructions"));
}

#[test]
fn cross_claude_to_openai() {
    let claude_msgs = vec![
        ClaudeMessage {
            role: "user".into(),
            content: "hi".into(),
        },
        ClaudeMessage {
            role: "assistant".into(),
            content: "hello".into(),
        },
    ];
    let ir = claude_low::to_ir(&claude_msgs, Some("Be nice"));
    let openai_msgs = openai_low::from_ir(&ir);
    assert_eq!(openai_msgs.len(), 3);
    assert_eq!(openai_msgs[0].role, "system");
    assert_eq!(openai_msgs[0].content.as_deref(), Some("Be nice"));
    assert_eq!(openai_msgs[1].role, "user");
    assert_eq!(openai_msgs[2].role, "assistant");
}

#[test]
fn cross_gemini_to_openai() {
    let gemini = vec![
        GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("question".into())],
        },
        GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::Text("answer".into())],
        },
    ];
    let ir = gemini_low::to_ir(&gemini, None);
    let openai_msgs = openai_low::from_ir(&ir);
    assert_eq!(openai_msgs.len(), 2);
    assert_eq!(openai_msgs[0].role, "user");
    assert_eq!(openai_msgs[1].role, "assistant");
    assert_eq!(openai_msgs[1].content.as_deref(), Some("answer"));
}

#[test]
fn cross_openai_tool_call_to_gemini() {
    let openai_msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "search".into(),
                arguments: r#"{"q":"test"}"#.into(),
            },
        }]),
        tool_call_id: None,
    }];
    let ir = openai_low::to_ir(&openai_msgs);
    let gemini = gemini_low::from_ir(&ir);
    assert_eq!(gemini[0].role, "model");
    match &gemini[0].parts[0] {
        GeminiPart::FunctionCall { name, args } => {
            assert_eq!(name, "search");
            assert_eq!(args, &json!({"q": "test"}));
        }
        other => panic!("expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn cross_kimi_to_copilot() {
    let kimi_msgs = vec![
        KimiMessage {
            role: "system".into(),
            content: Some("sys prompt".into()),
            tool_call_id: None,
            tool_calls: None,
        },
        KimiMessage {
            role: "user".into(),
            content: Some("hi kimi".into()),
            tool_call_id: None,
            tool_calls: None,
        },
    ];
    let ir = kimi_low::to_ir(&kimi_msgs);
    let copilot_msgs = copilot_low::from_ir(&ir);
    assert_eq!(copilot_msgs.len(), 2);
    assert_eq!(copilot_msgs[0].role, "system");
    assert_eq!(copilot_msgs[0].content, "sys prompt");
    assert_eq!(copilot_msgs[1].content, "hi kimi");
}

// ═══════════════════════════════════════════════════════════════════════════
// §6  Fidelity scoring
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fidelity_lossless_check() {
    let f = Fidelity::Lossless;
    assert!(f.is_lossless());
    assert!(!f.is_unsupported());
}

#[test]
fn fidelity_lossy_labeled() {
    let f = Fidelity::LossyLabeled {
        warning: "thinking blocks stripped".into(),
    };
    assert!(!f.is_lossless());
    assert!(!f.is_unsupported());
}

#[test]
fn fidelity_unsupported() {
    let f = Fidelity::Unsupported {
        reason: "no image support".into(),
    };
    assert!(f.is_unsupported());
    assert!(!f.is_lossless());
}

#[test]
fn mapping_registry_rank_targets() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "tool_use".into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "streaming".into(),
        fidelity: Fidelity::Lossless,
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Gemini,
        feature: "tool_use".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "no call IDs".into(),
        },
    });
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Gemini,
        feature: "streaming".into(),
        fidelity: Fidelity::Lossless,
    });

    let ranked = reg.rank_targets(Dialect::OpenAi, &["tool_use", "streaming"]);
    // Claude has 2 lossless, Gemini has 1
    assert!(!ranked.is_empty());
    assert_eq!(ranked[0].0, Dialect::Claude);
    assert_eq!(ranked[0].1, 2);
}

#[test]
fn mapping_registry_lookup() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::Claude,
        target_dialect: Dialect::OpenAi,
        feature: "thinking".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "flattened to text".into(),
        },
    });
    let rule = reg.lookup(Dialect::Claude, Dialect::OpenAi, "thinking");
    assert!(rule.is_some());
    assert!(!rule.unwrap().fidelity.is_lossless());

    let missing = reg.lookup(Dialect::Claude, Dialect::Gemini, "thinking");
    assert!(missing.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
// §7  Content type mapping through IR
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn content_type_text_through_ir() {
    let ir = IrContentBlock::Text {
        text: "hello".into(),
    };
    let json = serde_json::to_value(&ir).unwrap();
    assert_eq!(json["type"], "text");
    assert_eq!(json["text"], "hello");
}

#[test]
fn content_type_image_through_ir() {
    let ir = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "iVBOR...".into(),
    };
    let json = serde_json::to_value(&ir).unwrap();
    assert_eq!(json["type"], "image");
    assert_eq!(json["media_type"], "image/png");
}

#[test]
fn content_type_tool_use_through_ir() {
    let ir = IrContentBlock::ToolUse {
        id: "t1".into(),
        name: "grep".into(),
        input: json!({"pattern": "fn main"}),
    };
    let json = serde_json::to_value(&ir).unwrap();
    assert_eq!(json["type"], "tool_use");
    assert_eq!(json["name"], "grep");
}

#[test]
fn content_type_tool_result_through_ir() {
    let ir = IrContentBlock::ToolResult {
        tool_use_id: "t1".into(),
        content: vec![IrContentBlock::Text {
            text: "result".into(),
        }],
        is_error: true,
    };
    let json = serde_json::to_value(&ir).unwrap();
    assert_eq!(json["type"], "tool_result");
    assert!(json["is_error"].as_bool().unwrap());
}

#[test]
fn content_type_thinking_through_ir() {
    let ir = IrContentBlock::Thinking {
        text: "Let me think...".into(),
    };
    let json = serde_json::to_value(&ir).unwrap();
    assert_eq!(json["type"], "thinking");
    assert_eq!(json["text"], "Let me think...");
}

#[test]
fn image_cross_dialect_claude_to_gemini() {
    let blocks = vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Base64 {
            media_type: "image/jpeg".into(),
            data: "abc123".into(),
        },
    }];
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let ir = claude_low::to_ir(&msgs, None);
    let gemini = gemini_low::from_ir(&ir);
    match &gemini[0].parts[0] {
        GeminiPart::InlineData(d) => {
            assert_eq!(d.mime_type, "image/jpeg");
            assert_eq!(d.data, "abc123");
        }
        other => panic!("expected InlineData, got {other:?}"),
    }
}

#[test]
fn thinking_block_claude_to_openai_becomes_text() {
    let blocks = vec![ClaudeContentBlock::Thinking {
        thinking: "reasoning...".into(),
        signature: None,
    }];
    let msgs = vec![ClaudeMessage {
        role: "assistant".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let ir = claude_low::to_ir(&msgs, None);
    let openai = openai_low::from_ir(&ir);
    // Thinking blocks are merged into text content
    assert_eq!(openai[0].content.as_deref(), Some("reasoning..."));
}

#[test]
fn thinking_block_claude_to_copilot_becomes_text() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::Thinking {
            text: "let me think".into(),
        }],
    )]);
    let copilot = copilot_low::from_ir(&conv);
    assert_eq!(copilot[0].content, "let me think");
}

// ═══════════════════════════════════════════════════════════════════════════
// §8  Token usage tracking through IR layer
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_usage_from_io() {
    let usage = IrUsage::from_io(100, 50);
    assert_eq!(usage.input_tokens, 100);
    assert_eq!(usage.output_tokens, 50);
    assert_eq!(usage.total_tokens, 150);
    assert_eq!(usage.cache_read_tokens, 0);
    assert_eq!(usage.cache_write_tokens, 0);
}

#[test]
fn ir_usage_with_cache() {
    let usage = IrUsage::with_cache(200, 80, 50, 30);
    assert_eq!(usage.total_tokens, 280);
    assert_eq!(usage.cache_read_tokens, 50);
    assert_eq!(usage.cache_write_tokens, 30);
}

#[test]
fn ir_usage_merge() {
    let a = IrUsage::from_io(100, 50);
    let b = IrUsage::with_cache(200, 80, 10, 5);
    let merged = a.merge(b);
    assert_eq!(merged.input_tokens, 300);
    assert_eq!(merged.output_tokens, 130);
    assert_eq!(merged.total_tokens, 430);
    assert_eq!(merged.cache_read_tokens, 10);
    assert_eq!(merged.cache_write_tokens, 5);
}

#[test]
fn ir_usage_default() {
    let usage = IrUsage::default();
    assert_eq!(usage.input_tokens, 0);
    assert_eq!(usage.output_tokens, 0);
    assert_eq!(usage.total_tokens, 0);
}

#[test]
fn codex_usage_to_ir() {
    let codex_usage = CodexUsage {
        input_tokens: 150,
        output_tokens: 75,
        total_tokens: 225,
    };
    let ir = codex_low::usage_to_ir(&codex_usage);
    assert_eq!(ir.input_tokens, 150);
    assert_eq!(ir.output_tokens, 75);
    assert_eq!(ir.total_tokens, 225);
}

#[test]
fn kimi_usage_to_ir() {
    let kimi_usage = KimiUsage {
        prompt_tokens: 300,
        completion_tokens: 120,
        total_tokens: 420,
    };
    let ir = kimi_low::usage_to_ir(&kimi_usage);
    assert_eq!(ir.input_tokens, 300);
    assert_eq!(ir.output_tokens, 120);
    assert_eq!(ir.total_tokens, 420);
}

// ═══════════════════════════════════════════════════════════════════════════
// §9  System message handling differences
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn system_handling_openai_inline() {
    // OpenAI keeps system as a regular message
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    let msgs = openai_low::from_ir(&conv);
    assert_eq!(msgs[0].role, "system");
    assert_eq!(msgs[0].content.as_deref(), Some("sys"));
}

#[test]
fn system_handling_claude_extracted() {
    // Claude extracts system to request level
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    let msgs = claude_low::from_ir(&conv);
    assert_eq!(msgs.len(), 1); // system stripped
    let sys = claude_low::extract_system_prompt(&conv);
    assert_eq!(sys.as_deref(), Some("sys"));
}

#[test]
fn system_handling_gemini_instruction() {
    // Gemini extracts system to system_instruction
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    let contents = gemini_low::from_ir(&conv);
    assert_eq!(contents.len(), 1); // system stripped
    let instruction = gemini_low::extract_system_instruction(&conv);
    assert!(instruction.is_some());
}

#[test]
fn system_handling_kimi_inline() {
    // Kimi keeps system as regular message (like OpenAI)
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    let msgs = kimi_low::from_ir(&conv);
    assert_eq!(msgs[0].role, "system");
}

#[test]
fn system_handling_copilot_inline() {
    // Copilot keeps system as regular message
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    let msgs = copilot_low::from_ir(&conv);
    assert_eq!(msgs[0].role, "system");
}

#[test]
fn system_handling_codex_skipped() {
    // Codex from_ir skips system/user (output items only)
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hi"),
    ]);
    let items = codex_low::from_ir(&conv);
    assert!(items.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// §10  Tool definition mapping through IR
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_tool_definition_construction() {
    let tool = IrToolDefinition {
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
    assert_eq!(tool.name, "read_file");
    assert_eq!(tool.description, "Read a file");
}

#[test]
fn ir_tool_definition_serde_roundtrip() {
    let tool = IrToolDefinition {
        name: "search".into(),
        description: "Search code".into(),
        parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

// ═══════════════════════════════════════════════════════════════════════════
// §11  Error cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mapping_error_feature_unsupported() {
    let err = MappingError::FeatureUnsupported {
        feature: "logprobs".into(),
        from: Dialect::Claude,
        to: Dialect::Gemini,
    };
    assert!(err.to_string().contains("logprobs"));
    assert!(err.to_string().contains("Claude"));
}

#[test]
fn mapping_error_fidelity_loss() {
    let err = MappingError::FidelityLoss {
        feature: "thinking".into(),
        warning: "no native support".into(),
    };
    assert!(err.to_string().contains("thinking"));
}

#[test]
fn mapping_error_dialect_mismatch() {
    let err = MappingError::DialectMismatch {
        from: Dialect::Codex,
        to: Dialect::Kimi,
    };
    assert!(err.to_string().contains("Codex"));
}

#[test]
fn mapping_error_invalid_input() {
    let err = MappingError::InvalidInput {
        reason: "empty messages".into(),
    };
    assert!(err.to_string().contains("empty messages"));
}

#[test]
fn copilot_tool_role_mapped_to_user() {
    // Copilot has no tool role — IR Tool becomes "user"
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
    let msgs = copilot_low::from_ir(&conv);
    assert_eq!(msgs[0].role, "user");
}

#[test]
fn openai_empty_content_produces_no_text_block() {
    let msgs = vec![OpenAIMessage {
        role: "user".into(),
        content: Some(String::new()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let conv = openai_low::to_ir(&msgs);
    assert!(conv.messages[0].content.is_empty());
}

#[test]
fn openai_malformed_tool_args_kept_as_string() {
    let msgs = vec![OpenAIMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![OpenAIToolCall {
            id: "c_bad".into(),
            call_type: "function".into(),
            function: OpenAIFunctionCall {
                name: "foo".into(),
                arguments: "invalid json".into(),
            },
        }]),
        tool_call_id: None,
    }];
    let conv = openai_low::to_ir(&msgs);
    match &conv.messages[0].content[0] {
        IrContentBlock::ToolUse { input, .. } => {
            assert_eq!(input, &serde_json::Value::String("invalid json".into()));
        }
        other => panic!("expected ToolUse, got {other:?}"),
    }
}

#[test]
fn claude_url_image_degrades_to_text() {
    let blocks = vec![ClaudeContentBlock::Image {
        source: ClaudeImageSource::Url {
            url: "https://example.com/img.png".into(),
        },
    }];
    let msgs = vec![ClaudeMessage {
        role: "user".into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }];
    let conv = claude_low::to_ir(&msgs, None);
    // URL images become text placeholders
    match &conv.messages[0].content[0] {
        IrContentBlock::Text { text } => assert!(text.contains("example.com")),
        other => panic!("expected Text placeholder, got {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §12  Serde roundtrip for all IR types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn serde_roundtrip_ir_role() {
    for role in &[
        IrRole::System,
        IrRole::User,
        IrRole::Assistant,
        IrRole::Tool,
    ] {
        let json = serde_json::to_string(role).unwrap();
        let back: IrRole = serde_json::from_str(&json).unwrap();
        assert_eq!(*role, back);
    }
}

#[test]
fn serde_roundtrip_ir_content_block_text() {
    let block = IrContentBlock::Text {
        text: "hello world".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_roundtrip_ir_content_block_image() {
    let block = IrContentBlock::Image {
        media_type: "image/png".into(),
        data: "base64data".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_roundtrip_ir_content_block_tool_use() {
    let block = IrContentBlock::ToolUse {
        id: "tu_1".into(),
        name: "read".into(),
        input: json!({"path": "a.rs"}),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_roundtrip_ir_content_block_tool_result() {
    let block = IrContentBlock::ToolResult {
        tool_use_id: "tu_1".into(),
        content: vec![IrContentBlock::Text {
            text: "content".into(),
        }],
        is_error: false,
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_roundtrip_ir_content_block_thinking() {
    let block = IrContentBlock::Thinking {
        text: "deep thoughts".into(),
    };
    let json = serde_json::to_string(&block).unwrap();
    let back: IrContentBlock = serde_json::from_str(&json).unwrap();
    assert_eq!(block, back);
}

#[test]
fn serde_roundtrip_ir_message_with_metadata() {
    let mut metadata = BTreeMap::new();
    metadata.insert("key".into(), json!("value"));
    metadata.insert("num".into(), json!(42));
    let msg = IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text {
            text: "hello".into(),
        }],
        metadata,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: IrMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn serde_roundtrip_ir_message_empty_metadata_omitted() {
    let msg = IrMessage::text(IrRole::User, "hi");
    let json = serde_json::to_string(&msg).unwrap();
    assert!(!json.contains("metadata"));
    let back: IrMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg, back);
}

#[test]
fn serde_roundtrip_ir_conversation() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "sys"),
        IrMessage::text(IrRole::User, "hello"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text { text: "hi".into() },
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "read".into(),
                    input: json!({}),
                },
            ],
        ),
    ]);
    let json = serde_json::to_string(&conv).unwrap();
    let back: IrConversation = serde_json::from_str(&json).unwrap();
    assert_eq!(conv, back);
}

#[test]
fn serde_roundtrip_ir_usage() {
    let usage = IrUsage::with_cache(100, 50, 20, 10);
    let json = serde_json::to_string(&usage).unwrap();
    let back: IrUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(usage, back);
}

#[test]
fn serde_roundtrip_ir_tool_definition() {
    let tool = IrToolDefinition {
        name: "write".into(),
        description: "Write file".into(),
        parameters: json!({"type": "object"}),
    };
    let json = serde_json::to_string(&tool).unwrap();
    let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(tool, back);
}

#[test]
fn serde_roundtrip_mapping_error() {
    let err = MappingError::FeatureUnsupported {
        feature: "images".into(),
        from: Dialect::Kimi,
        to: Dialect::Copilot,
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MappingError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn serde_roundtrip_fidelity() {
    for f in [
        Fidelity::Lossless,
        Fidelity::LossyLabeled {
            warning: "partial".into(),
        },
        Fidelity::Unsupported {
            reason: "N/A".into(),
        },
    ] {
        let json = serde_json::to_string(&f).unwrap();
        let back: Fidelity = serde_json::from_str(&json).unwrap();
        assert_eq!(f, back);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §13  BTreeMap / deterministic ordering
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_message_metadata_deterministic_order() {
    let mut metadata = BTreeMap::new();
    metadata.insert("z_key".into(), json!("last"));
    metadata.insert("a_key".into(), json!("first"));
    metadata.insert("m_key".into(), json!("middle"));
    let msg = IrMessage {
        role: IrRole::User,
        content: vec![],
        metadata,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let a_pos = json.find("a_key").unwrap();
    let m_pos = json.find("m_key").unwrap();
    let z_pos = json.find("z_key").unwrap();
    assert!(a_pos < m_pos);
    assert!(m_pos < z_pos);
}

#[test]
fn ir_message_metadata_deterministic_across_serializations() {
    let mut metadata = BTreeMap::new();
    metadata.insert("b".into(), json!(2));
    metadata.insert("a".into(), json!(1));
    metadata.insert("c".into(), json!(3));
    let msg = IrMessage {
        role: IrRole::User,
        content: vec![IrContentBlock::Text { text: "hi".into() }],
        metadata,
    };
    let json1 = serde_json::to_string(&msg).unwrap();
    let json2 = serde_json::to_string(&msg).unwrap();
    assert_eq!(json1, json2);
}

#[test]
fn copilot_reference_metadata_btreemap_ordering() {
    let mut meta = BTreeMap::new();
    meta.insert("z".into(), json!("z"));
    meta.insert("a".into(), json!("a"));
    let r = CopilotReference {
        ref_type: CopilotReferenceType::File,
        id: "f1".into(),
        data: json!({}),
        metadata: Some(meta),
    };
    let json = serde_json::to_string(&r).unwrap();
    let a_pos = json.find("\"a\"").unwrap();
    let z_pos = json.find("\"z\"").unwrap();
    assert!(a_pos < z_pos);
}

// ═══════════════════════════════════════════════════════════════════════════
// §bonus  Additional edge-case and normalization tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ir_conversation_from_messages_preserves_order() {
    let msgs = vec![
        IrMessage::text(IrRole::User, "first"),
        IrMessage::text(IrRole::Assistant, "second"),
        IrMessage::text(IrRole::User, "third"),
    ];
    let conv = IrConversation::from_messages(msgs);
    assert_eq!(conv.messages[0].text_content(), "first");
    assert_eq!(conv.messages[1].text_content(), "second");
    assert_eq!(conv.messages[2].text_content(), "third");
}

#[test]
fn multiple_text_blocks_concatenated() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Hello ".into(),
            },
            IrContentBlock::Text {
                text: "world".into(),
            },
        ],
    );
    assert_eq!(msg.text_content(), "Hello world");
    assert!(msg.is_text_only());
}

#[test]
fn tool_use_blocks_filter_correctly() {
    let msg = IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "let me".into(),
            },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "read".into(),
                input: json!({}),
            },
            IrContentBlock::Thinking { text: "hmm".into() },
            IrContentBlock::ToolUse {
                id: "t2".into(),
                name: "write".into(),
                input: json!({}),
            },
        ],
    );
    let tools = msg.tool_use_blocks();
    assert_eq!(tools.len(), 2);
}

#[test]
fn codex_reasoning_to_ir_thinking() {
    let items = vec![CodexResponseItem::Reasoning {
        summary: vec![
            ReasoningSummary {
                text: "Step 1: analyze".into(),
            },
            ReasoningSummary {
                text: "Step 2: implement".into(),
            },
        ],
    }];
    let conv = codex_low::to_ir(&items);
    match &conv.messages[0].content[0] {
        IrContentBlock::Thinking { text } => {
            assert!(text.contains("Step 1"));
            assert!(text.contains("Step 2"));
        }
        other => panic!("expected Thinking, got {other:?}"),
    }
}

#[test]
fn gemini_inline_data_cross_to_claude_image() {
    let gemini = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::InlineData(GeminiInlineData {
            mime_type: "image/png".into(),
            data: "encoded".into(),
        })],
    }];
    let ir = gemini_low::to_ir(&gemini, None);
    let claude = claude_low::from_ir(&ir);
    // Should produce structured content with image block
    let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&claude[0].content).unwrap();
    match &parsed[0] {
        ClaudeContentBlock::Image { source } => match source {
            ClaudeImageSource::Base64 { media_type, data } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "encoded");
            }
            other => panic!("expected Base64, got {other:?}"),
        },
        other => panic!("expected Image, got {other:?}"),
    }
}

#[test]
fn gemini_function_response_through_ir_to_openai() {
    let gemini = vec![GeminiContent {
        role: "user".into(),
        parts: vec![GeminiPart::FunctionResponse {
            name: "search".into(),
            response: json!("results here"),
        }],
    }];
    let ir = gemini_low::to_ir(&gemini, None);
    // IR should have a ToolResult
    match &ir.messages[0].content[0] {
        IrContentBlock::ToolResult { tool_use_id, .. } => {
            assert_eq!(tool_use_id, "gemini_search");
        }
        other => panic!("expected ToolResult, got {other:?}"),
    }
}

#[test]
fn dialect_all_returns_six_dialects() {
    let all = Dialect::all();
    assert_eq!(all.len(), 6);
    assert!(all.contains(&Dialect::OpenAi));
    assert!(all.contains(&Dialect::Claude));
    assert!(all.contains(&Dialect::Gemini));
    assert!(all.contains(&Dialect::Codex));
    assert!(all.contains(&Dialect::Kimi));
    assert!(all.contains(&Dialect::Copilot));
}

#[test]
fn dialect_label_display() {
    assert_eq!(Dialect::OpenAi.label(), "OpenAI");
    assert_eq!(Dialect::Claude.label(), "Claude");
    assert_eq!(Dialect::Gemini.label(), "Gemini");
    assert_eq!(Dialect::Codex.label(), "Codex");
    assert_eq!(Dialect::Kimi.label(), "Kimi");
    assert_eq!(Dialect::Copilot.label(), "Copilot");
    assert_eq!(format!("{}", Dialect::OpenAi), "OpenAI");
}

#[test]
fn dialect_serde_roundtrip() {
    for d in Dialect::all() {
        let json = serde_json::to_string(d).unwrap();
        let back: Dialect = serde_json::from_str(&json).unwrap();
        assert_eq!(*d, back);
    }
}

#[test]
fn mapping_registry_empty() {
    let reg = MappingRegistry::new();
    assert!(reg.is_empty());
    assert_eq!(reg.len(), 0);
    assert!(
        reg.lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
            .is_none()
    );
}

#[test]
fn mapping_registry_replace_rule() {
    let mut reg = MappingRegistry::new();
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "streaming".into(),
        fidelity: Fidelity::Lossless,
    });
    assert_eq!(reg.len(), 1);

    // Replace with updated fidelity
    reg.insert(MappingRule {
        source_dialect: Dialect::OpenAi,
        target_dialect: Dialect::Claude,
        feature: "streaming".into(),
        fidelity: Fidelity::LossyLabeled {
            warning: "partial".into(),
        },
    });
    assert_eq!(reg.len(), 1);
    let rule = reg
        .lookup(Dialect::OpenAi, Dialect::Claude, "streaming")
        .unwrap();
    assert!(!rule.fidelity.is_lossless());
}
