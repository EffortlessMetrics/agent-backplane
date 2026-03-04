// SPDX-License-Identifier: MIT OR Apache-2.0

//! Comprehensive mapper roundtrip and validation tests.
//!
//! Covers bidirectional roundtrip fidelity, lossy mapping detection,
//! content block mapping, factory creation, and error handling across
//! all supported dialect pairs.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;
use abp_mapper::{
    ClaudeGeminiIrMapper, ClaudeKimiIrMapper, CodexClaudeIrMapper, GeminiKimiIrMapper,
    IrIdentityMapper, IrMapper, MapError, OpenAiClaudeIrMapper, OpenAiCodexIrMapper,
    OpenAiCopilotIrMapper, OpenAiGeminiIrMapper, OpenAiKimiIrMapper,
};
use abp_mapper::{default_ir_mapper, supported_ir_pairs};
use serde_json::json;

// ── Helpers ─────────────────────────────────────────────────────────────

fn simple_text_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi there!"),
    ])
}

fn tool_call_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "What is the weather?"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me check.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "get_weather".into(),
                    input: json!({"city": "NYC"}),
                },
            ],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "72°F, sunny".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(IrRole::Assistant, "It's 72°F and sunny in NYC."),
    ])
}

fn thinking_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Solve this puzzle"),
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

fn multi_turn_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be concise."),
        IrMessage::text(IrRole::User, "What is Rust?"),
        IrMessage::text(IrRole::Assistant, "A systems programming language."),
        IrMessage::text(IrRole::User, "What about Go?"),
        IrMessage::text(IrRole::Assistant, "Another systems language by Google."),
        IrMessage::text(IrRole::User, "Which is faster?"),
        IrMessage::text(IrRole::Assistant, "Rust is generally faster."),
    ])
}

fn image_conversation() -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "What is this?".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "iVBORw0KGgo=".into(),
            },
        ],
    )])
}

fn multi_tool_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Search and read"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "search".into(),
                    input: json!({"q": "rust"}),
                },
                IrContentBlock::ToolUse {
                    id: "t2".into(),
                    name: "read_file".into(),
                    input: json!({"path": "main.rs"}),
                },
            ],
        ),
        IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "result1".into(),
                    }],
                    is_error: false,
                },
                IrContentBlock::ToolResult {
                    tool_use_id: "t2".into(),
                    content: vec![IrContentBlock::Text {
                        text: "result2".into(),
                    }],
                    is_error: false,
                },
            ],
        ),
    ])
}

