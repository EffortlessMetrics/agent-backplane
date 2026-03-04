#![allow(clippy::all)]
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
//! Comprehensive tests for the dialect IR (Intermediate Representation) system.
//!
//! Covers IR node construction, dialect lowering/lifting, and cross-dialect
//! semantic equivalence.

use std::collections::BTreeMap;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use serde_json::json;

// ============================================================================
// 1. IR node construction (10+ tests)
// ============================================================================

mod ir_node_construction {
    use super::*;

    #[test]
    fn text_block_construction() {
        let block = IrContentBlock::Text {
            text: "hello".into(),
        };
        match &block {
            IrContentBlock::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn image_block_construction() {
        let block = IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "base64data".into(),
        };
        match &block {
            IrContentBlock::Image {
                media_type, data, ..
            } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "base64data");
            }
            _ => panic!("expected Image variant"),
        }
    }

    #[test]
    fn tool_use_block_construction() {
        let block = IrContentBlock::ToolUse {
            id: "call_1".into(),
            name: "get_weather".into(),
            input: json!({"city": "Seattle"}),
        };
        match &block {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "get_weather");
                assert_eq!(input["city"], "Seattle");
            }
            _ => panic!("expected ToolUse variant"),
        }
    }

    #[test]
    fn tool_result_block_construction() {
        let block = IrContentBlock::ToolResult {
            tool_use_id: "call_1".into(),
            content: vec![IrContentBlock::Text {
                text: "72°F".into(),
            }],
            is_error: false,
        };
        match &block {
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "call_1");
                assert_eq!(content.len(), 1);
                assert!(!is_error);
            }
            _ => panic!("expected ToolResult variant"),
        }
    }

    #[test]
    fn thinking_block_construction() {
        let block = IrContentBlock::Thinking {
            text: "Let me reason...".into(),
        };
        match &block {
            IrContentBlock::Thinking { text } => assert_eq!(text, "Let me reason..."),
            _ => panic!("expected Thinking variant"),
        }
    }

    #[test]
    fn ir_message_text_helper() {
        let msg = IrMessage::text(IrRole::User, "Hello");
        assert_eq!(msg.role, IrRole::User);
        assert!(msg.is_text_only());
        assert_eq!(msg.text_content(), "Hello");
        assert!(msg.metadata.is_empty());
    }

    #[test]
    fn ir_message_new_with_multiple_blocks() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "reasoning".into(),
                },
                IrContentBlock::Text {
                    text: "answer".into(),
                },
            ],
        );
        assert_eq!(msg.role, IrRole::Assistant);
        assert_eq!(msg.content.len(), 2);
        assert!(!msg.is_text_only());
        assert_eq!(msg.text_content(), "answer");
    }

    #[test]
    fn ir_message_tool_use_blocks() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "I'll look that up".into(),
                },
                IrContentBlock::ToolUse {
                    id: "tc_1".into(),
                    name: "search".into(),
                    input: json!({"q": "rust"}),
                },
                IrContentBlock::ToolUse {
                    id: "tc_2".into(),
                    name: "fetch".into(),
                    input: json!({"url": "http://example.com"}),
                },
            ],
        );
        let tool_blocks = msg.tool_use_blocks();
        assert_eq!(tool_blocks.len(), 2);
    }

    #[test]
    fn ir_tool_definition_construction() {
        let tool = IrToolDefinition {
            name: "calculator".into(),
            description: "Performs math".into(),
            parameters: json!({"type": "object", "properties": {"expr": {"type": "string"}}}),
        };
        assert_eq!(tool.name, "calculator");
        assert_eq!(tool.description, "Performs math");
    }

    #[test]
    fn ir_conversation_builder_pattern() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "You are helpful."))
            .push(IrMessage::text(IrRole::User, "Hi"))
            .push(IrMessage::text(IrRole::Assistant, "Hello!"));

        assert_eq!(conv.len(), 3);
        assert!(!conv.is_empty());
        assert!(conv.system_message().is_some());
        assert_eq!(
            conv.system_message().unwrap().text_content(),
            "You are helpful."
        );
        assert_eq!(conv.last_assistant().unwrap().text_content(), "Hello!");
    }

    #[test]
    fn ir_conversation_from_messages() {
        let msgs = vec![
            IrMessage::text(IrRole::User, "a"),
            IrMessage::text(IrRole::Assistant, "b"),
        ];
        let conv = IrConversation::from_messages(msgs);
        assert_eq!(conv.len(), 2);
    }

    #[test]
    fn ir_conversation_messages_by_role() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "one"))
            .push(IrMessage::text(IrRole::Assistant, "two"))
            .push(IrMessage::text(IrRole::User, "three"));
        let user_msgs = conv.messages_by_role(IrRole::User);
        assert_eq!(user_msgs.len(), 2);
    }

    #[test]
    fn ir_conversation_tool_calls() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "tc1".into(),
                name: "fn".into(),
                input: json!({}),
            }],
        ));
        assert_eq!(conv.tool_calls().len(), 1);
    }

    #[test]
    fn ir_usage_from_io() {
        let u = IrUsage::from_io(100, 50);
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 50);
        assert_eq!(u.total_tokens, 150);
        assert_eq!(u.cache_read_tokens, 0);
        assert_eq!(u.cache_write_tokens, 0);
    }

    #[test]
    fn ir_usage_with_cache() {
        let u = IrUsage::with_cache(100, 50, 20, 10);
        assert_eq!(u.total_tokens, 150);
        assert_eq!(u.cache_read_tokens, 20);
        assert_eq!(u.cache_write_tokens, 10);
    }

    #[test]
    fn ir_usage_merge() {
        let a = IrUsage::from_io(100, 50);
        let b = IrUsage::from_io(200, 75);
        let merged = a.merge(b);
        assert_eq!(merged.input_tokens, 300);
        assert_eq!(merged.output_tokens, 125);
        assert_eq!(merged.total_tokens, 425);
    }

    #[test]
    fn ir_content_block_serialize_deterministic() {
        let block = IrContentBlock::ToolUse {
            id: "call_1".into(),
            name: "search".into(),
            input: json!({"a": 1, "b": 2}),
        };
        let json1 = serde_json::to_string(&block).unwrap();
        let json2 = serde_json::to_string(&block).unwrap();
        assert_eq!(json1, json2);
    }

    #[test]
    fn ir_message_roundtrip_serde() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text { text: "hi".into() },
                IrContentBlock::ToolUse {
                    id: "tc".into(),
                    name: "fn".into(),
                    input: json!({"x": 1}),
                },
            ],
        );
        let json = serde_json::to_string(&msg).unwrap();
        let roundtripped: IrMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, roundtripped);
    }

    #[test]
    fn ir_conversation_roundtrip_serde() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "hello"))
            .push(IrMessage::text(IrRole::Assistant, "hi"));
        let json = serde_json::to_string(&conv).unwrap();
        let roundtripped: IrConversation = serde_json::from_str(&json).unwrap();
        assert_eq!(conv, roundtripped);
    }

    #[test]
    fn ir_usage_roundtrip_serde() {
        let usage = IrUsage::with_cache(500, 200, 50, 30);
        let json = serde_json::to_string(&usage).unwrap();
        let roundtripped: IrUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(usage, roundtripped);
    }

    #[test]
    fn ir_role_all_variants_serialize() {
        for (role, expected) in [
            (IrRole::System, "\"system\""),
            (IrRole::User, "\"user\""),
            (IrRole::Assistant, "\"assistant\""),
            (IrRole::Tool, "\"tool\""),
        ] {
            let json = serde_json::to_string(&role).unwrap();
            assert_eq!(json, expected, "role {role:?} serialization mismatch");
        }
    }
}

