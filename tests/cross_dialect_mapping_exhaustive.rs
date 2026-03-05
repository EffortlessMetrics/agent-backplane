#![allow(dead_code, unused_imports, unused_variables)]
#![allow(clippy::all)]
#![allow(unknown_lints)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive cross-dialect mapping tests that verify translation correctness
//! between ALL SDK dialect pairs.
//!
//! Covers identity mappings, every supported cross-dialect pair, feature
//! mapping matrix, streaming event mapping, and edge cases.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};
use abp_dialect::Dialect;
use abp_mapper::capabilities::{Support, check_feature_support, dialect_capabilities};
use abp_mapper::{
    ClaudeGeminiIrMapper, ClaudeKimiIrMapper, CodexClaudeIrMapper, GeminiKimiIrMapper,
    IrIdentityMapper, IrMapper, MapError, OpenAiClaudeIrMapper, OpenAiCodexIrMapper,
    OpenAiCopilotIrMapper, OpenAiGeminiIrMapper, OpenAiKimiIrMapper, default_ir_mapper,
    supported_ir_pairs,
};
use serde_json::json;
use std::collections::BTreeMap;

// ===========================================================================
// Helpers
// ===========================================================================

/// Build a simple single-text-message conversation.
fn text_conversation(role: IrRole, text: &str) -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::text(role, text)])
}

/// Build a multi-turn conversation with system, user, and assistant messages.
fn multi_turn_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "Hello, world!"),
        IrMessage::text(IrRole::Assistant, "Hi! How can I help you today?"),
        IrMessage::text(IrRole::User, "What is 2+2?"),
        IrMessage::text(IrRole::Assistant, "2+2 equals 4."),
    ])
}

/// Build a conversation with tool use and tool result blocks.
fn tool_use_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You can use tools."),
        IrMessage::text(IrRole::User, "Read the file src/main.rs"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me read that file.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "read_file".into(),
                    input: json!({"path": "src/main.rs"}),
                },
            ],
        ),
    ])
}

/// Build a conversation containing a tool result block in User role (Claude-style).
fn tool_result_user_role_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Read the file"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_1".into(),
                name: "read_file".into(),
                input: json!({"path": "src/main.rs"}),
            }],
        ),
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "fn main() {}".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(IrRole::Assistant, "The file contains a main function."),
    ])
}

/// Build a conversation containing a tool result in Tool role (OpenAI-style).
fn tool_result_tool_role_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Read the file"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_1".into(),
                name: "read_file".into(),
                input: json!({"path": "src/main.rs"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "fn main() {}".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(IrRole::Assistant, "The file contains a main function."),
    ])
}

/// Conversation with thinking blocks (Claude-specific).
fn thinking_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Solve the puzzle"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Let me think step by step...".into(),
                },
                IrContentBlock::Text {
                    text: "The answer is 42.".into(),
                },
            ],
        ),
    ])
}

/// Conversation with image content blocks.
fn image_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "What is in this image?"),
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==".into(),
            }],
        ),
    ])
}

/// Conversation with an image in the system message.
fn system_image_conversation() -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::new(
        IrRole::System,
        vec![
            IrContentBlock::Text {
                text: "You are an assistant.".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64data".into(),
            },
        ],
    )])
}

/// An empty conversation.
fn empty_conversation() -> IrConversation {
    IrConversation::new()
}

/// Conversation with unicode content.
fn unicode_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "こんにちは世界 🌍 مرحبا Привет"),
        IrMessage::text(IrRole::Assistant, "Héllo wörld! 你好 🎉 Ωmega"),
    ])
}

/// Conversation with very large content.
fn large_content_conversation() -> IrConversation {
    let large_text = "x".repeat(100_000);
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, &large_text),
        IrMessage::text(IrRole::Assistant, &large_text),
    ])
}

/// Conversation with nested tool calls (assistant makes multiple tool calls).
fn nested_tool_calls_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Find and read multiple files"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "I'll search for files first.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "search".into(),
                    input: json!({"query": "*.rs"}),
                },
                IrContentBlock::ToolUse {
                    id: "call_2".into(),
                    name: "read_file".into(),
                    input: json!({"path": "src/lib.rs"}),
                },
            ],
        ),
    ])
}

/// Conversation with a tool result that is an error.
fn tool_error_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Read the file"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_1".into(),
                name: "read_file".into(),
                input: json!({"path": "/nonexistent"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "File not found".into(),
                }],
                is_error: true,
            }],
        ),
    ])
}

/// Helper: assert conversation text content is preserved through mapping.
fn assert_text_preserved(original: &IrConversation, mapped: &IrConversation) {
    let orig_texts: Vec<String> = original.messages.iter().map(|m| m.text_content()).collect();
    let mapped_texts: Vec<String> = mapped.messages.iter().map(|m| m.text_content()).collect();

    // The number of messages may change (role splitting), but all original text
    // should appear somewhere in the mapped output.
    let all_orig = orig_texts.concat();
    let all_mapped = mapped_texts.concat();
    // Allow for prefix additions like "[System] " in lossy conversions
    for text in &orig_texts {
        if !text.is_empty() {
            // Text should appear in at least one mapped message (possibly prefixed)
            assert!(
                all_mapped.contains(text) || all_mapped.contains(&text.replace("[System] ", "")),
                "Text '{}' not found in mapped output",
                &text[..text.len().min(100)]
            );
        }
    }
}

// ===========================================================================
// 1. Identity mapping tests (6 tests, one per dialect)
// ===========================================================================

mod identity {
    use super::*;

    fn verify_identity(dialect: Dialect) {
        let mapper = IrIdentityMapper;
        let conversations = vec![
            multi_turn_conversation(),
            tool_use_conversation(),
            thinking_conversation(),
            empty_conversation(),
            unicode_conversation(),
        ];

        for conv in &conversations {
            let req = mapper.map_request(dialect, dialect, conv).unwrap();
            assert_eq!(
                &req, conv,
                "Identity request mapping changed data for {dialect}"
            );

            let resp = mapper.map_response(dialect, dialect, conv).unwrap();
            assert_eq!(
                &resp, conv,
                "Identity response mapping changed data for {dialect}"
            );
        }
    }

    #[test]
    fn identity_openai() {
        verify_identity(Dialect::OpenAi);
    }

    #[test]
    fn identity_claude() {
        verify_identity(Dialect::Claude);
    }

    #[test]
    fn identity_gemini() {
        verify_identity(Dialect::Gemini);
    }

    #[test]
    fn identity_codex() {
        verify_identity(Dialect::Codex);
    }

    #[test]
    fn identity_kimi() {
        verify_identity(Dialect::Kimi);
    }

    #[test]
    fn identity_copilot() {
        verify_identity(Dialect::Copilot);
    }

