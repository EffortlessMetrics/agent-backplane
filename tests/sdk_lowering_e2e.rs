// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end tests for the SDK lowering pipeline:
//!   SDK-specific types → IR → SDK-specific types
//!
//! Categories:
//!   1. Lowering from each SDK's native types to IR
//!   2. Lifting from IR back to each SDK's native types
//!   3. Cross-SDK roundtrip (SDK A → IR → SDK B)
//!   4. Fidelity loss tracking in cross-SDK mapping
//!   5. Edge cases: empty messages, tool-heavy conversations, streaming chunks

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};
use serde_json::json;

// ── SDK re-imports ──────────────────────────────────────────────────────

use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};
use abp_openai_sdk::lowering as openai;

use abp_claude_sdk::dialect::{ClaudeContentBlock, ClaudeImageSource, ClaudeMessage};
use abp_claude_sdk::lowering as claude;

use abp_gemini_sdk::dialect::{GeminiContent, GeminiInlineData, GeminiPart};
use abp_gemini_sdk::lowering as gemini;

use abp_codex_sdk::dialect::{
    CodexContentPart, CodexInputItem, CodexResponseItem, CodexUsage, ReasoningSummary,
};
use abp_codex_sdk::lowering as codex;

use abp_kimi_sdk::dialect::{KimiFunctionCall, KimiMessage, KimiToolCall, KimiUsage};
use abp_kimi_sdk::lowering as kimi;

use abp_copilot_sdk::dialect::{CopilotMessage, CopilotReference, CopilotReferenceType};
use abp_copilot_sdk::lowering as copilot;

use abp_dialect::Dialect;
use abp_mapping::{Fidelity, MappingError, MappingRegistry, MappingRule};

// ═══════════════════════════════════════════════════════════════════════
// Helper constructors
// ═══════════════════════════════════════════════════════════════════════

fn openai_text(role: &str, text: &str) -> OpenAIMessage {
    OpenAIMessage {
        role: role.into(),
        content: Some(text.into()),
        tool_calls: None,
        tool_call_id: None,
    }
}

fn openai_tool_call(id: &str, name: &str, args: &str) -> OpenAIToolCall {
    OpenAIToolCall {
        id: id.into(),
        call_type: "function".into(),
        function: OpenAIFunctionCall {
            name: name.into(),
            arguments: args.into(),
        },
    }
}

fn claude_text(role: &str, text: &str) -> ClaudeMessage {
    ClaudeMessage {
        role: role.into(),
        content: text.into(),
    }
}

fn claude_blocks(role: &str, blocks: Vec<ClaudeContentBlock>) -> ClaudeMessage {
    ClaudeMessage {
        role: role.into(),
        content: serde_json::to_string(&blocks).unwrap(),
    }
}

fn gemini_text(role: &str, text: &str) -> GeminiContent {
    GeminiContent {
        role: role.into(),
        parts: vec![GeminiPart::Text(text.into())],
    }
}

fn kimi_text(role: &str, text: &str) -> KimiMessage {
    KimiMessage {
        role: role.into(),
        content: Some(text.into()),
        tool_call_id: None,
        tool_calls: None,
    }
}

