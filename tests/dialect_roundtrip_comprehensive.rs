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
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive round-trip dialect tests verifying every SDK's dialect can be:
//!   1. Parsed from native format → IR (intermediate representation)
//!   2. Lowered from IR → native format
//!   3. Re-parsed to verify fidelity (no data loss)
//!
//! Covers: simple text, multi-turn, tool use, images, thinking blocks, usage,
//! and edge cases for all 6 SDKs (Claude, OpenAI, Gemini, Kimi, Codex, Copilot).

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use serde_json::json;

// Lowering modules.
use abp_claude_sdk::lowering as claude_ir;
use abp_codex_sdk::lowering as codex_ir;
use abp_copilot_sdk::lowering as copilot_ir;
use abp_gemini_sdk::lowering as gemini_ir;
use abp_kimi_sdk::lowering as kimi_ir;
use abp_openai_sdk::lowering as openai_ir;

// Dialect types.
use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeImageSource, ClaudeMessage};
use abp_codex_sdk::dialect::{
    CodexContentPart, CodexInputItem, CodexResponseItem, CodexUsage, ReasoningSummary,
};
use abp_copilot_sdk::dialect::CopilotMessage;
use abp_gemini_sdk::dialect::{GeminiContent, GeminiInlineData, GeminiPart};
use abp_kimi_sdk::dialect::{KimiFunctionCall, KimiMessage, KimiToolCall, KimiUsage};
use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn openai_msg(role: &str, content: &str) -> OpenAIMessage {
    OpenAIMessage {
        role: role.into(),
        content: Some(content.into()),
        tool_calls: None,
        tool_call_id: None,
    }
}

fn kimi_msg(role: &str, content: &str) -> KimiMessage {
    KimiMessage {
        role: role.into(),
        content: Some(content.into()),
        tool_call_id: None,
        tool_calls: None,
    }
}

fn copilot_msg(role: &str, content: &str) -> CopilotMessage {
    CopilotMessage {
        role: role.into(),
        content: content.into(),
        name: None,
        copilot_references: vec![],
    }
}

fn claude_msg(role: &str, content: &str) -> ClaudeMessage {
    ClaudeMessage {
        role: role.into(),
        content: content.into(),
    }
}

fn gemini_text(role: &str, text: &str) -> GeminiContent {
    GeminiContent {
        role: role.into(),
        parts: vec![GeminiPart::Text(text.into())],
    }
}

/// Verify an IR conversation has exactly the expected roles in order.
fn assert_roles(conv: &IrConversation, expected: &[IrRole]) {
    let actual: Vec<IrRole> = conv.messages.iter().map(|m| m.role).collect();
    assert_eq!(actual, expected, "role sequence mismatch");
}

// ═══════════════════════════════════════════════════════════════════════════
// §1  OpenAI round-trip tests
// ═══════════════════════════════════════════════════════════════════════════

mod openai_roundtrip {
    use super::*;

    #[test]
    fn simple_text_system_user_assistant() {
        let msgs = vec![
            openai_msg("system", "You are helpful."),
            openai_msg("user", "Hello"),
            openai_msg("assistant", "Hi there!"),
        ];

        // Step 1: native → IR
        let ir = openai_ir::to_ir(&msgs);
        assert_eq!(ir.len(), 3);
        assert_roles(&ir, &[IrRole::System, IrRole::User, IrRole::Assistant]);
        assert_eq!(ir.messages[0].text_content(), "You are helpful.");
        assert_eq!(ir.messages[1].text_content(), "Hello");
        assert_eq!(ir.messages[2].text_content(), "Hi there!");

        // Step 2: IR → native
        let native = openai_ir::from_ir(&ir);
        assert_eq!(native.len(), 3);
        assert_eq!(native[0].role, "system");
        assert_eq!(native[0].content.as_deref(), Some("You are helpful."));

        // Step 3: re-parse → IR and verify fidelity
        let ir2 = openai_ir::to_ir(&native);
        assert_eq!(ir, ir2, "round-trip fidelity lost for OpenAI text messages");
    }

    #[test]
    fn multi_turn_conversation() {
        let msgs = vec![
            openai_msg("system", "Be concise."),
            openai_msg("user", "What is 2+2?"),
            openai_msg("assistant", "4"),
            openai_msg("user", "And 3+3?"),
            openai_msg("assistant", "6"),
        ];
        let ir = openai_ir::to_ir(&msgs);
        let native = openai_ir::from_ir(&ir);
        let ir2 = openai_ir::to_ir(&native);
        assert_eq!(ir, ir2);
        assert_eq!(ir.len(), 5);
    }

    #[test]
    fn tool_call_and_result() {
        let msgs = vec![
            openai_msg("user", "Read main.rs"),
            OpenAIMessage {
                role: "assistant".into(),
                content: Some("Let me read that.".into()),
                tool_calls: Some(vec![OpenAIToolCall {
                    id: "call_1".into(),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: "read_file".into(),
                        arguments: r#"{"path":"main.rs"}"#.into(),
                    },
                }]),
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "tool".into(),
                content: Some("fn main() {}".into()),
                tool_calls: None,
                tool_call_id: Some("call_1".into()),
            },
            openai_msg("assistant", "It contains a main function."),
        ];

        let ir = openai_ir::to_ir(&msgs);
        assert_eq!(ir.len(), 4);
        assert_eq!(ir.messages[1].role, IrRole::Assistant);