/// Verify that text content is preserved across a roundtrip for a given mapper.
fn assert_text_roundtrip(mapper: &dyn IrMapper, from: Dialect, to: Dialect, conv: &IrConversation) {
    let mapped = mapper.map_request(from, to, conv).unwrap();
    let back = mapper.map_request(to, from, &mapped).unwrap();

    // All text content from the original should survive the roundtrip
    let orig_text: Vec<String> = conv
        .messages
        .iter()
        .map(|m| m.text_content())
        .filter(|t| !t.is_empty())
        .collect();
    let back_text: Vec<String> = back
        .messages
        .iter()
        .map(|m| m.text_content())
        .filter(|t| !t.is_empty())
        .collect();
    assert_eq!(
        orig_text, back_text,
        "text content lost in {from} -> {to} -> {from} roundtrip"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Bidirectional roundtrip tests (20+)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_openai_claude_text() {
    let mapper = OpenAiClaudeIrMapper;
    assert_text_roundtrip(
        &mapper,
        Dialect::OpenAi,
        Dialect::Claude,
        &simple_text_conversation(),
    );
}

#[test]
fn roundtrip_claude_openai_text() {
    let mapper = OpenAiClaudeIrMapper;
    assert_text_roundtrip(
        &mapper,
        Dialect::Claude,
        Dialect::OpenAi,
        &simple_text_conversation(),
    );
}

#[test]
fn roundtrip_openai_gemini_text() {
    let mapper = OpenAiGeminiIrMapper;
    assert_text_roundtrip(
        &mapper,
        Dialect::OpenAi,
        Dialect::Gemini,
        &simple_text_conversation(),
    );
}

#[test]
fn roundtrip_gemini_openai_text() {
    let mapper = OpenAiGeminiIrMapper;
    assert_text_roundtrip(
        &mapper,
        Dialect::Gemini,
        Dialect::OpenAi,
        &simple_text_conversation(),
    );
}

#[test]
fn roundtrip_claude_gemini_text() {
    let mapper = ClaudeGeminiIrMapper;
    assert_text_roundtrip(
        &mapper,
        Dialect::Claude,
        Dialect::Gemini,
        &simple_text_conversation(),
    );
}

#[test]
fn roundtrip_gemini_claude_text() {
    let mapper = ClaudeGeminiIrMapper;
    assert_text_roundtrip(
        &mapper,
        Dialect::Gemini,
        Dialect::Claude,
        &simple_text_conversation(),
    );
}

#[test]
fn roundtrip_openai_kimi_text() {
    let mapper = OpenAiKimiIrMapper;
    assert_text_roundtrip(
        &mapper,
        Dialect::OpenAi,
        Dialect::Kimi,
        &simple_text_conversation(),
    );
}

#[test]
fn roundtrip_kimi_openai_text() {
    let mapper = OpenAiKimiIrMapper;
    assert_text_roundtrip(
        &mapper,
        Dialect::Kimi,
        Dialect::OpenAi,
        &simple_text_conversation(),
    );
}

#[test]
fn roundtrip_openai_copilot_text() {
    let mapper = OpenAiCopilotIrMapper;
    assert_text_roundtrip(
        &mapper,
        Dialect::OpenAi,
        Dialect::Copilot,
        &simple_text_conversation(),
    );
}

#[test]
fn roundtrip_copilot_openai_text() {
    let mapper = OpenAiCopilotIrMapper;
    assert_text_roundtrip(
        &mapper,
        Dialect::Copilot,
        Dialect::OpenAi,
        &simple_text_conversation(),
    );
}

#[test]
fn roundtrip_claude_kimi_text() {
    let mapper = ClaudeKimiIrMapper;
    assert_text_roundtrip(
        &mapper,
        Dialect::Claude,
        Dialect::Kimi,
        &simple_text_conversation(),
    );
}

#[test]
fn roundtrip_kimi_claude_text() {
    let mapper = ClaudeKimiIrMapper;
    assert_text_roundtrip(
        &mapper,
        Dialect::Kimi,
        Dialect::Claude,
        &simple_text_conversation(),
    );
}

#[test]
fn roundtrip_gemini_kimi_text() {
    let mapper = GeminiKimiIrMapper;
    assert_text_roundtrip(
        &mapper,
        Dialect::Gemini,
        Dialect::Kimi,
        &simple_text_conversation(),
    );
}

#[test]
fn roundtrip_openai_claude_tool_calls() {
    let mapper = OpenAiClaudeIrMapper;
    let orig = tool_call_conversation();
    let claude = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &claude)
        .unwrap();

    let orig_tools = orig.tool_calls();
    let back_tools = back.tool_calls();
    assert_eq!(orig_tools.len(), back_tools.len());
    for (ot, bt) in orig_tools.iter().zip(back_tools.iter()) {
        if let (
            IrContentBlock::ToolUse {
                name: on,
                input: oi,
                ..
            },
            IrContentBlock::ToolUse {
                name: bn,
                input: bi,
                ..
            },
        ) = (ot, bt)
        {
            assert_eq!(on, bn);
            assert_eq!(oi, bi);
        } else {
            panic!("expected ToolUse blocks");
        }
    }
}

#[test]
fn roundtrip_openai_gemini_tool_calls() {
    let mapper = OpenAiGeminiIrMapper;
    let orig = tool_call_conversation();
    let gemini = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &gemini)
        .unwrap();

    let orig_tools = orig.tool_calls();
    let back_tools = back.tool_calls();
    assert_eq!(orig_tools.len(), back_tools.len());
}

#[test]
fn roundtrip_openai_claude_system_message() {
    let mapper = OpenAiClaudeIrMapper;
    let orig = simple_text_conversation();
    let claude = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &claude)
        .unwrap();

    let orig_sys = orig.system_message().unwrap();
    let back_sys = back.system_message().unwrap();
    assert_eq!(orig_sys.text_content(), back_sys.text_content());
}