    #[test]
    fn identity_preserves_metadata() {
        let mut meta = BTreeMap::new();
        meta.insert("vendor_id".into(), json!("msg_123"));
        meta.insert("custom_field".into(), json!({"nested": true}));

        let conv = IrConversation::from_messages(vec![IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text {
                text: "Hello".into(),
            }],
            metadata: meta.clone(),
        }]);

        for &dialect in Dialect::all() {
            let mapper = IrIdentityMapper;
            let result = mapper.map_request(dialect, dialect, &conv).unwrap();
            assert_eq!(
                result.messages[0].metadata, meta,
                "Metadata lost for identity mapping on {dialect}"
            );
        }
    }

    #[test]
    fn identity_factory_returns_identity_mapper_for_same_dialect() {
        for &dialect in Dialect::all() {
            let mapper = default_ir_mapper(dialect, dialect);
            assert!(
                mapper.is_some(),
                "No mapper found for identity pair ({dialect}, {dialect})"
            );
            let mapper = mapper.unwrap();
            let conv = text_conversation(IrRole::User, "test");
            let result = mapper.map_request(dialect, dialect, &conv).unwrap();
            assert_eq!(result, conv);
        }
    }
}

// ===========================================================================
// 2. Cross-dialect pair tests
// ===========================================================================

mod openai_claude {
    use super::*;

    #[test]
    fn openai_to_claude_text_preserved() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = multi_turn_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        // System, user, assistant text should all be preserved
        assert_eq!(result.messages.len(), conv.messages.len());
        for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
            assert_eq!(orig.text_content(), mapped.text_content());
        }
    }

    #[test]
    fn claude_to_openai_text_preserved() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = multi_turn_conversation();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
            assert_eq!(orig.text_content(), mapped.text_content());
        }
    }

    #[test]
    fn openai_to_claude_tool_role_becomes_user() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = tool_result_tool_role_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        // The Tool-role message should become User-role (Claude convention)
        let tool_result_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| {
                m.content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
            })
            .collect();
        for msg in &tool_result_msgs {
            assert_eq!(
                msg.role,
                IrRole::User,
                "Tool result should be User-role in Claude dialect"
            );
        }
    }

    #[test]
    fn claude_to_openai_user_tool_result_becomes_tool_role() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = tool_result_user_role_conversation();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        // User messages with only ToolResult blocks should become Tool-role
        let tool_result_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| {
                m.content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
            })
            .collect();
        for msg in &tool_result_msgs {
            assert_eq!(
                msg.role,
                IrRole::Tool,
                "User+ToolResult should become Tool-role in OpenAI dialect"
            );
        }
    }

    #[test]
    fn openai_to_claude_tool_use_preserved() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = tool_use_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let tool_calls = result.tool_calls();
        assert_eq!(tool_calls.len(), 1);
        if let IrContentBlock::ToolUse { name, id, input } = tool_calls[0] {
            assert_eq!(name, "read_file");
            assert_eq!(id, "call_1");
        } else {
            panic!("Expected ToolUse block");
        }
    }

    #[test]
    fn claude_to_openai_thinking_blocks_dropped() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = thinking_conversation();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();
        // Thinking blocks should be dropped for OpenAI
        for msg in &result.messages {
            for block in &msg.content {
                assert!(
                    !matches!(block, IrContentBlock::Thinking { .. }),
                    "Thinking block should not appear in OpenAI output"
                );
            }
        }
        // But text should be preserved
        let last = result.messages.last().unwrap();
        assert_eq!(last.text_content(), "The answer is 42.");
    }

    #[test]
    fn openai_to_claude_thinking_blocks_preserved() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = thinking_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        // Claude supports thinking blocks, so they should be preserved
        let has_thinking = result.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
        });
        assert!(
            has_thinking,
            "Thinking blocks should be preserved for Claude"
        );
    }

    #[test]
    fn openai_to_claude_rejects_system_images() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = system_image_conversation();
        let result = mapper.map_request(Dialect::OpenAi, Dialect::Claude, &conv);
        assert!(result.is_err());
        match result.unwrap_err() {
            MapError::UnmappableContent { field, .. } => {
                assert_eq!(field, "system");
            }
            other => panic!("Expected UnmappableContent, got {:?}", other),
        }
    }

    #[test]
    fn openai_to_claude_images_in_user_messages_preserved() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = image_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let has_image = result.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Image { .. }))
        });
        assert!(
            has_image,
            "Images in user messages should be preserved for Claude"
        );
    }

    #[test]
    fn openai_claude_unsupported_pair_rejected() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = text_conversation(IrRole::User, "test");
        let result = mapper.map_request(Dialect::Gemini, Dialect::Kimi, &conv);
        assert!(matches!(result, Err(MapError::UnsupportedPair { .. })));
    }

    #[test]
    fn openai_claude_supported_pairs_correct() {
        let mapper = OpenAiClaudeIrMapper;
        let pairs = mapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
        assert!(pairs.contains(&(Dialect::Claude, Dialect::OpenAi)));
        assert_eq!(pairs.len(), 2);
    }
}

mod openai_gemini {
    use super::*;

    #[test]
    fn openai_to_gemini_text_preserved() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = multi_turn_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
            assert_eq!(orig.text_content(), mapped.text_content());
        }
    }

    #[test]
    fn gemini_to_openai_text_preserved() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = multi_turn_conversation();
        let result = mapper
            .map_request(Dialect::Gemini, Dialect::OpenAi, &conv)
            .unwrap();
        for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
            assert_eq!(orig.text_content(), mapped.text_content());
        }
    }

    #[test]
    fn openai_to_gemini_tool_role_becomes_user() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = tool_result_tool_role_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        // Gemini uses user-role for function responses
        let tool_result_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| {
                m.content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
            })
            .collect();
        for msg in &tool_result_msgs {
            assert_eq!(
                msg.role,
                IrRole::User,
                "Gemini uses User-role for tool results"
            );
        }
    }

    #[test]
    fn gemini_to_openai_user_tool_result_becomes_tool_role() {
        let mapper = OpenAiGeminiIrMapper;
        // Gemini-style: tool results in User role
        let conv = tool_result_user_role_conversation();
        let result = mapper
            .map_request(Dialect::Gemini, Dialect::OpenAi, &conv)
            .unwrap();
        let tool_result_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| {
                m.content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
            })
            .collect();
        for msg in &tool_result_msgs {
            assert_eq!(
                msg.role,
                IrRole::Tool,
                "OpenAI uses Tool-role for tool results"
            );
        }
    }

    #[test]
    fn openai_to_gemini_thinking_dropped() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = thinking_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        for msg in &result.messages {
            for block in &msg.content {
                assert!(
                    !matches!(block, IrContentBlock::Thinking { .. }),
                    "Thinking blocks should be dropped for Gemini"
                );
            }
        }
    }

    #[test]
    fn gemini_to_openai_thinking_dropped() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = thinking_conversation();
        let result = mapper
            .map_request(Dialect::Gemini, Dialect::OpenAi, &conv)
            .unwrap();
        for msg in &result.messages {
            for block in &msg.content {
                assert!(!matches!(block, IrContentBlock::Thinking { .. }));
            }
        }
    }

    #[test]
    fn openai_to_gemini_system_preserved() {
        let mapper = OpenAiGeminiIrMapper;
        let conv = text_conversation(IrRole::System, "Be helpful.");
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        assert_eq!(result.messages[0].role, IrRole::System);
        assert_eq!(result.messages[0].text_content(), "Be helpful.");
    }

    #[test]
    fn openai_gemini_supported_pairs() {
        let mapper = OpenAiGeminiIrMapper;
        let pairs = mapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Gemini)));
        assert!(pairs.contains(&(Dialect::Gemini, Dialect::OpenAi)));
        assert_eq!(pairs.len(), 2);
    }
}

