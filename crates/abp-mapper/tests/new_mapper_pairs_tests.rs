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

//! Tests for the three new IR mapper pairs:
//! - OpenAI ↔ Copilot (near-identity)
//! - Gemini ↔ Kimi (tool-role bridging)
//! - Codex ↔ Claude (lossy — Codex is output-only)

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;
use abp_mapper::{
    CodexClaudeIrMapper, GeminiKimiIrMapper, IrMapper, MapError, OpenAiCopilotIrMapper,
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

fn empty_conversation() -> IrConversation {
    IrConversation::new()
}

fn large_payload_conversation() -> IrConversation {
    let large_text = "x".repeat(100_000);
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, &large_text),
        IrMessage::text(IrRole::Assistant, &large_text),
    ])
}

// ═══════════════════════════════════════════════════════════════════════
// OpenAI ↔ Copilot (near-identity)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_copilot_simple_conversation() {
    let m = OpenAiCopilotIrMapper;
    let conv = simple_conversation();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Copilot, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 3);
    assert_eq!(mapped.messages[0].role, IrRole::System);
    assert_eq!(mapped.messages[1].text_content(), "Hello");
    assert_eq!(mapped.messages[2].text_content(), "Hi!");
}

#[test]
fn copilot_openai_simple_conversation() {
    let m = OpenAiCopilotIrMapper;
    let conv = simple_conversation();
    let mapped = m
        .map_request(Dialect::Copilot, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 3);
    assert_eq!(mapped.messages[0].role, IrRole::System);
    assert_eq!(mapped.messages[2].text_content(), "Hi!");
}

#[test]
fn openai_copilot_thinking_dropped() {
    let m = OpenAiCopilotIrMapper;
    let conv = thinking_conversation();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Copilot, &conv)
        .unwrap();
    let asst = &mapped.messages[1];
    assert_eq!(asst.content.len(), 1);
    assert!(
        matches!(&asst.content[0], IrContentBlock::Text { text } if text == "The answer is 42.")
    );
}

#[test]
fn openai_copilot_tool_calls_preserved() {
    let m = OpenAiCopilotIrMapper;
    let conv = tool_call_conversation();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Copilot, &conv)
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
fn openai_copilot_roundtrip_simple() {
    let m = OpenAiCopilotIrMapper;
    let orig = simple_conversation();
    let copilot = m
        .map_request(Dialect::OpenAi, Dialect::Copilot, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Copilot, Dialect::OpenAi, &copilot)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    for (o, b) in orig.messages.iter().zip(back.messages.iter()) {
        assert_eq!(o.role, b.role);
        assert_eq!(o.text_content(), b.text_content());
    }
}

#[test]
fn openai_copilot_empty_conversation() {
    let m = OpenAiCopilotIrMapper;
    let conv = empty_conversation();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Copilot, &conv)
        .unwrap();
    assert!(mapped.is_empty());
}

#[test]
fn openai_copilot_unsupported_pair() {
    let m = OpenAiCopilotIrMapper;
    let conv = simple_conversation();
    let err = m
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn openai_copilot_supported_pairs() {
    let m = OpenAiCopilotIrMapper;
    let pairs = m.supported_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Copilot)));
    assert!(pairs.contains(&(Dialect::Copilot, Dialect::OpenAi)));
    assert_eq!(pairs.len(), 2);
}

#[test]
fn openai_copilot_response_mapping() {
    let m = OpenAiCopilotIrMapper;
    let conv = simple_conversation();
    let result = m
        .map_response(Dialect::OpenAi, Dialect::Copilot, &conv)
        .unwrap();
    assert_eq!(result.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════
// Gemini ↔ Kimi (tool-role bridging)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn gemini_kimi_simple_conversation() {
    let m = GeminiKimiIrMapper;
    let conv = simple_conversation();
    let mapped = m
        .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 3);
    assert_eq!(mapped.messages[0].role, IrRole::System);
    assert_eq!(mapped.messages[1].text_content(), "Hello");
    assert_eq!(mapped.messages[2].text_content(), "Hi!");
}