        // Verify tool use block
        let tool_use = ir.tool_calls();
        assert_eq!(tool_use.len(), 1);
        match tool_use[0] {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "read_file");
                assert_eq!(input, &json!({"path": "main.rs"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }

        // Verify tool result block
        match &ir.messages[2].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "call_1");
                assert!(!is_error);
                assert_eq!(content.len(), 1);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }

        // Round-trip fidelity
        let native = openai_ir::from_ir(&ir);
        let ir2 = openai_ir::to_ir(&native);
        assert_eq!(ir, ir2, "round-trip fidelity lost for OpenAI tool calls");
    }

    #[test]
    fn multiple_tool_calls_in_single_message() {
        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![
                OpenAIToolCall {
                    id: "c1".into(),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: "read".into(),
                        arguments: r#"{"path":"a.rs"}"#.into(),
                    },
                },
                OpenAIToolCall {
                    id: "c2".into(),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: "read".into(),
                        arguments: r#"{"path":"b.rs"}"#.into(),
                    },
                },
            ]),
            tool_call_id: None,
        }];

        let ir = openai_ir::to_ir(&msgs);
        assert_eq!(ir.messages[0].content.len(), 2);

        let native = openai_ir::from_ir(&ir);
        assert_eq!(native[0].tool_calls.as_ref().unwrap().len(), 2);

        let ir2 = openai_ir::to_ir(&native);
        assert_eq!(ir, ir2);
    }

    #[test]
    fn empty_conversation() {
        let ir = openai_ir::to_ir(&[]);
        assert!(ir.is_empty());
        let native = openai_ir::from_ir(&ir);
        assert!(native.is_empty());
        let ir2 = openai_ir::to_ir(&native);
        assert_eq!(ir, ir2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §2  Claude round-trip tests
// ═══════════════════════════════════════════════════════════════════════════

mod claude_roundtrip {
    use super::*;

    #[test]
    fn simple_text_with_system_prompt() {
        let msgs = vec![claude_msg("user", "Hello"), claude_msg("assistant", "Hi!")];
        let system = Some("You are helpful.");

        // Step 1: native → IR
        let ir = claude_ir::to_ir(&msgs, system);
        assert_eq!(ir.len(), 3);
        assert_roles(&ir, &[IrRole::System, IrRole::User, IrRole::Assistant]);
        assert_eq!(ir.messages[0].text_content(), "You are helpful.");
        assert_eq!(ir.messages[1].text_content(), "Hello");
        assert_eq!(ir.messages[2].text_content(), "Hi!");

        // Step 2: IR → native
        let sys_extracted = claude_ir::extract_system_prompt(&ir);
        let native = claude_ir::from_ir(&ir);
        assert_eq!(sys_extracted.as_deref(), Some("You are helpful."));
        assert_eq!(native.len(), 2); // system stripped
        assert_eq!(native[0].role, "user");
        assert_eq!(native[0].content, "Hello");

        // Step 3: re-parse → IR
        let ir2 = claude_ir::to_ir(&native, sys_extracted.as_deref());
        assert_eq!(ir, ir2, "round-trip fidelity lost for Claude text");
    }

    #[test]
    fn multi_turn_conversation() {
        let msgs = vec![
            claude_msg("user", "Hi"),
            claude_msg("assistant", "Hello!"),
            claude_msg("user", "How are you?"),
            claude_msg("assistant", "I'm well, thanks!"),
        ];
        let ir = claude_ir::to_ir(&msgs, Some("Be nice."));
        let sys = claude_ir::extract_system_prompt(&ir);
        let native = claude_ir::from_ir(&ir);
        let ir2 = claude_ir::to_ir(&native, sys.as_deref());
        assert_eq!(ir, ir2);
        assert_eq!(ir.len(), 5); // 1 system + 4 messages
    }

    #[test]
    fn tool_use_roundtrip() {
        let tool_blocks = vec![ClaudeContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "read_file".into(),
            input: json!({"path": "lib.rs"}),
        }];
        let result_blocks = vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: Some("pub fn hello() {}".into()),
            is_error: None,
        }];

        let msgs = vec![
            claude_msg("user", "Show me lib.rs"),
            ClaudeMessage {
                role: "assistant".into(),
                content: serde_json::to_string(&tool_blocks).unwrap(),
            },
            ClaudeMessage {
                role: "user".into(),
                content: serde_json::to_string(&result_blocks).unwrap(),
            },
            claude_msg("assistant", "It defines a hello function."),
        ];

        let ir = claude_ir::to_ir(&msgs, None);
        assert_eq!(ir.len(), 4);

        // Verify tool use in IR
        match &ir.messages[1].content[0] {
            IrContentBlock::ToolUse { id, name, .. } => {
                assert_eq!(id, "tu_1");
                assert_eq!(name, "read_file");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }

        // Verify tool result in IR
        match &ir.messages[2].content[0] {
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

        // Round-trip
        let native = claude_ir::from_ir(&ir);
        let ir2 = claude_ir::to_ir(&native, None);
        assert_eq!(ir, ir2, "round-trip fidelity lost for Claude tool use");
    }

    #[test]
    fn thinking_block_roundtrip() {
        let blocks = vec![
            ClaudeContentBlock::Thinking {
                thinking: "Let me reason step by step...".into(),
                signature: Some("sig_abc".into()),
            },
            ClaudeContentBlock::Text {
                text: "The answer is 42.".into(),
            },
        ];
        let msgs = vec![ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];

        let ir = claude_ir::to_ir(&msgs, None);
        assert_eq!(ir.messages[0].content.len(), 2);
        match &ir.messages[0].content[0] {
            IrContentBlock::Thinking { text } => {
                assert_eq!(text, "Let me reason step by step...");
            }
            other => panic!("expected Thinking, got {other:?}"),
        }

        // Round-trip (signature is lost in IR, but thinking text survives)
        let native = claude_ir::from_ir(&ir);
        let ir2 = claude_ir::to_ir(&native, None);
        assert_eq!(ir, ir2, "round-trip fidelity lost for Claude thinking");
    }

    #[test]
    fn image_block_roundtrip() {
        let blocks = vec![
            ClaudeContentBlock::Text {
                text: "Describe this image:".into(),
            },
            ClaudeContentBlock::Image {
                source: ClaudeImageSource::Base64 {
                    media_type: "image/png".into(),
                    data: "iVBORw0KGgo=".into(),
                },
            },
        ];
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];

        let ir = claude_ir::to_ir(&msgs, None);
        assert_eq!(ir.messages[0].content.len(), 2);
        match &ir.messages[0].content[1] {
            IrContentBlock::Image { media_type, data } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "iVBORw0KGgo=");
            }
            other => panic!("expected Image, got {other:?}"),
        }

        // Round-trip
        let native = claude_ir::from_ir(&ir);
        let ir2 = claude_ir::to_ir(&native, None);
        assert_eq!(ir, ir2, "round-trip fidelity lost for Claude image");
    }

    #[test]
    fn empty_conversation() {
        let ir = claude_ir::to_ir(&[], None);
        assert!(ir.is_empty());
        let native = claude_ir::from_ir(&ir);
        let ir2 = claude_ir::to_ir(&native, None);
        assert_eq!(ir, ir2);
    }

    #[test]
    fn tool_result_with_error_flag() {
        let result_blocks = vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "tu_err".into(),
            content: Some("file not found".into()),
            is_error: Some(true),
        }];
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: serde_json::to_string(&result_blocks).unwrap(),
        }];

        let ir = claude_ir::to_ir(&msgs, None);
        match &ir.messages[0].content[0] {
            IrContentBlock::ToolResult { is_error, .. } => assert!(is_error),
            other => panic!("expected ToolResult, got {other:?}"),
        }

        let native = claude_ir::from_ir(&ir);
        let ir2 = claude_ir::to_ir(&native, None);
        assert_eq!(ir, ir2, "error flag fidelity lost");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §3  Gemini round-trip tests