mod claude_gemini {
    use super::*;

    #[test]
    fn claude_to_gemini_text_preserved() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = multi_turn_conversation();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
            assert_eq!(orig.text_content(), mapped.text_content());
        }
    }

    #[test]
    fn gemini_to_claude_text_preserved() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = multi_turn_conversation();
        let result = mapper
            .map_request(Dialect::Gemini, Dialect::Claude, &conv)
            .unwrap();
        for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
            assert_eq!(orig.text_content(), mapped.text_content());
        }
    }

    #[test]
    fn claude_to_gemini_thinking_dropped() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = thinking_conversation();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        for msg in &result.messages {
            for block in &msg.content {
                assert!(
                    !matches!(block, IrContentBlock::Thinking { .. }),
                    "Thinking blocks dropped for Gemini"
                );
            }
        }
        // Text answer should survive
        let last = result.messages.last().unwrap();
        assert_eq!(last.text_content(), "The answer is 42.");
    }

    #[test]
    fn gemini_to_claude_tool_role_becomes_user() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = tool_result_tool_role_conversation();
        let result = mapper
            .map_request(Dialect::Gemini, Dialect::Claude, &conv)
            .unwrap();
        // Claude uses User-role for tool results
        let tool_result_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| {
                m.content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
            })
            .collect();
        for msg in &tool_result_msgs {
            assert_eq!(
                msg.role,
                IrRole::User,
                "Claude uses User-role for tool results"
            );
        }
    }

    #[test]
    fn claude_to_gemini_rejects_system_images() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = system_image_conversation();
        let result = mapper.map_request(Dialect::Claude, Dialect::Gemini, &conv);
        assert!(result.is_err());
        match result.unwrap_err() {
            MapError::UnmappableContent { field, .. } => assert_eq!(field, "system"),
            other => panic!("Expected UnmappableContent, got {:?}", other),
        }
    }

    #[test]
    fn claude_to_gemini_user_images_preserved() {
        let mapper = ClaudeGeminiIrMapper;
        let conv = image_conversation();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        let has_image = result.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Image { .. }))
        });
        assert!(has_image, "Images in user messages preserved for Gemini");
    }

    #[test]
    fn claude_gemini_supported_pairs() {
        let mapper = ClaudeGeminiIrMapper;
        let pairs = mapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::Claude, Dialect::Gemini)));
        assert!(pairs.contains(&(Dialect::Gemini, Dialect::Claude)));
        assert_eq!(pairs.len(), 2);
    }
}

mod codex_openai {
    use super::*;

    #[test]
    fn openai_to_codex_system_emulated_as_user() {
        let mapper = OpenAiCodexIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be helpful."),
            IrMessage::text(IrRole::User, "Hello"),
        ]);
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
            .unwrap();
        // System message emulated as [System]-prefixed User message
        let first = &result.messages[0];
        assert_eq!(first.role, IrRole::User);
        assert!(
            first.text_content().starts_with("[System]"),
            "System message should be emulated with [System] prefix"
        );
        assert!(first.text_content().contains("Be helpful."));
    }

    #[test]
    fn openai_to_codex_tool_calls_dropped() {
        let mapper = OpenAiCodexIrMapper;
        let conv = tool_use_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
            .unwrap();
        // No ToolUse blocks should remain
        for msg in &result.messages {
            for block in &msg.content {
                assert!(
                    !matches!(block, IrContentBlock::ToolUse { .. }),
                    "ToolUse blocks should be dropped for Codex"
                );
            }
        }
    }

    #[test]
    fn openai_to_codex_tool_role_dropped() {
        let mapper = OpenAiCodexIrMapper;
        let conv = tool_result_tool_role_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
            .unwrap();
        // No Tool-role messages should remain
        for msg in &result.messages {
            assert_ne!(
                msg.role,
                IrRole::Tool,
                "Tool-role messages should be dropped for Codex"
            );
        }
    }

    #[test]
    fn openai_to_codex_thinking_dropped() {
        let mapper = OpenAiCodexIrMapper;
        let conv = thinking_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
            .unwrap();
        for msg in &result.messages {
            for block in &msg.content {
                assert!(!matches!(block, IrContentBlock::Thinking { .. }));
            }
        }
    }

    #[test]
    fn openai_to_codex_images_become_placeholders() {
        let mapper = OpenAiCodexIrMapper;
        let conv = image_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
            .unwrap();
        let has_placeholder = result.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Text { text } if text.starts_with("[Image:")))
        });
        assert!(
            has_placeholder,
            "Images should become [Image: ...] placeholders"
        );
        // No actual image blocks should remain
        for msg in &result.messages {
            for block in &msg.content {
                assert!(!matches!(block, IrContentBlock::Image { .. }));
            }
        }
    }

    #[test]
    fn codex_to_openai_is_lossless() {
        let mapper = OpenAiCodexIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Hello"),
            IrMessage::text(IrRole::Assistant, "World"),
        ]);
        let result = mapper
            .map_request(Dialect::Codex, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(result, conv, "Codex → OpenAI should be lossless identity");
    }

    #[test]
    fn codex_openai_supported_pairs() {
        let mapper = OpenAiCodexIrMapper;
        let pairs = mapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Codex)));
        assert!(pairs.contains(&(Dialect::Codex, Dialect::OpenAi)));
    }
}

mod kimi_openai {
    use super::*;

    #[test]
    fn openai_to_kimi_text_preserved() {
        let mapper = OpenAiKimiIrMapper;
        let conv = multi_turn_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Kimi, &conv)
            .unwrap();
        for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
            assert_eq!(orig.text_content(), mapped.text_content());
        }
    }

    #[test]
    fn kimi_to_openai_text_preserved() {
        let mapper = OpenAiKimiIrMapper;
        let conv = multi_turn_conversation();
        let result = mapper
            .map_request(Dialect::Kimi, Dialect::OpenAi, &conv)
            .unwrap();
        for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
            assert_eq!(orig.text_content(), mapped.text_content());
        }
    }

    #[test]
    fn openai_to_kimi_rejects_images() {
        let mapper = OpenAiKimiIrMapper;
        let conv = image_conversation();
        let result = mapper.map_request(Dialect::OpenAi, Dialect::Kimi, &conv);
        assert!(result.is_err());
        match result.unwrap_err() {
            MapError::UnmappableContent { field, reason } => {
                assert!(reason.contains("Kimi"));
                assert!(reason.contains("image"));
            }
            other => panic!("Expected UnmappableContent, got {:?}", other),
        }
    }

    #[test]
    fn openai_to_kimi_thinking_dropped() {
        let mapper = OpenAiKimiIrMapper;
        let conv = thinking_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Kimi, &conv)
            .unwrap();
        for msg in &result.messages {
            for block in &msg.content {
                assert!(!matches!(block, IrContentBlock::Thinking { .. }));
            }
        }
    }

    #[test]
    fn kimi_to_openai_thinking_dropped() {
        let mapper = OpenAiKimiIrMapper;
        let conv = thinking_conversation();
        let result = mapper
            .map_request(Dialect::Kimi, Dialect::OpenAi, &conv)
            .unwrap();
        for msg in &result.messages {
            for block in &msg.content {
                assert!(!matches!(block, IrContentBlock::Thinking { .. }));
            }
        }
    }

    #[test]
    fn openai_to_kimi_tool_use_preserved() {
        let mapper = OpenAiKimiIrMapper;
        let conv = tool_use_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Kimi, &conv)
            .unwrap();
        let tool_calls = result.tool_calls();
        assert_eq!(tool_calls.len(), 1);
    }

    #[test]
    fn kimi_openai_supported_pairs() {
        let mapper = OpenAiKimiIrMapper;
        let pairs = mapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Kimi)));
        assert!(pairs.contains(&(Dialect::Kimi, Dialect::OpenAi)));
    }
}