#[test]
fn kimi_gemini_simple_conversation() {
    let m = GeminiKimiIrMapper;
    let conv = simple_conversation();
    let mapped = m
        .map_request(Dialect::Kimi, Dialect::Gemini, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 3);
    assert_eq!(mapped.messages[0].role, IrRole::System);
}

#[test]
fn gemini_kimi_user_tool_results_become_tool_role() {
    // Gemini uses User role for tool results; Kimi uses Tool role
    let m = GeminiKimiIrMapper;
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Search"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "search".into(),
                input: json!({"q": "rust"}),
            }],
        ),
        IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "result".into(),
                }],
                is_error: false,
            }],
        ),
    ]);
    let mapped = m
        .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
        .unwrap();
    // User message with ToolResult should become Tool-role
    let tool_msgs: Vec<_> = mapped
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert_eq!(tool_msgs.len(), 1);
}

#[test]
fn kimi_gemini_tool_role_becomes_user() {
    // Kimi Tool role → Gemini User role
    let m = GeminiKimiIrMapper;
    let conv = tool_call_conversation();
    let mapped = m
        .map_request(Dialect::Kimi, Dialect::Gemini, &conv)
        .unwrap();
    // Tool-role message should become User role
    assert_eq!(mapped.messages[2].role, IrRole::User);
}

#[test]
fn gemini_kimi_thinking_dropped() {
    let m = GeminiKimiIrMapper;
    let conv = thinking_conversation();
    let mapped = m
        .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
        .unwrap();
    assert!(!mapped.messages.iter().any(|msg| {
        msg.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    }));
}

#[test]
fn gemini_kimi_unsupported_pair() {
    let m = GeminiKimiIrMapper;
    let conv = simple_conversation();
    let err = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn gemini_kimi_empty_conversation() {
    let m = GeminiKimiIrMapper;
    let conv = empty_conversation();
    let mapped = m
        .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
        .unwrap();
    assert!(mapped.is_empty());
}

#[test]
fn gemini_kimi_supported_pairs() {
    let m = GeminiKimiIrMapper;
    let pairs = m.supported_pairs();
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::Kimi)));
    assert!(pairs.contains(&(Dialect::Kimi, Dialect::Gemini)));
    assert_eq!(pairs.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// Codex ↔ Claude (lossy — Codex is output-only)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn claude_codex_simple_conversation() {
    let m = CodexClaudeIrMapper;
    let conv = simple_conversation();
    let mapped = m
        .map_request(Dialect::Claude, Dialect::Codex, &conv)
        .unwrap();
    // System message is emulated as [System]-prefixed user message for Codex
    assert_eq!(mapped.len(), 3);
    assert_eq!(mapped.messages[0].role, IrRole::User);
    assert!(mapped.messages[0].text_content().starts_with("[System]"));
    assert_eq!(mapped.messages[1].role, IrRole::User);
    assert_eq!(mapped.messages[2].role, IrRole::Assistant);
}

#[test]
fn codex_claude_lossless_passthrough() {
    let m = CodexClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Write code"),
        IrMessage::text(IrRole::Assistant, "fn main() {}"),
    ]);
    let mapped = m
        .map_request(Dialect::Codex, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(mapped, conv);
}

#[test]
fn claude_codex_system_message_lost() {
    let m = CodexClaudeIrMapper;
    let conv =
        IrConversation::from_messages(vec![IrMessage::text(IrRole::System, "System prompt only.")]);
    let mapped = m
        .map_request(Dialect::Claude, Dialect::Codex, &conv)
        .unwrap();
    // System message is emulated as a [System]-prefixed user message
    assert_eq!(mapped.len(), 1);
    assert_eq!(mapped.messages[0].role, IrRole::User);
    assert!(
        mapped.messages[0]
            .text_content()
            .contains("System prompt only.")
    );
}