fn copilot_text(role: &str, text: &str) -> CopilotMessage {
    CopilotMessage {
        role: role.into(),
        content: text.into(),
        name: None,
        copilot_references: vec![],
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 1. LOWERING: SDK → IR
// ═══════════════════════════════════════════════════════════════════════

mod openai_to_ir {
    use super::*;

    #[test]
    fn system_message() {
        let conv = openai::to_ir(&[openai_text("system", "Be helpful")]);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "Be helpful");
    }

    #[test]
    fn user_message() {
        let conv = openai::to_ir(&[openai_text("user", "Hello")]);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello");
    }

    #[test]
    fn assistant_message() {
        let conv = openai::to_ir(&[openai_text("assistant", "Hi there")]);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        assert_eq!(conv.messages[0].text_content(), "Hi there");
    }

    #[test]
    fn tool_call_becomes_tool_use() {
        let msg = OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![openai_tool_call("c1", "read", r#"{"p":"x"}"#)]),
            tool_call_id: None,
        };
        let conv = openai::to_ir(&[msg]);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "c1");
                assert_eq!(name, "read");
                assert_eq!(input, &json!({"p": "x"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_becomes_tool_result() {
        let msg = OpenAIMessage {
            role: "tool".into(),
            content: Some("file data".into()),
            tool_calls: None,
            tool_call_id: Some("c1".into()),
        };
        let conv = openai::to_ir(&[msg]);
        assert_eq!(conv.messages[0].role, IrRole::Tool);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "c1");
                assert!(!is_error);
                assert_eq!(content.len(), 1);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn unknown_role_maps_to_user() {
        let conv = openai::to_ir(&[openai_text("developer", "hi")]);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    #[test]
    fn multiple_tool_calls() {
        let msg = OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![
                openai_tool_call("c1", "a", "{}"),
                openai_tool_call("c2", "b", "{}"),
                openai_tool_call("c3", "c", "{}"),
            ]),
            tool_call_id: None,
        };
        let conv = openai::to_ir(&[msg]);
        assert_eq!(conv.messages[0].content.len(), 3);
    }

    #[test]
    fn malformed_arguments_kept() {
        let msg = OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![openai_tool_call("c1", "f", "bad-json")]),
            tool_call_id: None,
        };
        let conv = openai::to_ir(&[msg]);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { input, .. } => {
                assert_eq!(input, &serde_json::Value::String("bad-json".into()));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }
}

mod claude_to_ir {
    use super::*;

    #[test]
    fn user_text() {
        let conv = claude::to_ir(&[claude_text("user", "Hello")], None);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello");
    }

    #[test]
    fn system_prompt_prepended() {
        let conv = claude::to_ir(&[claude_text("user", "Hi")], Some("Be nice"));
        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "Be nice");
    }

    #[test]
    fn empty_system_prompt_skipped() {
        let conv = claude::to_ir(&[claude_text("user", "Hi")], Some(""));
        assert_eq!(conv.len(), 1);
    }

    #[test]
    fn tool_use_block() {
        let blocks = vec![ClaudeContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "search".into(),
            input: json!({"q": "rust"}),
        }];
        let conv = claude::to_ir(&[claude_blocks("assistant", blocks)], None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { id, name, .. } => {
                assert_eq!(id, "tu_1");
                assert_eq!(name, "search");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_block() {
        let blocks = vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: Some("data".into()),
            is_error: None,
        }];
        let conv = claude::to_ir(&[claude_blocks("user", blocks)], None);
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
    fn thinking_block() {
        let blocks = vec![ClaudeContentBlock::Thinking {
            thinking: "let me think".into(),
            signature: Some("sig".into()),
        }];
        let conv = claude::to_ir(&[claude_blocks("assistant", blocks)], None);
        match &conv.messages[0].content[0] {
            IrContentBlock::Thinking { text } => assert_eq!(text, "let me think"),
            other => panic!("expected Thinking, got {other:?}"),
        }
    }

    #[test]
    fn image_base64_block() {
        let blocks = vec![ClaudeContentBlock::Image {
            source: ClaudeImageSource::Base64 {
                media_type: "image/png".into(),
                data: "abc".into(),
            },
        }];
        let conv = claude::to_ir(&[claude_blocks("user", blocks)], None);
        match &conv.messages[0].content[0] {
            IrContentBlock::Image { media_type, data } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "abc");
            }
            other => panic!("expected Image, got {other:?}"),
        }
    }

    #[test]
    fn image_url_becomes_text() {
        let blocks = vec![ClaudeContentBlock::Image {
            source: ClaudeImageSource::Url {
                url: "https://example.com/img.png".into(),
            },
        }];
        let conv = claude::to_ir(&[claude_blocks("user", blocks)], None);
        match &conv.messages[0].content[0] {
            IrContentBlock::Text { text } => {
                assert!(text.contains("https://example.com/img.png"));
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }
}

mod gemini_to_ir {
    use super::*;

    #[test]
    fn user_text() {
        let conv = gemini::to_ir(&[gemini_text("user", "Hello")], None);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello");
    }

    #[test]
    fn model_maps_to_assistant() {
        let conv = gemini::to_ir(&[gemini_text("model", "Hi")], None);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
    }

    #[test]
    fn system_instruction() {
        let sys = gemini_text("user", "Be helpful");
        let conv = gemini::to_ir(&[gemini_text("user", "Hi")], Some(&sys));
        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "Be helpful");
    }

    #[test]
    fn function_call() {
        let content = GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "search".into(),
                args: json!({"q": "rust"}),
            }],
        };
        let conv = gemini::to_ir(&[content], None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { name, id, .. } => {
                assert_eq!(name, "search");
                assert_eq!(id, "gemini_search");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn function_response() {
        let content = GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::FunctionResponse {
                name: "search".into(),
                response: json!("results"),
            }],
        };
        let conv = gemini::to_ir(&[content], None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult { tool_use_id, .. } => {
                assert_eq!(tool_use_id, "gemini_search");
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn inline_data_becomes_image() {
        let content = GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::InlineData(GeminiInlineData {
                mime_type: "image/jpeg".into(),
                data: "b64data".into(),
            })],
        };
        let conv = gemini::to_ir(&[content], None);
        match &conv.messages[0].content[0] {
            IrContentBlock::Image { media_type, data } => {
                assert_eq!(media_type, "image/jpeg");
                assert_eq!(data, "b64data");
            }
            other => panic!("expected Image, got {other:?}"),
        }
    }
}

mod codex_to_ir {
    use super::*;

    #[test]
    fn input_user_message() {
        let conv = codex::input_to_ir(&[CodexInputItem::Message {
            role: "user".into(),
            content: "Hello".into(),
        }]);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello");
    }

    #[test]
    fn input_system_message() {
        let conv = codex::input_to_ir(&[CodexInputItem::Message {
            role: "system".into(),
            content: "Be helpful".into(),
        }]);
        assert_eq!(conv.messages[0].role, IrRole::System);
    }

    #[test]
    fn response_message() {
        let conv = codex::to_ir(&[CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "Done".into(),
            }],
        }]);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        assert_eq!(conv.messages[0].text_content(), "Done");
    }

    #[test]
    fn function_call_item() {
        let conv = codex::to_ir(&[CodexResponseItem::FunctionCall {
            id: "fc_1".into(),
            call_id: None,
            name: "shell".into(),
            arguments: r#"{"cmd":"ls"}"#.into(),
        }]);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "fc_1");
                assert_eq!(name, "shell");
                assert_eq!(input, &json!({"cmd": "ls"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn function_call_output_item() {
        let conv = codex::to_ir(&[CodexResponseItem::FunctionCallOutput {
            call_id: "fc_1".into(),
            output: "ok".into(),
        }]);
        assert_eq!(conv.messages[0].role, IrRole::Tool);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                is_error,
                ..
            } => {
                assert_eq!(tool_use_id, "fc_1");
                assert!(!is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn reasoning_item() {
        let conv = codex::to_ir(&[CodexResponseItem::Reasoning {
            summary: vec![
                ReasoningSummary {
                    text: "Step 1".into(),
                },
                ReasoningSummary {
                    text: "Step 2".into(),
                },
            ],
        }]);
        match &conv.messages[0].content[0] {
            IrContentBlock::Thinking { text } => {
                assert!(text.contains("Step 1"));
                assert!(text.contains("Step 2"));
            }
            other => panic!("expected Thinking, got {other:?}"),
        }
    }

    #[test]
    fn usage_conversion() {
        let usage = CodexUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
        };
        let ir = codex::usage_to_ir(&usage);
        assert_eq!(ir.input_tokens, 100);
        assert_eq!(ir.output_tokens, 50);
        assert_eq!(ir.total_tokens, 150);
    }
}

mod kimi_to_ir {
    use super::*;

    #[test]
    fn system_message() {
        let conv = kimi::to_ir(&[kimi_text("system", "Be helpful")]);
        assert_eq!(conv.messages[0].role, IrRole::System);
    }

    #[test]
    fn user_message() {
        let conv = kimi::to_ir(&[kimi_text("user", "Hi")]);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hi");
    }

    #[test]
    fn assistant_tool_call() {
        let msg = KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: Some(vec![KimiToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "web_search".into(),
                    arguments: r#"{"q":"rust"}"#.into(),
                },
            }]),
        };
        let conv = kimi::to_ir(&[msg]);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { name, .. } => assert_eq!(name, "web_search"),
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn tool_result() {
        let msg = KimiMessage {
            role: "tool".into(),
            content: Some("results".into()),
            tool_call_id: Some("c1".into()),
            tool_calls: None,
        };
        let conv = kimi::to_ir(&[msg]);
        assert_eq!(conv.messages[0].role, IrRole::Tool);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult { tool_use_id, .. } => {
                assert_eq!(tool_use_id, "c1");
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn usage_conversion() {
        let usage = KimiUsage {
            prompt_tokens: 200,
            completion_tokens: 80,
            total_tokens: 280,
        };
        let ir = kimi::usage_to_ir(&usage);
        assert_eq!(ir.input_tokens, 200);
        assert_eq!(ir.output_tokens, 80);
        assert_eq!(ir.total_tokens, 280);
    }
}

mod copilot_to_ir {
    use super::*;

    #[test]
    fn system_message() {
        let conv = copilot::to_ir(&[copilot_text("system", "Be concise")]);
        assert_eq!(conv.messages[0].role, IrRole::System);
    }

    #[test]
    fn user_message() {
        let conv = copilot::to_ir(&[copilot_text("user", "Hi")]);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    #[test]
    fn assistant_message() {
        let conv = copilot::to_ir(&[copilot_text("assistant", "Hello!")]);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
    }

    #[test]
    fn references_stored_in_metadata() {
        let msg = CopilotMessage {
            role: "user".into(),
            content: "Check this".into(),
            name: None,
            copilot_references: vec![CopilotReference {
                ref_type: CopilotReferenceType::File,
                id: "f1".into(),
                data: json!({"path": "main.rs"}),
                metadata: None,
            }],
        };
        let conv = copilot::to_ir(&[msg]);
        assert!(conv.messages[0].metadata.contains_key("copilot_references"));
    }

    #[test]
    fn name_stored_in_metadata() {
        let msg = CopilotMessage {
            role: "user".into(),
            content: "Hi".into(),
            name: Some("alice".into()),
            copilot_references: vec![],
        };
        let conv = copilot::to_ir(&[msg]);
        assert_eq!(
            conv.messages[0]
                .metadata
                .get("copilot_name")
                .and_then(|v| v.as_str()),
            Some("alice")
        );
    }

    #[test]
    fn unknown_role_maps_to_user() {
        let conv = copilot::to_ir(&[copilot_text("developer", "hi")]);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. LIFTING: IR → SDK
// ═══════════════════════════════════════════════════════════════════════

mod ir_to_openai {
    use super::*;

    #[test]
    fn system_message() {
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::System, "Hi")]);
        let msgs = openai::from_ir(&conv);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[0].content.as_deref(), Some("Hi"));
    }

    #[test]
    fn user_message() {
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "Hello")]);
        let msgs = openai::from_ir(&conv);
        assert_eq!(msgs[0].role, "user");
    }

    #[test]
    fn assistant_with_tool_use() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "read".into(),
                input: json!({"p": "x"}),
            }],
        )]);
        let msgs = openai::from_ir(&conv);
        let tc = &msgs[0].tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.id, "c1");
        assert_eq!(tc.function.name, "read");
    }

    #[test]
    fn tool_result_message() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![IrContentBlock::Text {
                    text: "data".into(),
                }],
                is_error: false,
            }],
        )]);
        let msgs = openai::from_ir(&conv);
        assert_eq!(msgs[0].role, "tool");
        assert_eq!(msgs[0].tool_call_id.as_deref(), Some("c1"));
        assert_eq!(msgs[0].content.as_deref(), Some("data"));
    }

    #[test]
    fn thinking_block_becomes_text() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "reasoning".into(),
            }],
        )]);
        let msgs = openai::from_ir(&conv);
        assert_eq!(msgs[0].content.as_deref(), Some("reasoning"));
    }
}