// ============================================================================
// 2. Dialect lowering — SDK → IR (20+ tests)
// ============================================================================

mod dialect_lowering {
    use super::*;

    // ---- OpenAI → IR ----

    mod openai {
        use super::*;
        use abp_openai_sdk::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};
        use abp_openai_sdk::lowering;

        #[test]
        fn system_message_lowers() {
            let msgs = vec![OpenAIMessage {
                role: "system".into(),
                content: Some("You are a helpful assistant.".into()),
                tool_calls: None,
                tool_call_id: None,
            }];
            let conv = lowering::to_ir(&msgs);
            assert_eq!(conv.len(), 1);
            let msg = &conv.messages[0];
            assert_eq!(msg.role, IrRole::System);
            assert_eq!(msg.text_content(), "You are a helpful assistant.");
        }

        #[test]
        fn user_message_lowers() {
            let msgs = vec![OpenAIMessage {
                role: "user".into(),
                content: Some("Hello".into()),
                tool_calls: None,
                tool_call_id: None,
            }];
            let conv = lowering::to_ir(&msgs);
            assert_eq!(conv.messages[0].role, IrRole::User);
            assert_eq!(conv.messages[0].text_content(), "Hello");
        }

        #[test]
        fn assistant_message_lowers() {
            let msgs = vec![OpenAIMessage {
                role: "assistant".into(),
                content: Some("Hi there".into()),
                tool_calls: None,
                tool_call_id: None,
            }];
            let conv = lowering::to_ir(&msgs);
            assert_eq!(conv.messages[0].role, IrRole::Assistant);
            assert_eq!(conv.messages[0].text_content(), "Hi there");
        }