// ═══════════════════════════════════════════════════════════════════════════

mod gemini_roundtrip {
    use super::*;

    #[test]
    fn simple_text_with_system_instruction() {
        let sys = GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("You are helpful.".into())],
        };
        let contents = vec![
            gemini_text("user", "Hello"),
            gemini_text("model", "Hi there!"),
        ];

        // Step 1: native → IR
        let ir = gemini_ir::to_ir(&contents, Some(&sys));
        assert_eq!(ir.len(), 3);
        assert_roles(&ir, &[IrRole::System, IrRole::User, IrRole::Assistant]);
        assert_eq!(ir.messages[0].text_content(), "You are helpful.");
        assert_eq!(ir.messages[2].text_content(), "Hi there!");

        // Step 2: IR → native
        let sys_extracted = gemini_ir::extract_system_instruction(&ir);
        let native = gemini_ir::from_ir(&ir);
        assert!(sys_extracted.is_some());
        assert_eq!(native.len(), 2); // system stripped

        // Step 3: re-parse → IR
        let ir2 = gemini_ir::to_ir(&native, sys_extracted.as_ref());
        assert_eq!(ir, ir2, "round-trip fidelity lost for Gemini text");
    }

    #[test]
    fn multi_turn_conversation() {
        let contents = vec![
            gemini_text("user", "Hi"),
            gemini_text("model", "Hello!"),
            gemini_text("user", "Bye"),
            gemini_text("model", "Goodbye!"),
        ];
        let ir = gemini_ir::to_ir(&contents, None);
        let native = gemini_ir::from_ir(&ir);
        let ir2 = gemini_ir::to_ir(&native, None);
        assert_eq!(ir, ir2);
        assert_eq!(ir.len(), 4);
    }

    #[test]
    fn function_call_and_response_roundtrip() {
        let contents = vec![
            gemini_text("user", "Search for Rust tutorials"),
            GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::FunctionCall {
                    name: "web_search".into(),
                    args: json!({"query": "Rust tutorials"}),
                }],
            },
            GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::FunctionResponse {
                    name: "web_search".into(),
                    response: json!("Found 10 results"),
                }],
            },
            gemini_text("model", "Here are the results."),
        ];

        let ir = gemini_ir::to_ir(&contents, None);
        assert_eq!(ir.len(), 4);

        // Verify function call
        match &ir.messages[1].content[0] {
            IrContentBlock::ToolUse { name, input, .. } => {
                assert_eq!(name, "web_search");
                assert_eq!(input, &json!({"query": "Rust tutorials"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }

        // Verify function response
        match &ir.messages[2].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                is_error,
                ..
            } => {
                assert_eq!(tool_use_id, "gemini_web_search");
                assert!(!is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }

        // Round-trip
        let native = gemini_ir::from_ir(&ir);
        let ir2 = gemini_ir::to_ir(&native, None);
        assert_eq!(
            ir, ir2,
            "round-trip fidelity lost for Gemini function calls"
        );
    }

    #[test]
    fn inline_data_image_roundtrip() {
        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![
                GeminiPart::Text("Describe this image:".into()),
                GeminiPart::InlineData(GeminiInlineData {
                    mime_type: "image/png".into(),
                    data: "iVBORw0KGgo=".into(),
                }),
            ],
        }];

        let ir = gemini_ir::to_ir(&contents, None);
        assert_eq!(ir.messages[0].content.len(), 2);
        match &ir.messages[0].content[1] {
            IrContentBlock::Image { media_type, data } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "iVBORw0KGgo=");
            }
            other => panic!("expected Image, got {other:?}"),
        }

        // Round-trip
        let native = gemini_ir::from_ir(&ir);
        let ir2 = gemini_ir::to_ir(&native, None);
        assert_eq!(ir, ir2, "round-trip fidelity lost for Gemini image");
    }

    #[test]
    fn empty_conversation() {
        let ir = gemini_ir::to_ir(&[], None);
        assert!(ir.is_empty());
        let native = gemini_ir::from_ir(&ir);
        let ir2 = gemini_ir::to_ir(&native, None);
        assert_eq!(ir, ir2);
    }

    #[test]
    fn multiple_parts_in_one_content() {
        let contents = vec![GeminiContent {
            role: "model".into(),
            parts: vec![
                GeminiPart::Text("Searching...".into()),
                GeminiPart::FunctionCall {
                    name: "search".into(),
                    args: json!({"q": "test"}),
                },
            ],
        }];

        let ir = gemini_ir::to_ir(&contents, None);
        assert_eq!(ir.messages[0].content.len(), 2);

        let native = gemini_ir::from_ir(&ir);
        assert_eq!(native[0].parts.len(), 2);

        let ir2 = gemini_ir::to_ir(&native, None);
        assert_eq!(ir, ir2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §4  Kimi round-trip tests
// ═══════════════════════════════════════════════════════════════════════════

mod kimi_roundtrip {
    use super::*;

    #[test]
    fn simple_text_system_user_assistant() {
        let msgs = vec![
            kimi_msg("system", "You are helpful."),
            kimi_msg("user", "Hello"),
            kimi_msg("assistant", "Hi there!"),
        ];

        let ir = kimi_ir::to_ir(&msgs);
        assert_eq!(ir.len(), 3);
        assert_roles(&ir, &[IrRole::System, IrRole::User, IrRole::Assistant]);
        assert_eq!(ir.messages[0].text_content(), "You are helpful.");

        let native = kimi_ir::from_ir(&ir);
        let ir2 = kimi_ir::to_ir(&native);
        assert_eq!(ir, ir2, "round-trip fidelity lost for Kimi text");
    }

    #[test]
    fn multi_turn_conversation() {
        let msgs = vec![
            kimi_msg("system", "Be concise."),
            kimi_msg("user", "Hi"),
            kimi_msg("assistant", "Hello!"),
            kimi_msg("user", "Bye"),
            kimi_msg("assistant", "Goodbye!"),
        ];
        let ir = kimi_ir::to_ir(&msgs);
        let native = kimi_ir::from_ir(&ir);
        let ir2 = kimi_ir::to_ir(&native);
        assert_eq!(ir, ir2);
    }

    #[test]
    fn tool_call_and_result_roundtrip() {
        let msgs = vec![
            kimi_msg("user", "Search for Rust"),
            KimiMessage {
                role: "assistant".into(),
                content: None,
                tool_call_id: None,
                tool_calls: Some(vec![KimiToolCall {
                    id: "call_1".into(),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: "web_search".into(),
                        arguments: r#"{"query":"Rust"}"#.into(),
                    },
                }]),
            },
            KimiMessage {
                role: "tool".into(),
                content: Some("Found results".into()),
                tool_call_id: Some("call_1".into()),
                tool_calls: None,
            },
            kimi_msg("assistant", "Here are the results."),
        ];

        let ir = kimi_ir::to_ir(&msgs);
        assert_eq!(ir.len(), 4);

        // Verify tool call
        match &ir.messages[1].content[0] {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "web_search");
                assert_eq!(input, &json!({"query": "Rust"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }

        // Verify tool result
        match &ir.messages[2].content[0] {
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

        let native = kimi_ir::from_ir(&ir);
        let ir2 = kimi_ir::to_ir(&native);
        assert_eq!(ir, ir2, "round-trip fidelity lost for Kimi tool calls");
    }

    #[test]
    fn usage_conversion() {
        let usage = KimiUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        let ir_usage = kimi_ir::usage_to_ir(&usage);
        assert_eq!(ir_usage.input_tokens, 100);
        assert_eq!(ir_usage.output_tokens, 50);
        assert_eq!(ir_usage.total_tokens, 150);
    }

    #[test]
    fn empty_conversation() {
        let ir = kimi_ir::to_ir(&[]);
        assert!(ir.is_empty());
        let native = kimi_ir::from_ir(&ir);
        let ir2 = kimi_ir::to_ir(&native);
        assert_eq!(ir, ir2);
    }

    #[test]
    fn multiple_tool_calls_roundtrip() {
        let msgs = vec![KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: Some(vec![
                KimiToolCall {
                    id: "c1".into(),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: "read".into(),
                        arguments: r#"{"path":"a.rs"}"#.into(),
                    },
                },
                KimiToolCall {
                    id: "c2".into(),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: "read".into(),
                        arguments: r#"{"path":"b.rs"}"#.into(),
                    },
                },
            ]),
        }];

        let ir = kimi_ir::to_ir(&msgs);
        assert_eq!(ir.messages[0].content.len(), 2);

        let native = kimi_ir::from_ir(&ir);
        let ir2 = kimi_ir::to_ir(&native);
        assert_eq!(ir, ir2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §5  Codex round-trip tests
// ═══════════════════════════════════════════════════════════════════════════

mod codex_roundtrip {
    use super::*;

    #[test]
    fn input_message_roundtrip() {
        let items = vec![
            CodexInputItem::Message {
                role: "system".into(),
                content: "You are helpful.".into(),
            },
            CodexInputItem::Message {
                role: "user".into(),
                content: "Hello".into(),
            },
        ];

        let ir = codex_ir::input_to_ir(&items);
        assert_eq!(ir.len(), 2);
        assert_eq!(ir.messages[0].role, IrRole::System);
        assert_eq!(ir.messages[0].text_content(), "You are helpful.");
        assert_eq!(ir.messages[1].role, IrRole::User);
        assert_eq!(ir.messages[1].text_content(), "Hello");
    }

    #[test]
    fn response_message_roundtrip() {
        let items = vec![CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "Hello!".into(),
            }],
        }];

        let ir = codex_ir::to_ir(&items);
        assert_eq!(ir.len(), 1);
        assert_eq!(ir.messages[0].role, IrRole::Assistant);
        assert_eq!(ir.messages[0].text_content(), "Hello!");

        let native = codex_ir::from_ir(&ir);
        let ir2 = codex_ir::to_ir(&native);
        assert_eq!(ir, ir2, "round-trip fidelity lost for Codex message");
    }

    #[test]
    fn function_call_roundtrip() {
        let items = vec![CodexResponseItem::FunctionCall {
            id: "fc_1".into(),
            call_id: Some("corr_1".into()),
            name: "read_file".into(),
            arguments: r#"{"path":"main.rs"}"#.into(),
        }];

        let ir = codex_ir::to_ir(&items);
        assert_eq!(ir.messages[0].role, IrRole::Assistant);
        match &ir.messages[0].content[0] {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "fc_1");
                assert_eq!(name, "read_file");
                assert_eq!(input, &json!({"path": "main.rs"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }

        let native = codex_ir::from_ir(&ir);
        match &native[0] {
            CodexResponseItem::FunctionCall {
                id,
                name,
                arguments,
                ..
            } => {
                assert_eq!(id, "fc_1");
                assert_eq!(name, "read_file");
                assert!(arguments.contains("main.rs"));
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }

        let ir2 = codex_ir::to_ir(&native);
        assert_eq!(ir, ir2, "round-trip fidelity lost for Codex function call");
    }

    #[test]
    fn function_call_output_roundtrip() {
        let items = vec![CodexResponseItem::FunctionCallOutput {
            call_id: "fc_1".into(),
            output: "fn main() {}".into(),
        }];

        let ir = codex_ir::to_ir(&items);
        assert_eq!(ir.messages[0].role, IrRole::Tool);
        match &ir.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                is_error,
                content,
            } => {
                assert_eq!(tool_use_id, "fc_1");
                assert!(!is_error);
                assert_eq!(content.len(), 1);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }

        let native = codex_ir::from_ir(&ir);
        let ir2 = codex_ir::to_ir(&native);
        assert_eq!(
            ir, ir2,
            "round-trip fidelity lost for Codex function output"
        );
    }

    #[test]
    fn reasoning_roundtrip() {
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

        let ir = codex_ir::to_ir(&items);
        assert_eq!(ir.messages[0].role, IrRole::Assistant);
        match &ir.messages[0].content[0] {
            IrContentBlock::Thinking { text } => {
                assert!(text.contains("Step 1"));
                assert!(text.contains("Step 2"));
            }
            other => panic!("expected Thinking, got {other:?}"),
        }

        // Reasoning roundtrips through a single summary (joined text)
        let native = codex_ir::from_ir(&ir);
        match &native[0] {
            CodexResponseItem::Reasoning { summary } => {
                assert_eq!(summary.len(), 1);
                assert!(summary[0].text.contains("Step 1"));
                assert!(summary[0].text.contains("Step 2"));
            }
            other => panic!("expected Reasoning, got {other:?}"),
        }
    }

    #[test]
    fn multi_item_conversation_roundtrip() {
        let items = vec![
            CodexResponseItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "Let me check.".into(),
                }],
            },
            CodexResponseItem::FunctionCall {
                id: "fc_1".into(),
                call_id: None,
                name: "read".into(),
                arguments: r#"{"file":"x"}"#.into(),
            },
            CodexResponseItem::FunctionCallOutput {
                call_id: "fc_1".into(),
                output: "data".into(),
            },
            CodexResponseItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "Got the data.".into(),
                }],
            },
        ];

        let ir = codex_ir::to_ir(&items);
        assert_eq!(ir.len(), 4);

        let native = codex_ir::from_ir(&ir);
        let ir2 = codex_ir::to_ir(&native);
        assert_eq!(ir, ir2, "round-trip fidelity lost for Codex multi-item");
    }

    #[test]
    fn usage_conversion() {
        let usage = CodexUsage {
            input_tokens: 200,
            output_tokens: 100,
            total_tokens: 300,
        };
        let ir_usage = codex_ir::usage_to_ir(&usage);
        assert_eq!(ir_usage.input_tokens, 200);
        assert_eq!(ir_usage.output_tokens, 100);
        assert_eq!(ir_usage.total_tokens, 300);
    }

    #[test]
    fn system_and_user_skipped_in_from_ir() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "instructions"),
            IrMessage::text(IrRole::User, "hello"),
            IrMessage::text(IrRole::Assistant, "hi"),
        ]);
        let items = codex_ir::from_ir(&conv);
        assert_eq!(items.len(), 1);
        match &items[0] {
            CodexResponseItem::Message { role, .. } => assert_eq!(role, "assistant"),
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn empty_conversation() {
        let ir = codex_ir::to_ir(&[]);
        assert!(ir.is_empty());
        let native = codex_ir::from_ir(&ir);
        assert!(native.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §6  Copilot round-trip tests
// ═══════════════════════════════════════════════════════════════════════════

mod copilot_roundtrip {
    use super::*;

    #[test]
    fn simple_text_system_user_assistant() {
        let msgs = vec![
            copilot_msg("system", "You are helpful."),
            copilot_msg("user", "Hello"),
            copilot_msg("assistant", "Hi there!"),
        ];

        let ir = copilot_ir::to_ir(&msgs);
        assert_eq!(ir.len(), 3);
        assert_roles(&ir, &[IrRole::System, IrRole::User, IrRole::Assistant]);
        assert_eq!(ir.messages[0].text_content(), "You are helpful.");
        assert_eq!(ir.messages[1].text_content(), "Hello");
        assert_eq!(ir.messages[2].text_content(), "Hi there!");

        let native = copilot_ir::from_ir(&ir);
        let ir2 = copilot_ir::to_ir(&native);
        assert_eq!(ir, ir2, "round-trip fidelity lost for Copilot text");
    }

    #[test]
    fn multi_turn_conversation() {
        let msgs = vec![
            copilot_msg("system", "Be concise."),
            copilot_msg("user", "Hi"),
            copilot_msg("assistant", "Hello!"),
            copilot_msg("user", "Bye"),
            copilot_msg("assistant", "Goodbye!"),
        ];
        let ir = copilot_ir::to_ir(&msgs);
        let native = copilot_ir::from_ir(&ir);
        let ir2 = copilot_ir::to_ir(&native);
        assert_eq!(ir, ir2);
        assert_eq!(ir.len(), 5);
    }

    #[test]
    fn name_preserved_through_roundtrip() {
        let msgs = vec![CopilotMessage {
            role: "user".into(),
            content: "Hello".into(),
            name: Some("alice".into()),
            copilot_references: vec![],
        }];

        let ir = copilot_ir::to_ir(&msgs);
        assert_eq!(
            ir.messages[0]
                .metadata
                .get("copilot_name")
                .and_then(|v| v.as_str()),
            Some("alice")
        );

        let native = copilot_ir::from_ir(&ir);
        assert_eq!(native[0].name.as_deref(), Some("alice"));

        let ir2 = copilot_ir::to_ir(&native);
        assert_eq!(ir, ir2, "name fidelity lost in Copilot round-trip");
    }

    #[test]
    fn tool_role_mapped_to_user() {
        // Copilot has no tool role; tool results map to user
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![IrContentBlock::Text {
                    text: "result data".into(),
                }],
                is_error: false,
            }],
        )]);
        let native = copilot_ir::from_ir(&conv);
        assert_eq!(native[0].role, "user");
    }

    #[test]
    fn thinking_block_becomes_text() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "reasoning...".into(),
                },
                IrContentBlock::Text {
                    text: "answer".into(),
                },
            ],
        )]);
        let native = copilot_ir::from_ir(&conv);
        assert_eq!(native[0].role, "assistant");
        assert_eq!(native[0].content, "reasoning...answer");
    }

    #[test]
    fn empty_conversation() {
        let ir = copilot_ir::to_ir(&[]);
        assert!(ir.is_empty());
        let native = copilot_ir::from_ir(&ir);
        let ir2 = copilot_ir::to_ir(&native);
        assert_eq!(ir, ir2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §7  Cross-dialect fidelity checks (IR constructed, lowered, re-parsed)
// ═══════════════════════════════════════════════════════════════════════════

mod cross_dialect_fidelity {
    use super::*;

    /// Build a canonical IR conversation for fidelity testing.
    fn canonical_ir() -> IrConversation {
        IrConversation::new()
            .push(IrMessage::text(
                IrRole::System,
                "You are a coding assistant.",
            ))
            .push(IrMessage::text(IrRole::User, "Read main.rs"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "Let me read that file.".into(),
                    },
                    IrContentBlock::ToolUse {
                        id: "call_1".into(),
                        name: "read_file".into(),
                        input: json!({"path": "main.rs"}),
                    },
                ],
            ))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "call_1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "fn main() {}".into(),
                    }],
                    is_error: false,
                }],
            ))
            .push(IrMessage::text(
                IrRole::Assistant,
                "The file contains a main function.",
            ))
    }

    #[test]
    fn openai_preserves_all_fields() {
        let ir = canonical_ir();
        let native = openai_ir::from_ir(&ir);
        let ir2 = openai_ir::to_ir(&native);
        assert_eq!(ir, ir2, "OpenAI round-trip lost data");
    }

    #[test]
    fn claude_preserves_semantic_content() {
        let ir = canonical_ir();
        let sys = claude_ir::extract_system_prompt(&ir);
        let native = claude_ir::from_ir(&ir);
        let ir2 = claude_ir::to_ir(&native, sys.as_deref());

        // System prompt preserved
        assert_eq!(
            ir2.system_message().unwrap().text_content(),
            "You are a coding assistant."
        );
        // Message count preserved
        assert_eq!(ir2.len(), ir.len());
        // Tool call name preserved
        assert_eq!(ir2.tool_calls().len(), 1);
        match ir2.tool_calls()[0] {
            IrContentBlock::ToolUse { name, .. } => assert_eq!(name, "read_file"),
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn gemini_preserves_semantic_content() {
        let ir = canonical_ir();
        let sys = gemini_ir::extract_system_instruction(&ir);
        let native = gemini_ir::from_ir(&ir);
        let ir2 = gemini_ir::to_ir(&native, sys.as_ref());

        assert_eq!(
            ir2.system_message().unwrap().text_content(),
            "You are a coding assistant."
        );
        assert_eq!(ir2.len(), ir.len());
        // Gemini synthesizes IDs; tool call name must survive
        assert_eq!(ir2.tool_calls().len(), 1);
        match ir2.tool_calls()[0] {
            IrContentBlock::ToolUse { name, .. } => assert_eq!(name, "read_file"),
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn kimi_preserves_all_fields() {
        let ir = canonical_ir();
        let native = kimi_ir::from_ir(&ir);
        let ir2 = kimi_ir::to_ir(&native);
        assert_eq!(ir, ir2, "Kimi round-trip lost data");
    }

    #[test]
    fn codex_preserves_assistant_and_tool_content() {
        let ir = canonical_ir();
        let native = codex_ir::from_ir(&ir);
        // Codex drops system + user messages
        let ir2 = codex_ir::to_ir(&native);

        // Only assistant+tool messages survive
        assert!(ir2.len() >= 3);
        // Tool call name preserved
        let tool_calls = ir2.tool_calls();
        assert_eq!(tool_calls.len(), 1);
        match tool_calls[0] {
            IrContentBlock::ToolUse { name, .. } => assert_eq!(name, "read_file"),
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn copilot_preserves_text_content() {
        let ir = canonical_ir();
        let native = copilot_ir::from_ir(&ir);
        let ir2 = copilot_ir::to_ir(&native);

        // Message count preserved
        assert_eq!(ir2.len(), ir.len());
        // System text preserved
        assert_eq!(
            ir2.system_message().unwrap().text_content(),
            "You are a coding assistant."
        );
        // User text preserved
        assert_eq!(ir2.messages[1].text_content(), "Read main.rs");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §8  IrToolDefinition basic validation
// ═══════════════════════════════════════════════════════════════════════════

mod tool_definition_tests {
    use super::*;

    #[test]
    fn ir_tool_definition_serialization_roundtrip() {
        let def = IrToolDefinition {
            name: "read_file".into(),
            description: "Read a file from disk".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                },
                "required": ["path"]
            }),
        };

        let serialized = serde_json::to_string(&def).unwrap();
        let deserialized: IrToolDefinition = serde_json::from_str(&serialized).unwrap();
        assert_eq!(def, deserialized);
    }

    #[test]
    fn ir_tool_definition_fields_accessible() {
        let def = IrToolDefinition {
            name: "search".into(),
            description: "Search code".into(),
            parameters: json!({"type": "object"}),
        };
        assert_eq!(def.name, "search");
        assert_eq!(def.description, "Search code");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §9  IrUsage round-trip validation
// ═══════════════════════════════════════════════════════════════════════════

mod usage_roundtrip {
    use super::*;

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
        let usage = IrUsage::with_cache(100, 50, 20, 10);
        assert_eq!(usage.total_tokens, 150);
        assert_eq!(usage.cache_read_tokens, 20);
        assert_eq!(usage.cache_write_tokens, 10);
    }

    #[test]
    fn ir_usage_merge() {
        let a = IrUsage::from_io(100, 50);
        let b = IrUsage::from_io(200, 100);
        let merged = a.merge(b);
        assert_eq!(merged.input_tokens, 300);
        assert_eq!(merged.output_tokens, 150);
        assert_eq!(merged.total_tokens, 450);
    }

    #[test]
    fn ir_usage_serialization_roundtrip() {
        let usage = IrUsage::with_cache(100, 50, 20, 10);
        let json = serde_json::to_string(&usage).unwrap();
        let deserialized: IrUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(usage, deserialized);
    }

    #[test]
    fn codex_usage_to_ir() {
        let usage = CodexUsage {
            input_tokens: 500,
            output_tokens: 200,
            total_tokens: 700,
        };
        let ir = codex_ir::usage_to_ir(&usage);
        assert_eq!(ir.input_tokens, 500);
        assert_eq!(ir.output_tokens, 200);
        assert_eq!(ir.total_tokens, 700);
    }

    #[test]
    fn kimi_usage_to_ir() {
        let usage = KimiUsage {
            prompt_tokens: 300,
            completion_tokens: 150,
            total_tokens: 450,
        };
        let ir = kimi_ir::usage_to_ir(&usage);
        assert_eq!(ir.input_tokens, 300);
        assert_eq!(ir.output_tokens, 150);
        assert_eq!(ir.total_tokens, 450);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// §10  Edge cases and exhaustive fidelity checks
// ═══════════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;

    #[test]
    fn openai_assistant_with_text_and_tool_call_roundtrip() {
        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: Some("Searching...".into()),
            tool_calls: Some(vec![OpenAIToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"test"}"#.into(),
                },
            }]),
            tool_call_id: None,
        }];

        let ir = openai_ir::to_ir(&msgs);
        assert_eq!(ir.messages[0].content.len(), 2);

        let native = openai_ir::from_ir(&ir);
        assert!(native[0].content.is_some());
        assert!(native[0].tool_calls.is_some());

        let ir2 = openai_ir::to_ir(&native);
        assert_eq!(ir, ir2);
    }

    #[test]
    fn claude_mixed_blocks_roundtrip() {
        let blocks = vec![
            ClaudeContentBlock::Thinking {
                thinking: "hmm".into(),
                signature: None,
            },
            ClaudeContentBlock::Text {
                text: "Here:".into(),
            },
            ClaudeContentBlock::ToolUse {
                id: "t1".into(),
                name: "search".into(),
                input: json!({"q": "rust"}),
            },
        ];
        let msgs = vec![ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];

        let ir = claude_ir::to_ir(&msgs, None);
        assert_eq!(ir.messages[0].content.len(), 3);

        let native = claude_ir::from_ir(&ir);
        let ir2 = claude_ir::to_ir(&native, None);
        assert_eq!(ir, ir2, "Claude mixed blocks fidelity lost");
    }

    #[test]
    fn gemini_function_response_with_object_payload() {
        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::FunctionResponse {
                name: "api_call".into(),
                response: json!({"status": 200, "body": "ok"}),
            }],
        }];

        let ir = gemini_ir::to_ir(&contents, None);
        match &ir.messages[0].content[0] {
            IrContentBlock::ToolResult { content, .. } => {
                assert_eq!(content.len(), 1);
                let text = match &content[0] {
                    IrContentBlock::Text { text } => text.as_str(),
                    _ => panic!("expected text"),
                };
                assert!(text.contains("200"));
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn codex_assistant_with_text_and_tool_use_from_ir() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me check.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "read".into(),
                    input: json!({"path": "x"}),
                },
            ],
        )]);

        let items = codex_ir::from_ir(&conv);
        // Should produce Message + FunctionCall
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[0], CodexResponseItem::Message { .. }));
        assert!(matches!(&items[1], CodexResponseItem::FunctionCall { .. }));

        let ir2 = codex_ir::to_ir(&items);
        // Re-parsed as 2 separate messages (Codex splits them)
        assert_eq!(ir2.len(), 2);
    }

    #[test]
    fn copilot_empty_content_string_roundtrip() {
        let msgs = vec![CopilotMessage {
            role: "user".into(),
            content: String::new(),
            name: None,
            copilot_references: vec![],
        }];

        let ir = copilot_ir::to_ir(&msgs);
        assert!(ir.messages[0].content.is_empty());

        let native = copilot_ir::from_ir(&ir);
        assert!(native[0].content.is_empty());

        let ir2 = copilot_ir::to_ir(&native);
        assert_eq!(ir, ir2);
    }

    #[test]
    fn kimi_text_and_tool_call_combined() {
        let msgs = vec![KimiMessage {
            role: "assistant".into(),
            content: Some("Searching...".into()),
            tool_call_id: None,
            tool_calls: Some(vec![KimiToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"rust"}"#.into(),
                },
            }]),
        }];

        let ir = kimi_ir::to_ir(&msgs);
        assert_eq!(ir.messages[0].content.len(), 2);

        let native = kimi_ir::from_ir(&ir);
        assert!(native[0].content.is_some());
        assert!(native[0].tool_calls.is_some());

        let ir2 = kimi_ir::to_ir(&native);
        assert_eq!(ir, ir2);
    }

    #[test]
    fn ir_conversation_accessors() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "hello"))
            .push(IrMessage::text(IrRole::Assistant, "hi"));

        assert_eq!(conv.system_message().unwrap().text_content(), "sys");
        assert_eq!(conv.last_assistant().unwrap().text_content(), "hi");
        assert_eq!(conv.last_message().unwrap().text_content(), "hi");
        assert_eq!(conv.messages_by_role(IrRole::User).len(), 1);
        assert!(!conv.is_empty());
    }

    #[test]
    fn ir_message_is_text_only() {
        let text_msg = IrMessage::text(IrRole::User, "hello");
        assert!(text_msg.is_text_only());

        let mixed_msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text { text: "hi".into() },
                IrContentBlock::ToolUse {
                    id: "t".into(),
                    name: "n".into(),
                    input: json!({}),
                },
            ],
        );
        assert!(!mixed_msg.is_text_only());
    }

    #[test]
    fn openai_unknown_role_defaults_to_user() {
        let msgs = vec![openai_msg("developer", "hi")];
        let ir = openai_ir::to_ir(&msgs);
        assert_eq!(ir.messages[0].role, IrRole::User);
    }

    #[test]
    fn kimi_unknown_role_defaults_to_user() {
        let msgs = vec![kimi_msg("developer", "hi")];
        let ir = kimi_ir::to_ir(&msgs);
        assert_eq!(ir.messages[0].role, IrRole::User);
    }

    #[test]
    fn copilot_unknown_role_defaults_to_user() {
        let msgs = vec![copilot_msg("developer", "hi")];
        let ir = copilot_ir::to_ir(&msgs);
        assert_eq!(ir.messages[0].role, IrRole::User);
    }

    #[test]
    fn claude_no_system_prompt_roundtrip() {
        let msgs = vec![claude_msg("user", "Hello"), claude_msg("assistant", "Hi!")];
        let ir = claude_ir::to_ir(&msgs, None);
        assert!(ir.system_message().is_none());

        let native = claude_ir::from_ir(&ir);
        let ir2 = claude_ir::to_ir(&native, None);
        assert_eq!(ir, ir2);
    }

    #[test]
    fn gemini_no_system_instruction_roundtrip() {
        let contents = vec![gemini_text("user", "Hello"), gemini_text("model", "Hi!")];
        let ir = gemini_ir::to_ir(&contents, None);
        assert!(ir.system_message().is_none());

        let sys = gemini_ir::extract_system_instruction(&ir);
        assert!(sys.is_none());

        let native = gemini_ir::from_ir(&ir);
        let ir2 = gemini_ir::to_ir(&native, None);
        assert_eq!(ir, ir2);
    }
}
