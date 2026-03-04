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

//! Cross-shim mapping integration tests.
//!
//! Verifies that the IR-level mappers correctly translate conversations
//! between SDK dialects (OpenAI ↔ Claude ↔ Gemini), covering text,
//! tool calls, thinking blocks, metadata, and error cases.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;
use abp_mapper::{
    ClaudeGeminiIrMapper, IrMapper, MapError, OpenAiClaudeIrMapper, OpenAiGeminiIrMapper,
    default_ir_mapper,
};
use serde_json::json;

// ── Helpers ─────────────────────────────────────────────────────────────

fn text_conv(messages: Vec<(IrRole, &str)>) -> IrConversation {
    IrConversation::from_messages(
        messages
            .into_iter()
            .map(|(role, text)| IrMessage::text(role, text))
            .collect(),
    )
}

fn tool_call_msg(id: &str, name: &str, input: serde_json::Value) -> IrMessage {
    IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: id.into(),
            name: name.into(),
            input,
        }],
    )
}

fn tool_result_msg(role: IrRole, tool_use_id: &str, text: &str, is_error: bool) -> IrMessage {
    IrMessage::new(
        role,
        vec![IrContentBlock::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: vec![IrContentBlock::Text { text: text.into() }],
            is_error,
        }],
    )
}

// ═══════════════════════════════════════════════════════════════════════
// Module: openai_to_claude
// ═══════════════════════════════════════════════════════════════════════

mod openai_to_claude {
    use super::*;

    #[test]
    fn simple_text_message_maps_correctly() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = text_conv(vec![(IrRole::User, "Hello, Claude!")]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result.messages[0].role, IrRole::User);
        assert_eq!(result.messages[0].text_content(), "Hello, Claude!");
    }

    #[test]
    fn system_message_preserved() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = text_conv(vec![
            (IrRole::System, "You are a coding assistant."),
            (IrRole::User, "Write hello world"),
        ]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.messages[0].role, IrRole::System);
        assert_eq!(
            result.messages[0].text_content(),
            "You are a coding assistant."
        );
    }

    #[test]
    fn tool_definitions_map_via_tool_use_blocks() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "What's the weather?"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "call_abc".into(),
                    name: "get_weather".into(),
                    input: json!({"location": "San Francisco", "unit": "celsius"}),
                }],
            ),
        ]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let tool_calls = result.tool_calls();
        assert_eq!(tool_calls.len(), 1);
        if let IrContentBlock::ToolUse { name, input, .. } = tool_calls[0] {
            assert_eq!(name, "get_weather");
            assert_eq!(input["location"], "San Francisco");
        } else {
            panic!("expected ToolUse");
        }
    }

    #[test]
    fn assistant_with_tool_calls_maps_correctly() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Search for files"),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "I'll search for that.".into(),
                    },
                    IrContentBlock::ToolUse {
                        id: "tc_1".into(),
                        name: "search".into(),
                        input: json!({"query": "*.rs"}),
                    },
                    IrContentBlock::ToolUse {
                        id: "tc_2".into(),
                        name: "grep".into(),
                        input: json!({"pattern": "fn main"}),
                    },
                ],
            ),
        ]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let asst = &result.messages[1];
        assert_eq!(asst.role, IrRole::Assistant);
        assert_eq!(asst.content.len(), 3);
        assert_eq!(asst.text_content(), "I'll search for that.");
        assert_eq!(asst.tool_use_blocks().len(), 2);
    }

    #[test]
    fn streaming_response_maps_correctly() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Tell me a story"),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "Once upon ".into(),
                    },
                    IrContentBlock::Text {
                        text: "a time...".into(),
                    },
                ],
            ),
        ]);
        let result = mapper
            .map_response(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let asst = &result.messages[1];
        assert_eq!(asst.text_content(), "Once upon a time...");
    }

    #[test]
    fn metadata_preserved_through_mapping() {
        let mapper = OpenAiClaudeIrMapper;
        let mut msg = IrMessage::text(IrRole::User, "test");
        msg.metadata.insert("temperature".into(), json!(0.7));
        msg.metadata.insert("top_p".into(), json!(0.9));
        let conv = IrConversation::from_messages(vec![msg]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(
            result.messages[0].metadata.get("temperature"),
            Some(&json!(0.7))
        );
        assert_eq!(result.messages[0].metadata.get("top_p"), Some(&json!(0.9)));
    }

    #[test]
    fn max_tokens_metadata_preserved() {
        let mapper = OpenAiClaudeIrMapper;
        let mut msg = IrMessage::text(IrRole::User, "test");
        msg.metadata.insert("max_tokens".into(), json!(4096));
        let conv = IrConversation::from_messages(vec![msg]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(
            result.messages[0].metadata.get("max_tokens"),
            Some(&json!(4096))
        );
    }

    #[test]
    fn multi_turn_conversation_maps_correctly() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be concise."),
            IrMessage::text(IrRole::User, "What is 2+2?"),
            IrMessage::text(IrRole::Assistant, "4"),
            IrMessage::text(IrRole::User, "And 3+3?"),
            IrMessage::text(IrRole::Assistant, "6"),
            IrMessage::text(IrRole::User, "Thanks!"),
        ]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.len(), 6);
        assert_eq!(result.messages[0].role, IrRole::System);
        assert_eq!(result.messages[1].role, IrRole::User);
        assert_eq!(result.messages[2].role, IrRole::Assistant);
        assert_eq!(result.messages[3].role, IrRole::User);
        assert_eq!(result.messages[4].role, IrRole::Assistant);
        assert_eq!(result.messages[5].role, IrRole::User);
        assert_eq!(result.messages[4].text_content(), "6");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: claude_to_openai