        #[test]
        fn tool_call_message_lowers() {
            let msgs = vec![OpenAIMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![OpenAIToolCall {
                    id: "call_abc123".into(),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: "get_weather".into(),
                        arguments: r#"{"city":"NYC"}"#.into(),
                    },
                }]),
                tool_call_id: None,
            }];
            let conv = lowering::to_ir(&msgs);
            let blocks = conv.messages[0].tool_use_blocks();
            assert_eq!(blocks.len(), 1);
            match blocks[0] {
                IrContentBlock::ToolUse { id, name, input } => {
                    assert_eq!(id, "call_abc123");
                    assert_eq!(name, "get_weather");
                    assert_eq!(input["city"], "NYC");
                }
                _ => panic!("expected ToolUse"),
            }
        }

        #[test]
        fn tool_result_message_lowers() {
            let msgs = vec![OpenAIMessage {
                role: "tool".into(),
                content: Some("72°F".into()),
                tool_calls: None,
                tool_call_id: Some("call_abc123".into()),
            }];
            let conv = lowering::to_ir(&msgs);
            assert_eq!(conv.messages[0].role, IrRole::Tool);
        }

        #[test]
        fn multi_turn_conversation_ordering() {
            let msgs = vec![
                OpenAIMessage {
                    role: "system".into(),
                    content: Some("sys".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                OpenAIMessage {
                    role: "user".into(),
                    content: Some("q1".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                OpenAIMessage {
                    role: "assistant".into(),
                    content: Some("a1".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                OpenAIMessage {
                    role: "user".into(),
                    content: Some("q2".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            ];
            let conv = lowering::to_ir(&msgs);
            assert_eq!(conv.len(), 4);
            assert_eq!(conv.messages[0].role, IrRole::System);
            assert_eq!(conv.messages[1].role, IrRole::User);
            assert_eq!(conv.messages[2].role, IrRole::Assistant);
            assert_eq!(conv.messages[3].role, IrRole::User);
        }

        #[test]
        fn multiple_tool_calls_in_single_message() {
            let msgs = vec![OpenAIMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![
                    OpenAIToolCall {
                        id: "call_1".into(),
                        call_type: "function".into(),
                        function: OpenAIFunctionCall {
                            name: "fn_a".into(),
                            arguments: "{}".into(),
                        },
                    },
                    OpenAIToolCall {
                        id: "call_2".into(),
                        call_type: "function".into(),
                        function: OpenAIFunctionCall {
                            name: "fn_b".into(),
                            arguments: "{}".into(),
                        },
                    },
                ]),
                tool_call_id: None,
            }];
            let conv = lowering::to_ir(&msgs);
            let tool_blocks = conv.messages[0].tool_use_blocks();
            assert_eq!(tool_blocks.len(), 2);
        }
    }

    // ---- Claude → IR ----

    mod claude {
        use super::*;
        use abp_claude_sdk::dialect::ClaudeMessage;
        use abp_claude_sdk::lowering;

        #[test]
        fn simple_user_message_lowers() {
            let msgs = vec![ClaudeMessage {
                role: "user".into(),
                content: "What is Rust?".into(),
            }];
            let conv = lowering::to_ir(&msgs, None);
            assert_eq!(conv.len(), 1);
            assert_eq!(conv.messages[0].role, IrRole::User);
            assert_eq!(conv.messages[0].text_content(), "What is Rust?");
        }

        #[test]
        fn system_prompt_lowers_as_system_message() {
            let msgs = vec![ClaudeMessage {
                role: "user".into(),
                content: "Hi".into(),
            }];
            let conv = lowering::to_ir(&msgs, Some("You are helpful"));
            let sys = conv.system_message().unwrap();
            assert_eq!(sys.role, IrRole::System);
            assert_eq!(sys.text_content(), "You are helpful");
        }

        #[test]
        fn assistant_message_lowers() {
            let msgs = vec![ClaudeMessage {
                role: "assistant".into(),
                content: "Here's the answer.".into(),
            }];
            let conv = lowering::to_ir(&msgs, None);
            assert_eq!(conv.messages[0].role, IrRole::Assistant);
        }

        #[test]
        fn structured_content_with_tool_use() {
            let content_json = serde_json::to_string(&json!([
                {"type": "text", "text": "Let me check"},
                {"type": "tool_use", "id": "toolu_1", "name": "search", "input": {"q": "rust"}}
            ]))
            .unwrap();
            let msgs = vec![ClaudeMessage {
                role: "assistant".into(),
                content: content_json,
            }];
            let conv = lowering::to_ir(&msgs, None);
            let blocks = &conv.messages[0].content;
            assert!(blocks.len() >= 2);
        }

        #[test]
        fn extract_system_prompt_from_ir() {
            let conv = IrConversation::new()
                .push(IrMessage::text(IrRole::System, "Be concise"))
                .push(IrMessage::text(IrRole::User, "Hi"));
            let sys = lowering::extract_system_prompt(&conv);
            assert_eq!(sys.unwrap(), "Be concise");
        }

        #[test]
        fn multi_turn_claude_conversation() {
            let msgs = vec![
                ClaudeMessage {
                    role: "user".into(),
                    content: "Question 1".into(),
                },
                ClaudeMessage {
                    role: "assistant".into(),
                    content: "Answer 1".into(),
                },
                ClaudeMessage {
                    role: "user".into(),
                    content: "Question 2".into(),
                },
            ];
            let conv = lowering::to_ir(&msgs, Some("System"));
            assert_eq!(conv.len(), 4); // system + 3 messages
            assert_eq!(conv.messages[0].role, IrRole::System);
            assert_eq!(conv.messages[1].text_content(), "Question 1");
            assert_eq!(conv.messages[2].text_content(), "Answer 1");
            assert_eq!(conv.messages[3].text_content(), "Question 2");
        }
    }

    // ---- Gemini → IR ----

    mod gemini {
        use super::*;
        use abp_gemini_sdk::dialect::{GeminiContent, GeminiPart};
        use abp_gemini_sdk::lowering;

        #[test]
        fn user_text_content_lowers() {
            let contents = vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Hello Gemini".into())],
            }];
            let conv = lowering::to_ir(&contents, None);
            assert_eq!(conv.len(), 1);
            assert_eq!(conv.messages[0].role, IrRole::User);
            assert_eq!(conv.messages[0].text_content(), "Hello Gemini");
        }

        #[test]
        fn model_role_maps_to_assistant() {
            let contents = vec![GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("Response".into())],
            }];
            let conv = lowering::to_ir(&contents, None);
            assert_eq!(conv.messages[0].role, IrRole::Assistant);
        }

        #[test]
        fn system_instruction_lowers() {
            let contents = vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Hi".into())],
            }];
            let sys = GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Be helpful".into())],
            };
            let conv = lowering::to_ir(&contents, Some(&sys));
            let sys_msg = conv.system_message().unwrap();
            assert_eq!(sys_msg.role, IrRole::System);
            assert_eq!(sys_msg.text_content(), "Be helpful");
        }

        #[test]
        fn function_call_lowers_to_tool_use() {
            let contents = vec![GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::FunctionCall {
                    name: "get_weather".into(),
                    args: json!({"city": "Tokyo"}),
                }],
            }];
            let conv = lowering::to_ir(&contents, None);
            let blocks = conv.messages[0].tool_use_blocks();
            assert_eq!(blocks.len(), 1);
            match blocks[0] {
                IrContentBlock::ToolUse { name, input, .. } => {
                    assert_eq!(name, "get_weather");
                    assert_eq!(input["city"], "Tokyo");
                }
                _ => panic!("expected ToolUse"),
            }
        }

        #[test]
        fn function_response_lowers_to_tool_result() {
            let contents = vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::FunctionResponse {
                    name: "get_weather".into(),
                    response: json!({"temp": "20°C"}),
                }],
            }];
            let conv = lowering::to_ir(&contents, None);
            let block = &conv.messages[0].content[0];
            match block {
                IrContentBlock::ToolResult { .. } => {}
                _ => panic!("expected ToolResult, got {block:?}"),
            }
        }

        #[test]
        fn inline_data_lowers_to_image() {
            let contents = vec![GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::InlineData(
                    abp_gemini_sdk::dialect::GeminiInlineData {
                        mime_type: "image/jpeg".into(),
                        data: "abc123".into(),
                    },
                )],
            }];
            let conv = lowering::to_ir(&contents, None);
            match &conv.messages[0].content[0] {
                IrContentBlock::Image {
                    media_type, data, ..
                } => {
                    assert_eq!(media_type, "image/jpeg");
                    assert_eq!(data, "abc123");
                }
                other => panic!("expected Image, got {other:?}"),
            }
        }

        #[test]
        fn extract_system_instruction_from_ir() {
            let conv = IrConversation::new()
                .push(IrMessage::text(IrRole::System, "sys text"))
                .push(IrMessage::text(IrRole::User, "hi"));
            let sys = lowering::extract_system_instruction(&conv);
            assert!(sys.is_some());
            let sys_content = sys.unwrap();
            assert_eq!(sys_content.parts.len(), 1);
        }
    }

    // ---- Kimi → IR ----

    mod kimi {
        use super::*;
        use abp_kimi_sdk::dialect::{KimiFunctionCall, KimiMessage, KimiToolCall, KimiUsage};
        use abp_kimi_sdk::lowering;

        #[test]
        fn user_message_lowers() {
            let msgs = vec![KimiMessage {
                role: "user".into(),
                content: Some("Hi Kimi".into()),
                tool_calls: None,
                tool_call_id: None,
            }];
            let conv = lowering::to_ir(&msgs);
            assert_eq!(conv.messages[0].role, IrRole::User);
            assert_eq!(conv.messages[0].text_content(), "Hi Kimi");
        }

        #[test]
        fn system_message_lowers() {
            let msgs = vec![KimiMessage {
                role: "system".into(),
                content: Some("You are Kimi".into()),
                tool_calls: None,
                tool_call_id: None,
            }];
            let conv = lowering::to_ir(&msgs);
            assert_eq!(conv.messages[0].role, IrRole::System);
        }

        #[test]
        fn tool_call_lowers() {
            let msgs = vec![KimiMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![KimiToolCall {
                    id: "kimi_tc_1".into(),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: "search_web".into(),
                        arguments: r#"{"query":"rust lang"}"#.into(),
                    },
                }]),
                tool_call_id: None,
            }];
            let conv = lowering::to_ir(&msgs);
            let blocks = conv.messages[0].tool_use_blocks();
            assert_eq!(blocks.len(), 1);
            match blocks[0] {
                IrContentBlock::ToolUse { id, name, .. } => {
                    assert_eq!(id, "kimi_tc_1");
                    assert_eq!(name, "search_web");
                }
                _ => panic!("expected ToolUse"),
            }
        }

        #[test]
        fn usage_to_ir_conversion() {
            let usage = KimiUsage {
                prompt_tokens: 100,
                completion_tokens: 50,
                total_tokens: 150,
            };
            let ir = lowering::usage_to_ir(&usage);
            assert_eq!(ir.input_tokens, 100);
            assert_eq!(ir.output_tokens, 50);
            assert_eq!(ir.total_tokens, 150);
        }
    }

    // ---- Codex → IR ----

    mod codex {
        use super::*;
        use abp_codex_sdk::dialect::{
            CodexContentPart, CodexInputItem, CodexResponseItem, CodexUsage,
        };
        use abp_codex_sdk::lowering;

        #[test]
        fn input_message_lowers() {
            let items = vec![CodexInputItem::Message {
                role: "user".into(),
                content: "Write hello world".into(),
            }];
            let conv = lowering::input_to_ir(&items);
            assert_eq!(conv.messages[0].role, IrRole::User);
            assert_eq!(conv.messages[0].text_content(), "Write hello world");
        }

        #[test]
        fn system_input_lowers() {
            let items = vec![CodexInputItem::Message {
                role: "system".into(),
                content: "You are a coder".into(),
            }];
            let conv = lowering::input_to_ir(&items);
            assert_eq!(conv.messages[0].role, IrRole::System);
        }

        #[test]
        fn response_message_to_ir() {
            let items = vec![CodexResponseItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "fn main() {}".into(),
                }],
            }];
            let conv = lowering::to_ir(&items);
            assert_eq!(conv.messages[0].role, IrRole::Assistant);
            assert_eq!(conv.messages[0].text_content(), "fn main() {}");
        }

        #[test]
        fn function_call_response_to_ir() {
            let items = vec![CodexResponseItem::FunctionCall {
                id: "fc_1".into(),
                call_id: None,
                name: "exec_code".into(),
                arguments: r#"{"code":"print('hi')"}"#.into(),
            }];
            let conv = lowering::to_ir(&items);
            let blocks = conv.messages[0].tool_use_blocks();
            assert_eq!(blocks.len(), 1);
        }

        #[test]
        fn usage_to_ir_conversion() {
            let usage = CodexUsage {
                input_tokens: 200,
                output_tokens: 100,
                total_tokens: 300,
            };
            let ir = lowering::usage_to_ir(&usage);
            assert_eq!(ir.input_tokens, 200);
            assert_eq!(ir.output_tokens, 100);
            assert_eq!(ir.total_tokens, 300);
        }
    }

    // ---- Copilot → IR ----

    mod copilot {
        use super::*;
        use abp_copilot_sdk::dialect::CopilotMessage;
        use abp_copilot_sdk::lowering;

        #[test]
        fn user_message_lowers() {
            let msgs = vec![CopilotMessage {
                role: "user".into(),
                content: "Help me with code".into(),
                name: None,
                copilot_references: vec![],
            }];
            let conv = lowering::to_ir(&msgs);
            assert_eq!(conv.messages[0].role, IrRole::User);
            assert_eq!(conv.messages[0].text_content(), "Help me with code");
        }

        #[test]
        fn system_message_lowers() {
            let msgs = vec![CopilotMessage {
                role: "system".into(),
                content: "You are GitHub Copilot".into(),
                name: None,
                copilot_references: vec![],
            }];
            let conv = lowering::to_ir(&msgs);
            assert_eq!(conv.messages[0].role, IrRole::System);
        }

        #[test]
        fn assistant_message_lowers() {
            let msgs = vec![CopilotMessage {
                role: "assistant".into(),
                content: "Here's a snippet.".into(),
                name: None,
                copilot_references: vec![],
            }];
            let conv = lowering::to_ir(&msgs);
            assert_eq!(conv.messages[0].role, IrRole::Assistant);
        }

        #[test]
        fn multi_turn_preserves_order() {
            let msgs = vec![
                CopilotMessage {
                    role: "system".into(),
                    content: "sys".into(),
                    name: None,
                    copilot_references: vec![],
                },
                CopilotMessage {
                    role: "user".into(),
                    content: "q1".into(),
                    name: None,
                    copilot_references: vec![],
                },
                CopilotMessage {
                    role: "assistant".into(),
                    content: "a1".into(),
                    name: None,
                    copilot_references: vec![],
                },
            ];
            let conv = lowering::to_ir(&msgs);
            assert_eq!(conv.len(), 3);
            assert_eq!(conv.messages[0].role, IrRole::System);
            assert_eq!(conv.messages[1].role, IrRole::User);
            assert_eq!(conv.messages[2].role, IrRole::Assistant);
        }
    }
}