mod ir_to_claude {
    use super::*;

    #[test]
    fn system_messages_skipped() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "instructions"),
            IrMessage::text(IrRole::User, "hi"),
        ]);
        let msgs = claude::from_ir(&conv);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
    }

    #[test]
    fn system_prompt_extraction() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be helpful"),
            IrMessage::text(IrRole::User, "hi"),
        ]);
        let sys = claude::extract_system_prompt(&conv);
        assert_eq!(sys.as_deref(), Some("Be helpful"));
    }

    #[test]
    fn tool_use_serialized_as_json() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "search".into(),
                input: json!({"q": "r"}),
            }],
        )]);
        let msgs = claude::from_ir(&conv);
        let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&msgs[0].content).unwrap();
        assert!(matches!(&blocks[0], ClaudeContentBlock::ToolUse { .. }));
    }

    #[test]
    fn tool_result_ir_to_claude() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text { text: "ok".into() }],
                is_error: false,
            }],
        )]);
        let msgs = claude::from_ir(&conv);
        let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&msgs[0].content).unwrap();
        match &blocks[0] {
            ClaudeContentBlock::ToolResult {
                tool_use_id,
                content,
                ..
            } => {
                assert_eq!(tool_use_id, "t1");
                assert_eq!(content.as_deref(), Some("ok"));
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn thinking_preserved() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking { text: "hmm".into() }],
        )]);
        let msgs = claude::from_ir(&conv);
        let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&msgs[0].content).unwrap();
        match &blocks[0] {
            ClaudeContentBlock::Thinking { thinking, .. } => assert_eq!(thinking, "hmm"),
            other => panic!("expected Thinking, got {other:?}"),
        }
    }

    #[test]
    fn image_becomes_base64_block() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "abc".into(),
            }],
        )]);
        let msgs = claude::from_ir(&conv);
        let blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&msgs[0].content).unwrap();
        match &blocks[0] {
            ClaudeContentBlock::Image {
                source: ClaudeImageSource::Base64 { media_type, data },
            } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "abc");
            }
            other => panic!("expected Image, got {other:?}"),
        }
    }
}

mod ir_to_gemini {
    use super::*;

    #[test]
    fn system_messages_skipped() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "instructions"),
            IrMessage::text(IrRole::User, "hi"),
        ]);
        let contents = gemini::from_ir(&conv);
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0].role, "user");
    }

    #[test]
    fn system_instruction_extraction() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be concise"),
            IrMessage::text(IrRole::User, "hi"),
        ]);
        let sys = gemini::extract_system_instruction(&conv).unwrap();
        match &sys.parts[0] {
            GeminiPart::Text(t) => assert_eq!(t, "Be concise"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn assistant_maps_to_model_role() {
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::Assistant, "Hi")]);
        let contents = gemini::from_ir(&conv);
        assert_eq!(contents[0].role, "model");
    }

    #[test]
    fn tool_use_becomes_function_call() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "gemini_search".into(),
                name: "search".into(),
                input: json!({"q": "r"}),
            }],
        )]);
        let contents = gemini::from_ir(&conv);
        match &contents[0].parts[0] {
            GeminiPart::FunctionCall { name, args } => {
                assert_eq!(name, "search");
                assert_eq!(args, &json!({"q": "r"}));
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_becomes_function_response() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "gemini_read".into(),
                content: vec![IrContentBlock::Text {
                    text: "data".into(),
                }],
                is_error: false,
            }],
        )]);
        let contents = gemini::from_ir(&conv);
        match &contents[0].parts[0] {
            GeminiPart::FunctionResponse { name, response } => {
                assert_eq!(name, "read");
                assert_eq!(response, &json!("data"));
            }
            other => panic!("expected FunctionResponse, got {other:?}"),
        }
    }

    #[test]
    fn thinking_becomes_text() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "reasoning".into(),
            }],
        )]);
        let contents = gemini::from_ir(&conv);
        match &contents[0].parts[0] {
            GeminiPart::Text(t) => assert_eq!(t, "reasoning"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn image_becomes_inline_data() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "xyz".into(),
            }],
        )]);
        let contents = gemini::from_ir(&conv);
        match &contents[0].parts[0] {
            GeminiPart::InlineData(d) => {
                assert_eq!(d.mime_type, "image/png");
                assert_eq!(d.data, "xyz");
            }
            other => panic!("expected InlineData, got {other:?}"),
        }
    }
}

mod ir_to_codex {
    use super::*;

    #[test]
    fn system_and_user_skipped() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "inst"),
            IrMessage::text(IrRole::User, "hi"),
            IrMessage::text(IrRole::Assistant, "hello"),
        ]);
        let items = codex::from_ir(&conv);
        assert_eq!(items.len(), 1);
        assert!(matches!(&items[0], CodexResponseItem::Message { .. }));
    }

    #[test]
    fn tool_use_becomes_function_call() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "fc1".into(),
                name: "shell".into(),
                input: json!({"cmd": "ls"}),
            }],
        )]);
        let items = codex::from_ir(&conv);
        match &items[0] {
            CodexResponseItem::FunctionCall { id, name, .. } => {
                assert_eq!(id, "fc1");
                assert_eq!(name, "shell");
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_becomes_function_call_output() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "fc1".into(),
                content: vec![IrContentBlock::Text {
                    text: "output".into(),
                }],
                is_error: false,
            }],
        )]);
        let items = codex::from_ir(&conv);
        match &items[0] {
            CodexResponseItem::FunctionCallOutput { call_id, output } => {
                assert_eq!(call_id, "fc1");
                assert_eq!(output, "output");
            }
            other => panic!("expected FunctionCallOutput, got {other:?}"),
        }
    }

    #[test]
    fn thinking_becomes_reasoning() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "reasoning...".into(),
            }],
        )]);
        let items = codex::from_ir(&conv);
        match &items[0] {
            CodexResponseItem::Reasoning { summary } => {
                assert_eq!(summary[0].text, "reasoning...");
            }
            other => panic!("expected Reasoning, got {other:?}"),
        }
    }

    #[test]
    fn mixed_text_and_tool_use_splits() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "let me check".into(),
                },
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "read".into(),
                    input: json!({}),
                },
            ],
        )]);
        let items = codex::from_ir(&conv);
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[0], CodexResponseItem::Message { .. }));
        assert!(matches!(&items[1], CodexResponseItem::FunctionCall { .. }));
    }
}

