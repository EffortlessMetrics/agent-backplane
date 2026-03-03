// SPDX-License-Identifier: MIT OR Apache-2.0

//! Comprehensive projection-matrix integration tests for abp-mapper.
//!
//! Validates every supported dialect×dialect pair through the IR mapping
//! engine, including lossy pairs (Codex), identity mappings, error cases,
//! and factory resolution.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;
use abp_mapper::{
    ClaudeGeminiIrMapper, ClaudeKimiIrMapper, IrIdentityMapper, IrMapper, MapError,
    OpenAiClaudeIrMapper, OpenAiCodexIrMapper, OpenAiGeminiIrMapper, OpenAiKimiIrMapper,
    default_ir_mapper, supported_ir_pairs,
};
use serde_json::json;

// ── Helpers ─────────────────────────────────────────────────────────────

fn simple_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are helpful."),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi!"),
    ])
}

fn tool_call_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Check the weather"),
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
        IrMessage::text(IrRole::Assistant, "72°F and sunny in NYC."),
    ])
}

fn thinking_conversation() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Solve this"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Step by step reasoning...".into(),
                },
                IrContentBlock::Text {
                    text: "The answer is 42.".into(),
                },
            ],
        ),
    ])
}

fn system_only_conversation() -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::text(IrRole::System, "System prompt only.")])
}

// ═══════════════════════════════════════════════════════════════════════
// 1. OpenAI ↔ Claude — simple, tools, system
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_claude_simple_conversation() {
    let m = OpenAiClaudeIrMapper;
    let conv = simple_conversation();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 3);
    assert_eq!(mapped.messages[0].role, IrRole::System);
    assert_eq!(mapped.messages[1].text_content(), "Hello");
    assert_eq!(mapped.messages[2].text_content(), "Hi!");
}

#[test]
fn claude_openai_simple_conversation() {
    let m = OpenAiClaudeIrMapper;
    let conv = simple_conversation();
    let mapped = m
        .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 3);
    assert_eq!(mapped.messages[0].role, IrRole::System);
}

#[test]
fn openai_claude_tool_calls() {
    let m = OpenAiClaudeIrMapper;
    let conv = tool_call_conversation();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    // Tool role → User role in Claude
    assert_eq!(mapped.messages[2].role, IrRole::User);
    assert!(matches!(
        &mapped.messages[2].content[0],
        IrContentBlock::ToolResult { .. }
    ));
}

#[test]
fn openai_claude_system_message() {
    let m = OpenAiClaudeIrMapper;
    let conv = system_only_conversation();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 1);
    assert_eq!(mapped.messages[0].role, IrRole::System);
    assert_eq!(mapped.messages[0].text_content(), "System prompt only.");
}

// ═══════════════════════════════════════════════════════════════════════
// 2. OpenAI ↔ Gemini — simple, tools, system
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_gemini_simple_conversation() {
    let m = OpenAiGeminiIrMapper;
    let conv = simple_conversation();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 3);
    assert_eq!(mapped.messages[0].role, IrRole::System);
    assert_eq!(mapped.messages[2].text_content(), "Hi!");
}

#[test]
fn gemini_openai_simple_conversation() {
    let m = OpenAiGeminiIrMapper;
    let conv = simple_conversation();
    let mapped = m
        .map_request(Dialect::Gemini, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 3);
    assert_eq!(mapped.messages[1].role, IrRole::User);
}

#[test]
fn openai_gemini_tool_calls() {
    let m = OpenAiGeminiIrMapper;
    let conv = tool_call_conversation();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    // Tool role → User role in Gemini
    assert_eq!(mapped.messages[2].role, IrRole::User);
}

#[test]
fn openai_gemini_system_message() {
    let m = OpenAiGeminiIrMapper;
    let conv = system_only_conversation();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 1);
    assert_eq!(mapped.messages[0].role, IrRole::System);
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Claude ↔ Gemini — simple, tools, system
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn claude_gemini_simple_conversation() {
    let m = ClaudeGeminiIrMapper;
    let conv = simple_conversation();
    let mapped = m
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 3);
    assert_eq!(mapped.messages[2].text_content(), "Hi!");
}

#[test]
fn gemini_claude_simple_conversation() {
    let m = ClaudeGeminiIrMapper;
    let conv = simple_conversation();
    let mapped = m
        .map_request(Dialect::Gemini, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 3);
    assert_eq!(mapped.messages[0].role, IrRole::System);
}

#[test]
fn claude_gemini_tool_calls() {
    let m = ClaudeGeminiIrMapper;
    let conv = tool_call_conversation();
    let mapped = m
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap();
    // Tool role → User role for Gemini
    assert_eq!(mapped.messages[2].role, IrRole::User);
}

#[test]
fn claude_gemini_system_message() {
    let m = ClaudeGeminiIrMapper;
    let conv = system_only_conversation();
    let mapped = m
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 1);
    assert_eq!(mapped.messages[0].role, IrRole::System);
}