// ============================================================================
// 3. IR lifting — IR → SDK (15+ tests)
// ============================================================================

mod ir_lifting {
    use super::*;

    // ---- IR → OpenAI ----

    mod openai {
        use super::*;
        use abp_openai_sdk::lowering;

        #[test]
        fn simple_text_lifts() {
            let conv = IrConversation::new()
                .push(IrMessage::text(IrRole::System, "sys"))
                .push(IrMessage::text(IrRole::User, "hi"))
                .push(IrMessage::text(IrRole::Assistant, "hello"));
            let msgs = lowering::from_ir(&conv);
            assert_eq!(msgs.len(), 3);
            assert_eq!(msgs[0].role, "system");
            assert_eq!(msgs[1].role, "user");
            assert_eq!(msgs[2].role, "assistant");
        }

        #[test]
        fn tool_use_lifts_to_function_call() {
            let conv = IrConversation::new().push(IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "search".into(),
                    input: json!({"q": "test"}),
                }],
            ));
            let msgs = lowering::from_ir(&conv);
            assert!(msgs[0].tool_calls.is_some());
            let tcs = msgs[0].tool_calls.as_ref().unwrap();
            assert_eq!(tcs[0].id, "call_1");
            assert_eq!(tcs[0].function.name, "search");
        }

        #[test]
        fn tool_result_lifts_to_tool_message() {
            let conv = IrConversation::new().push(IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "call_1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "result data".into(),
                    }],
                    is_error: false,
                }],
            ));
            let msgs = lowering::from_ir(&conv);
            assert_eq!(msgs[0].role, "tool");
        }

        #[test]
        fn roundtrip_preserves_roles() {
            let original = vec![
                abp_openai_sdk::dialect::OpenAIMessage {
                    role: "system".into(),
                    content: Some("sys".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                abp_openai_sdk::dialect::OpenAIMessage {
                    role: "user".into(),
                    content: Some("hello".into()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            ];
            let conv = lowering::to_ir(&original);
            let lifted = lowering::from_ir(&conv);
            assert_eq!(lifted[0].role, "system");
            assert_eq!(lifted[1].role, "user");
            assert_eq!(lifted[0].content.as_deref(), Some("sys"));
            assert_eq!(lifted[1].content.as_deref(), Some("hello"));
        }
    }

    // ---- IR → Claude ----

    mod claude {
        use super::*;
        use abp_claude_sdk::lowering;

        #[test]
        fn simple_messages_lift() {
            let conv = IrConversation::new()
                .push(IrMessage::text(IrRole::User, "hi"))
                .push(IrMessage::text(IrRole::Assistant, "hello"));
            let msgs = lowering::from_ir(&conv);
            // Claude from_ir skips system messages
            assert_eq!(msgs.len(), 2);
            assert_eq!(msgs[0].role, "user");
            assert_eq!(msgs[1].role, "assistant");
        }

        #[test]
        fn system_message_skipped_in_lift() {
            let conv = IrConversation::new()
                .push(IrMessage::text(IrRole::System, "sys"))
                .push(IrMessage::text(IrRole::User, "hi"));
            let msgs = lowering::from_ir(&conv);
            // System should be extracted separately, not in messages
            assert!(msgs.iter().all(|m| m.role != "system"));
        }

        #[test]
        fn tool_use_lifts_to_structured_content() {
            let conv = IrConversation::new().push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "Let me search".into(),
                    },
                    IrContentBlock::ToolUse {
                        id: "toolu_1".into(),
                        name: "search".into(),
                        input: json!({"q": "rust"}),
                    },
                ],
            ));
            let msgs = lowering::from_ir(&conv);
            assert_eq!(msgs[0].role, "assistant");
            // Content should be structured JSON for multi-block messages
            let content = &msgs[0].content;
            assert!(!content.is_empty());
        }

        #[test]
        fn extract_system_prompt_works() {
            let conv = IrConversation::new()
                .push(IrMessage::text(IrRole::System, "Be terse"))
                .push(IrMessage::text(IrRole::User, "hi"));
            let sys = lowering::extract_system_prompt(&conv);
            assert_eq!(sys.unwrap(), "Be terse");
        }
    }

    // ---- IR → Gemini ----

    mod gemini {
        use super::*;
        use abp_gemini_sdk::lowering;

        #[test]
        fn messages_lift_to_contents() {
            let conv = IrConversation::new()
                .push(IrMessage::text(IrRole::User, "hello"))
                .push(IrMessage::text(IrRole::Assistant, "hi"));
            let contents = lowering::from_ir(&conv);
            assert_eq!(contents.len(), 2);
            assert_eq!(contents[0].role, "user");
            assert_eq!(contents[1].role, "model");
        }

        #[test]
        fn system_message_skipped_in_lift() {
            let conv = IrConversation::new()
                .push(IrMessage::text(IrRole::System, "sys"))
                .push(IrMessage::text(IrRole::User, "hi"));
            let contents = lowering::from_ir(&conv);
            assert!(contents.iter().all(|c| c.role != "system"));
        }

        #[test]
        fn tool_use_lifts_to_function_call() {
            let conv = IrConversation::new().push(IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "tc_1".into(),
                    name: "search".into(),
                    input: json!({"q": "test"}),
                }],
            ));
            let contents = lowering::from_ir(&conv);
            assert_eq!(contents[0].role, "model");
            let has_fn_call = contents[0]
                .parts
                .iter()
                .any(|p| matches!(p, abp_gemini_sdk::dialect::GeminiPart::FunctionCall { .. }));
            assert!(has_fn_call);
        }

        #[test]
        fn image_lifts_to_inline_data() {
            let conv = IrConversation::new().push(IrMessage::new(
                IrRole::User,
                vec![IrContentBlock::Image {
                    media_type: "image/png".into(),
                    data: "imgdata".into(),
                }],
            ));
            let contents = lowering::from_ir(&conv);
            let has_inline = contents[0]
                .parts
                .iter()
                .any(|p| matches!(p, abp_gemini_sdk::dialect::GeminiPart::InlineData(..)));
            assert!(has_inline);
        }

        #[test]
        fn extract_system_instruction() {
            let conv = IrConversation::new()
                .push(IrMessage::text(IrRole::System, "instructions"))
                .push(IrMessage::text(IrRole::User, "hello"));
            let sys = lowering::extract_system_instruction(&conv);
            assert!(sys.is_some());
        }
    }

    // ---- IR → Codex ----

    mod codex {
        use super::*;
        use abp_codex_sdk::lowering;

        #[test]
        fn assistant_text_lifts_to_message_item() {
            let conv = IrConversation::new().push(IrMessage::text(IrRole::Assistant, "Done!"));
            let items = lowering::from_ir(&conv);
            assert!(!items.is_empty());
        }

        #[test]
        fn tool_use_lifts_to_function_call_item() {
            let conv = IrConversation::new().push(IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "fc_1".into(),
                    name: "run_code".into(),
                    input: json!({"code": "print(1)"}),
                }],
            ));
            let items = lowering::from_ir(&conv);
            let has_fc = items.iter().any(|item| {
                matches!(
                    item,
                    abp_codex_sdk::dialect::CodexResponseItem::FunctionCall { .. }
                )
            });
            assert!(has_fc);
        }
    }

    // ---- IR → Copilot ----

    mod copilot {
        use super::*;
        use abp_copilot_sdk::lowering;

        #[test]
        fn simple_messages_lift() {
            let conv = IrConversation::new()
                .push(IrMessage::text(IrRole::System, "sys"))
                .push(IrMessage::text(IrRole::User, "hi"))
                .push(IrMessage::text(IrRole::Assistant, "hello"));
            let msgs = lowering::from_ir(&conv);
            assert!(msgs.len() >= 2);
        }

        #[test]
        fn roundtrip_preserves_content() {
            let original = vec![
                abp_copilot_sdk::dialect::CopilotMessage {
                    role: "user".into(),
                    content: "test query".into(),
                    name: None,
                    copilot_references: vec![],
                },
                abp_copilot_sdk::dialect::CopilotMessage {
                    role: "assistant".into(),
                    content: "test answer".into(),
                    name: None,
                    copilot_references: vec![],
                },
            ];
            let conv = lowering::to_ir(&original);
            let lifted = lowering::from_ir(&conv);
            assert!(lifted.iter().any(|m| m.content == "test query"));
            assert!(lifted.iter().any(|m| m.content == "test answer"));
        }
    }
}