#[test]
fn roundtrip_openai_gemini_system_message() {
    let mapper = OpenAiGeminiIrMapper;
    let orig = simple_text_conversation();
    let gemini = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &gemini)
        .unwrap();

    let orig_sys = orig.system_message().unwrap();
    let back_sys = back.system_message().unwrap();
    assert_eq!(orig_sys.text_content(), back_sys.text_content());
}

#[test]
fn roundtrip_openai_claude_multi_turn() {
    let mapper = OpenAiClaudeIrMapper;
    assert_text_roundtrip(
        &mapper,
        Dialect::OpenAi,
        Dialect::Claude,
        &multi_turn_conversation(),
    );
}

#[test]
fn roundtrip_openai_gemini_multi_turn() {
    let mapper = OpenAiGeminiIrMapper;
    assert_text_roundtrip(
        &mapper,
        Dialect::OpenAi,
        Dialect::Gemini,
        &multi_turn_conversation(),
    );
}

#[test]
fn roundtrip_claude_kimi_multi_turn() {
    let mapper = ClaudeKimiIrMapper;
    assert_text_roundtrip(
        &mapper,
        Dialect::Claude,
        Dialect::Kimi,
        &multi_turn_conversation(),
    );
}

#[test]
fn roundtrip_identity_all_dialects() {
    let mapper = IrIdentityMapper;
    let conv = simple_text_conversation();
    for &d in Dialect::all() {
        let result = mapper.map_request(d, d, &conv).unwrap();
        assert_eq!(result, conv, "identity roundtrip failed for {d}");
    }
}

#[test]
fn roundtrip_openai_copilot_multi_turn() {
    let mapper = OpenAiCopilotIrMapper;
    assert_text_roundtrip(
        &mapper,
        Dialect::OpenAi,
        Dialect::Copilot,
        &multi_turn_conversation(),
    );
}

#[test]
fn roundtrip_gemini_kimi_multi_turn() {
    let mapper = GeminiKimiIrMapper;
    assert_text_roundtrip(
        &mapper,
        Dialect::Gemini,
        Dialect::Kimi,
        &multi_turn_conversation(),
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Lossy mapping detection (10+)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn lossy_claude_thinking_to_openai_dropped() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = thinking_conversation();
    let openai = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
        .unwrap();

    // Thinking block should be gone
    assert!(
        !openai.messages.iter().any(|m| m
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))),
        "thinking block should be dropped when mapping to OpenAI"
    );
    // Text should survive
    assert_eq!(openai.messages[1].text_content(), "The answer is 42.");
}

#[test]
fn lossy_claude_thinking_to_gemini_dropped() {
    let mapper = ClaudeGeminiIrMapper;
    let conv = thinking_conversation();
    let gemini = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap();

    assert!(
        !gemini.messages.iter().any(|m| m
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))),
        "thinking block should be dropped when mapping to Gemini"
    );
    assert_eq!(gemini.messages[1].text_content(), "The answer is 42.");
}

#[test]
fn lossy_claude_thinking_to_kimi_dropped() {
    let mapper = ClaudeKimiIrMapper;
    let conv = thinking_conversation();
    let kimi = mapper
        .map_request(Dialect::Claude, Dialect::Kimi, &conv)
        .unwrap();

    assert!(
        !kimi.messages.iter().any(|m| m
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))),
        "thinking block should be dropped when mapping to Kimi"
    );
}

#[test]
fn lossy_thinking_to_copilot_dropped() {
    let mapper = OpenAiCopilotIrMapper;
    let conv = thinking_conversation();
    let copilot = mapper
        .map_request(Dialect::OpenAi, Dialect::Copilot, &conv)
        .unwrap();

    assert!(
        !copilot.messages.iter().any(|m| m
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))),
        "thinking block should be dropped when mapping to Copilot"
    );
}

