// SPDX-License-Identifier: MIT OR Apache-2.0

//! Comprehensive tests for the IR-level mapping engine.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;
use serde_json::json;

use crate::MapError;
use crate::factory::{default_ir_mapper, supported_ir_pairs};
use crate::ir_identity::IrIdentityMapper;
use crate::ir_mapper::IrMapper;
use crate::ir_openai_claude::OpenAiClaudeIrMapper;
use crate::ir_openai_gemini::OpenAiGeminiIrMapper;

// ── Helpers ─────────────────────────────────────────────────────────────

fn simple_conversation() -> IrConversation {
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

// ═══════════════════════════════════════════════════════════════════════
// Identity mapper tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn identity_simple_passthrough() {
    let mapper = IrIdentityMapper;
    let conv = simple_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(result, conv);
}

#[test]
fn identity_preserves_tool_calls() {
    let mapper = IrIdentityMapper;
    let conv = tool_call_conversation();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(result, conv);
}

#[test]
fn identity_preserves_thinking() {
    let mapper = IrIdentityMapper;
    let conv = thinking_conversation();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(result, conv);
}

#[test]
fn identity_response_passthrough() {
    let mapper = IrIdentityMapper;
    let conv = simple_conversation();
    let result = mapper
        .map_response(Dialect::Gemini, Dialect::Gemini, &conv)
        .unwrap();
    assert_eq!(result, conv);
}

#[test]
fn identity_empty_conversation() {
    let mapper = IrIdentityMapper;
    let conv = IrConversation::new();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
        .unwrap();
    assert!(result.is_empty());
}

#[test]
fn identity_supported_pairs_all_dialects() {
    let mapper = IrIdentityMapper;
    let pairs = mapper.supported_pairs();
    for &d in Dialect::all() {
        assert!(pairs.contains(&(d, d)), "missing identity pair for {d}");
    }
}

#[test]
fn identity_any_cross_dialect_still_works() {
    // Identity mapper accepts any pair, even cross-dialect
    let mapper = IrIdentityMapper;
    let conv = simple_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(result, conv);
}

#[test]
fn identity_is_truly_identity() {
    let mapper = IrIdentityMapper;
    let conv = tool_call_conversation();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
        .unwrap();
    // Verify every message and content block is identical
    assert_eq!(conv.len(), mapped.len());
    for (orig, m) in conv.messages.iter().zip(mapped.messages.iter()) {
        assert_eq!(orig.role, m.role);
        assert_eq!(orig.content.len(), m.content.len());
        for (ob, mb) in orig.content.iter().zip(m.content.iter()) {
            assert_eq!(ob, mb);
        }
        assert_eq!(orig.metadata, m.metadata);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// OpenAI ↔ Claude mapper tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_claude_simple() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = simple_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result.messages[0].role, IrRole::System);
    assert_eq!(result.messages[1].role, IrRole::User);
    assert_eq!(result.messages[2].role, IrRole::Assistant);
}

#[test]
fn openai_to_claude_preserves_text() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = simple_conversation();
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
fn openai_to_claude_tool_results_become_user() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = tool_call_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    // Tool message (index 2) should now be a User message
    assert_eq!(result.messages[2].role, IrRole::User);
    assert!(matches!(
        &result.messages[2].content[0],
        IrContentBlock::ToolResult { .. }
    ));
}

#[test]
fn openai_to_claude_tool_use_preserved() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = tool_call_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    let asst = &result.messages[1];
    assert_eq!(asst.content.len(), 2);
    assert!(matches!(&asst.content[0], IrContentBlock::Text { text } if text == "Let me check."));
    assert!(
        matches!(&asst.content[1], IrContentBlock::ToolUse { name, .. } if name == "get_weather")
    );
}

#[test]
fn claude_to_openai_simple() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = simple_conversation();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result.messages[0].role, IrRole::System);
    assert_eq!(result.messages[1].role, IrRole::User);
    assert_eq!(result.messages[2].role, IrRole::Assistant);
}