// ═══════════════════════════════════════════════════════════════════════
// 4. OpenAI ↔ Codex (lossy — Codex is output-only)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_codex_simple_conversation() {
    let m = OpenAiCodexIrMapper;
    let conv = simple_conversation();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
        .unwrap();
    // System message is dropped for Codex
    assert_eq!(mapped.len(), 2);
    assert_eq!(mapped.messages[0].role, IrRole::User);
    assert_eq!(mapped.messages[1].role, IrRole::Assistant);
}

#[test]
fn openai_codex_system_message_lost() {
    // Codex has no system instruction — system messages are dropped.
    let m = OpenAiCodexIrMapper;
    let conv = system_only_conversation();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
        .unwrap();
    assert!(
        mapped.is_empty(),
        "system message should be lost in Codex mapping"
    );
}

#[test]
fn openai_codex_tool_calls_lost() {
    // Codex is output-only — tool calls are dropped.
    let m = OpenAiCodexIrMapper;
    let conv = tool_call_conversation();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
        .unwrap();

    // ToolUse and ToolResult blocks should be gone; Tool-role messages dropped
    let tool_calls = mapped.tool_calls();
    assert!(tool_calls.is_empty(), "tool calls should be lost in Codex");

    // Only text blocks survive
    for msg in &mapped.messages {
        for block in &msg.content {
            assert!(
                matches!(block, IrContentBlock::Text { .. }),
                "only text blocks should survive Codex mapping, got: {block:?}"
            );
        }
    }
}

#[test]
fn openai_codex_thinking_lost() {
    // Thinking blocks are also dropped for Codex.
    let m = OpenAiCodexIrMapper;
    let conv = thinking_conversation();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
        .unwrap();
    assert!(!mapped.messages.iter().any(|msg| {
        msg.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    }));
}

#[test]
fn codex_openai_lossless_passthrough() {
    // Codex → OpenAI is lossless (Codex output is simple text).
    let m = OpenAiCodexIrMapper;
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Write code"),
        IrMessage::text(IrRole::Assistant, "fn main() {}"),
    ]);
    let mapped = m
        .map_request(Dialect::Codex, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(mapped, conv);
}

#[test]
fn openai_codex_assistant_text_preserved() {
    // Text content in assistant messages survives Codex mapping.
    let m = OpenAiCodexIrMapper;
    let conv = tool_call_conversation();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
        .unwrap();
    let texts: Vec<String> = mapped
        .messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|b| match b {
            IrContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|t| t.contains("Let me check")));
}

// ═══════════════════════════════════════════════════════════════════════
// 5. OpenAI ↔ Kimi — simple, tools, system
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_kimi_simple_conversation() {
    let m = OpenAiKimiIrMapper;
    let conv = simple_conversation();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Kimi, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 3);
    assert_eq!(mapped.messages[0].role, IrRole::System);
    assert_eq!(mapped.messages[1].text_content(), "Hello");
}

#[test]
fn kimi_openai_simple_conversation() {
    let m = OpenAiKimiIrMapper;
    let conv = simple_conversation();
    let mapped = m
        .map_request(Dialect::Kimi, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 3);
    assert_eq!(mapped.messages[2].text_content(), "Hi!");
}

#[test]
fn openai_kimi_tool_calls_preserved() {
    // Kimi supports tool calling (OpenAI-compatible), so tool calls survive.
    let m = OpenAiKimiIrMapper;
    let conv = tool_call_conversation();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Kimi, &conv)
        .unwrap();
    let tools = mapped.tool_calls();
    assert_eq!(tools.len(), 1);
    if let IrContentBlock::ToolUse { name, .. } = tools[0] {
        assert_eq!(name, "get_weather");
    } else {
        panic!("expected ToolUse");
    }
}

#[test]
fn openai_kimi_system_message() {
    let m = OpenAiKimiIrMapper;
    let conv = system_only_conversation();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Kimi, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 1);
    assert_eq!(mapped.messages[0].role, IrRole::System);
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Claude ↔ Kimi — simple, tools, system
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn claude_kimi_simple_conversation() {
    let m = ClaudeKimiIrMapper;
    let conv = simple_conversation();
    let mapped = m
        .map_request(Dialect::Claude, Dialect::Kimi, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 3);
    assert_eq!(mapped.messages[0].role, IrRole::System);
    assert_eq!(mapped.messages[2].text_content(), "Hi!");
}

#[test]
fn kimi_claude_simple_conversation() {
    let m = ClaudeKimiIrMapper;
    let conv = simple_conversation();
    let mapped = m
        .map_request(Dialect::Kimi, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 3);
    assert_eq!(mapped.messages[1].text_content(), "Hello");
}