#[test]
fn lossy_thinking_roundtrip_not_recoverable() {
    let mapper = OpenAiClaudeIrMapper;
    let orig = thinking_conversation();
    let openai = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &openai)
        .unwrap();

    // Original had 2 content blocks (thinking + text), roundtripped has 1 (text only)
    assert_eq!(orig.messages[1].content.len(), 2);
    assert_eq!(back.messages[1].content.len(), 1);
    assert!(
        !back.messages[1]
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. })),
        "thinking block should not survive OpenAI roundtrip"
    );
}

#[test]
fn lossy_openai_system_to_codex_dropped() {
    let mapper = OpenAiCodexIrMapper;
    let conv = simple_text_conversation();
    let codex = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
        .unwrap();

    // System message should be dropped for Codex
    assert!(
        !codex.messages.iter().any(|m| m.role == IrRole::System),
        "system messages should be dropped for Codex"
    );
}

#[test]
fn lossy_openai_tools_to_codex_dropped() {
    let mapper = OpenAiCodexIrMapper;
    let conv = tool_call_conversation();
    let codex = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
        .unwrap();

    // ToolUse and ToolResult blocks should be dropped
    assert!(
        codex.tool_calls().is_empty(),
        "tool calls should be dropped for Codex"
    );
    assert!(
        !codex.messages.iter().any(|m| m.role == IrRole::Tool),
        "tool-role messages should be dropped for Codex"
    );
}

#[test]
fn lossy_claude_to_codex_system_and_tools_dropped() {
    let mapper = CodexClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are helpful."),
        IrMessage::text(IrRole::User, "Hi"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me help.".into(),
                },
                IrContentBlock::Thinking {
                    text: "thinking...".into(),
                },
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "search".into(),
                    input: json!({}),
                },
            ],
        ),
    ]);
    let codex = mapper
        .map_request(Dialect::Claude, Dialect::Codex, &conv)
        .unwrap();

    // System dropped, thinking dropped, tool_use dropped
    assert!(!codex.messages.iter().any(|m| m.role == IrRole::System));
    assert!(codex.tool_calls().is_empty());
    // Only text blocks survive
    for msg in &codex.messages {
        for block in &msg.content {
            assert!(
                matches!(block, IrContentBlock::Text { .. }),
                "only text blocks should survive Codex mapping"
            );
        }
    }
}

#[test]
fn lossy_codex_to_openai_is_lossless() {
    let mapper = OpenAiCodexIrMapper;
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi there!"),
    ]);
    let openai = mapper
        .map_request(Dialect::Codex, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(openai, conv, "Codex→OpenAI should be lossless for text");
}

#[test]
fn lossy_image_to_codex_dropped() {
    let mapper = OpenAiCodexIrMapper;
    let conv = image_conversation();
    let codex = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
        .unwrap();

    // Image blocks should be dropped for Codex (text-only)
    for msg in &codex.messages {
        assert!(
            !msg.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Image { .. })),
            "image blocks should be dropped for Codex"
        );
    }
}

#[test]
fn lossy_tool_role_convention_claude_to_kimi() {
    // Claude uses User role for tool results; Kimi uses Tool role
    let mapper = ClaudeKimiIrMapper;
    let conv = multi_tool_conversation();
    let kimi = mapper
        .map_request(Dialect::Claude, Dialect::Kimi, &conv)
        .unwrap();

    // Tool results in user messages should become Tool-role messages
    let tool_msgs: Vec<_> = kimi
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert_eq!(
        tool_msgs.len(),
        2,
        "user-role tool results should become Tool-role messages in Kimi"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Content block mapping (10+)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn content_text_preserved_openai_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = simple_text_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(
        result.messages[0].text_content(),
        "You are a helpful assistant."
    );
    assert_eq!(result.messages[1].text_content(), "Hello");
    assert_eq!(result.messages[2].text_content(), "Hi there!");
}

#[test]
fn content_text_preserved_openai_gemini() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = simple_text_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    assert_eq!(result.messages[1].text_content(), "Hello");
    assert_eq!(result.messages[2].text_content(), "Hi there!");
}