#[test]
fn claude_to_openai_thinking_dropped() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = thinking_conversation();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
        .unwrap();
    let asst = &result.messages[1];
    // Thinking block should be dropped, only text remains
    assert_eq!(asst.content.len(), 1);
    assert!(
        matches!(&asst.content[0], IrContentBlock::Text { text } if text == "The answer is 42.")
    );
}

#[test]
fn claude_to_openai_user_tool_results_become_tool_role() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = multi_tool_conversation();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
        .unwrap();
    // The user message with two ToolResult blocks should split into two Tool messages
    let tool_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert_eq!(tool_msgs.len(), 2);
}

#[test]
fn openai_claude_unsupported_pair() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = simple_conversation();
    let err = mapper
        .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn openai_claude_supported_pairs() {
    let mapper = OpenAiClaudeIrMapper;
    let pairs = mapper.supported_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::OpenAi)));
    assert_eq!(pairs.len(), 2);
}

#[test]
fn openai_to_claude_empty_conversation() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = IrConversation::new();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert!(result.is_empty());
}

#[test]
fn claude_to_openai_preserves_tool_use_in_assistant() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = tool_call_conversation();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
        .unwrap();
    let asst = &result.messages[1];
    assert_eq!(asst.role, IrRole::Assistant);
    assert!(
        asst.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::ToolUse { name, .. } if name == "get_weather"))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// OpenAI ↔ Gemini mapper tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_gemini_simple() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = simple_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result.messages[0].role, IrRole::System);
    assert_eq!(result.messages[2].text_content(), "Hi there!");
}

#[test]
fn openai_to_gemini_tool_results_become_user() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = tool_call_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    // Tool message should become User role in Gemini
    assert_eq!(result.messages[2].role, IrRole::User);
}

#[test]
fn openai_to_gemini_thinking_dropped() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = thinking_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    let asst = &result.messages[1];
    assert_eq!(asst.content.len(), 1);
    assert!(
        !asst
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    );
}

#[test]
fn gemini_to_openai_simple() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = simple_conversation();
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result.messages[0].role, IrRole::System);
}

#[test]
fn gemini_to_openai_user_tool_results_split() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = multi_tool_conversation();
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &conv)
        .unwrap();
    let tool_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert_eq!(tool_msgs.len(), 2);
}

#[test]
fn openai_gemini_unsupported_pair() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = simple_conversation();
    let err = mapper
        .map_request(Dialect::Claude, Dialect::Kimi, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn openai_gemini_supported_pairs() {
    let mapper = OpenAiGeminiIrMapper;
    let pairs = mapper.supported_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Gemini)));
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::OpenAi)));
    assert_eq!(pairs.len(), 2);
}

#[test]
fn openai_to_gemini_empty_conversation() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = IrConversation::new();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    assert!(result.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// Roundtrip fidelity tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_openai_claude_simple() {
    let mapper = OpenAiClaudeIrMapper;
    let orig = simple_conversation();
    let claude = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &claude)
        .unwrap();
    // Message count and roles should be preserved
    assert_eq!(orig.len(), back.len());
    for (o, b) in orig.messages.iter().zip(back.messages.iter()) {
        assert_eq!(o.role, b.role);
        assert_eq!(o.text_content(), b.text_content());
    }
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

    // Check tool calls survived the roundtrip
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
        }
    }
}

#[test]
fn roundtrip_openai_gemini_simple() {
    let mapper = OpenAiGeminiIrMapper;
    let orig = simple_conversation();
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
fn roundtrip_thinking_lost_openai() {
    // Thinking blocks are dropped when going to OpenAI, so roundtrip is lossy
    let mapper = OpenAiClaudeIrMapper;
    let orig = thinking_conversation();
    let openai = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &orig)
        .unwrap();
    let back = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &openai)
        .unwrap();
    // Text content should survive
    assert_eq!(
        back.messages[1].text_content(),
        orig.messages[1].text_content()
    );
    // But thinking block should be gone
    assert!(!back.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    }));
}

// ═══════════════════════════════════════════════════════════════════════
// Factory tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn factory_identity_mapper() {
    for &d in Dialect::all() {
        let mapper = default_ir_mapper(d, d);
        assert!(mapper.is_some(), "no identity mapper for {d}");
    }
}