// ═══════════════════════════════════════════════════════════════════════

mod claude_to_openai {
    use super::*;

    #[test]
    fn simple_text_response_maps_correctly() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = text_conv(vec![
            (IrRole::User, "Hello"),
            (IrRole::Assistant, "Hi! How can I help?"),
        ]);
        let result = mapper
            .map_response(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result.messages[1].role, IrRole::Assistant);
        assert_eq!(result.messages[1].text_content(), "Hi! How can I help?");
    }

    #[test]
    fn content_blocks_map_to_proper_format() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Describe this image"),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "I see a ".into(),
                    },
                    IrContentBlock::Text {
                        text: "beautiful landscape.".into(),
                    },
                ],
            ),
        ]);
        let result = mapper
            .map_response(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(
            result.messages[1].text_content(),
            "I see a beautiful landscape."
        );
    }

    #[test]
    fn tool_use_blocks_map_to_tool_calls() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Read main.rs"),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "Let me read that file.".into(),
                    },
                    IrContentBlock::ToolUse {
                        id: "toolu_01".into(),
                        name: "read_file".into(),
                        input: json!({"path": "src/main.rs"}),
                    },
                ],
            ),
        ]);
        let result = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        let asst = &result.messages[1];
        assert_eq!(asst.role, IrRole::Assistant);
        let tool_uses = asst.tool_use_blocks();
        assert_eq!(tool_uses.len(), 1);
        if let IrContentBlock::ToolUse { name, input, .. } = tool_uses[0] {
            assert_eq!(name, "read_file");
            assert_eq!(input["path"], "src/main.rs");
        }
    }

    #[test]
    fn system_message_maps_through() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = text_conv(vec![
            (IrRole::System, "You are a helpful AI."),
            (IrRole::User, "Hello"),
        ]);
        let result = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(result.messages[0].role, IrRole::System);
        assert_eq!(result.messages[0].text_content(), "You are a helpful AI.");
    }

    #[test]
    fn thinking_blocks_dropped_for_openai() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Hard problem"),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Thinking {
                        text: "Let me reason step by step...".into(),
                    },
                    IrContentBlock::Text {
                        text: "The answer is 42.".into(),
                    },
                ],
            ),
        ]);
        let result = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        let asst = &result.messages[1];
        assert_eq!(asst.content.len(), 1);
        assert!(matches!(&asst.content[0], IrContentBlock::Text { .. }));
        assert_eq!(asst.text_content(), "The answer is 42.");
    }

    #[test]
    fn user_tool_results_become_tool_role() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Run command"),
            tool_call_msg("tc_1", "bash", json!({"cmd": "ls"})),
            // Claude-style: tool result in user message
            IrMessage::new(
                IrRole::User,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "tc_1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "file1.rs\nfile2.rs".into(),
                    }],
                    is_error: false,
                }],
            ),
        ]);
        let result = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        // The user message with ToolResult should become a Tool-role message
        let tool_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| m.role == IrRole::Tool)
            .collect();
        assert_eq!(tool_msgs.len(), 1);
        if let IrContentBlock::ToolResult {
            tool_use_id,
            is_error,
            ..
        } = &tool_msgs[0].content[0]
        {
            assert_eq!(tool_use_id, "tc_1");
            assert!(!is_error);
        }
    }

    #[test]
    fn multiple_tool_results_split_into_separate_messages() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Do two things"),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::ToolUse {
                        id: "t1".into(),
                        name: "search".into(),
                        input: json!({"q": "test"}),
                    },
                    IrContentBlock::ToolUse {
                        id: "t2".into(),
                        name: "read".into(),
                        input: json!({"p": "file.rs"}),
                    },
                ],
            ),
            IrMessage::new(
                IrRole::User,
                vec![
                    IrContentBlock::ToolResult {
                        tool_use_id: "t1".into(),
                        content: vec![IrContentBlock::Text {
                            text: "found it".into(),
                        }],
                        is_error: false,
                    },
                    IrContentBlock::ToolResult {
                        tool_use_id: "t2".into(),
                        content: vec![IrContentBlock::Text {
                            text: "file contents".into(),
                        }],
                        is_error: false,
                    },
                ],
            ),
        ]);
        let result = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        let tool_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| m.role == IrRole::Tool)
            .collect();
        assert_eq!(tool_msgs.len(), 2);
    }

    #[test]
    fn image_content_preserved() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text {
                    text: "What's in this image?".into(),
                },
                IrContentBlock::Image {
                    media_type: "image/jpeg".into(),
                    data: "base64encodeddata".into(),
                },
            ],
        )]);
        let result = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(result.messages[0].content.len(), 2);
        assert!(matches!(
            &result.messages[0].content[1],
            IrContentBlock::Image { media_type, .. } if media_type == "image/jpeg"
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: openai_to_gemini
// ═══════════════════════════════════════════════════════════════════════

mod openai_to_gemini {
    use super::*;

    #[test]
    fn messages_map_to_contents() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = text_conv(vec![
            (IrRole::User, "Hello Gemini"),
            (IrRole::Assistant, "Hello! How can I help?"),
        ]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result.messages[0].role, IrRole::User);
        assert_eq!(result.messages[1].role, IrRole::Assistant);
        assert_eq!(result.messages[1].text_content(), "Hello! How can I help?");
    }

    #[test]
    fn system_message_becomes_system_instruction() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = text_conv(vec![
            (IrRole::System, "You are a math tutor."),
            (IrRole::User, "What is pi?"),
        ]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        // System messages are preserved for Gemini (supports system instructions)
        assert_eq!(result.messages[0].role, IrRole::System);
        assert_eq!(result.messages[0].text_content(), "You are a math tutor.");
    }

    #[test]
    fn tool_calls_map_to_function_calls() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "What's the weather?"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "get_weather".into(),
                    input: json!({"city": "Tokyo"}),
                }],
            ),
        ]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        let tools = result.tool_calls();
        assert_eq!(tools.len(), 1);
        if let IrContentBlock::ToolUse { name, input, .. } = tools[0] {
            assert_eq!(name, "get_weather");
            assert_eq!(input["city"], "Tokyo");
        }
    }

    #[test]
    fn tool_role_maps_to_user_role() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Check weather"),
            tool_call_msg("c1", "get_weather", json!({"city": "LA"})),
            tool_result_msg(IrRole::Tool, "c1", "75°F sunny", false),
        ]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        // Tool-role message becomes User-role in Gemini
        assert_eq!(result.messages[2].role, IrRole::User);
        assert!(matches!(
            &result.messages[2].content[0],
            IrContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "c1"
        ));
    }

    #[test]
    fn thinking_blocks_dropped() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Solve this"),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Thinking {
                        text: "Step 1: analyze...".into(),
                    },
                    IrContentBlock::Text {
                        text: "The answer is 7.".into(),
                    },
                ],
            ),
        ]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        let asst = &result.messages[1];
        assert_eq!(asst.content.len(), 1);
        assert!(matches!(&asst.content[0], IrContentBlock::Text { .. }));
        assert_eq!(asst.text_content(), "The answer is 7.");
    }

    #[test]
    fn multi_turn_with_tools_maps_correctly() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "You are a helper."),
            IrMessage::text(IrRole::User, "Search for docs"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "search".into(),
                    input: json!({"q": "docs"}),
                }],
            ),
            tool_result_msg(IrRole::Tool, "t1", "Found 3 docs", false),
            IrMessage::text(IrRole::Assistant, "I found 3 documents."),
        ]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        assert_eq!(result.len(), 5);
        assert_eq!(result.messages[0].role, IrRole::System);
        // Tool result became User role
        assert_eq!(result.messages[3].role, IrRole::User);
        assert_eq!(result.messages[4].text_content(), "I found 3 documents.");
    }

    #[test]
    fn gemini_to_openai_user_tool_results_become_tool_role() {
        let mapper = OpenAiGeminiIrMapper;
        // Simulate Gemini-style: tool results in user messages
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Do something"),
            tool_call_msg("fc_1", "action", json!({})),
            IrMessage::new(
                IrRole::User,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "fc_1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "done".into(),
                    }],
                    is_error: false,
                }],
            ),
        ]);
        let result = mapper
            .map_request(Dialect::Gemini, Dialect::OpenAi, &conv)
            .unwrap();
        let tool_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| m.role == IrRole::Tool)
            .collect();
        assert_eq!(tool_msgs.len(), 1);
    }

    #[test]
    fn image_blocks_preserved() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text {
                    text: "Describe this".into(),
                },
                IrContentBlock::Image {
                    media_type: "image/png".into(),
                    data: "iVBORw0KGgo=".into(),
                },
            ],
        )]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        assert_eq!(result.messages[0].content.len(), 2);
        assert!(matches!(
            &result.messages[0].content[1],
            IrContentBlock::Image { .. }
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: claude_to_gemini (transitive path)
// ═══════════════════════════════════════════════════════════════════════

mod claude_to_gemini {
    use super::*;

    #[test]
    fn simple_text_maps_correctly() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = text_conv(vec![
            (IrRole::User, "Hello"),
            (IrRole::Assistant, "Hi there"),
        ]);
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result.messages[1].text_content(), "Hi there");
    }

    #[test]
    fn thinking_blocks_dropped_claude_to_gemini() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Complex question"),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Thinking {
                        text: "Hmm, let me think...".into(),
                    },
                    IrContentBlock::Text {
                        text: "Here's my answer.".into(),
                    },
                ],
            ),
        ]);
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        let asst = &result.messages[1];
        assert_eq!(asst.content.len(), 1);
        assert_eq!(asst.text_content(), "Here's my answer.");
    }

    #[test]
    fn tool_role_maps_to_user_in_gemini() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Run tool"),
            tool_call_msg("t1", "exec", json!({"cmd": "test"})),
            tool_result_msg(IrRole::Tool, "t1", "ok", false),
        ]);
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        assert_eq!(result.messages[2].role, IrRole::User);
    }

    #[test]
    fn gemini_to_claude_tool_role_maps_to_user() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Run tool"),
            tool_call_msg("t1", "exec", json!({"cmd": "test"})),
            tool_result_msg(IrRole::Tool, "t1", "result", false),
        ]);
        let result = mapper
            .map_request(Dialect::Gemini, Dialect::Claude, &conv)
            .unwrap();
        // Tool-role becomes User-role for Claude
        assert_eq!(result.messages[2].role, IrRole::User);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: mapping_error_cases