#[test]
fn claude_kimi_thinking_dropped() {
    // Thinking blocks are dropped when mapping Claude → Kimi.
    let m = ClaudeKimiIrMapper;
    let conv = thinking_conversation();
    let mapped = m
        .map_request(Dialect::Claude, Dialect::Kimi, &conv)
        .unwrap();
    let asst = &mapped.messages[1];
    assert_eq!(asst.content.len(), 1);
    assert!(matches!(
        &asst.content[0],
        IrContentBlock::Text { text } if text == "The answer is 42."
    ));
}

#[test]
fn claude_kimi_tool_calls() {
    let m = ClaudeKimiIrMapper;
    let conv = tool_call_conversation();
    let mapped = m
        .map_request(Dialect::Claude, Dialect::Kimi, &conv)
        .unwrap();
    let tools = mapped.tool_calls();
    assert_eq!(tools.len(), 1);
}

#[test]
fn kimi_claude_tool_role_becomes_user() {
    // Kimi Tool role → Claude User role
    let m = ClaudeKimiIrMapper;
    let conv = tool_call_conversation();
    let mapped = m
        .map_request(Dialect::Kimi, Dialect::Claude, &conv)
        .unwrap();
    // Tool-role message should become User role
    assert_eq!(mapped.messages[2].role, IrRole::User);
}

#[test]
fn claude_kimi_system_message() {
    let m = ClaudeKimiIrMapper;
    let conv = system_only_conversation();
    let mapped = m
        .map_request(Dialect::Claude, Dialect::Kimi, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 1);
    assert_eq!(mapped.messages[0].role, IrRole::System);
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Identity mapping — same-dialect pairs
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn identity_all_dialects_passthrough() {
    let m = IrIdentityMapper;
    let conv = simple_conversation();
    for &d in Dialect::all() {
        let result = m.map_request(d, d, &conv).unwrap();
        assert_eq!(result, conv, "identity failed for {d}");
    }
}

#[test]
fn identity_preserves_tool_calls() {
    let m = IrIdentityMapper;
    let conv = tool_call_conversation();
    let result = m
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(result, conv);
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Unknown / unsupported dialect pairs → error
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn unsupported_pair_codex_gemini() {
    // No direct Codex ↔ Gemini mapper exists.
    let mapper = default_ir_mapper(Dialect::Codex, Dialect::Gemini);
    assert!(mapper.is_none());
}

#[test]
fn unsupported_pair_kimi_codex() {
    let mapper = default_ir_mapper(Dialect::Kimi, Dialect::Codex);
    assert!(mapper.is_none());
}

#[test]
fn unsupported_pair_copilot_kimi() {
    let mapper = default_ir_mapper(Dialect::Copilot, Dialect::Kimi);
    assert!(mapper.is_none());
}

#[test]
fn mapper_wrong_pair_returns_error() {
    // Feed the wrong dialect pair to a specific mapper.
    let m = OpenAiCodexIrMapper;
    let conv = simple_conversation();
    let err = m
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// 9. default_ir_mapper() returns correct mapper for each pair
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn factory_identity_all_dialects() {
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
fn factory_claude_kimi_both_directions() {
    assert!(default_ir_mapper(Dialect::Claude, Dialect::Kimi).is_some());
    assert!(default_ir_mapper(Dialect::Kimi, Dialect::Claude).is_some());
}

#[test]
fn factory_resolves_and_maps_each_pair() {
    let conv = simple_conversation();
    let cross_pairs = [
        (Dialect::OpenAi, Dialect::Claude),
        (Dialect::OpenAi, Dialect::Gemini),
        (Dialect::Claude, Dialect::Gemini),
        (Dialect::OpenAi, Dialect::Codex),
        (Dialect::OpenAi, Dialect::Kimi),
        (Dialect::Claude, Dialect::Kimi),
    ];
    for (from, to) in cross_pairs {
        let mapper =
            default_ir_mapper(from, to).unwrap_or_else(|| panic!("no mapper for {from} → {to}"));
        let result = mapper.map_request(from, to, &conv);
        assert!(
            result.is_ok(),
            "mapping failed for {from} → {to}: {:?}",
            result.err()
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. supported_ir_pairs() lists all expected pairs
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn supported_pairs_includes_identity() {
    let pairs = supported_ir_pairs();
    for &d in Dialect::all() {
        assert!(pairs.contains(&(d, d)), "missing identity pair for {d}");
    }
}

#[test]
fn supported_pairs_includes_all_cross_dialect() {
    let pairs = supported_ir_pairs();
    let expected = [
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
    ];
    for (from, to) in expected {
        assert!(pairs.contains(&(from, to)), "missing pair ({from}, {to})");
    }
}

#[test]
fn supported_pairs_all_resolvable() {
    // Every pair listed by supported_ir_pairs() must resolve via default_ir_mapper().
    for (from, to) in supported_ir_pairs() {
        assert!(
            default_ir_mapper(from, to).is_some(),
            "supported_ir_pairs lists ({from}, {to}) but default_ir_mapper returns None"
        );
    }
}