// ============================================================================
// 4. Cross-dialect IR equivalence (15+ tests)
// ============================================================================

mod cross_dialect_equivalence {
    use super::*;

    /// Helper: extracts text from the first user message in a conversation
    fn first_user_text(conv: &IrConversation) -> String {
        conv.messages_by_role(IrRole::User)
            .first()
            .map(|m| m.text_content())
            .unwrap_or_default()
    }

    /// Helper: extracts text from the first assistant message
    fn first_assistant_text(conv: &IrConversation) -> String {
        conv.messages_by_role(IrRole::Assistant)
            .first()
            .map(|m| m.text_content())
            .unwrap_or_default()
    }

    #[test]
    fn simple_user_message_equivalent_openai_claude() {
        let openai_conv =
            abp_openai_sdk::lowering::to_ir(&[abp_openai_sdk::dialect::OpenAIMessage {
                role: "user".into(),
                content: Some("What is 2+2?".into()),
                tool_calls: None,
                tool_call_id: None,
            }]);
        let claude_conv = abp_claude_sdk::lowering::to_ir(
            &[abp_claude_sdk::dialect::ClaudeMessage {
                role: "user".into(),
                content: "What is 2+2?".into(),
            }],
            None,
        );
        assert_eq!(first_user_text(&openai_conv), first_user_text(&claude_conv));
    }