mod ir_to_kimi {
    use super::*;

    #[test]
    fn system_message() {
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::System, "inst")]);
        let msgs = kimi::from_ir(&conv);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[0].content.as_deref(), Some("inst"));
    }

    #[test]
    fn tool_use_becomes_tool_calls() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "search".into(),
                input: json!({"q": "r"}),
            }],
        )]);
        let msgs = kimi::from_ir(&conv);
        let tc = &msgs[0].tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.id, "c1");
        assert_eq!(tc.function.name, "search");
    }

    #[test]
    fn tool_result_message() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![IrContentBlock::Text {
                    text: "data".into(),
                }],
                is_error: false,
            }],
        )]);
        let msgs = kimi::from_ir(&conv);
        assert_eq!(msgs[0].role, "tool");
        assert_eq!(msgs[0].tool_call_id.as_deref(), Some("c1"));
        assert_eq!(msgs[0].content.as_deref(), Some("data"));
    }
}

mod ir_to_copilot {
    use super::*;

    #[test]
    fn all_roles_mapped() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "sys"),
            IrMessage::text(IrRole::User, "usr"),
            IrMessage::text(IrRole::Assistant, "asst"),
        ]);
        let msgs = copilot::from_ir(&conv);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[1].role, "user");
        assert_eq!(msgs[2].role, "assistant");
    }

    #[test]
    fn tool_role_mapped_to_user() {
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
        let msgs = copilot::from_ir(&conv);
        assert_eq!(msgs[0].role, "user");
    }

    #[test]
    fn thinking_becomes_text() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "reasoning".into(),
            }],
        )]);
        let msgs = copilot::from_ir(&conv);
        assert_eq!(msgs[0].content, "reasoning");
    }

    #[test]
    fn references_restored_from_metadata() {
        let msg = CopilotMessage {
            role: "user".into(),
            content: "Check".into(),
            name: None,
            copilot_references: vec![CopilotReference {
                ref_type: CopilotReferenceType::File,
                id: "f1".into(),
                data: json!({"path": "a.rs"}),
                metadata: None,
            }],
        };
        let conv = copilot::to_ir(&[msg]);
        let back = copilot::from_ir(&conv);
        assert_eq!(back[0].copilot_references.len(), 1);
        assert_eq!(back[0].copilot_references[0].id, "f1");
    }

    #[test]
    fn extract_references_works() {
        let msgs = vec![
            CopilotMessage {
                role: "user".into(),
                content: "a".into(),
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
                content: "b".into(),
                name: None,
                copilot_references: vec![CopilotReference {
                    ref_type: CopilotReferenceType::Snippet,
                    id: "s1".into(),
                    data: json!({}),
                    metadata: None,
                }],
            },
        ];
        let conv = copilot::to_ir(&msgs);
        let refs = copilot::extract_references(&conv);
        assert_eq!(refs.len(), 2);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. CROSS-SDK ROUNDTRIP (SDK A → IR → SDK B)
// ═══════════════════════════════════════════════════════════════════════

mod cross_sdk_roundtrip {
    use super::*;

    #[test]
    fn openai_to_claude_text() {
        let ir = openai::to_ir(&[
            openai_text("system", "Be nice"),
            openai_text("user", "Hello"),
            openai_text("assistant", "Hi!"),
        ]);

        let sys = claude::extract_system_prompt(&ir);
        assert_eq!(sys.as_deref(), Some("Be nice"));

        let claude_msgs = claude::from_ir(&ir);
        assert_eq!(claude_msgs.len(), 2); // system skipped
        assert_eq!(claude_msgs[0].role, "user");
        assert_eq!(claude_msgs[0].content, "Hello");
        assert_eq!(claude_msgs[1].role, "assistant");
        assert_eq!(claude_msgs[1].content, "Hi!");
    }

    #[test]
    fn openai_to_gemini_text() {
        let ir = openai::to_ir(&[
            openai_text("system", "Be concise"),
            openai_text("user", "Hello"),
            openai_text("assistant", "Hi!"),
        ]);

        let sys = gemini::extract_system_instruction(&ir).unwrap();
        let gemini_msgs = gemini::from_ir(&ir);
        assert_eq!(gemini_msgs.len(), 2); // system skipped
        assert_eq!(gemini_msgs[0].role, "user");
        assert_eq!(gemini_msgs[1].role, "model");
        match &sys.parts[0] {
            GeminiPart::Text(t) => assert_eq!(t, "Be concise"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn openai_to_kimi_text() {
        let ir = openai::to_ir(&[
            openai_text("system", "Be nice"),
            openai_text("user", "Hello"),
        ]);
        let kimi_msgs = kimi::from_ir(&ir);
        assert_eq!(kimi_msgs.len(), 2);
        assert_eq!(kimi_msgs[0].role, "system");
        assert_eq!(kimi_msgs[0].content.as_deref(), Some("Be nice"));
        assert_eq!(kimi_msgs[1].role, "user");
    }

    #[test]
    fn openai_to_copilot_text() {
        let ir = openai::to_ir(&[
            openai_text("user", "Hello"),
            openai_text("assistant", "Hi!"),
        ]);
        let copilot_msgs = copilot::from_ir(&ir);
        assert_eq!(copilot_msgs.len(), 2);
        assert_eq!(copilot_msgs[0].content, "Hello");
        assert_eq!(copilot_msgs[1].content, "Hi!");
    }

    #[test]
    fn openai_to_codex_text() {
        let ir = openai::to_ir(&[openai_text("assistant", "Done!")]);
        let codex_items = codex::from_ir(&ir);
        assert_eq!(codex_items.len(), 1);
        match &codex_items[0] {
            CodexResponseItem::Message { content, .. } => match &content[0] {
                CodexContentPart::OutputText { text } => assert_eq!(text, "Done!"),
            },
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn claude_to_openai_text() {
        let ir = claude::to_ir(
            &[claude_text("user", "Hi"), claude_text("assistant", "Hey!")],
            Some("Be helpful"),
        );
        let openai_msgs = openai::from_ir(&ir);
        assert_eq!(openai_msgs.len(), 3);
        assert_eq!(openai_msgs[0].role, "system");
        assert_eq!(openai_msgs[0].content.as_deref(), Some("Be helpful"));
        assert_eq!(openai_msgs[1].role, "user");
        assert_eq!(openai_msgs[2].role, "assistant");
    }

    #[test]
    fn claude_to_gemini_text() {
        let ir = claude::to_ir(
            &[claude_text("user", "Hi"), claude_text("assistant", "Hey!")],
            Some("Be nice"),
        );
        let sys = gemini::extract_system_instruction(&ir).unwrap();
        let gemini_msgs = gemini::from_ir(&ir);
        assert_eq!(gemini_msgs.len(), 2);
        assert_eq!(gemini_msgs[1].role, "model");
        match &sys.parts[0] {
            GeminiPart::Text(t) => assert_eq!(t, "Be nice"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn gemini_to_openai_text() {
        let sys = gemini_text("user", "Instructions");
        let ir = gemini::to_ir(
            &[gemini_text("user", "Hi"), gemini_text("model", "Hey!")],
            Some(&sys),
        );
        let openai_msgs = openai::from_ir(&ir);
        assert_eq!(openai_msgs.len(), 3);
        assert_eq!(openai_msgs[0].role, "system");
        assert_eq!(openai_msgs[1].role, "user");
        assert_eq!(openai_msgs[2].role, "assistant");
    }

    #[test]
    fn gemini_to_claude_text() {
        let ir = gemini::to_ir(
            &[gemini_text("user", "Hello"), gemini_text("model", "Hi!")],
            None,
        );
        let claude_msgs = claude::from_ir(&ir);
        assert_eq!(claude_msgs.len(), 2);
        assert_eq!(claude_msgs[0].role, "user");
        assert_eq!(claude_msgs[1].role, "assistant");
    }

    #[test]
    fn kimi_to_openai_text() {
        let ir = kimi::to_ir(&[
            kimi_text("system", "Be nice"),
            kimi_text("user", "Hi"),
            kimi_text("assistant", "Hello!"),
        ]);
        let openai_msgs = openai::from_ir(&ir);
        assert_eq!(openai_msgs.len(), 3);
        assert_eq!(openai_msgs[0].role, "system");
        assert_eq!(openai_msgs[1].role, "user");
        assert_eq!(openai_msgs[2].role, "assistant");
    }

    #[test]
    fn copilot_to_openai_text() {
        let ir = copilot::to_ir(&[
            copilot_text("system", "Be nice"),
            copilot_text("user", "Hi"),
            copilot_text("assistant", "Hello!"),
        ]);
        let openai_msgs = openai::from_ir(&ir);
        assert_eq!(openai_msgs.len(), 3);
        assert_eq!(openai_msgs[0].role, "system");
    }

    #[test]
    fn openai_tool_call_to_claude() {
        let msg = OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![openai_tool_call("c1", "read", r#"{"p":"x"}"#)]),
            tool_call_id: None,
        };
        let ir = openai::to_ir(&[msg]);
        let claude_msgs = claude::from_ir(&ir);
        let blocks: Vec<ClaudeContentBlock> =
            serde_json::from_str(&claude_msgs[0].content).unwrap();
        match &blocks[0] {
            ClaudeContentBlock::ToolUse { id, name, .. } => {
                assert_eq!(id, "c1");
                assert_eq!(name, "read");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn openai_tool_call_to_gemini() {
        let msg = OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![openai_tool_call("c1", "search", r#"{"q":"r"}"#)]),
            tool_call_id: None,
        };
        let ir = openai::to_ir(&[msg]);
        let gemini_msgs = gemini::from_ir(&ir);
        match &gemini_msgs[0].parts[0] {
            GeminiPart::FunctionCall { name, .. } => assert_eq!(name, "search"),
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn openai_tool_call_to_codex() {
        let msg = OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![openai_tool_call("c1", "shell", r#"{"cmd":"ls"}"#)]),
            tool_call_id: None,
        };
        let ir = openai::to_ir(&[msg]);
        let codex_items = codex::from_ir(&ir);
        match &codex_items[0] {
            CodexResponseItem::FunctionCall { name, .. } => assert_eq!(name, "shell"),
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn openai_tool_call_to_kimi() {
        let msg = OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![openai_tool_call("c1", "search", r#"{"q":"r"}"#)]),
            tool_call_id: None,
        };
        let ir = openai::to_ir(&[msg]);
        let kimi_msgs = kimi::from_ir(&ir);
        let tc = &kimi_msgs[0].tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.id, "c1");
        assert_eq!(tc.function.name, "search");
    }

    #[test]
    fn claude_thinking_to_codex_reasoning() {
        let blocks = vec![
            ClaudeContentBlock::Thinking {
                thinking: "let me think".into(),
                signature: None,
            },
            ClaudeContentBlock::Text {
                text: "Answer".into(),
            },
        ];
        let ir = claude::to_ir(&[claude_blocks("assistant", blocks)], None);
        let codex_items = codex::from_ir(&ir);
        assert!(
            codex_items
                .iter()
                .any(|i| matches!(i, CodexResponseItem::Reasoning { .. }))
        );
        assert!(
            codex_items
                .iter()
                .any(|i| matches!(i, CodexResponseItem::Message { .. }))
        );
    }

    #[test]
    fn codex_reasoning_to_claude_thinking() {
        let ir = codex::to_ir(&[CodexResponseItem::Reasoning {
            summary: vec![ReasoningSummary {
                text: "reasoning...".into(),
            }],
        }]);
        let claude_msgs = claude::from_ir(&ir);
        let blocks: Vec<ClaudeContentBlock> =
            serde_json::from_str(&claude_msgs[0].content).unwrap();
        match &blocks[0] {
            ClaudeContentBlock::Thinking { thinking, .. } => {
                assert_eq!(thinking, "reasoning...");
            }
            other => panic!("expected Thinking, got {other:?}"),
        }
    }

    #[test]
    fn gemini_function_call_to_openai_tool_call() {
        let content = GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "search".into(),
                args: json!({"q": "rust"}),
            }],
        };
        let ir = gemini::to_ir(&[content], None);
        let openai_msgs = openai::from_ir(&ir);
        let tc = &openai_msgs[0].tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.function.name, "search");
    }

    #[test]
    fn kimi_tool_call_to_claude() {
        let msg = KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: Some(vec![KimiToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "web_search".into(),
                    arguments: r#"{"q":"rust"}"#.into(),
                },
            }]),
        };
        let ir = kimi::to_ir(&[msg]);
        let claude_msgs = claude::from_ir(&ir);
        let blocks: Vec<ClaudeContentBlock> =
            serde_json::from_str(&claude_msgs[0].content).unwrap();
        match &blocks[0] {
            ClaudeContentBlock::ToolUse { id, name, .. } => {
                assert_eq!(id, "c1");
                assert_eq!(name, "web_search");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. FIDELITY LOSS TRACKING
// ═══════════════════════════════════════════════════════════════════════

mod fidelity_tracking {
    use super::*;

    #[test]
    fn openai_to_copilot_tool_use_lost() {
        // Copilot has no tool_calls field — tool-use info flattened to text
        let msg = OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![openai_tool_call("c1", "read", r#"{"p":"x"}"#)]),
            tool_call_id: None,
        };
        let ir = openai::to_ir(&[msg]);
        let copilot_msgs = copilot::from_ir(&ir);
        // Tool use info is lost (becomes empty text since no Text blocks)
        assert_eq!(copilot_msgs[0].role, "assistant");
        // Content won't contain structured tool call info
        assert!(copilot_msgs[0].copilot_references.is_empty());
    }

    #[test]
    fn claude_thinking_signature_lost_in_ir() {
        let blocks = vec![ClaudeContentBlock::Thinking {
            thinking: "hmm".into(),
            signature: Some("sig123".into()),
        }];
        let ir = claude::to_ir(&[claude_blocks("assistant", blocks)], None);
        // IR only stores the text, not the signature
        match &ir.messages[0].content[0] {
            IrContentBlock::Thinking { text } => assert_eq!(text, "hmm"),
            other => panic!("expected Thinking, got {other:?}"),
        }
        let back = claude::from_ir(&ir);
        let back_blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
        match &back_blocks[0] {
            ClaudeContentBlock::Thinking { signature, .. } => {
                assert!(signature.is_none()); // signature lost
            }
            other => panic!("expected Thinking, got {other:?}"),
        }
    }

    #[test]
    fn claude_image_url_lossy_to_text() {
        let blocks = vec![ClaudeContentBlock::Image {
            source: ClaudeImageSource::Url {
                url: "https://example.com/img.png".into(),
            },
        }];
        let ir = claude::to_ir(&[claude_blocks("user", blocks)], None);
        // URL images become text placeholders — fidelity loss
        match &ir.messages[0].content[0] {
            IrContentBlock::Text { text } => {
                assert!(text.contains("example.com/img.png"));
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn gemini_synthesized_id_convention() {
        // Gemini doesn't have per-call IDs; they're synthesized
        let content = GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "search".into(),
                args: json!({}),
            }],
        };
        let ir = gemini::to_ir(&[content], None);
        match &ir.messages[0].content[0] {
            IrContentBlock::ToolUse { id, .. } => {
                assert!(id.starts_with("gemini_"));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn codex_call_id_lost_in_roundtrip() {
        let items = vec![CodexResponseItem::FunctionCall {
            id: "fc_1".into(),
            call_id: Some("corr_123".into()),
            name: "read".into(),
            arguments: "{}".into(),
        }];
        let ir = codex::to_ir(&items);
        let back = codex::from_ir(&ir);
        match &back[0] {
            CodexResponseItem::FunctionCall { call_id, .. } => {
                assert!(call_id.is_none()); // correlation id lost
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn copilot_references_lost_in_openai() {
        let msg = CopilotMessage {
            role: "user".into(),
            content: "Check file".into(),
            name: None,
            copilot_references: vec![CopilotReference {
                ref_type: CopilotReferenceType::File,
                id: "f1".into(),
                data: json!({"path": "a.rs"}),
                metadata: None,
            }],
        };
        let ir = copilot::to_ir(&[msg]);
        // References are in metadata, but OpenAI doesn't know about them
        let openai_msgs = openai::from_ir(&ir);
        assert_eq!(openai_msgs[0].role, "user");
        assert_eq!(openai_msgs[0].content.as_deref(), Some("Check file"));
        // No way to represent references in OpenAI format
    }

    #[test]
    fn copilot_name_lost_in_other_sdks() {
        let msg = CopilotMessage {
            role: "user".into(),
            content: "Hi".into(),
            name: Some("alice".into()),
            copilot_references: vec![],
        };
        let ir = copilot::to_ir(&[msg]);
        let openai_msgs = openai::from_ir(&ir);
        // OpenAI message has no name field in our struct
        assert_eq!(openai_msgs[0].role, "user");
    }

    #[test]
    fn thinking_lost_in_copilot_roundtrip() {
        // Claude thinking → IR → Copilot → IR → Claude: thinking becomes plain text
        let blocks = vec![ClaudeContentBlock::Thinking {
            thinking: "reasoning".into(),
            signature: None,
        }];
        let ir = claude::to_ir(&[claude_blocks("assistant", blocks)], None);
        let copilot_msgs = copilot::from_ir(&ir);
        assert_eq!(copilot_msgs[0].content, "reasoning");

        // Going back: Copilot text → IR → Claude = plain text, not thinking
        let ir2 = copilot::to_ir(&copilot_msgs);
        let claude_back = claude::from_ir(&ir2);
        // Now it's plain text, not a structured thinking block
        assert_eq!(claude_back[0].content, "reasoning");
    }

    #[test]
    fn mapping_registry_fidelity_rules() {
        let mut reg = MappingRegistry::new();
        reg.insert(MappingRule {
            source_dialect: Dialect::OpenAi,
            target_dialect: Dialect::Claude,
            feature: "tool_use".into(),
            fidelity: Fidelity::Lossless,
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::Copilot,
            feature: "thinking".into(),
            fidelity: Fidelity::LossyLabeled {
                warning: "Thinking blocks become plain text".into(),
            },
        });
        reg.insert(MappingRule {
            source_dialect: Dialect::Claude,
            target_dialect: Dialect::Copilot,
            feature: "tool_use".into(),
            fidelity: Fidelity::Unsupported {
                reason: "Copilot has no tool use".into(),
            },
        });

        let tool_rule = reg
            .lookup(Dialect::OpenAi, Dialect::Claude, "tool_use")
            .unwrap();
        assert!(tool_rule.fidelity.is_lossless());

        let think_rule = reg
            .lookup(Dialect::Claude, Dialect::Copilot, "thinking")
            .unwrap();
        assert!(!think_rule.fidelity.is_lossless());

        let unsup = reg
            .lookup(Dialect::Claude, Dialect::Copilot, "tool_use")
            .unwrap();
        assert!(unsup.fidelity.is_unsupported());
    }

    #[test]
    fn mapping_error_types() {
        let err = MappingError::FeatureUnsupported {
            feature: "logprobs".into(),
            from: Dialect::Claude,
            to: Dialect::Gemini,
        };
        assert!(err.to_string().contains("logprobs"));

        let err2 = MappingError::FidelityLoss {
            feature: "thinking".into(),
            warning: "becomes text".into(),
        };
        assert!(err2.to_string().contains("thinking"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. EDGE CASES
// ═══════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;

    // ── Empty messages ──────────────────────────────────────────────

    #[test]
    fn openai_empty() {
        let conv = openai::to_ir(&[]);
        assert!(conv.is_empty());
        assert!(openai::from_ir(&conv).is_empty());
    }

    #[test]
    fn claude_empty() {
        let conv = claude::to_ir(&[], None);
        assert!(conv.is_empty());
        assert!(claude::from_ir(&conv).is_empty());
    }

    #[test]
    fn gemini_empty() {
        let conv = gemini::to_ir(&[], None);
        assert!(conv.is_empty());
        assert!(gemini::from_ir(&conv).is_empty());
    }

    #[test]
    fn codex_empty() {
        let conv = codex::to_ir(&[]);
        assert!(conv.is_empty());
        assert!(codex::from_ir(&conv).is_empty());
    }

    #[test]
    fn codex_input_empty() {
        let conv = codex::input_to_ir(&[]);
        assert!(conv.is_empty());
    }

    #[test]
    fn kimi_empty() {
        let conv = kimi::to_ir(&[]);
        assert!(conv.is_empty());
        assert!(kimi::from_ir(&conv).is_empty());
    }

    #[test]
    fn copilot_empty() {
        let conv = copilot::to_ir(&[]);
        assert!(conv.is_empty());
        assert!(copilot::from_ir(&conv).is_empty());
    }

    // ── Empty content ───────────────────────────────────────────────

    #[test]
    fn openai_none_content() {
        let msg = OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: None,
            tool_call_id: None,
        };
        let conv = openai::to_ir(&[msg]);
        assert!(conv.messages[0].content.is_empty());
    }

    #[test]
    fn openai_empty_string_content() {
        let conv = openai::to_ir(&[openai_text("user", "")]);
        assert!(conv.messages[0].content.is_empty());
    }

    #[test]
    fn claude_empty_content() {
        let conv = claude::to_ir(&[claude_text("user", "")], None);
        assert_eq!(conv.messages[0].text_content(), "");
    }

    #[test]
    fn kimi_none_content() {
        let msg = KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: None,
        };
        let conv = kimi::to_ir(&[msg]);
        assert!(conv.messages[0].content.is_empty());
    }

    #[test]
    fn copilot_empty_content() {
        let msg = CopilotMessage {
            role: "user".into(),
            content: String::new(),
            name: None,
            copilot_references: vec![],
        };
        let conv = copilot::to_ir(&[msg]);
        assert!(conv.messages[0].content.is_empty());
    }

    #[test]
    fn codex_input_empty_content() {
        let conv = codex::input_to_ir(&[CodexInputItem::Message {
            role: "user".into(),
            content: String::new(),
        }]);
        assert!(conv.messages[0].content.is_empty());
    }

    // ── Tool-heavy conversations ────────────────────────────────────

    #[test]
    fn openai_many_tool_calls_roundtrip() {
        let calls: Vec<OpenAIToolCall> = (0..10)
            .map(|i| openai_tool_call(&format!("c{i}"), &format!("tool_{i}"), "{}"))
            .collect();
        let msg = OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(calls),
            tool_call_id: None,
        };
        let ir = openai::to_ir(&[msg]);
        assert_eq!(ir.messages[0].content.len(), 10);
        let back = openai::from_ir(&ir);
        assert_eq!(back[0].tool_calls.as_ref().unwrap().len(), 10);
    }

    #[test]
    fn kimi_many_tool_calls_roundtrip() {
        let calls: Vec<KimiToolCall> = (0..5)
            .map(|i| KimiToolCall {
                id: format!("c{i}"),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: format!("tool_{i}"),
                    arguments: "{}".into(),
                },
            })
            .collect();
        let msg = KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: Some(calls),
        };
        let ir = kimi::to_ir(&[msg]);
        assert_eq!(ir.messages[0].content.len(), 5);
        let back = kimi::from_ir(&ir);
        assert_eq!(back[0].tool_calls.as_ref().unwrap().len(), 5);
    }

    #[test]
    fn multi_turn_tool_conversation_across_sdks() {
        // OpenAI multi-turn tool conversation → IR → Claude
        let openai_msgs = vec![
            openai_text("user", "Search for rust"),
            OpenAIMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![openai_tool_call("c1", "search", r#"{"q":"rust"}"#)]),
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "tool".into(),
                content: Some("found 10 results".into()),
                tool_calls: None,
                tool_call_id: Some("c1".into()),
            },
            openai_text("assistant", "Found 10 results for rust."),
        ];
        let ir = openai::to_ir(&openai_msgs);
        assert_eq!(ir.len(), 4);

        let claude_msgs = claude::from_ir(&ir);
        assert_eq!(claude_msgs.len(), 4); // no system to skip
        assert_eq!(claude_msgs[0].role, "user");
        assert_eq!(claude_msgs[1].role, "assistant");
        // Tool result mapped to user in Claude
        assert_eq!(claude_msgs[2].role, "user");
        assert_eq!(claude_msgs[3].role, "assistant");
    }

    // ── Gemini multi-part content ───────────────────────────────────

    #[test]
    fn gemini_multi_part_roundtrip() {
        let content = GeminiContent {
            role: "model".into(),
            parts: vec![
                GeminiPart::Text("Let me search.".into()),
                GeminiPart::FunctionCall {
                    name: "search".into(),
                    args: json!({"q": "rust"}),
                },
            ],
        };
        let ir = gemini::to_ir(&[content], None);
        assert_eq!(ir.messages[0].content.len(), 2);
        let back = gemini::from_ir(&ir);
        assert_eq!(back[0].parts.len(), 2);
    }

    // ── Mixed block types ───────────────────────────────────────────

    #[test]
    fn claude_mixed_blocks_roundtrip() {
        let blocks = vec![
            ClaudeContentBlock::Text {
                text: "Here:".into(),
            },
            ClaudeContentBlock::ToolUse {
                id: "t1".into(),
                name: "search".into(),
                input: json!({"q": "rust"}),
            },
        ];
        let ir = claude::to_ir(&[claude_blocks("assistant", blocks)], None);
        assert_eq!(ir.messages[0].content.len(), 2);
        let back = claude::from_ir(&ir);
        let back_blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
        assert_eq!(back_blocks.len(), 2);
    }

    #[test]
    fn claude_tool_result_no_content() {
        let blocks = vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "tu_x".into(),
            content: None,
            is_error: None,
        }];
        let ir = claude::to_ir(&[claude_blocks("user", blocks)], None);
        match &ir.messages[0].content[0] {
            IrContentBlock::ToolResult { content, .. } => assert!(content.is_empty()),
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn claude_tool_result_error_roundtrip() {
        let blocks = vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "tu_err".into(),
            content: Some("not found".into()),
            is_error: Some(true),
        }];
        let ir = claude::to_ir(&[claude_blocks("user", blocks)], None);
        match &ir.messages[0].content[0] {
            IrContentBlock::ToolResult { is_error, .. } => assert!(is_error),
            other => panic!("expected ToolResult, got {other:?}"),
        }
        let back = claude::from_ir(&ir);
        let back_blocks: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
        match &back_blocks[0] {
            ClaudeContentBlock::ToolResult { is_error, .. } => {
                assert_eq!(*is_error, Some(true));
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    // ── OpenAI tool result without content ──────────────────────────

    #[test]
    fn openai_tool_result_no_content() {
        let msg = OpenAIMessage {
            role: "tool".into(),
            content: None,
            tool_calls: None,
            tool_call_id: Some("c1".into()),
        };
        let conv = openai::to_ir(&[msg]);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult { content, .. } => assert!(content.is_empty()),
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    // ── Kimi tool result without content ────────────────────────────

    #[test]
    fn kimi_tool_result_no_content() {
        let msg = KimiMessage {
            role: "tool".into(),
            content: None,
            tool_call_id: Some("c1".into()),
            tool_calls: None,
        };
        let conv = kimi::to_ir(&[msg]);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult { content, .. } => assert!(content.is_empty()),
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    // ── Gemini empty system instruction ─────────────────────────────

    #[test]
    fn gemini_empty_system_instruction_skipped() {
        let sys = GeminiContent {
            role: "user".into(),
            parts: vec![],
        };
        let conv = gemini::to_ir(&[gemini_text("user", "hi")], Some(&sys));
        assert_eq!(conv.len(), 1);
    }

    // ── Gemini function response with object payload ────────────────

    #[test]
    fn gemini_function_response_object_payload() {
        let content = GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::FunctionResponse {
                name: "api".into(),
                response: json!({"status": 200, "body": "ok"}),
            }],
        };
        let conv = gemini::to_ir(&[content], None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult { content, .. } => {
                let text = match &content[0] {
                    IrContentBlock::Text { text } => text.as_str(),
                    _ => panic!("expected text block"),
                };
                assert!(text.contains("200"));
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    // ── IR conversation accessors ───────────────────────────────────

    #[test]
    fn ir_conversation_accessors() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "usr"))
            .push(IrMessage::text(IrRole::Assistant, "asst"));

        assert_eq!(conv.len(), 3);
        assert!(!conv.is_empty());
        assert_eq!(conv.system_message().unwrap().text_content(), "sys");
        assert_eq!(conv.last_assistant().unwrap().text_content(), "asst");
        assert_eq!(conv.last_message().unwrap().text_content(), "asst");
        assert_eq!(conv.messages_by_role(IrRole::User).len(), 1);
    }

    #[test]
    fn ir_message_helpers() {
        let msg = IrMessage::text(IrRole::User, "hello");
        assert!(msg.is_text_only());
        assert_eq!(msg.text_content(), "hello");
        assert!(msg.tool_use_blocks().is_empty());

        let msg2 = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "check".into(),
                },
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "read".into(),
                    input: json!({}),
                },
            ],
        );
        assert!(!msg2.is_text_only());
        assert_eq!(msg2.tool_use_blocks().len(), 1);
    }

    #[test]
    fn ir_usage_operations() {
        let u1 = IrUsage::from_io(100, 50);
        assert_eq!(u1.total_tokens, 150);

        let u2 = IrUsage::with_cache(100, 50, 10, 5);
        assert_eq!(u2.cache_read_tokens, 10);
        assert_eq!(u2.cache_write_tokens, 5);

        let merged = u1.merge(u2);
        assert_eq!(merged.input_tokens, 200);
        assert_eq!(merged.output_tokens, 100);
        assert_eq!(merged.total_tokens, 300);
    }

    // ── Copilot reference types ─────────────────────────────────────

    #[test]
    fn copilot_all_reference_types_roundtrip() {
        use std::collections::BTreeMap;
        let refs = vec![
            CopilotReference {
                ref_type: CopilotReferenceType::File,
                id: "f1".into(),
                data: json!({"path": "a.rs"}),
                metadata: None,
            },
            CopilotReference {
                ref_type: CopilotReferenceType::Snippet,
                id: "s1".into(),
                data: json!({"content": "fn main()"}),
                metadata: Some({
                    let mut m = BTreeMap::new();
                    m.insert("label".into(), json!("main"));
                    m
                }),
            },
            CopilotReference {
                ref_type: CopilotReferenceType::Repository,
                id: "r1".into(),
                data: json!({"owner": "octocat"}),
                metadata: None,
            },
            CopilotReference {
                ref_type: CopilotReferenceType::WebSearchResult,
                id: "w1".into(),
                data: json!({"url": "https://rust-lang.org"}),
                metadata: None,
            },
        ];
        let msg = CopilotMessage {
            role: "user".into(),
            content: "check".into(),
            name: None,
            copilot_references: refs,
        };
        let conv = copilot::to_ir(&[msg]);
        let back = copilot::from_ir(&conv);
        assert_eq!(back[0].copilot_references.len(), 4);
        assert_eq!(
            back[0].copilot_references[0].ref_type,
            CopilotReferenceType::File
        );
        assert_eq!(
            back[0].copilot_references[1].ref_type,
            CopilotReferenceType::Snippet
        );
        assert_eq!(
            back[0].copilot_references[2].ref_type,
            CopilotReferenceType::Repository
        );
        assert_eq!(
            back[0].copilot_references[3].ref_type,
            CopilotReferenceType::WebSearchResult
        );
    }

    // ── Large conversation roundtrip ────────────────────────────────

    #[test]
    fn large_openai_conversation_roundtrip() {
        let msgs: Vec<OpenAIMessage> = (0..50)
            .map(|i| {
                if i % 2 == 0 {
                    openai_text("user", &format!("Message {i}"))
                } else {
                    openai_text("assistant", &format!("Reply {i}"))
                }
            })
            .collect();
        let ir = openai::to_ir(&msgs);
        assert_eq!(ir.len(), 50);
        let back = openai::from_ir(&ir);
        assert_eq!(back.len(), 50);
        assert_eq!(back[0].content.as_deref(), Some("Message 0"));
        assert_eq!(back[49].content.as_deref(), Some("Reply 49"));
    }

    #[test]
    fn large_kimi_conversation_roundtrip() {
        let msgs: Vec<KimiMessage> = (0..50)
            .map(|i| {
                if i % 2 == 0 {
                    kimi_text("user", &format!("Msg {i}"))
                } else {
                    kimi_text("assistant", &format!("Re {i}"))
                }
            })
            .collect();
        let ir = kimi::to_ir(&msgs);
        assert_eq!(ir.len(), 50);
        let back = kimi::from_ir(&ir);
        assert_eq!(back.len(), 50);
    }

    // ── Codex multi-item with reasoning ─────────────────────────────

    #[test]
    fn codex_full_flow_roundtrip() {
        let items = vec![
            CodexResponseItem::Reasoning {
                summary: vec![ReasoningSummary {
                    text: "thinking".into(),
                }],
            },
            CodexResponseItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "Let me run that.".into(),
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
            CodexResponseItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "Found file.txt".into(),
                }],
            },
        ];
        let ir = codex::to_ir(&items);
        assert_eq!(ir.len(), 5);
        let back = codex::from_ir(&ir);
        // Reasoning, message, function_call, function_call_output, message
        assert!(
            back.iter()
                .any(|i| matches!(i, CodexResponseItem::Reasoning { .. }))
        );
        assert!(
            back.iter()
                .any(|i| matches!(i, CodexResponseItem::FunctionCall { .. }))
        );
        assert!(
            back.iter()
                .any(|i| matches!(i, CodexResponseItem::FunctionCallOutput { .. }))
        );
    }

    // ── OpenAI text + tool call in same message ─────────────────────

    #[test]
    fn openai_text_and_tool_call_roundtrip() {
        let msg = OpenAIMessage {
            role: "assistant".into(),
            content: Some("Let me check.".into()),
            tool_calls: Some(vec![openai_tool_call("c1", "search", "{}")]),
            tool_call_id: None,
        };
        let ir = openai::to_ir(&[msg]);
        assert_eq!(ir.messages[0].content.len(), 2);
        let back = openai::from_ir(&ir);
        assert_eq!(back[0].content.as_deref(), Some("Let me check."));
        assert_eq!(back[0].tool_calls.as_ref().unwrap().len(), 1);
    }

    // ── Kimi text + tool call in same message ───────────────────────

    #[test]
    fn kimi_text_and_tool_call_roundtrip() {
        let msg = KimiMessage {
            role: "assistant".into(),
            content: Some("Searching...".into()),
            tool_call_id: None,
            tool_calls: Some(vec![KimiToolCall {
                id: "c1".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "search".into(),
                    arguments: "{}".into(),
                },
            }]),
        };
        let ir = kimi::to_ir(&[msg]);
        assert_eq!(ir.messages[0].content.len(), 2);
        let back = kimi::from_ir(&ir);
        assert_eq!(back[0].content.as_deref(), Some("Searching..."));
        assert!(back[0].tool_calls.is_some());
    }
}