#[test]
fn content_image_preserved_openai_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = image_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(result.messages[0].content.len(), 2);
    assert!(matches!(
        &result.messages[0].content[1],
        IrContentBlock::Image { media_type, data }
        if media_type == "image/png" && data == "iVBORw0KGgo="
    ));
}

#[test]
fn content_image_preserved_claude_gemini() {
    let mapper = ClaudeGeminiIrMapper;
    let conv = image_conversation();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap();
    assert!(
        result.messages[0]
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Image { .. }))
    );
}

#[test]
fn content_tool_use_preserved_openai_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = tool_call_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    let asst = &result.messages[1];
    assert!(
        asst.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::ToolUse { name, .. } if name == "get_weather"))
    );
}

#[test]
fn content_tool_result_preserved_openai_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = tool_call_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();

    // Tool result should be preserved (in User role for Claude)
    let tool_result_msg = result
        .messages
        .iter()
        .find(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
        })
        .expect("tool result should be preserved");
    if let IrContentBlock::ToolResult {
        tool_use_id,
        is_error,
        ..
    } = &tool_result_msg.content[0]
    {
        assert_eq!(tool_use_id, "call_1");
        assert!(!is_error);
    }
}

#[test]
fn content_tool_error_result_preserved() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "run"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "bash".into(),
                input: json!({"cmd": "ls"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "permission denied".into(),
                }],
                is_error: true,
            }],
        ),
    ]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    let tool_msg = result
        .messages
        .iter()
        .find(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolResult { is_error: true, .. }))
        })
        .expect("error tool result should be preserved");
    if let IrContentBlock::ToolResult { is_error, .. } = &tool_msg.content[0] {
        assert!(is_error, "is_error flag should be preserved");
    }
}

#[test]
fn content_mixed_text_and_tool_handling() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Here's what I found:".into(),
            },
            IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "search".into(),
                input: json!({"q": "test"}),
            },
            IrContentBlock::Text {
                text: "Let me search more.".into(),
            },
        ],
    )]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();

    // All three blocks should be preserved
    assert_eq!(result.messages[0].content.len(), 3);
}

#[test]
fn content_empty_conversation_all_mappers() {
    let empty = IrConversation::new();
    let mappers: Vec<(&dyn IrMapper, Dialect, Dialect)> = vec![
        (&OpenAiClaudeIrMapper, Dialect::OpenAi, Dialect::Claude),
        (&OpenAiGeminiIrMapper, Dialect::OpenAi, Dialect::Gemini),
        (&ClaudeGeminiIrMapper, Dialect::Claude, Dialect::Gemini),
        (&OpenAiKimiIrMapper, Dialect::OpenAi, Dialect::Kimi),
        (&OpenAiCopilotIrMapper, Dialect::OpenAi, Dialect::Copilot),
        (&ClaudeKimiIrMapper, Dialect::Claude, Dialect::Kimi),
        (&GeminiKimiIrMapper, Dialect::Gemini, Dialect::Kimi),
        (&OpenAiCodexIrMapper, Dialect::OpenAi, Dialect::Codex),
        (&CodexClaudeIrMapper, Dialect::Codex, Dialect::Claude),
    ];
    for (mapper, from, to) in &mappers {
        let result = mapper.map_request(*from, *to, &empty).unwrap();
        assert!(
            result.is_empty(),
            "empty conversation should remain empty for {from} -> {to}"
        );
    }
}

#[test]
fn content_multiple_text_blocks_preserved() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "First part.".into(),
            },
            IrContentBlock::Text {
                text: "Second part.".into(),
            },
            IrContentBlock::Text {
                text: "Third part.".into(),
            },
        ],
    )]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(result.messages[0].content.len(), 3);
}