mod copilot_openai {
    use super::*;

    #[test]
    fn openai_to_copilot_text_preserved() {
        let mapper = OpenAiCopilotIrMapper;
        let conv = multi_turn_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Copilot, &conv)
            .unwrap();
        for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
            assert_eq!(orig.text_content(), mapped.text_content());
        }
    }

    #[test]
    fn copilot_to_openai_text_preserved() {
        let mapper = OpenAiCopilotIrMapper;
        let conv = multi_turn_conversation();
        let result = mapper
            .map_request(Dialect::Copilot, Dialect::OpenAi, &conv)
            .unwrap();
        for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
            assert_eq!(orig.text_content(), mapped.text_content());
        }
    }

    #[test]
    fn openai_to_copilot_rejects_images() {
        let mapper = OpenAiCopilotIrMapper;
        let conv = image_conversation();
        let result = mapper.map_request(Dialect::OpenAi, Dialect::Copilot, &conv);
        assert!(result.is_err());
        match result.unwrap_err() {
            MapError::UnmappableContent { field, reason } => {
                assert!(reason.contains("Copilot"));
                assert!(reason.contains("image"));
            }
            other => panic!("Expected UnmappableContent, got {:?}", other),
        }
    }

    #[test]
    fn openai_to_copilot_thinking_dropped() {
        let mapper = OpenAiCopilotIrMapper;
        let conv = thinking_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Copilot, &conv)
            .unwrap();
        for msg in &result.messages {
            for block in &msg.content {
                assert!(!matches!(block, IrContentBlock::Thinking { .. }));
            }
        }
    }

    #[test]
    fn copilot_to_openai_tool_use_preserved() {
        let mapper = OpenAiCopilotIrMapper;
        let conv = tool_use_conversation();
        let result = mapper
            .map_request(Dialect::Copilot, Dialect::OpenAi, &conv)
            .unwrap();
        let tool_calls = result.tool_calls();
        assert_eq!(tool_calls.len(), 1);
    }

    #[test]
    fn copilot_openai_supported_pairs() {
        let mapper = OpenAiCopilotIrMapper;
        let pairs = mapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Copilot)));
        assert!(pairs.contains(&(Dialect::Copilot, Dialect::OpenAi)));
    }
}

mod claude_kimi {
    use super::*;

    #[test]
    fn claude_to_kimi_text_preserved() {
        let mapper = ClaudeKimiIrMapper;
        let conv = multi_turn_conversation();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Kimi, &conv)
            .unwrap();
        for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
            assert_eq!(orig.text_content(), mapped.text_content());
        }
    }

    #[test]
    fn kimi_to_claude_text_preserved() {
        let mapper = ClaudeKimiIrMapper;
        let conv = multi_turn_conversation();
        let result = mapper
            .map_request(Dialect::Kimi, Dialect::Claude, &conv)
            .unwrap();
        for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
            assert_eq!(orig.text_content(), mapped.text_content());
        }
    }

    #[test]
    fn claude_to_kimi_thinking_dropped() {
        let mapper = ClaudeKimiIrMapper;
        let conv = thinking_conversation();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Kimi, &conv)
            .unwrap();
        for msg in &result.messages {
            for block in &msg.content {
                assert!(!matches!(block, IrContentBlock::Thinking { .. }));
            }
        }
    }

    #[test]
    fn claude_to_kimi_user_tool_results_become_tool_role() {
        let mapper = ClaudeKimiIrMapper;
        let conv = tool_result_user_role_conversation();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Kimi, &conv)
            .unwrap();
        let tool_result_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| {
                m.content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
            })
            .collect();
        for msg in &tool_result_msgs {
            assert_eq!(
                msg.role,
                IrRole::Tool,
                "Kimi uses Tool-role for tool results"
            );
        }
    }

    #[test]
    fn kimi_to_claude_tool_role_becomes_user() {
        let mapper = ClaudeKimiIrMapper;
        let conv = tool_result_tool_role_conversation();
        let result = mapper
            .map_request(Dialect::Kimi, Dialect::Claude, &conv)
            .unwrap();
        let tool_result_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| {
                m.content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
            })
            .collect();
        for msg in &tool_result_msgs {
            assert_eq!(
                msg.role,
                IrRole::User,
                "Claude uses User-role for tool results"
            );
        }
    }

    #[test]
    fn claude_to_kimi_rejects_images() {
        let mapper = ClaudeKimiIrMapper;
        let conv = image_conversation();
        let result = mapper.map_request(Dialect::Claude, Dialect::Kimi, &conv);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MapError::UnmappableContent { .. }
        ));
    }

    #[test]
    fn claude_kimi_supported_pairs() {
        let mapper = ClaudeKimiIrMapper;
        let pairs = mapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::Claude, Dialect::Kimi)));
        assert!(pairs.contains(&(Dialect::Kimi, Dialect::Claude)));
    }
}

mod gemini_kimi {
    use super::*;