#[test]
fn claude_codex_tool_calls_lost() {
    let m = CodexClaudeIrMapper;
    let conv = tool_call_conversation();
    let mapped = m
        .map_request(Dialect::Claude, Dialect::Codex, &conv)
        .unwrap();
    let tool_calls = mapped.tool_calls();
    assert!(tool_calls.is_empty());
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
fn claude_codex_thinking_lost() {
    let m = CodexClaudeIrMapper;
    let conv = thinking_conversation();
    let mapped = m
        .map_request(Dialect::Claude, Dialect::Codex, &conv)
        .unwrap();
    assert!(!mapped.messages.iter().any(|msg| {
        msg.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    }));
    // Text content should survive
    assert_eq!(mapped.messages[1].text_content(), "The answer is 42.");
}

#[test]
fn codex_claude_unsupported_pair() {
    let m = CodexClaudeIrMapper;
    let conv = simple_conversation();
    let err = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn codex_claude_empty_conversation() {
    let m = CodexClaudeIrMapper;
    let conv = empty_conversation();
    let mapped = m
        .map_request(Dialect::Codex, Dialect::Claude, &conv)
        .unwrap();
    assert!(mapped.is_empty());
}

#[test]
fn codex_claude_supported_pairs() {
    let m = CodexClaudeIrMapper;
    let pairs = m.supported_pairs();
    assert!(pairs.contains(&(Dialect::Codex, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::Codex)));
    assert_eq!(pairs.len(), 2);
}

#[test]
fn claude_codex_response_mapping() {
    let m = CodexClaudeIrMapper;
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "hello"),
        IrMessage::text(IrRole::Assistant, "world"),
    ]);
    let result = m
        .map_response(Dialect::Claude, Dialect::Codex, &conv)
        .unwrap();
    assert_eq!(result.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// Factory integration
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn factory_openai_copilot_both_directions() {
    assert!(default_ir_mapper(Dialect::OpenAi, Dialect::Copilot).is_some());
    assert!(default_ir_mapper(Dialect::Copilot, Dialect::OpenAi).is_some());
}

#[test]
fn factory_gemini_kimi_both_directions() {
    assert!(default_ir_mapper(Dialect::Gemini, Dialect::Kimi).is_some());
    assert!(default_ir_mapper(Dialect::Kimi, Dialect::Gemini).is_some());
}

#[test]
fn factory_codex_claude_both_directions() {
    assert!(default_ir_mapper(Dialect::Codex, Dialect::Claude).is_some());
    assert!(default_ir_mapper(Dialect::Claude, Dialect::Codex).is_some());
}

#[test]
fn supported_pairs_includes_new_mappers() {
    let pairs = supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Copilot)));
    assert!(pairs.contains(&(Dialect::Copilot, Dialect::OpenAi)));
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::Kimi)));
    assert!(pairs.contains(&(Dialect::Kimi, Dialect::Gemini)));
    assert!(pairs.contains(&(Dialect::Codex, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::Codex)));
}

#[test]
fn factory_resolves_and_maps_new_pairs() {
    let conv = simple_conversation();
    let new_pairs = [
        (Dialect::OpenAi, Dialect::Copilot),
        (Dialect::Copilot, Dialect::OpenAi),
        (Dialect::Gemini, Dialect::Kimi),
        (Dialect::Kimi, Dialect::Gemini),
        (Dialect::Codex, Dialect::Claude),
        (Dialect::Claude, Dialect::Codex),
    ];
    for (from, to) in new_pairs {
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

#[test]
fn large_payload_openai_copilot() {
    let m = OpenAiCopilotIrMapper;
    let conv = large_payload_conversation();
    let mapped = m
        .map_request(Dialect::OpenAi, Dialect::Copilot, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 2);
    assert_eq!(mapped.messages[0].text_content().len(), 100_000);
}

#[test]
fn large_payload_codex_claude() {
    let m = CodexClaudeIrMapper;
    let conv = large_payload_conversation();
    let mapped = m
        .map_request(Dialect::Codex, Dialect::Claude, &conv)
        .unwrap();
    assert_eq!(mapped.len(), 2);
    assert_eq!(mapped.messages[0].text_content().len(), 100_000);
}

#[test]
fn new_mappers_are_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<OpenAiCopilotIrMapper>();
    assert_send_sync::<GeminiKimiIrMapper>();
    assert_send_sync::<CodexClaudeIrMapper>();
}