#[test]
fn content_metadata_preserved_through_mapping() {
    let mapper = OpenAiClaudeIrMapper;
    let mut msg = IrMessage::text(IrRole::User, "hello");
    msg.metadata.insert("request_id".into(), json!("req-123"));
    msg.metadata.insert("source".into(), json!("test"));
    let conv = IrConversation::from_messages(vec![msg]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(
        result.messages[0].metadata.get("request_id"),
        Some(&json!("req-123"))
    );
    assert_eq!(
        result.messages[0].metadata.get("source"),
        Some(&json!("test"))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Factory creation (10+)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn factory_identity_for_all_dialects() {
    for &d in Dialect::all() {
        let mapper = default_ir_mapper(d, d);
        assert!(mapper.is_some(), "no identity mapper for {d}");
    }
}

#[test]
fn factory_openai_claude_both_directions() {
    assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Claude).is_some());
    assert!(default_ir_mapper(Dialect::Claude, Dialect::OpenAi).is_some());
}

#[test]
fn factory_openai_gemini_both_directions() {
    assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).is_some());
    assert!(default_ir_mapper(Dialect::Gemini, Dialect::OpenAi).is_some());
}

#[test]
fn factory_claude_gemini_both_directions() {
    assert!(default_ir_mapper(Dialect::Claude, Dialect::Gemini).is_some());
    assert!(default_ir_mapper(Dialect::Gemini, Dialect::Claude).is_some());
}

#[test]
fn factory_openai_codex_both_directions() {
    assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Codex).is_some());
    assert!(default_ir_mapper(Dialect::Codex, Dialect::OpenAi).is_some());
}

#[test]
fn factory_openai_kimi_both_directions() {
    assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Kimi).is_some());
    assert!(default_ir_mapper(Dialect::Kimi, Dialect::OpenAi).is_some());
}

#[test]
fn factory_openai_copilot_both_directions() {
    assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Copilot).is_some());
    assert!(default_ir_mapper(Dialect::Copilot, Dialect::OpenAi).is_some());
}

#[test]
fn factory_codex_claude_both_directions() {
    assert!(default_ir_mapper(Dialect::Codex, Dialect::Claude).is_some());
    assert!(default_ir_mapper(Dialect::Claude, Dialect::Codex).is_some());
}

#[test]
fn factory_unsupported_pair_returns_none() {
    // These pairs have no direct mapper registered
    assert!(default_ir_mapper(Dialect::Kimi, Dialect::Copilot).is_none());
    assert!(default_ir_mapper(Dialect::Copilot, Dialect::Kimi).is_none());
    assert!(default_ir_mapper(Dialect::Codex, Dialect::Gemini).is_none());
    assert!(default_ir_mapper(Dialect::Codex, Dialect::Kimi).is_none());
    assert!(default_ir_mapper(Dialect::Copilot, Dialect::Claude).is_none());
}

#[test]
fn factory_supported_pairs_is_complete() {
    let pairs = supported_ir_pairs();

    // Identity pairs
    for &d in Dialect::all() {
        assert!(pairs.contains(&(d, d)), "missing identity pair for {d}");
    }

    // All cross-dialect pairs
    let expected_cross = [
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
    for &(from, to) in &expected_cross {
        assert!(
            pairs.contains(&(from, to)),
            "missing pair ({from}, {to}) from supported_ir_pairs()"
        );
    }
}

#[test]
fn factory_mapper_produces_correct_output() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let conv = simple_text_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result.messages[0].role, IrRole::System);
    assert_eq!(result.messages[1].role, IrRole::User);
    assert_eq!(result.messages[2].role, IrRole::Assistant);
}