    #[test]
    fn simple_user_message_equivalent_gemini_kimi() {
        let gemini_conv = abp_gemini_sdk::lowering::to_ir(
            &[abp_gemini_sdk::dialect::GeminiContent {
                role: "user".into(),
                parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text(
                    "What is 2+2?".into(),
                )],
            }],
            None,
        );
        let kimi_conv = abp_kimi_sdk::lowering::to_ir(&[abp_kimi_sdk::dialect::KimiMessage {
            role: "user".into(),
            content: Some("What is 2+2?".into()),
            tool_calls: None,
            tool_call_id: None,
        }]);
        assert_eq!(first_user_text(&gemini_conv), first_user_text(&kimi_conv));
    }

    #[test]
    fn simple_user_message_equivalent_codex_copilot() {
        let codex_conv = abp_codex_sdk::lowering::input_to_ir(&[
            abp_codex_sdk::dialect::CodexInputItem::Message {
                role: "user".into(),
                content: "What is 2+2?".into(),
            },
        ]);
        let copilot_conv =
            abp_copilot_sdk::lowering::to_ir(&[abp_copilot_sdk::dialect::CopilotMessage {
                role: "user".into(),
                content: "What is 2+2?".into(),
                name: None,
                copilot_references: vec![],
            }]);
        assert_eq!(first_user_text(&codex_conv), first_user_text(&copilot_conv));
    }

    #[test]
    fn all_six_dialects_produce_same_user_text() {
        let question = "What is Rust?";

        let openai = abp_openai_sdk::lowering::to_ir(&[abp_openai_sdk::dialect::OpenAIMessage {
            role: "user".into(),
            content: Some(question.into()),
            tool_calls: None,
            tool_call_id: None,
        }]);
        let claude = abp_claude_sdk::lowering::to_ir(
            &[abp_claude_sdk::dialect::ClaudeMessage {
                role: "user".into(),
                content: question.into(),
            }],
            None,
        );
        let gemini = abp_gemini_sdk::lowering::to_ir(
            &[abp_gemini_sdk::dialect::GeminiContent {
                role: "user".into(),
                parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text(question.into())],
            }],
            None,
        );
        let kimi = abp_kimi_sdk::lowering::to_ir(&[abp_kimi_sdk::dialect::KimiMessage {
            role: "user".into(),
            content: Some(question.into()),
            tool_calls: None,
            tool_call_id: None,
        }]);
        let codex = abp_codex_sdk::lowering::input_to_ir(&[
            abp_codex_sdk::dialect::CodexInputItem::Message {
                role: "user".into(),
                content: question.into(),
            },
        ]);
        let copilot =
            abp_copilot_sdk::lowering::to_ir(&[abp_copilot_sdk::dialect::CopilotMessage {
                role: "user".into(),
                content: question.into(),
                name: None,
                copilot_references: vec![],
            }]);

        let expected = question;
        assert_eq!(first_user_text(&openai), expected);
        assert_eq!(first_user_text(&claude), expected);
        assert_eq!(first_user_text(&gemini), expected);
        assert_eq!(first_user_text(&kimi), expected);
        assert_eq!(first_user_text(&codex), expected);
        assert_eq!(first_user_text(&copilot), expected);
    }

    #[test]
    fn system_message_equivalent_openai_kimi() {
        let openai = abp_openai_sdk::lowering::to_ir(&[abp_openai_sdk::dialect::OpenAIMessage {
            role: "system".into(),
            content: Some("Be helpful".into()),
            tool_calls: None,
            tool_call_id: None,
        }]);
        let kimi = abp_kimi_sdk::lowering::to_ir(&[abp_kimi_sdk::dialect::KimiMessage {
            role: "system".into(),
            content: Some("Be helpful".into()),
            tool_calls: None,
            tool_call_id: None,
        }]);
        let openai_sys = openai.system_message().unwrap();
        let kimi_sys = kimi.system_message().unwrap();
        assert_eq!(openai_sys.text_content(), kimi_sys.text_content());
        assert_eq!(openai_sys.role, kimi_sys.role);
    }

    #[test]
    fn system_message_equivalent_claude_gemini() {
        let claude = abp_claude_sdk::lowering::to_ir(
            &[abp_claude_sdk::dialect::ClaudeMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
            Some("Be helpful"),
        );
        let gemini = abp_gemini_sdk::lowering::to_ir(
            &[abp_gemini_sdk::dialect::GeminiContent {
                role: "user".into(),
                parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text("hi".into())],
            }],
            Some(&abp_gemini_sdk::dialect::GeminiContent {
                role: "user".into(),
                parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text(
                    "Be helpful".into(),
                )],
            }),
        );
        let claude_sys = claude.system_message().unwrap();
        let gemini_sys = gemini.system_message().unwrap();
        assert_eq!(claude_sys.text_content(), gemini_sys.text_content());
    }

    #[test]
    fn system_message_is_dialect_agnostic() {
        let sys_text = "You are a helpful coding assistant.";

        // OpenAI-style
        let openai = abp_openai_sdk::lowering::to_ir(&[abp_openai_sdk::dialect::OpenAIMessage {
            role: "system".into(),
            content: Some(sys_text.into()),
            tool_calls: None,
            tool_call_id: None,
        }]);
        // Claude-style
        let claude = abp_claude_sdk::lowering::to_ir(&[], Some(sys_text));
        // Codex-style
        let codex = abp_codex_sdk::lowering::input_to_ir(&[
            abp_codex_sdk::dialect::CodexInputItem::Message {
                role: "system".into(),
                content: sys_text.into(),
            },
        ]);
        // Copilot-style
        let copilot =
            abp_copilot_sdk::lowering::to_ir(&[abp_copilot_sdk::dialect::CopilotMessage {
                role: "system".into(),
                content: sys_text.into(),
                name: None,
                copilot_references: vec![],
            }]);

        // All should produce system message with same text
        assert_eq!(openai.system_message().unwrap().text_content(), sys_text);
        assert_eq!(claude.system_message().unwrap().text_content(), sys_text);
        assert_eq!(codex.system_message().unwrap().text_content(), sys_text);
        assert_eq!(copilot.system_message().unwrap().text_content(), sys_text);
    }

    #[test]
    fn tool_call_ir_from_openai_matches_kimi() {
        let openai = abp_openai_sdk::lowering::to_ir(&[abp_openai_sdk::dialect::OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![abp_openai_sdk::dialect::OpenAIToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: abp_openai_sdk::dialect::OpenAIFunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"test"}"#.into(),
                },
            }]),
            tool_call_id: None,
        }]);
        let kimi = abp_kimi_sdk::lowering::to_ir(&[abp_kimi_sdk::dialect::KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![abp_kimi_sdk::dialect::KimiToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: abp_kimi_sdk::dialect::KimiFunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"test"}"#.into(),
                },
            }]),
            tool_call_id: None,
        }]);

        let openai_tc = openai.tool_calls();
        let kimi_tc = kimi.tool_calls();
        assert_eq!(openai_tc.len(), kimi_tc.len());

        // Both should have same tool name and input
        match (openai_tc[0], kimi_tc[0]) {
            (
                IrContentBlock::ToolUse {
                    name: n1,
                    input: i1,
                    ..
                },
                IrContentBlock::ToolUse {
                    name: n2,
                    input: i2,
                    ..
                },
            ) => {
                assert_eq!(n1, n2);
                assert_eq!(i1, i2);
            }
            _ => panic!("expected ToolUse blocks"),
        }
    }

    #[test]
    fn image_ir_equivalent_gemini_inline_data() {
        let gemini = abp_gemini_sdk::lowering::to_ir(
            &[abp_gemini_sdk::dialect::GeminiContent {
                role: "user".into(),
                parts: vec![abp_gemini_sdk::dialect::GeminiPart::InlineData(
                    abp_gemini_sdk::dialect::GeminiInlineData {
                        mime_type: "image/png".into(),
                        data: "base64img".into(),
                    },
                )],
            }],
            None,
        );

        match &gemini.messages[0].content[0] {
            IrContentBlock::Image {
                media_type, data, ..
            } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "base64img");
            }
            other => panic!("expected Image, got {other:?}"),
        }
    }

    #[test]
    fn multi_turn_conversation_structure_matches_across_dialects() {
        // Build same conversation in OpenAI and Claude format
        let openai = abp_openai_sdk::lowering::to_ir(&[
            abp_openai_sdk::dialect::OpenAIMessage {
                role: "user".into(),
                content: Some("Hi".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            abp_openai_sdk::dialect::OpenAIMessage {
                role: "assistant".into(),
                content: Some("Hello!".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            abp_openai_sdk::dialect::OpenAIMessage {
                role: "user".into(),
                content: Some("How are you?".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ]);
        let claude = abp_claude_sdk::lowering::to_ir(
            &[
                abp_claude_sdk::dialect::ClaudeMessage {
                    role: "user".into(),
                    content: "Hi".into(),
                },
                abp_claude_sdk::dialect::ClaudeMessage {
                    role: "assistant".into(),
                    content: "Hello!".into(),
                },
                abp_claude_sdk::dialect::ClaudeMessage {
                    role: "user".into(),
                    content: "How are you?".into(),
                },
            ],
            None,
        );

        assert_eq!(openai.len(), claude.len());
        for (o, c) in openai.messages.iter().zip(claude.messages.iter()) {
            assert_eq!(o.role, c.role);
            assert_eq!(o.text_content(), c.text_content());
        }
    }

    #[test]
    fn assistant_text_identical_across_openai_copilot_kimi() {
        let answer = "The answer is 42.";

        let openai = abp_openai_sdk::lowering::to_ir(&[abp_openai_sdk::dialect::OpenAIMessage {
            role: "assistant".into(),
            content: Some(answer.into()),
            tool_calls: None,
            tool_call_id: None,
        }]);
        let copilot =
            abp_copilot_sdk::lowering::to_ir(&[abp_copilot_sdk::dialect::CopilotMessage {
                role: "assistant".into(),
                content: answer.into(),
                name: None,
                copilot_references: vec![],
            }]);
        let kimi = abp_kimi_sdk::lowering::to_ir(&[abp_kimi_sdk::dialect::KimiMessage {
            role: "assistant".into(),
            content: Some(answer.into()),
            tool_calls: None,
            tool_call_id: None,
        }]);

        assert_eq!(first_assistant_text(&openai), answer);
        assert_eq!(first_assistant_text(&copilot), answer);
        assert_eq!(first_assistant_text(&kimi), answer);
    }

    #[test]
    fn empty_conversation_produces_empty_ir() {
        let openai = abp_openai_sdk::lowering::to_ir(&[]);
        let claude = abp_claude_sdk::lowering::to_ir(&[], None);
        let gemini = abp_gemini_sdk::lowering::to_ir(&[], None);
        let kimi = abp_kimi_sdk::lowering::to_ir(&[]);
        let codex = abp_codex_sdk::lowering::input_to_ir(&[]);
        let copilot = abp_copilot_sdk::lowering::to_ir(&[]);

        assert!(openai.is_empty());
        assert!(claude.is_empty());
        assert!(gemini.is_empty());
        assert!(kimi.is_empty());
        assert!(codex.is_empty());
        assert!(copilot.is_empty());
    }

    #[test]
    fn role_mapping_consistency_user() {
        let openai = abp_openai_sdk::lowering::to_ir(&[abp_openai_sdk::dialect::OpenAIMessage {
            role: "user".into(),
            content: Some("x".into()),
            tool_calls: None,
            tool_call_id: None,
        }]);
        let claude = abp_claude_sdk::lowering::to_ir(
            &[abp_claude_sdk::dialect::ClaudeMessage {
                role: "user".into(),
                content: "x".into(),
            }],
            None,
        );
        let gemini = abp_gemini_sdk::lowering::to_ir(
            &[abp_gemini_sdk::dialect::GeminiContent {
                role: "user".into(),
                parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text("x".into())],
            }],
            None,
        );

        assert_eq!(openai.messages[0].role, IrRole::User);
        assert_eq!(claude.messages[0].role, IrRole::User);
        assert_eq!(gemini.messages[0].role, IrRole::User);
    }

    #[test]
    fn role_mapping_consistency_assistant() {
        let openai = abp_openai_sdk::lowering::to_ir(&[abp_openai_sdk::dialect::OpenAIMessage {
            role: "assistant".into(),
            content: Some("y".into()),
            tool_calls: None,
            tool_call_id: None,
        }]);
        let claude = abp_claude_sdk::lowering::to_ir(
            &[abp_claude_sdk::dialect::ClaudeMessage {
                role: "assistant".into(),
                content: "y".into(),
            }],
            None,
        );
        let gemini = abp_gemini_sdk::lowering::to_ir(
            &[abp_gemini_sdk::dialect::GeminiContent {
                role: "model".into(),
                parts: vec![abp_gemini_sdk::dialect::GeminiPart::Text("y".into())],
            }],
            None,
        );

        assert_eq!(openai.messages[0].role, IrRole::Assistant);
        assert_eq!(claude.messages[0].role, IrRole::Assistant);
        assert_eq!(gemini.messages[0].role, IrRole::Assistant);
    }

    #[test]
    fn ir_default_empty_conversation() {
        let conv = IrConversation::default();
        assert!(conv.is_empty());
        assert_eq!(conv.len(), 0);
        assert!(conv.system_message().is_none());
        assert!(conv.last_assistant().is_none());
        assert!(conv.last_message().is_none());
        assert!(conv.tool_calls().is_empty());
    }

    #[test]
    fn ir_metadata_preserved_through_serde() {
        let mut metadata = BTreeMap::new();
        metadata.insert("custom_key".to_string(), json!("custom_value"));
        metadata.insert("num".to_string(), json!(42));

        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text { text: "hi".into() }],
            metadata,
        };

        let json = serde_json::to_string(&msg).unwrap();
        let roundtripped: IrMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtripped.metadata["custom_key"], "custom_value");
        assert_eq!(roundtripped.metadata["num"], 42);
    }
}

// ============================================================================
// Additional edge-case tests
// ============================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn empty_text_content_block() {
        let block = IrContentBlock::Text {
            text: String::new(),
        };
        let json = serde_json::to_string(&block).unwrap();
        let rt: IrContentBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(block, rt);
    }

    #[test]
    fn tool_use_with_complex_input() {
        let complex_input = json!({
            "nested": {"deep": {"value": [1, 2, 3]}},
            "array": ["a", "b"],
            "null_val": null,
            "bool_val": true
        });
        let block = IrContentBlock::ToolUse {
            id: "tc_complex".into(),
            name: "process".into(),
            input: complex_input.clone(),
        };
        let json = serde_json::to_string(&block).unwrap();
        let rt: IrContentBlock = serde_json::from_str(&json).unwrap();
        match rt {
            IrContentBlock::ToolUse { input, .. } => assert_eq!(input, complex_input),
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn tool_result_with_error_flag() {
        let block = IrContentBlock::ToolResult {
            tool_use_id: "tc_err".into(),
            content: vec![IrContentBlock::Text {
                text: "Error: not found".into(),
            }],
            is_error: true,
        };
        match &block {
            IrContentBlock::ToolResult { is_error, .. } => assert!(is_error),
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn conversation_last_message() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "first"))
            .push(IrMessage::text(IrRole::Assistant, "second"))
            .push(IrMessage::text(IrRole::User, "third"));
        let last = conv.last_message().unwrap();
        assert_eq!(last.text_content(), "third");
    }

    #[test]
    fn ir_usage_default_is_zero() {
        let u = IrUsage::default();
        assert_eq!(u.input_tokens, 0);
        assert_eq!(u.output_tokens, 0);
        assert_eq!(u.total_tokens, 0);
        assert_eq!(u.cache_read_tokens, 0);
        assert_eq!(u.cache_write_tokens, 0);
    }

    #[test]
    fn ir_content_block_tagged_serialization() {
        let text = IrContentBlock::Text { text: "hi".into() };
        let json = serde_json::to_string(&text).unwrap();
        assert!(json.contains(r#""type":"text""#));

        let tool = IrContentBlock::ToolUse {
            id: "1".into(),
            name: "f".into(),
            input: json!({}),
        };
        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains(r#""type":"tool_use""#));
    }

    #[test]
    fn ir_conversation_clone_is_independent() {
        let conv1 = IrConversation::new().push(IrMessage::text(IrRole::User, "hello"));
        let conv2 = conv1.clone();
        assert_eq!(conv1, conv2);
        // They should be equal but independent
        assert_eq!(conv1.len(), conv2.len());
    }

    #[test]
    fn openai_malformed_json_arguments_handled() {
        let msgs = vec![abp_openai_sdk::dialect::OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![abp_openai_sdk::dialect::OpenAIToolCall {
                id: "call_bad".into(),
                call_type: "function".into(),
                function: abp_openai_sdk::dialect::OpenAIFunctionCall {
                    name: "fn".into(),
                    arguments: "not valid json".into(),
                },
            }]),
            tool_call_id: None,
        }];
        // Should not panic — malformed JSON is handled gracefully
        let conv = abp_openai_sdk::lowering::to_ir(&msgs);
        assert_eq!(conv.len(), 1);
    }

    #[test]
    fn kimi_malformed_json_arguments_handled() {
        let msgs = vec![abp_kimi_sdk::dialect::KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![abp_kimi_sdk::dialect::KimiToolCall {
                id: "call_bad".into(),
                call_type: "function".into(),
                function: abp_kimi_sdk::dialect::KimiFunctionCall {
                    name: "fn".into(),
                    arguments: "{{invalid".into(),
                },
            }]),
            tool_call_id: None,
        }];
        let conv = abp_kimi_sdk::lowering::to_ir(&msgs);
        assert_eq!(conv.len(), 1);
    }

    #[test]
    fn gemini_function_call_id_synthesis() {
        // Gemini doesn't have native tool call IDs — they're synthesized
        let contents = vec![abp_gemini_sdk::dialect::GeminiContent {
            role: "model".into(),
            parts: vec![abp_gemini_sdk::dialect::GeminiPart::FunctionCall {
                name: "get_time".into(),
                args: json!({}),
            }],
        }];
        let conv = abp_gemini_sdk::lowering::to_ir(&contents, None);
        let blocks = conv.messages[0].tool_use_blocks();
        assert_eq!(blocks.len(), 1);
        match blocks[0] {
            IrContentBlock::ToolUse { id, name, .. } => {
                assert!(!id.is_empty(), "synthesized ID should not be empty");
                assert_eq!(name, "get_time");
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn codex_response_item_reasoning_to_ir() {
        let items = vec![abp_codex_sdk::dialect::CodexResponseItem::Reasoning {
            summary: vec![abp_codex_sdk::dialect::ReasoningSummary {
                text: "I'm thinking about this...".into(),
            }],
        }];
        let conv = abp_codex_sdk::lowering::to_ir(&items);
        // Reasoning should map to Thinking blocks
        assert!(!conv.is_empty());
    }
}