    #[test]
    fn gemini_to_kimi_text_preserved() {
        let mapper = GeminiKimiIrMapper;
        let conv = multi_turn_conversation();
        let result = mapper
            .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
            .unwrap();
        for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
            assert_eq!(orig.text_content(), mapped.text_content());
        }
    }

    #[test]
    fn kimi_to_gemini_text_preserved() {
        let mapper = GeminiKimiIrMapper;
        let conv = multi_turn_conversation();
        let result = mapper
            .map_request(Dialect::Kimi, Dialect::Gemini, &conv)
            .unwrap();
        for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
            assert_eq!(orig.text_content(), mapped.text_content());
        }
    }

    #[test]
    fn gemini_to_kimi_user_tool_results_become_tool_role() {
        let mapper = GeminiKimiIrMapper;
        let conv = tool_result_user_role_conversation();
        let result = mapper
            .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
            .unwrap();
        let tool_result_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| {
                m.content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
            })
            .collect();
        for msg in &tool_result_msgs {
            assert_eq!(msg.role, IrRole::Tool);
        }
    }

    #[test]
    fn kimi_to_gemini_tool_role_becomes_user() {
        let mapper = GeminiKimiIrMapper;
        let conv = tool_result_tool_role_conversation();
        let result = mapper
            .map_request(Dialect::Kimi, Dialect::Gemini, &conv)
            .unwrap();
        let tool_result_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| {
                m.content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
            })
            .collect();
        for msg in &tool_result_msgs {
            assert_eq!(
                msg.role,
                IrRole::User,
                "Gemini uses User-role for tool results"
            );
        }
    }

    #[test]
    fn gemini_to_kimi_rejects_images() {
        let mapper = GeminiKimiIrMapper;
        let conv = image_conversation();
        let result = mapper.map_request(Dialect::Gemini, Dialect::Kimi, &conv);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MapError::UnmappableContent { .. }
        ));
    }

    #[test]
    fn gemini_to_kimi_thinking_dropped() {
        let mapper = GeminiKimiIrMapper;
        let conv = thinking_conversation();
        let result = mapper
            .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
            .unwrap();
        for msg in &result.messages {
            for block in &msg.content {
                assert!(!matches!(block, IrContentBlock::Thinking { .. }));
            }
        }
    }

    #[test]
    fn gemini_kimi_supported_pairs() {
        let mapper = GeminiKimiIrMapper;
        let pairs = mapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::Gemini, Dialect::Kimi)));
        assert!(pairs.contains(&(Dialect::Kimi, Dialect::Gemini)));
    }
}

mod codex_claude {
    use super::*;

    #[test]
    fn claude_to_codex_system_emulated() {
        let mapper = CodexClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be concise."),
            IrMessage::text(IrRole::User, "Hi"),
        ]);
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Codex, &conv)
            .unwrap();
        let first = &result.messages[0];
        assert_eq!(first.role, IrRole::User);
        assert!(first.text_content().starts_with("[System]"));
        assert!(first.text_content().contains("Be concise."));
    }

    #[test]
    fn claude_to_codex_thinking_dropped() {
        let mapper = CodexClaudeIrMapper;
        let conv = thinking_conversation();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Codex, &conv)
            .unwrap();
        for msg in &result.messages {
            for block in &msg.content {
                assert!(!matches!(block, IrContentBlock::Thinking { .. }));
            }
        }
    }

    #[test]
    fn claude_to_codex_tool_calls_dropped() {
        let mapper = CodexClaudeIrMapper;
        let conv = tool_use_conversation();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Codex, &conv)
            .unwrap();
        for msg in &result.messages {
            for block in &msg.content {
                assert!(!matches!(block, IrContentBlock::ToolUse { .. }));
            }
        }
    }

    #[test]
    fn claude_to_codex_images_become_placeholders() {
        let mapper = CodexClaudeIrMapper;
        let conv = image_conversation();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Codex, &conv)
            .unwrap();
        let has_placeholder = result.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Text { text } if text.starts_with("[Image:")))
        });
        assert!(has_placeholder);
    }

    #[test]
    fn codex_to_claude_lossless_text() {
        let mapper = CodexClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Hello"),
            IrMessage::text(IrRole::Assistant, "World"),
        ]);
        let result = mapper
            .map_request(Dialect::Codex, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result, conv);
    }

    #[test]
    fn codex_to_claude_rejects_unmappable_tools() {
        let mapper = CodexClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_1".into(),
                name: "apply_patch".into(),
                input: json!({"patch": "..."}),
            }],
        )]);
        let result = mapper.map_request(Dialect::Codex, Dialect::Claude, &conv);
        assert!(result.is_err());
        match result.unwrap_err() {
            MapError::UnmappableTool { name, .. } => assert_eq!(name, "apply_patch"),
            other => panic!("Expected UnmappableTool, got {:?}", other),
        }
    }

    #[test]
    fn codex_to_claude_rejects_apply_diff() {
        let mapper = CodexClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_1".into(),
                name: "apply_diff".into(),
                input: json!({"diff": "..."}),
            }],
        )]);
        let result = mapper.map_request(Dialect::Codex, Dialect::Claude, &conv);
        assert!(matches!(result, Err(MapError::UnmappableTool { .. })));
    }

    #[test]
    fn codex_claude_supported_pairs() {
        let mapper = CodexClaudeIrMapper;
        let pairs = mapper.supported_pairs();
        assert!(pairs.contains(&(Dialect::Codex, Dialect::Claude)));
        assert!(pairs.contains(&(Dialect::Claude, Dialect::Codex)));
    }
}

// ===========================================================================
// 3. Feature mapping matrix tests
// ===========================================================================

mod feature_matrix {
    use super::*;

    #[test]
    fn thinking_native_only_for_claude() {
        for &dialect in Dialect::all() {
            let caps = dialect_capabilities(dialect);
            if dialect == Dialect::Claude {
                assert_eq!(caps.thinking, Support::Native);
            } else {
                assert_eq!(caps.thinking, Support::None);
            }
        }
    }

    #[test]
    fn images_native_for_openai_claude_gemini() {
        let image_dialects = [Dialect::OpenAi, Dialect::Claude, Dialect::Gemini];
        let no_image_dialects = [Dialect::Codex, Dialect::Kimi, Dialect::Copilot];

        for &d in &image_dialects {
            assert!(
                dialect_capabilities(d).images.is_native(),
                "{d} should support images"
            );
        }
        for &d in &no_image_dialects {
            assert!(
                !dialect_capabilities(d).images.is_native(),
                "{d} should not support images"
            );
        }
    }

    #[test]
    fn tool_use_supported_by_most_dialects() {
        let tool_dialects = [
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Kimi,
            Dialect::Copilot,
        ];
        for &d in &tool_dialects {
            assert!(
                dialect_capabilities(d).tool_use.is_native(),
                "{d} should support tool use"
            );
        }
        assert!(
            !dialect_capabilities(Dialect::Codex).tool_use.is_native(),
            "Codex should NOT support tool use"
        );
    }

    #[test]
    fn tool_role_openai_compatible_dialects() {
        // OpenAI, Kimi, Copilot use dedicated Tool role
        let tool_role_dialects = [Dialect::OpenAi, Dialect::Kimi, Dialect::Copilot];
        for &d in &tool_role_dialects {
            assert!(
                dialect_capabilities(d).tool_role.is_native(),
                "{d} should have native tool role"
            );
        }
        // Claude and Gemini use User role for tool results
        assert!(!dialect_capabilities(Dialect::Claude).tool_role.is_native());
        assert!(!dialect_capabilities(Dialect::Gemini).tool_role.is_native());
        assert!(!dialect_capabilities(Dialect::Codex).tool_role.is_native());
    }

    #[test]
    fn system_prompt_supported_by_most() {
        for &d in Dialect::all() {
            let caps = dialect_capabilities(d);
            if d == Dialect::Codex {
                assert!(
                    !caps.system_prompt.is_native(),
                    "Codex should NOT support system prompts"
                );
            } else {
                assert!(
                    caps.system_prompt.is_native(),
                    "{d} should support system prompts"
                );
            }
        }
    }

    #[test]
    fn no_dialect_supports_system_images() {
        for &d in Dialect::all() {
            assert!(
                !dialect_capabilities(d).system_images.is_native(),
                "{d} should NOT support system images"
            );
        }
    }