#[test]
fn factory_multiple_calls_produce_working_mappers() {
    let conv = simple_text_conversation();
    let pairs_to_test = [
        (Dialect::OpenAi, Dialect::Claude),
        (Dialect::OpenAi, Dialect::Gemini),
        (Dialect::Claude, Dialect::Kimi),
        (Dialect::Gemini, Dialect::Kimi),
    ];
    for &(from, to) in &pairs_to_test {
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        assert!(
            !result.is_empty(),
            "factory mapper for {from} -> {to} should produce non-empty result"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Error handling (10+)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn error_unsupported_pair_openai_claude_mapper() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = simple_text_conversation();
    let err = mapper
        .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn error_unsupported_pair_openai_gemini_mapper() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = simple_text_conversation();
    let err = mapper
        .map_request(Dialect::Claude, Dialect::Kimi, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn error_unsupported_pair_claude_gemini_mapper() {
    let mapper = ClaudeGeminiIrMapper;
    let conv = simple_text_conversation();
    let err = mapper
        .map_request(Dialect::OpenAi, Dialect::Kimi, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn error_unsupported_pair_claude_kimi_mapper() {
    let mapper = ClaudeKimiIrMapper;
    let conv = simple_text_conversation();
    let err = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn error_unsupported_pair_openai_copilot_mapper() {
    let mapper = OpenAiCopilotIrMapper;
    let conv = simple_text_conversation();
    let err = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn error_unmappable_content_image_in_gemini_system() {
    let mapper = ClaudeGeminiIrMapper;
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::System,
        vec![
            IrContentBlock::Text {
                text: "You are helpful.".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64data".into(),
            },
        ],
    )]);
    let err = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap_err();
    assert!(
        matches!(err, MapError::UnmappableContent { field, .. } if field == "system"),
        "image in system prompt should produce UnmappableContent error for Gemini"
    );
}

#[test]
fn error_unmappable_tool_codex_to_claude() {
    let mapper = CodexClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "apply_patch".into(),
            input: json!({"patch": "..."}),
        }],
    )]);
    let err = mapper
        .map_request(Dialect::Codex, Dialect::Claude, &conv)
        .unwrap_err();
    assert!(
        matches!(err, MapError::UnmappableTool { name, .. } if name == "apply_patch"),
        "Codex apply_patch should produce UnmappableTool error"
    );
}

#[test]
fn error_unmappable_tool_apply_diff_codex_to_claude() {
    let mapper = CodexClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "apply_diff".into(),
            input: json!({"diff": "..."}),
        }],
    )]);
    let err = mapper
        .map_request(Dialect::Codex, Dialect::Claude, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnmappableTool { name, .. } if name == "apply_diff"));
}

#[test]
fn error_empty_conversation_succeeds_not_errors() {
    // Empty conversations should succeed, not error
    let mapper = OpenAiClaudeIrMapper;
    let conv = IrConversation::new();
    let result = mapper.map_request(Dialect::OpenAi, Dialect::Claude, &conv);
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[test]
fn error_single_message_conversation() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "Hi")]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result.messages[0].text_content(), "Hi");
}

#[test]
fn error_system_only_conversation() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::System, "Be concise.")]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result.messages[0].role, IrRole::System);
}

#[test]
fn error_empty_content_message() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![IrMessage::new(IrRole::User, vec![])]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(result.len(), 1);
    assert!(result.messages[0].content.is_empty());
}

#[test]
fn error_map_error_display_strings() {
    let err = MapError::UnsupportedPair {
        from: Dialect::Kimi,
        to: Dialect::Copilot,
    };
    let msg = err.to_string();
    assert!(msg.contains("Kimi"));
    assert!(msg.contains("Copilot"));

    let err = MapError::LossyConversion {
        field: "thinking".into(),
        reason: "no equivalent".into(),
    };
    assert!(err.to_string().contains("thinking"));

    let err = MapError::UnmappableTool {
        name: "bash".into(),
        reason: "restricted".into(),
    };
    assert!(err.to_string().contains("bash"));

    let err = MapError::IncompatibleCapability {
        capability: "vision".into(),
        reason: "unsupported".into(),
    };
    assert!(err.to_string().contains("vision"));

    let err = MapError::UnmappableContent {
        field: "system".into(),
        reason: "images".into(),
    };
    assert!(err.to_string().contains("system"));
}