#[test]
fn factory_openai_claude() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude);
    assert!(mapper.is_some());
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::OpenAi);
    assert!(mapper.is_some());
}

#[test]
fn factory_openai_gemini() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Gemini);
    assert!(mapper.is_some());
    let mapper = default_ir_mapper(Dialect::Gemini, Dialect::OpenAi);
    assert!(mapper.is_some());
}

#[test]
fn factory_unsupported_returns_none() {
    let mapper = default_ir_mapper(Dialect::Kimi, Dialect::Copilot);
    assert!(mapper.is_none());
}

#[test]
fn factory_supported_pairs_includes_all() {
    let pairs = supported_ir_pairs();
    // Should include identity pairs for all dialects
    for &d in Dialect::all() {
        assert!(pairs.contains(&(d, d)));
    }
    // Should include cross-dialect pairs
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::OpenAi)));
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Gemini)));
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::OpenAi)));
}

#[test]
fn factory_mapper_works_for_simple_conv() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let conv = simple_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(result.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════
// MapError tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn map_error_unsupported_pair_serialize() {
    let err = MapError::UnsupportedPair {
        from: Dialect::Kimi,
        to: Dialect::Copilot,
    };
    let json = serde_json::to_string(&err).unwrap();
    assert!(json.contains("unsupported_pair"));
    let back: MapError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn map_error_lossy_serialize() {
    let err = MapError::LossyConversion {
        field: "thinking".into(),
        reason: "dropped".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MapError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn map_error_unmappable_tool_serialize() {
    let err = MapError::UnmappableTool {
        name: "computer_use".into(),
        reason: "not supported".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MapError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

#[test]
fn map_error_incompatible_capability_serialize() {
    let err = MapError::IncompatibleCapability {
        capability: "vision".into(),
        reason: "no image support".into(),
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MapError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

// ═══════════════════════════════════════════════════════════════════════
// Edge case tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn image_content_preserved_openai_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "What is this?".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64data".into(),
            },
        ],
    )]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(result.messages[0].content.len(), 2);
    assert!(matches!(
        &result.messages[0].content[1],
        IrContentBlock::Image { media_type, .. } if media_type == "image/png"
    ));
}

#[test]
fn tool_error_result_preserved() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "run"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "bash".into(),
                input: json!({"cmd": "rm -rf /"}),
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
    if let IrContentBlock::ToolResult { is_error, .. } = &result.messages[2].content[0] {
        assert!(is_error);
    } else {
        panic!("expected ToolResult");
    }
}

#[test]
fn metadata_preserved_through_mapping() {
    let mapper = OpenAiClaudeIrMapper;
    let mut msg = IrMessage::text(IrRole::User, "hello");
    msg.metadata
        .insert("custom_key".into(), json!("custom_value"));
    let conv = IrConversation::from_messages(vec![msg]);
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(
        result.messages[0].metadata.get("custom_key"),
        Some(&json!("custom_value"))
    );
}

#[test]
fn gemini_to_openai_thinking_dropped() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = thinking_conversation();
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::OpenAi, &conv)
        .unwrap();
    assert!(!result.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    }));
}

#[test]
fn openai_to_gemini_preserves_tool_use() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = tool_call_conversation();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    let tools = result.tool_calls();
    assert_eq!(tools.len(), 1);
    if let IrContentBlock::ToolUse { name, .. } = tools[0] {
        assert_eq!(name, "get_weather");
    }
}

#[test]
fn ir_mapper_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<IrIdentityMapper>();
    assert_send_sync::<OpenAiClaudeIrMapper>();
    assert_send_sync::<OpenAiGeminiIrMapper>();
}

#[test]
fn openai_claude_response_mapping() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = simple_conversation();
    let result = mapper
        .map_response(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(result.len(), 3);
}

#[test]
fn openai_gemini_response_mapping() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = simple_conversation();
    let result = mapper
        .map_response(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    assert_eq!(result.len(), 3);
}