    #[test]
    fn streaming_supported_by_all_except_codex() {
        for &d in Dialect::all() {
            let caps = dialect_capabilities(d);
            if d == Dialect::Codex {
                assert!(!caps.streaming.is_native());
            } else {
                assert!(caps.streaming.is_native(), "{d} should support streaming");
            }
        }
    }

    #[test]
    fn check_feature_support_thinking_on_all_dialects() {
        for &d in Dialect::all() {
            let caps = dialect_capabilities(d);
            let result = check_feature_support("thinking", &caps);
            if d == Dialect::Claude {
                assert!(result.is_none(), "Claude supports thinking natively");
            } else {
                assert!(result.is_some(), "{d} does not support thinking");
            }
        }
    }

    #[test]
    fn check_feature_support_images_on_all_dialects() {
        for &d in Dialect::all() {
            let caps = dialect_capabilities(d);
            let result = check_feature_support("images", &caps);
            match d {
                Dialect::OpenAi | Dialect::Claude | Dialect::Gemini => {
                    assert!(result.is_none(), "{d} should support images");
                }
                _ => {
                    assert!(result.is_some(), "{d} should not support images");
                }
            }
        }
    }

    #[test]
    fn thinking_blocks_dropped_when_mapping_from_claude_to_any_non_claude() {
        let conv = thinking_conversation();
        for &target in Dialect::all() {
            if target == Dialect::Claude {
                continue;
            }
            let mapper = default_ir_mapper(Dialect::Claude, target);
            if let Some(mapper) = mapper {
                let result = mapper.map_request(Dialect::Claude, target, &conv);
                if let Ok(mapped) = result {
                    for msg in &mapped.messages {
                        for block in &msg.content {
                            assert!(
                                !matches!(block, IrContentBlock::Thinking { .. }),
                                "Thinking should be dropped for {target}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn images_rejected_by_kimi_copilot_through_any_source() {
        let conv = image_conversation();
        let image_rejecting = [Dialect::Kimi, Dialect::Copilot];

        for &target in &image_rejecting {
            for &source in &[Dialect::OpenAi, Dialect::Claude, Dialect::Gemini] {
                let mapper = default_ir_mapper(source, target);
                if let Some(mapper) = mapper {
                    let result = mapper.map_request(source, target, &conv);
                    assert!(
                        result.is_err(),
                        "Mapping {source} → {target} with images should fail"
                    );
                }
            }
        }
    }

    #[test]
    fn codex_drops_tools_from_all_sources() {
        let conv = tool_use_conversation();
        for &source in &[Dialect::OpenAi, Dialect::Claude] {
            let mapper = default_ir_mapper(source, Dialect::Codex);
            if let Some(mapper) = mapper {
                let result = mapper.map_request(source, Dialect::Codex, &conv);
                if let Ok(mapped) = result {
                    for msg in &mapped.messages {
                        for block in &msg.content {
                            assert!(
                                !matches!(block, IrContentBlock::ToolUse { .. }),
                                "ToolUse should be dropped for Codex from {source}"
                            );
                        }
                    }
                }
            }
        }
    }
}

// ===========================================================================
// 4. Factory and supported-pairs tests
// ===========================================================================

mod factory {
    use super::*;

    #[test]
    fn all_identity_pairs_have_mappers() {
        for &d in Dialect::all() {
            assert!(
                default_ir_mapper(d, d).is_some(),
                "No identity mapper for {d}"
            );
        }
    }

    #[test]
    fn all_documented_cross_pairs_have_mappers() {
        let expected_pairs = vec![
            (Dialect::OpenAi, Dialect::Claude),
            (Dialect::Claude, Dialect::OpenAi),
            (Dialect::OpenAi, Dialect::Gemini),
            (Dialect::Gemini, Dialect::OpenAi),
            (Dialect::Claude, Dialect::Gemini),
            (Dialect::Gemini, Dialect::Claude),
            (Dialect::OpenAi, Dialect::Codex),
            (Dialect::Codex, Dialect::OpenAi),
            (Dialect::OpenAi, Dialect::Kimi),
            (Dialect::Kimi, Dialect::OpenAi),
            (Dialect::Claude, Dialect::Kimi),
            (Dialect::Kimi, Dialect::Claude),
            (Dialect::OpenAi, Dialect::Copilot),
            (Dialect::Copilot, Dialect::OpenAi),
            (Dialect::Gemini, Dialect::Kimi),
            (Dialect::Kimi, Dialect::Gemini),
            (Dialect::Codex, Dialect::Claude),
            (Dialect::Claude, Dialect::Codex),
        ];

        for (from, to) in &expected_pairs {
            assert!(
                default_ir_mapper(*from, *to).is_some(),
                "No mapper for {from} → {to}"
            );
        }
    }

    #[test]
    fn supported_ir_pairs_is_complete() {
        let pairs = supported_ir_pairs();

        // Should include 6 identity pairs + 18 cross-dialect pairs = 24
        assert!(
            pairs.len() >= 24,
            "Expected at least 24 pairs, got {}",
            pairs.len()
        );

        // All identity pairs present
        for &d in Dialect::all() {
            assert!(pairs.contains(&(d, d)), "Missing identity pair ({d}, {d})");
        }
    }

    #[test]
    fn unsupported_pairs_return_none() {
        // These pairs currently have no direct mapper
        let unsupported = vec![
            (Dialect::Codex, Dialect::Copilot),
            (Dialect::Copilot, Dialect::Codex),
            (Dialect::Codex, Dialect::Kimi),
            (Dialect::Kimi, Dialect::Codex),
            (Dialect::Codex, Dialect::Gemini),
            (Dialect::Gemini, Dialect::Codex),
            (Dialect::Copilot, Dialect::Claude),
            (Dialect::Claude, Dialect::Copilot),
            (Dialect::Copilot, Dialect::Gemini),
            (Dialect::Gemini, Dialect::Copilot),
            (Dialect::Copilot, Dialect::Kimi),
            (Dialect::Kimi, Dialect::Copilot),
        ];

        for (from, to) in &unsupported {
            assert!(
                default_ir_mapper(*from, *to).is_none(),
                "Unexpected mapper for {from} → {to}"
            );
        }
    }

    #[test]
    fn factory_mappers_handle_basic_conversations() {
        let conv = text_conversation(IrRole::User, "Hello");
        let pairs = supported_ir_pairs();
        for (from, to) in &pairs {
            let mapper = default_ir_mapper(*from, *to).unwrap();
            let result = mapper.map_request(*from, *to, &conv);
            assert!(
                result.is_ok(),
                "Simple text mapping {from} → {to} should succeed"
            );
        }
    }
}

// ===========================================================================
// 5. Streaming event mapping tests (at IR conversation level)
// ===========================================================================

mod streaming_events {
    use super::*;

    #[test]
    fn text_delta_preserved_across_all_supported_pairs() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Text {
                text: "partial response...".into(),
            }],
        )]);

        for (from, to) in &supported_ir_pairs() {
            let mapper = default_ir_mapper(*from, *to).unwrap();
            let result = mapper.map_response(*from, *to, &conv);
            if let Ok(mapped) = result {
                let text = mapped
                    .messages
                    .iter()
                    .map(|m| m.text_content())
                    .collect::<String>();
                // Text should appear somewhere (possibly with prefix for Codex)
                assert!(
                    text.contains("partial response") || text.is_empty(),
                    "Text delta lost in {from} → {to}: got '{text}'"
                );
            }
        }
    }

    #[test]
    fn tool_call_events_mapped_across_tool_supporting_dialects() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "call_stream_1".into(),
                name: "search".into(),
                input: json!({"query": "test"}),
            }],
        )]);

        let tool_supporting = [
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Kimi,
            Dialect::Copilot,
        ];

        for &from in &tool_supporting {
            for &to in &tool_supporting {
                if let Some(mapper) = default_ir_mapper(from, to) {
                    let result = mapper.map_response(from, to, &conv);
                    if let Ok(mapped) = result {
                        let calls = mapped.tool_calls();
                        assert!(!calls.is_empty(), "Tool call lost in {from} → {to}");
                        if let IrContentBlock::ToolUse { name, .. } = calls[0] {
                            assert_eq!(name, "search");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn tool_result_error_flag_preserved() {
        let conv = tool_error_conversation();

        // Map through OpenAI-compatible pairs
        let mapper = OpenAiClaudeIrMapper;
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let has_error_result = result.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolResult { is_error: true, .. }))
        });
        assert!(has_error_result, "is_error flag should be preserved");
    }

    #[test]
    fn usage_serde_roundtrip_across_mapping() {
        let usage = IrUsage::from_io(100, 50);
        let json = serde_json::to_string(&usage).unwrap();
        let back: IrUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(usage, back);
        assert_eq!(back.input_tokens, 100);
        assert_eq!(back.output_tokens, 50);
        assert_eq!(back.total_tokens, 150);
    }

    #[test]
    fn usage_with_cache_roundtrip() {
        let usage = IrUsage::with_cache(200, 80, 50, 30);
        let json = serde_json::to_string(&usage).unwrap();
        let back: IrUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(usage, back);
    }

    #[test]
    fn usage_merge_is_additive() {
        let a = IrUsage::from_io(100, 50);
        let b = IrUsage::with_cache(200, 80, 30, 10);
        let merged = a.merge(b);
        assert_eq!(merged.input_tokens, 300);
        assert_eq!(merged.output_tokens, 130);
        assert_eq!(merged.total_tokens, 430);
        assert_eq!(merged.cache_read_tokens, 30);
        assert_eq!(merged.cache_write_tokens, 10);
    }
}