#[test]
fn error_map_error_serde_roundtrip_all_variants() {
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
            name: "tool".into(),
            reason: "reason".into(),
        },
        MapError::IncompatibleCapability {
            capability: "cap".into(),
            reason: "reason".into(),
        },
        MapError::UnmappableContent {
            field: "field".into(),
            reason: "reason".into(),
        },
    ];
    for err in &errors {
        let json = serde_json::to_string(err).unwrap();
        let back: MapError = serde_json::from_str(&json).unwrap();
        assert_eq!(*err, back, "serde roundtrip failed for {err}");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Validation pipeline integration
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn validation_roundtrip_identical_values() {
    use abp_mapper::validation::{DefaultMappingValidator, MappingValidator};

    let v = DefaultMappingValidator::new();
    let val = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let result = v.validate_roundtrip(&val, &val);
    assert!(result.is_lossless());
}

#[test]
fn validation_pre_mapping_openai_valid() {
    use abp_mapper::validation::{DefaultMappingValidator, MappingValidator};

    let v = DefaultMappingValidator::new();
    let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let result = v.validate_pre_mapping(Dialect::OpenAi, &req);
    assert!(result.is_valid());
    assert_eq!(result.field_coverage, 100.0);
}

#[test]
fn validation_pre_mapping_missing_required_fields() {
    use abp_mapper::validation::{DefaultMappingValidator, MappingValidator};

    let v = DefaultMappingValidator::new();
    let req = json!({"not_a_real_field": true});
    let result = v.validate_pre_mapping(Dialect::OpenAi, &req);
    assert!(!result.is_valid());
    assert!(result.error_count() > 0);
}

#[test]
fn validation_pipeline_full_pass() {
    use abp_mapper::validation::{DefaultMappingValidator, ValidationPipeline};

    let pipe = ValidationPipeline::new(
        DefaultMappingValidator::new(),
        Dialect::OpenAi,
        Dialect::OpenAi,
    );
    let req = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}]});
    let result = pipe.run(&req, |v| Ok(v.clone()));
    assert!(result.pre.is_valid());
    assert!(result.mapped.is_some());
    assert!(result.post.as_ref().unwrap().is_valid());
}

// ═══════════════════════════════════════════════════════════════════════
// Response mapping tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn response_mapping_openai_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = simple_text_conversation();
    let result = mapper
        .map_response(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(result.len(), conv.len());
}

#[test]
fn response_mapping_openai_gemini() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = simple_text_conversation();
    let result = mapper
        .map_response(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    assert_eq!(result.len(), conv.len());
}

#[test]
fn response_mapping_claude_kimi() {
    let mapper = ClaudeKimiIrMapper;
    let conv = simple_text_conversation();
    let result = mapper
        .map_response(Dialect::Claude, Dialect::Kimi, &conv)
        .unwrap();
    assert!(!result.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// Trait object and Send+Sync tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn all_ir_mappers_are_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<IrIdentityMapper>();
    assert_send_sync::<OpenAiClaudeIrMapper>();
    assert_send_sync::<OpenAiGeminiIrMapper>();
    assert_send_sync::<ClaudeGeminiIrMapper>();
    assert_send_sync::<OpenAiCodexIrMapper>();
    assert_send_sync::<OpenAiKimiIrMapper>();
    assert_send_sync::<ClaudeKimiIrMapper>();
    assert_send_sync::<OpenAiCopilotIrMapper>();
    assert_send_sync::<GeminiKimiIrMapper>();
    assert_send_sync::<CodexClaudeIrMapper>();
}

#[test]
fn factory_returns_trait_object() {
    let mapper: Box<dyn IrMapper> = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let conv = simple_text_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert!(!result.is_empty());
}

#[test]
fn supported_pairs_matches_factory() {
    let pairs = supported_ir_pairs();
    for &(from, to) in &pairs {
        assert!(
            default_ir_mapper(from, to).is_some(),
            "supported_ir_pairs() lists ({from}, {to}) but factory returns None"
        );
    }
}

#[test]
fn metadata_survives_roundtrip() {
    let mapper = OpenAiClaudeIrMapper;
    let mut msg = IrMessage::text(IrRole::User, "test");
    msg.metadata.insert("key".into(), json!("value"));
    msg.metadata.insert("count".into(), json!(42));
    let conv = IrConversation::from_messages(vec![msg]);

    let claude = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &claude)
        .unwrap();

    assert_eq!(back.messages[0].metadata.get("key"), Some(&json!("value")));
    assert_eq!(back.messages[0].metadata.get("count"), Some(&json!(42)));
}

#[test]
fn empty_metadata_preserved() {
    let mapper = OpenAiGeminiIrMapper;
    let msg = IrMessage::text(IrRole::User, "hello");
    assert!(msg.metadata.is_empty());
    let conv = IrConversation::from_messages(vec![msg]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    assert!(result.messages[0].metadata.is_empty());
}
