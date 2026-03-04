#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Focused lowering edge-case tests for Codex, Kimi, and Copilot SDK shims.
//!
//! These tests exercise the `to_ir` / `from_ir` lowering paths for each SDK,
//! with particular attention to **documented lossy behaviour** (e.g. Codex
//! `from_ir()` drops user/system messages because Codex response items only
//! represent model output).

use std::collections::BTreeMap;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// §1  Codex lowering
// ═══════════════════════════════════════════════════════════════════════════

mod codex {
    use super::*;
    use abp_codex_sdk::dialect::{
        CodexContentPart, CodexInputItem, CodexResponseItem, CodexUsage, ReasoningSummary,
    };
    use abp_codex_sdk::lowering;

    // ── input_to_ir ─────────────────────────────────────────────────────

    #[test]
    fn input_to_ir_preserves_user_message() {
        let items = vec![CodexInputItem::Message {
            role: "user".into(),
            content: "Explain closures".into(),
        }];
        let conv = lowering::input_to_ir(&items);
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Explain closures");
    }

    #[test]
    fn input_to_ir_preserves_system_message() {
        let items = vec![CodexInputItem::Message {
            role: "system".into(),
            content: "You are a Rust expert.".into(),
        }];
        let conv = lowering::input_to_ir(&items);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "You are a Rust expert.");
    }

    // ── to_ir ───────────────────────────────────────────────────────────

    #[test]
    fn to_ir_converts_message_item() {
        let items = vec![CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "Hello!".into(),
            }],
        }];
        let conv = lowering::to_ir(&items);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        assert_eq!(conv.messages[0].text_content(), "Hello!");
    }

    #[test]
    fn to_ir_converts_function_call_item() {
        let items = vec![CodexResponseItem::FunctionCall {
            id: "fc_1".into(),
            call_id: None,
            name: "read_file".into(),
            arguments: r#"{"path":"lib.rs"}"#.into(),
        }];
        let conv = lowering::to_ir(&items);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "fc_1");
                assert_eq!(name, "read_file");
                assert_eq!(input, &json!({"path": "lib.rs"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn to_ir_converts_function_call_output_item() {
        let items = vec![CodexResponseItem::FunctionCallOutput {
            call_id: "fc_1".into(),
            output: "fn main() {}".into(),
        }];
        let conv = lowering::to_ir(&items);
        assert_eq!(conv.messages[0].role, IrRole::Tool);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "fc_1");
                assert!(!is_error);
                assert_eq!(content.len(), 1);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn to_ir_converts_reasoning_item() {
        let items = vec![CodexResponseItem::Reasoning {
            summary: vec![
                ReasoningSummary {
                    text: "First I need to…".into(),
                },
                ReasoningSummary {
                    text: "Then I will…".into(),
                },
            ],
        }];
        let conv = lowering::to_ir(&items);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        match &conv.messages[0].content[0] {
            IrContentBlock::Thinking { text } => {
                assert!(text.contains("First I need to…"));
                assert!(text.contains("Then I will…"));
            }
            other => panic!("expected Thinking, got {other:?}"),
        }
    }

    // ── from_ir: lossy behaviour ────────────────────────────────────────

    #[test]
    fn from_ir_drops_user_messages_documented_lossy() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "What is Rust?"),
            IrMessage::text(IrRole::Assistant, "Rust is a systems language."),
        ]);
        let items = lowering::from_ir(&conv);
        // Only the assistant message survives
        assert_eq!(items.len(), 1);
        match &items[0] {
            CodexResponseItem::Message { role, .. } => assert_eq!(role, "assistant"),
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn from_ir_drops_system_messages_documented_lossy() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "You are helpful."),
            IrMessage::text(IrRole::Assistant, "Got it."),
        ]);
        let items = lowering::from_ir(&conv);
        assert_eq!(items.len(), 1);
        match &items[0] {
            CodexResponseItem::Message { role, .. } => assert_eq!(role, "assistant"),
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn from_ir_converts_assistant_text_to_message() {
        let conv =
            IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "Result text")]);
        let items = lowering::from_ir(&conv);
        assert_eq!(items.len(), 1);
        match &items[0] {
            CodexResponseItem::Message { role, content } => {
                assert_eq!(role, "assistant");
                match &content[0] {
                    CodexContentPart::OutputText { text } => assert_eq!(text, "Result text"),
                }
            }
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn from_ir_converts_tool_use_to_function_call() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t_1".into(),
                name: "grep".into(),
                input: json!({"pattern": "TODO"}),
            }],
        )]);
        let items = lowering::from_ir(&conv);
        assert_eq!(items.len(), 1);
        match &items[0] {
            CodexResponseItem::FunctionCall {
                id,
                name,
                arguments,
                ..
            } => {
                assert_eq!(id, "t_1");
                assert_eq!(name, "grep");
                assert!(arguments.contains("TODO"));
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn from_ir_converts_tool_result_to_function_call_output() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "match found".into(),
                }],
                is_error: false,
            }],
        )]);
        let items = lowering::from_ir(&conv);
        assert_eq!(items.len(), 1);
        match &items[0] {
            CodexResponseItem::FunctionCallOutput { call_id, output } => {
                assert_eq!(call_id, "t_1");
                assert_eq!(output, "match found");
            }
            other => panic!("expected FunctionCallOutput, got {other:?}"),
        }
    }

    // ── Roundtrips ──────────────────────────────────────────────────────

    #[test]
    fn roundtrip_assistant_message_is_lossless() {
        let original = vec![CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "Perfectly preserved.".into(),
            }],
        }];
        let conv = lowering::to_ir(&original);
        let back = lowering::from_ir(&conv);

        assert_eq!(back.len(), 1);
        match &back[0] {
            CodexResponseItem::Message { role, content } => {
                assert_eq!(role, "assistant");
                match &content[0] {
                    CodexContentPart::OutputText { text } => {
                        assert_eq!(text, "Perfectly preserved.")
                    }
                }
            }
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn roundtrip_user_message_through_codex_is_lossy() {
        // Construct IR with a user message then pass through from_ir
        let conv = IrConversation::from_messages(vec![IrMessage::text(
            IrRole::User,
            "This will be dropped",
        )]);
        let items = lowering::from_ir(&conv);
        assert!(
            items.is_empty(),
            "Codex from_ir must drop user messages (output-only representation)"
        );
    }

    #[test]
    fn empty_conversation_handling() {
        let conv = lowering::to_ir(&[]);
        assert!(conv.is_empty());
        let back = lowering::from_ir(&conv);
        assert!(back.is_empty());
    }

    #[test]
    fn multi_turn_mixed_roles() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be helpful."),
            IrMessage::text(IrRole::User, "Hello"),
            IrMessage::text(IrRole::Assistant, "Hi there!"),
            IrMessage::text(IrRole::User, "What is 2+2?"),
            IrMessage::text(IrRole::Assistant, "4"),
            IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "c1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "result".into(),
                    }],
                    is_error: false,
                }],
            ),
        ]);
        let items = lowering::from_ir(&conv);
        // System + 2 user messages dropped; 2 assistant + 1 tool remain
        assert_eq!(items.len(), 3);
        assert!(
            matches!(&items[0], CodexResponseItem::Message { role, .. } if role == "assistant")
        );
        assert!(
            matches!(&items[1], CodexResponseItem::Message { role, .. } if role == "assistant")
        );
        assert!(matches!(
            &items[2],
            CodexResponseItem::FunctionCallOutput { .. }
        ));
    }

    #[test]
    fn usage_to_ir_maps_fields() {
        let usage = CodexUsage {
            input_tokens: 150,
            output_tokens: 75,
            total_tokens: 225,
        };
        let ir = lowering::usage_to_ir(&usage);
        assert_eq!(ir.input_tokens, 150);
        assert_eq!(ir.output_tokens, 75);
        assert_eq!(ir.total_tokens, 225);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §2  Kimi lowering
// ═══════════════════════════════════════════════════════════════════════════

mod kimi {
    use super::*;
    use abp_kimi_sdk::dialect::{KimiFunctionCall, KimiMessage, KimiToolCall, KimiUsage};
    use abp_kimi_sdk::lowering;

    fn text_msg(role: &str, content: &str) -> KimiMessage {
        KimiMessage {
            role: role.into(),
            content: Some(content.into()),
            tool_call_id: None,
            tool_calls: None,
        }
    }

    // ── Basic text roundtrips ───────────────────────────────────────────

    #[test]
    fn basic_user_text_roundtrip() {
        let msgs = vec![text_msg("user", "Hi there")];
        let conv = lowering::to_ir(&msgs);
        let back = lowering::from_ir(&conv);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, "user");
        assert_eq!(back[0].content.as_deref(), Some("Hi there"));
    }

    #[test]
    fn basic_assistant_text_roundtrip() {
        let msgs = vec![text_msg("assistant", "Hello!")];
        let conv = lowering::to_ir(&msgs);
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].role, "assistant");
        assert_eq!(back[0].content.as_deref(), Some("Hello!"));
    }

    // ── System prompt handling ──────────────────────────────────────────

    #[test]
    fn system_prompt_preserved() {
        let msgs = vec![text_msg("system", "You are a translation engine.")];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::System);
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].role, "system");
        assert_eq!(
            back[0].content.as_deref(),
            Some("You are a translation engine.")
        );
    }

    // ── Tool calls ──────────────────────────────────────────────────────

    #[test]
    fn web_search_tool_call_roundtrip() {
        let msgs = vec![KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: Some(vec![KimiToolCall {
                id: "call_ws_1".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "web_search".into(),
                    arguments: r#"{"query":"latest Rust release"}"#.into(),
                },
            }]),
        }];
        let conv = lowering::to_ir(&msgs);
        let back = lowering::from_ir(&conv);
        let tc = &back[0].tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.function.name, "web_search");
        assert!(tc.function.arguments.contains("latest Rust release"));
    }

    #[test]
    fn search_plus_tool_call() {
        let msgs = vec![KimiMessage {
            role: "assistant".into(),
            content: Some("Searching…".into()),
            tool_call_id: None,
            tool_calls: Some(vec![KimiToolCall {
                id: "call_sp".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "search_plus".into(),
                    arguments: r#"{"q":"agent backplane"}"#.into(),
                },
            }]),
        }];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].content.len(), 2); // text + tool_use
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].content.as_deref(), Some("Searching…"));
        assert!(back[0].tool_calls.is_some());
    }

    // ── All roles preserved ─────────────────────────────────────────────

    #[test]
    fn all_roles_preserved_through_roundtrip() {
        let msgs = vec![
            text_msg("system", "sys"),
            text_msg("user", "usr"),
            text_msg("assistant", "ast"),
            KimiMessage {
                role: "tool".into(),
                content: Some("result".into()),
                tool_call_id: Some("c1".into()),
                tool_calls: None,
            },
        ];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[1].role, IrRole::User);
        assert_eq!(conv.messages[2].role, IrRole::Assistant);
        assert_eq!(conv.messages[3].role, IrRole::Tool);

        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].role, "system");
        assert_eq!(back[1].role, "user");
        assert_eq!(back[2].role, "assistant");
        assert_eq!(back[3].role, "tool");
    }

    // ── Empty messages ──────────────────────────────────────────────────

    #[test]
    fn empty_conversation() {
        let conv = lowering::to_ir(&[]);
        assert!(conv.is_empty());
        let back = lowering::from_ir(&conv);
        assert!(back.is_empty());
    }

    #[test]
    fn none_content_message() {
        let msgs = vec![KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: None,
        }];
        let conv = lowering::to_ir(&msgs);
        assert!(conv.messages[0].content.is_empty());
        let back = lowering::from_ir(&conv);
        assert!(back[0].content.is_none());
    }

    // ── Multi-turn conversation ─────────────────────────────────────────

    #[test]
    fn multi_turn_tool_flow() {
        let msgs = vec![
            text_msg("user", "Search for Rust async"),
            KimiMessage {
                role: "assistant".into(),
                content: None,
                tool_call_id: None,
                tool_calls: Some(vec![KimiToolCall {
                    id: "c1".into(),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: "web_search".into(),
                        arguments: r#"{"q":"rust async"}"#.into(),
                    },
                }]),
            },
            KimiMessage {
                role: "tool".into(),
                content: Some("async/await is stabilized".into()),
                tool_call_id: Some("c1".into()),
                tool_calls: None,
            },
            text_msg("assistant", "Async/await has been stable since Rust 1.39."),
        ];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.len(), 4);
        let back = lowering::from_ir(&conv);
        assert_eq!(back.len(), 4);
        assert_eq!(back[2].tool_call_id.as_deref(), Some("c1"));
        assert_eq!(
            back[3].content.as_deref(),
            Some("Async/await has been stable since Rust 1.39.")
        );
    }

    // ── Unicode content ─────────────────────────────────────────────────

    #[test]
    fn unicode_content_roundtrip() {
        let text = "こんにちは世界 🌍 — Ñoño «中文»";
        let msgs = vec![text_msg("user", text)];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].text_content(), text);
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].content.as_deref(), Some(text));
    }

    // ── Thinking block merged into text ─────────────────────────────────

    #[test]
    fn thinking_block_merged_into_text() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "Let me think about this…".into(),
            }],
        )]);
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].role, "assistant");
        assert_eq!(back[0].content.as_deref(), Some("Let me think about this…"));
    }

    #[test]
    fn usage_to_ir_maps_fields() {
        let usage = KimiUsage {
            prompt_tokens: 300,
            completion_tokens: 120,
            total_tokens: 420,
        };
        let ir = lowering::usage_to_ir(&usage);
        assert_eq!(ir.input_tokens, 300);
        assert_eq!(ir.output_tokens, 120);
        assert_eq!(ir.total_tokens, 420);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  Copilot lowering
// ═══════════════════════════════════════════════════════════════════════════

mod copilot {
    use super::*;
    use abp_copilot_sdk::dialect::{CopilotMessage, CopilotReference, CopilotReferenceType};
    use abp_copilot_sdk::lowering;

    fn simple_msg(role: &str, content: &str) -> CopilotMessage {
        CopilotMessage {
            role: role.into(),
            content: content.into(),
            name: None,
            copilot_references: vec![],
        }
    }

    // ── Basic text roundtrips ───────────────────────────────────────────

    #[test]
    fn basic_user_text_roundtrip() {
        let msgs = vec![simple_msg("user", "Help me")];
        let conv = lowering::to_ir(&msgs);
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].role, "user");
        assert_eq!(back[0].content, "Help me");
    }

    #[test]
    fn basic_assistant_text_roundtrip() {
        let msgs = vec![simple_msg("assistant", "Sure, I can help!")];
        let conv = lowering::to_ir(&msgs);
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].role, "assistant");
        assert_eq!(back[0].content, "Sure, I can help!");
    }

    // ── References preservation ─────────────────────────────────────────

    #[test]
    fn file_reference_preserved() {
        let refs = vec![CopilotReference {
            ref_type: CopilotReferenceType::File,
            id: "file-1".into(),
            data: json!({"path": "src/lib.rs", "language": "rust"}),
            metadata: None,
        }];
        let msgs = vec![CopilotMessage {
            role: "user".into(),
            content: "Check this file".into(),
            name: None,
            copilot_references: refs.clone(),
        }];
        let conv = lowering::to_ir(&msgs);
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].copilot_references.len(), 1);
        assert_eq!(back[0].copilot_references[0].id, "file-1");
        assert_eq!(
            back[0].copilot_references[0].ref_type,
            CopilotReferenceType::File
        );
    }

    #[test]
    fn repository_reference_preserved() {
        let refs = vec![CopilotReference {
            ref_type: CopilotReferenceType::Repository,
            id: "repo-0".into(),
            data: json!({"owner": "rust-lang", "name": "rust"}),
            metadata: None,
        }];
        let msgs = vec![CopilotMessage {
            role: "user".into(),
            content: "Look at this repo".into(),
            name: None,
            copilot_references: refs,
        }];
        let conv = lowering::to_ir(&msgs);
        let back = lowering::from_ir(&conv);
        assert_eq!(
            back[0].copilot_references[0].ref_type,
            CopilotReferenceType::Repository
        );
    }

    // ── extract_references across messages ──────────────────────────────

    #[test]
    fn extract_references_collects_from_all_messages() {
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
        let conv = lowering::to_ir(&msgs);
        let all_refs = lowering::extract_references(&conv);
        assert_eq!(all_refs.len(), 2);
    }

    // ── Display name (agent mode) ───────────────────────────────────────

    #[test]
    fn agent_name_preserved_through_roundtrip() {
        let msgs = vec![CopilotMessage {
            role: "assistant".into(),
            content: "Analyzing…".into(),
            name: Some("copilot-agent".into()),
            copilot_references: vec![],
        }];
        let conv = lowering::to_ir(&msgs);
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].name.as_deref(), Some("copilot-agent"));
    }

    // ── All roles preserved ─────────────────────────────────────────────

    #[test]
    fn all_roles_preserved() {
        let msgs = vec![
            simple_msg("system", "You are Copilot."),
            simple_msg("user", "Hello"),
            simple_msg("assistant", "Hi!"),
        ];
        let conv = lowering::to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[1].role, IrRole::User);
        assert_eq!(conv.messages[2].role, IrRole::Assistant);

        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].role, "system");
        assert_eq!(back[1].role, "user");
        assert_eq!(back[2].role, "assistant");
    }

    // ── Tool role mapped to user ────────────────────────────────────────

    #[test]
    fn tool_role_mapped_to_user() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![IrContentBlock::Text {
                    text: "tool output".into(),
                }],
                is_error: false,
            }],
        )]);
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].role, "user");
    }

    // ── Empty messages ──────────────────────────────────────────────────

    #[test]
    fn empty_conversation() {
        let conv = lowering::to_ir(&[]);
        assert!(conv.is_empty());
        let back = lowering::from_ir(&conv);
        assert!(back.is_empty());
    }

    #[test]
    fn empty_content_string() {
        let msgs = vec![simple_msg("user", "")];
        let conv = lowering::to_ir(&msgs);
        assert!(conv.messages[0].content.is_empty());
        let back = lowering::from_ir(&conv);
        assert!(back[0].content.is_empty());
    }

    // ── Tool use blocks dropped ─────────────────────────────────────────

    #[test]
    fn tool_use_blocks_dropped_in_from_ir() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Calling tool…".into(),
                },
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "read".into(),
                    input: json!({}),
                },
            ],
        )]);
        let back = lowering::from_ir(&conv);
        // ToolUse blocks are silently dropped; only text survives
        assert_eq!(back[0].content, "Calling tool…");
    }

    // ── Thinking block merged ───────────────────────────────────────────

    #[test]
    fn thinking_block_becomes_text() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "reasoning here".into(),
            }],
        )]);
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].role, "assistant");
        assert_eq!(back[0].content, "reasoning here");
    }

    // ── Confirmation-like metadata ──────────────────────────────────────

    #[test]
    fn snippet_reference_with_metadata_roundtrip() {
        let refs = vec![CopilotReference {
            ref_type: CopilotReferenceType::Snippet,
            id: "snippet-42".into(),
            data: json!({"name": "helper.rs", "content": "fn helper() {}"}),
            metadata: Some({
                let mut m = BTreeMap::new();
                m.insert("label".into(), json!("Code snippet"));
                m
            }),
        }];
        let msgs = vec![CopilotMessage {
            role: "user".into(),
            content: "Review this snippet".into(),
            name: None,
            copilot_references: refs,
        }];
        let conv = lowering::to_ir(&msgs);
        let back = lowering::from_ir(&conv);
        assert_eq!(back[0].copilot_references[0].id, "snippet-42");
        assert!(back[0].copilot_references[0].metadata.is_some());
    }
}