// ===========================================================================
// 6. Edge cases
// ===========================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn empty_conversation_maps_through_all_pairs() {
        let conv = empty_conversation();
        for (from, to) in &supported_ir_pairs() {
            let mapper = default_ir_mapper(*from, *to).unwrap();
            let result = mapper.map_request(*from, *to, &conv).unwrap();
            assert!(
                result.is_empty(),
                "Empty conv should stay empty for {from} → {to}"
            );
        }
    }

    #[test]
    fn unicode_preserved_through_all_pairs() {
        let conv = unicode_conversation();
        for (from, to) in &supported_ir_pairs() {
            let mapper = default_ir_mapper(*from, *to).unwrap();
            let result = mapper.map_request(*from, *to, &conv);
            if let Ok(mapped) = result {
                let all_text: String = mapped.messages.iter().map(|m| m.text_content()).collect();
                // Key unicode characters should be preserved
                assert!(
                    all_text.contains("こんにちは世界") || all_text.contains("🌍"),
                    "Unicode lost in {from} → {to}"
                );
            }
        }
    }

    #[test]
    fn large_content_maps_through_identity() {
        let conv = large_content_conversation();
        let mapper = IrIdentityMapper;
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
            .unwrap();
        assert_eq!(result.messages[0].text_content().len(), 100_000);
        assert_eq!(result.messages[1].text_content().len(), 100_000);
    }

    #[test]
    fn large_content_maps_through_openai_claude() {
        let conv = large_content_conversation();
        let mapper = OpenAiClaudeIrMapper;
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.messages[0].text_content().len(), 100_000);
    }

    #[test]
    fn nested_tool_calls_preserved() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = nested_tool_calls_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        let tool_calls = result.tool_calls();
        assert_eq!(tool_calls.len(), 2, "Both tool calls should be preserved");
    }

    #[test]
    fn nested_tool_calls_dropped_for_codex() {
        let mapper = OpenAiCodexIrMapper;
        let conv = nested_tool_calls_conversation();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
            .unwrap();
        let tool_calls = result.tool_calls();
        assert_eq!(
            tool_calls.len(),
            0,
            "All tool calls should be dropped for Codex"
        );
    }

    #[test]
    fn multi_turn_with_tools_maps_openai_to_claude() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "You can use tools."),
            IrMessage::text(IrRole::User, "Read file"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "read_file".into(),
                    input: json!({"path": "a.rs"}),
                }],
            ),
            IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "c1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "fn main(){}".into(),
                    }],
                    is_error: false,
                }],
            ),
            IrMessage::text(IrRole::Assistant, "Here is the file content."),
        ]);

        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();

        // Tool role should become User role
        let tool_result_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| {
                m.content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
            })
            .collect();
        assert_eq!(tool_result_msgs.len(), 1);
        assert_eq!(tool_result_msgs[0].role, IrRole::User);

        // System and assistant text preserved
        assert_eq!(
            result.system_message().unwrap().text_content(),
            "You can use tools."
        );
        assert_eq!(
            result.last_assistant().unwrap().text_content(),
            "Here is the file content."
        );
    }

    #[test]
    fn multi_turn_with_tools_maps_claude_to_openai() {
        let mapper = OpenAiClaudeIrMapper;
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "You can use tools."),
            IrMessage::text(IrRole::User, "Read file"),
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "read_file".into(),
                    input: json!({"path": "a.rs"}),
                }],
            ),
            // Claude-style: tool result in User role
            IrMessage::new(
                IrRole::User,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "c1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "fn main(){}".into(),
                    }],
                    is_error: false,
                }],
            ),
            IrMessage::text(IrRole::Assistant, "Here is the file content."),
        ]);

        let result = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();

        // User+ToolResult should become Tool role
        let tool_result_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| {
                m.content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
            })
            .collect();
        assert_eq!(tool_result_msgs.len(), 1);
        assert_eq!(tool_result_msgs[0].role, IrRole::Tool);
    }

    #[test]
    fn system_message_handling_across_dialects() {
        let conv = text_conversation(IrRole::System, "You are helpful.");

        // Dialects that support system messages natively
        let native_system = [
            Dialect::OpenAi,
            Dialect::Claude,
            Dialect::Gemini,
            Dialect::Kimi,
            Dialect::Copilot,
        ];

        for &target in &native_system {
            for &source in &native_system {
                if let Some(mapper) = default_ir_mapper(source, target) {
                    let result = mapper.map_request(source, target, &conv).unwrap();
                    assert_eq!(
                        result.messages[0].role,
                        IrRole::System,
                        "System role preserved for {source} → {target}"
                    );
                }
            }
        }
    }

    #[test]
    fn system_message_emulated_for_codex() {
        let conv = text_conversation(IrRole::System, "You are helpful.");

        // OpenAI → Codex
        let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Codex).unwrap();
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
            .unwrap();
        assert_eq!(result.messages[0].role, IrRole::User);
        assert!(result.messages[0].text_content().contains("[System]"));

        // Claude → Codex
        let mapper = default_ir_mapper(Dialect::Claude, Dialect::Codex).unwrap();
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Codex, &conv)
            .unwrap();
        assert_eq!(result.messages[0].role, IrRole::User);
        assert!(result.messages[0].text_content().contains("[System]"));
    }

    #[test]
    fn mixed_user_content_with_tool_results_split_correctly() {
        // User message with both text and tool results (Claude-style)
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text {
                    text: "Here's context:".into(),
                },
                IrContentBlock::ToolResult {
                    tool_use_id: "c1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "result data".into(),
                    }],
                    is_error: false,
                },
            ],
        )]);

        // Claude → OpenAI: mixed content should be split
        let mapper = OpenAiClaudeIrMapper;
        let result = mapper
            .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
            .unwrap();

        // Should have a User message with text and a Tool message with result
        let user_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| m.role == IrRole::User)
            .collect();
        let tool_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| m.role == IrRole::Tool)
            .collect();

        assert!(
            !user_msgs.is_empty(),
            "Should have user message for text content"
        );
        assert!(
            !tool_msgs.is_empty(),
            "Should have tool message for tool result"
        );
    }

    #[test]
    fn metadata_preserved_through_mapping() {
        let mut meta = BTreeMap::new();
        meta.insert("request_id".into(), json!("req_123"));
        meta.insert("custom".into(), json!({"nested": [1, 2, 3]}));

        let conv = IrConversation::from_messages(vec![IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text {
                text: "Hello".into(),
            }],
            metadata: meta.clone(),
        }]);

        // Test across a sample of mappers
        let test_pairs = vec![
            (Dialect::OpenAi, Dialect::Claude),
            (Dialect::Claude, Dialect::OpenAi),
            (Dialect::OpenAi, Dialect::Gemini),
            (Dialect::OpenAi, Dialect::Kimi),
            (Dialect::OpenAi, Dialect::Copilot),
        ];

        for (from, to) in &test_pairs {
            let mapper = default_ir_mapper(*from, *to).unwrap();
            let result = mapper.map_request(*from, *to, &conv).unwrap();
            let first_msg = &result.messages[0];
            assert_eq!(
                first_msg.metadata, meta,
                "Metadata should be preserved for {from} → {to}"
            );
        }
    }

    #[test]
    fn empty_text_messages_handled() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, ""),
            IrMessage::text(IrRole::Assistant, ""),
        ]);

        for (from, to) in &supported_ir_pairs() {
            let mapper = default_ir_mapper(*from, *to).unwrap();
            let result = mapper.map_request(*from, *to, &conv);
            // Should not panic
            assert!(
                result.is_ok(),
                "Empty text messages should not fail for {from} → {to}"
            );
        }
    }

    #[test]
    fn tool_definition_serde_roundtrip() {
        let def = IrToolDefinition {
            name: "read_file".into(),
            description: "Read a file from disk".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path"}
                },
                "required": ["path"]
            }),
        };
        let json = serde_json::to_string(&def).unwrap();
        let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(def, back);
    }

    #[test]
    fn conversation_accessors_consistent_after_mapping() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "System prompt"),
            IrMessage::text(IrRole::User, "User message"),
            IrMessage::text(IrRole::Assistant, "Assistant reply"),
        ]);

        let mapper = OpenAiClaudeIrMapper;
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();

        assert!(result.system_message().is_some());
        assert_eq!(
            result.system_message().unwrap().text_content(),
            "System prompt"
        );
        assert!(result.last_assistant().is_some());
        assert_eq!(
            result.last_assistant().unwrap().text_content(),
            "Assistant reply"
        );
        assert_eq!(result.len(), 3);
        assert!(!result.is_empty());
    }

    #[test]
    fn request_and_response_mapping_both_work() {
        let conv = multi_turn_conversation();

        for (from, to) in &supported_ir_pairs() {
            let mapper = default_ir_mapper(*from, *to).unwrap();
            let req_result = mapper.map_request(*from, *to, &conv);
            let resp_result = mapper.map_response(*from, *to, &conv);

            // Both should succeed or both fail for simple text conversations
            assert_eq!(
                req_result.is_ok(),
                resp_result.is_ok(),
                "request/response symmetry broken for {from} → {to}"
            );
        }
    }

    #[test]
    fn map_error_variants_are_descriptive() {
        let err = MapError::UnsupportedPair {
            from: Dialect::Kimi,
            to: Dialect::Copilot,
        };
        let msg = err.to_string();
        assert!(msg.contains("Kimi") && msg.contains("Copilot"));

        let err = MapError::LossyConversion {
            field: "thinking".into(),
            reason: "no equivalent".into(),
        };
        assert!(err.to_string().contains("thinking"));

        let err = MapError::UnmappableTool {
            name: "computer_use".into(),
            reason: "not supported".into(),
        };
        assert!(err.to_string().contains("computer_use"));

        let err = MapError::IncompatibleCapability {
            capability: "vision".into(),
            reason: "not available".into(),
        };
        assert!(err.to_string().contains("vision"));

        let err = MapError::UnmappableContent {
            field: "system".into(),
            reason: "image blocks".into(),
        };
        assert!(err.to_string().contains("system"));
    }

    #[test]
    fn map_error_serde_roundtrip() {
        let errors = vec![
            MapError::UnsupportedPair {
                from: Dialect::OpenAi,
                to: Dialect::Claude,
            },
            MapError::LossyConversion {
                field: "thinking".into(),
                reason: "dropped".into(),
            },
            MapError::UnmappableTool {
                name: "bash".into(),
                reason: "restricted".into(),
            },
            MapError::IncompatibleCapability {
                capability: "vision".into(),
                reason: "none".into(),
            },
            MapError::UnmappableContent {
                field: "system".into(),
                reason: "images".into(),
            },
        ];

        for err in &errors {
            let json = serde_json::to_string(err).unwrap();
            let back: MapError = serde_json::from_str(&json).unwrap();
            assert_eq!(*err, back);
        }
    }

    #[test]
    fn ir_conversation_helper_methods_after_mapping() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "sys"),
            IrMessage::text(IrRole::User, "usr"),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text { text: "txt".into() },
                    IrContentBlock::ToolUse {
                        id: "c1".into(),
                        name: "search".into(),
                        input: json!({}),
                    },
                ],
            ),
        ]);

        let mapper = OpenAiGeminiIrMapper;
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();

        assert_eq!(result.len(), 3);
        assert!(!result.is_empty());
        assert!(result.system_message().is_some());
        assert!(result.last_assistant().is_some());
        assert_eq!(result.tool_calls().len(), 1);
        assert_eq!(result.messages_by_role(IrRole::User).len(), 1);
    }
}