// ═══════════════════════════════════════════════════════════════════════

mod mapping_error_cases {
    use super::*;

    #[test]
    fn unsupported_pair_produces_error() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = text_conv(vec![(IrRole::User, "test")]);
        let err = mapper
            .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
            .unwrap_err();
        assert!(matches!(err, MapError::UnsupportedPair { .. }));
        let msg = err.to_string();
        assert!(msg.contains("Gemini"));
        assert!(msg.contains("Kimi"));
    }

    #[test]
    fn unsupported_pair_gemini_mapper() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = text_conv(vec![(IrRole::User, "test")]);
        let err = mapper
            .map_request(Dialect::Claude, Dialect::Kimi, &conv)
            .unwrap_err();
        assert!(matches!(err, MapError::UnsupportedPair { .. }));
    }

    #[test]
    fn unsupported_pair_claude_gemini_mapper() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = text_conv(vec![(IrRole::User, "test")]);
        let err = mapper
            .map_request(Dialect::OpenAi, Dialect::Kimi, &conv)
            .unwrap_err();
        assert!(matches!(err, MapError::UnsupportedPair { .. }));
    }

    #[test]
    fn empty_conversation_produces_empty_result() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::new();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn empty_conversation_gemini() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = IrConversation::new();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn factory_returns_none_for_unsupported_pair() {
        let result = default_ir_mapper(Dialect::Kimi, Dialect::Copilot);
        assert!(result.is_none());
    }

    #[test]
    fn error_types_are_serializable() {
        let err = MapError::UnsupportedPair {
            from: Dialect::OpenAi,
            to: Dialect::Codex,
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: MapError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }

    #[test]
    fn tool_error_result_preserved_through_mapping() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "run"),
            tool_call_msg("t1", "dangerous_cmd", json!({"cmd": "rm -rf /"})),
            tool_result_msg(IrRole::Tool, "t1", "permission denied", true),
        ]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        if let IrContentBlock::ToolResult { is_error, .. } = &result.messages[2].content[0] {
            assert!(is_error);
        } else {
            panic!("expected ToolResult");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: roundtrip_fidelity
// ═══════════════════════════════════════════════════════════════════════

mod roundtrip_fidelity {
    use super::*;

    #[test]
    fn openai_claude_roundtrip_preserves_text() {
        let mapper = OpenAiClaudeIrMapper;
        let orig = text_conv(vec![
            (IrRole::System, "Be helpful."),
            (IrRole::User, "What is Rust?"),
            (IrRole::Assistant, "Rust is a systems programming language."),
        ]);
        let claude = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
            .unwrap();
        let back = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &claude)
            .unwrap();
        assert_eq!(orig.len(), back.len());
        for (o, b) in orig.messages.iter().zip(back.messages.iter()) {
            assert_eq!(o.role, b.role);
            assert_eq!(o.text_content(), b.text_content());
        }
    }

    #[test]
    fn openai_gemini_roundtrip_preserves_text() {
        let mapper = OpenAiGeminiIrMapper;
        let orig = text_conv(vec![
            (IrRole::System, "You are concise."),
            (IrRole::User, "Hi"),
            (IrRole::Assistant, "Hello!"),
        ]);
        let gemini = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
            .unwrap();
        let back = mapper
            .map_request(Dialect::Gemini, Dialect::OpenAi, &gemini)
            .unwrap();
        assert_eq!(orig.len(), back.len());
        for (o, b) in orig.messages.iter().zip(back.messages.iter()) {
            assert_eq!(o.role, b.role);
            assert_eq!(o.text_content(), b.text_content());
        }
    }

    #[test]
    fn openai_claude_tool_call_roundtrip() {
        let mapper = OpenAiClaudeIrMapper;
        let orig = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Check weather"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "call_x".into(),
                    name: "weather".into(),
                    input: json!({"city": "NYC"}),
                }],
            ),
        ]);
        let claude = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
            .unwrap();
        let back = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &claude)
            .unwrap();
        let orig_tools = orig.tool_calls();
        let back_tools = back.tool_calls();
        assert_eq!(orig_tools.len(), back_tools.len());
    }

    #[test]
    fn thinking_is_lossy_through_openai() {
        let mapper = OpenAiClaudeIrMapper;
        let orig = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Think hard"),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Thinking {
                        text: "Deep thoughts...".into(),
                    },
                    IrContentBlock::Text {
                        text: "Answer.".into(),
                    },
                ],
            ),
        ]);
        let openai = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &orig)
            .unwrap();
        let back = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &openai)
            .unwrap();
        // Text survives
        assert_eq!(back.messages[1].text_content(), "Answer.");
        // Thinking block is lost
        assert!(
            !back.messages[1]
                .content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
        );
    }
}
