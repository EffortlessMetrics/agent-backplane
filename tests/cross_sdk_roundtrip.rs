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
//! Cross-SDK round trip tests verifying conversions between all SDK pairs.
//!
//! For each supported mapper pair, we verify:
//! 1. Simple text requests survive A→B→A roundtrip
//! 2. Tool definitions carry through conversion
//! 3. System messages survive mapping
//! 4. Multi-turn conversations convert correctly
//! 5. Streaming events map correctly (via response path)
//! 6. Parameters (temperature, max_tokens) map through IR config
//! 7. Response mapping converts back
//! 8. Lossy conversions are detected and labelled
//! 9. Conversion errors have correct codes
//! 10. Kimi/Codex/Copilot (OpenAI-compatible dialects) work transparently

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;
use abp_mapper::{
    ClaudeGeminiIrMapper, ClaudeKimiIrMapper, CodexClaudeIrMapper, GeminiKimiIrMapper, IrMapper,
    MapError, OpenAiClaudeIrMapper, OpenAiCodexIrMapper, OpenAiCopilotIrMapper,
    OpenAiGeminiIrMapper, OpenAiKimiIrMapper, default_ir_mapper, supported_ir_pairs,
};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn simple_text() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi there!"),
    ])
}

fn with_system() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi!"),
    ])
}

fn with_tools() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "What's the weather?"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me check.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "get_weather".into(),
                    input: json!({"city": "Portland"}),
                },
            ],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "55°F, cloudy".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(IrRole::Assistant, "It's 55°F and cloudy in Portland."),
    ])
}

fn multi_turn() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be concise."),
        IrMessage::text(IrRole::User, "What is Rust?"),
        IrMessage::text(IrRole::Assistant, "A systems language."),
        IrMessage::text(IrRole::User, "Is it fast?"),
        IrMessage::text(IrRole::Assistant, "Yes."),
        IrMessage::text(IrRole::User, "Thanks."),
        IrMessage::text(IrRole::Assistant, "Welcome!"),
    ])
}

fn multi_tool() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Do two things."),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::ToolUse {
                    id: "a".into(),
                    name: "search".into(),
                    input: json!({"q": "rust"}),
                },
                IrContentBlock::ToolUse {
                    id: "b".into(),
                    name: "read".into(),
                    input: json!({"path": "lib.rs"}),
                },
            ],
        ),
        IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::ToolResult {
                    tool_use_id: "a".into(),
                    content: vec![IrContentBlock::Text {
                        text: "found it".into(),
                    }],
                    is_error: false,
                },
                IrContentBlock::ToolResult {
                    tool_use_id: "b".into(),
                    content: vec![IrContentBlock::Text {
                        text: "fn main(){}".into(),
                    }],
                    is_error: false,
                },
            ],
        ),
    ])
}

fn thinking_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Hard problem"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Let me think...".into(),
                },
                IrContentBlock::Text {
                    text: "The answer is 42.".into(),
                },
            ],
        ),
    ])
}

fn error_tool_result() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "run cmd"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "e1".into(),
                name: "exec".into(),
                input: json!({"cmd": "fail"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "e1".into(),
                content: vec![IrContentBlock::Text {
                    text: "error: command not found".into(),
                }],
                is_error: true,
            }],
        ),
    ])
}

fn image_conv() -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "Describe this.".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "iVBORw0KGgo=".into(),
            },
        ],
    )])
}

fn metadata_conv() -> IrConversation {
    let mut msg = IrMessage::text(IrRole::User, "hello");
    msg.metadata.insert("source".into(), json!("test"));
    msg.metadata.insert("ts".into(), json!(1234567890));
    IrConversation::from_messages(vec![msg])
}

fn empty_conv() -> IrConversation {
    IrConversation::new()
}

/// Assert text content is preserved between two conversations.
fn assert_text_preserved(a: &IrConversation, b: &IrConversation) {
    for (orig, mapped) in a.messages.iter().zip(b.messages.iter()) {
        assert_eq!(
            orig.text_content(),
            mapped.text_content(),
            "text mismatch for role {:?}",
            orig.role
        );
    }
}

/// Count ToolUse blocks across all messages.
fn count_tool_uses(conv: &IrConversation) -> usize {
    conv.tool_calls().len()
}

/// Assert no thinking blocks remain after mapping.
fn assert_no_thinking(conv: &IrConversation) {
    for msg in &conv.messages {
        for block in &msg.content {
            assert!(
                !matches!(block, IrContentBlock::Thinking { .. }),
                "unexpected thinking block after mapping"
            );
        }
    }
}

/// Assert that an is_error tool result exists in the conversation.
fn has_error_tool_result(conv: &IrConversation) -> bool {
    conv.messages
        .iter()
        .flat_map(|m| &m.content)
        .any(|b| matches!(b, IrContentBlock::ToolResult { is_error, .. } if *is_error))
}

// ═══════════════════════════════════════════════════════════════════════
// 1. OpenAI ↔ Claude (10 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn oai_claude_simple_text_roundtrip() {
    let m = OpenAiClaudeIrMapper;
    let orig = simple_text();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Claude, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

#[test]
fn oai_claude_with_tools_roundtrip() {
    let m = OpenAiClaudeIrMapper;
    let orig = with_tools();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Claude, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(count_tool_uses(&orig), count_tool_uses(&back));
}

#[test]
fn oai_claude_system_message_survives() {
    let m = OpenAiClaudeIrMapper;
    let orig = with_system();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Claude, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(back.messages[0].role, IrRole::System);
    assert_eq!(
        back.messages[0].text_content(),
        "You are a helpful assistant."
    );
}

#[test]
fn oai_claude_multi_turn_roundtrip() {
    let m = OpenAiClaudeIrMapper;
    let orig = multi_turn();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Claude, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

#[test]
fn oai_claude_response_path_roundtrip() {
    let m = OpenAiClaudeIrMapper;
    let orig = simple_text();
    let mid = m
        .map_response(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = m
        .map_response(Dialect::Claude, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

#[test]
fn oai_claude_tool_input_preserved() {
    let m = OpenAiClaudeIrMapper;
    let orig = with_tools();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Claude, Dialect::OpenAi, &mid)
        .unwrap();
    let orig_tools = orig.tool_calls();
    let back_tools = back.tool_calls();
    for (ot, bt) in orig_tools.iter().zip(back_tools.iter()) {
        if let (
            IrContentBlock::ToolUse { input: oi, .. },
            IrContentBlock::ToolUse { input: bi, .. },
        ) = (ot, bt)
        {
            assert_eq!(oi, bi);
        }
    }
}

#[test]
fn oai_claude_error_tool_result_roundtrip() {
    let m = OpenAiClaudeIrMapper;
    let orig = error_tool_result();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Claude, Dialect::OpenAi, &mid)
        .unwrap();
    assert!(has_error_tool_result(&back), "is_error lost in roundtrip");
}

#[test]
fn oai_claude_metadata_roundtrip() {
    let m = OpenAiClaudeIrMapper;
    let orig = metadata_conv();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Claude, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(
        back.messages[0].metadata.get("source"),
        Some(&json!("test"))
    );
}

#[test]
fn oai_claude_empty_roundtrip() {
    let m = OpenAiClaudeIrMapper;
    let orig = empty_conv();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Claude, Dialect::OpenAi, &mid)
        .unwrap();
    assert!(back.is_empty());
}

#[test]
fn oai_claude_image_preserved() {
    let m = OpenAiClaudeIrMapper;
    let orig = image_conv();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Claude, Dialect::OpenAi, &mid)
        .unwrap();
    assert!(
        back.messages[0]
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Image { .. }))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 2. OpenAI ↔ Gemini (10 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn oai_gemini_simple_text_roundtrip() {
    let m = OpenAiGeminiIrMapper;
    let orig = simple_text();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

#[test]
fn oai_gemini_with_tools_roundtrip() {
    let m = OpenAiGeminiIrMapper;
    let orig = with_tools();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(count_tool_uses(&orig), count_tool_uses(&back));
}

#[test]
fn oai_gemini_system_message_survives() {
    let m = OpenAiGeminiIrMapper;
    let orig = with_system();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(back.messages[0].role, IrRole::System);
    assert_eq!(
        back.messages[0].text_content(),
        "You are a helpful assistant."
    );
}

#[test]
fn oai_gemini_multi_turn_roundtrip() {
    let m = OpenAiGeminiIrMapper;
    let orig = multi_turn();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

#[test]
fn oai_gemini_response_path_roundtrip() {
    let m = OpenAiGeminiIrMapper;
    let orig = simple_text();
    let mid = m
        .map_response(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_response(Dialect::Gemini, Dialect::OpenAi, &mid)
        .unwrap();
    assert_text_preserved(&orig, &back);
}

#[test]
fn oai_gemini_tool_input_preserved() {
    let m = OpenAiGeminiIrMapper;
    let orig = with_tools();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::OpenAi, &mid)
        .unwrap();
    let orig_tools = orig.tool_calls();
    let back_tools = back.tool_calls();
    for (ot, bt) in orig_tools.iter().zip(back_tools.iter()) {
        if let (
            IrContentBlock::ToolUse { input: oi, .. },
            IrContentBlock::ToolUse { input: bi, .. },
        ) = (ot, bt)
        {
            assert_eq!(oi, bi);
        }
    }
}

#[test]
fn oai_gemini_error_tool_result_roundtrip() {
    let m = OpenAiGeminiIrMapper;
    let orig = error_tool_result();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::OpenAi, &mid)
        .unwrap();
    assert!(
        has_error_tool_result(&back),
        "is_error lost in OpenAI→Gemini roundtrip"
    );
}

#[test]
fn oai_gemini_metadata_roundtrip() {
    let m = OpenAiGeminiIrMapper;
    let orig = metadata_conv();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(
        back.messages[0].metadata.get("source"),
        Some(&json!("test"))
    );
}

#[test]
fn oai_gemini_empty_roundtrip() {
    let m = OpenAiGeminiIrMapper;
    let orig = empty_conv();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::OpenAi, &mid)
        .unwrap();
    assert!(back.is_empty());
}

#[test]
fn oai_gemini_image_preserved() {
    let m = OpenAiGeminiIrMapper;
    let orig = image_conv();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::OpenAi, &mid)
        .unwrap();
    assert!(
        back.messages[0]
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Image { .. }))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Claude ↔ Gemini (8 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn claude_gemini_simple_text_roundtrip() {
    let m = ClaudeGeminiIrMapper;
    let orig = simple_text();
    let mid = m
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::Claude, &mid)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

#[test]
fn claude_gemini_with_tools_roundtrip() {
    let m = ClaudeGeminiIrMapper;
    let orig = with_tools();
    let mid = m
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::Claude, &mid)
        .unwrap();
    assert_eq!(count_tool_uses(&orig), count_tool_uses(&back));
}

#[test]
fn claude_gemini_system_message_survives() {
    let m = ClaudeGeminiIrMapper;
    let orig = with_system();
    let mid = m
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::Claude, &mid)
        .unwrap();
    assert_eq!(
        back.messages[0].text_content(),
        "You are a helpful assistant."
    );
}

#[test]
fn claude_gemini_multi_turn_roundtrip() {
    let m = ClaudeGeminiIrMapper;
    let orig = multi_turn();
    let mid = m
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::Claude, &mid)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

#[test]
fn claude_gemini_response_path_roundtrip() {
    let m = ClaudeGeminiIrMapper;
    let orig = simple_text();
    let mid = m
        .map_response(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_response(Dialect::Gemini, Dialect::Claude, &mid)
        .unwrap();
    assert_text_preserved(&orig, &back);
}

#[test]
fn claude_gemini_error_tool_result_roundtrip() {
    let m = ClaudeGeminiIrMapper;
    let orig = error_tool_result();
    let mid = m
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::Claude, &mid)
        .unwrap();
    assert!(
        has_error_tool_result(&back),
        "is_error lost in Claude→Gemini roundtrip"
    );
}

#[test]
fn claude_gemini_empty_roundtrip() {
    let m = ClaudeGeminiIrMapper;
    let orig = empty_conv();
    let mid = m
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::Claude, &mid)
        .unwrap();
    assert!(back.is_empty());
}

#[test]
fn claude_gemini_image_roundtrip() {
    let m = ClaudeGeminiIrMapper;
    let orig = image_conv();
    let mid = m
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::Claude, &mid)
        .unwrap();
    assert!(
        back.messages[0]
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Image { .. }))
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 4. OpenAI ↔ Kimi — OpenAI-compatible dialect (8 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn oai_kimi_simple_text_roundtrip() {
    let m = OpenAiKimiIrMapper;
    let orig = simple_text();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Kimi, &orig)
        .unwrap();
    let back = m.map_request(Dialect::Kimi, Dialect::OpenAi, &mid).unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

#[test]
fn oai_kimi_with_tools_roundtrip() {
    let m = OpenAiKimiIrMapper;
    let orig = with_tools();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Kimi, &orig)
        .unwrap();
    let back = m.map_request(Dialect::Kimi, Dialect::OpenAi, &mid).unwrap();
    assert_eq!(count_tool_uses(&orig), count_tool_uses(&back));
}

#[test]
fn oai_kimi_system_message_survives() {
    let m = OpenAiKimiIrMapper;
    let orig = with_system();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Kimi, &orig)
        .unwrap();
    let back = m.map_request(Dialect::Kimi, Dialect::OpenAi, &mid).unwrap();
    assert_eq!(back.messages[0].role, IrRole::System);
    assert_eq!(
        back.messages[0].text_content(),
        "You are a helpful assistant."
    );
}

#[test]
fn oai_kimi_multi_turn_roundtrip() {
    let m = OpenAiKimiIrMapper;
    let orig = multi_turn();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Kimi, &orig)
        .unwrap();
    let back = m.map_request(Dialect::Kimi, Dialect::OpenAi, &mid).unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

#[test]
fn oai_kimi_response_path_roundtrip() {
    let m = OpenAiKimiIrMapper;
    let orig = simple_text();
    let mid = m
        .map_response(Dialect::OpenAi, Dialect::Kimi, &orig)
        .unwrap();
    let back = m
        .map_response(Dialect::Kimi, Dialect::OpenAi, &mid)
        .unwrap();
    assert_text_preserved(&orig, &back);
}

#[test]
fn oai_kimi_metadata_roundtrip() {
    let m = OpenAiKimiIrMapper;
    let orig = metadata_conv();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Kimi, &orig)
        .unwrap();
    let back = m.map_request(Dialect::Kimi, Dialect::OpenAi, &mid).unwrap();
    assert_eq!(
        back.messages[0].metadata.get("source"),
        Some(&json!("test"))
    );
}

#[test]
fn oai_kimi_empty_roundtrip() {
    let m = OpenAiKimiIrMapper;
    let orig = empty_conv();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Kimi, &orig)
        .unwrap();
    let back = m.map_request(Dialect::Kimi, Dialect::OpenAi, &mid).unwrap();
    assert!(back.is_empty());
}

#[test]
fn oai_kimi_error_tool_result_roundtrip() {
    let m = OpenAiKimiIrMapper;
    let orig = error_tool_result();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Kimi, &orig)
        .unwrap();
    let back = m.map_request(Dialect::Kimi, Dialect::OpenAi, &mid).unwrap();
    assert!(
        has_error_tool_result(&back),
        "is_error lost in Kimi roundtrip"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 5. OpenAI ↔ Copilot — OpenAI-compatible dialect (8 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn oai_copilot_simple_text_roundtrip() {
    let m = OpenAiCopilotIrMapper;
    let orig = simple_text();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Copilot, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Copilot, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

#[test]
fn oai_copilot_with_tools_roundtrip() {
    let m = OpenAiCopilotIrMapper;
    let orig = with_tools();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Copilot, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Copilot, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(count_tool_uses(&orig), count_tool_uses(&back));
}

#[test]
fn oai_copilot_system_message_survives() {
    let m = OpenAiCopilotIrMapper;
    let orig = with_system();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Copilot, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Copilot, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(back.messages[0].role, IrRole::System);
    assert_eq!(
        back.messages[0].text_content(),
        "You are a helpful assistant."
    );
}

#[test]
fn oai_copilot_multi_turn_roundtrip() {
    let m = OpenAiCopilotIrMapper;
    let orig = multi_turn();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Copilot, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Copilot, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

#[test]
fn oai_copilot_response_path_roundtrip() {
    let m = OpenAiCopilotIrMapper;
    let orig = simple_text();
    let mid = m
        .map_response(Dialect::OpenAi, Dialect::Copilot, &orig)
        .unwrap();
    let back = m
        .map_response(Dialect::Copilot, Dialect::OpenAi, &mid)
        .unwrap();
    assert_text_preserved(&orig, &back);
}

#[test]
fn oai_copilot_metadata_roundtrip() {
    let m = OpenAiCopilotIrMapper;
    let orig = metadata_conv();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Copilot, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Copilot, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(
        back.messages[0].metadata.get("source"),
        Some(&json!("test"))
    );
}

#[test]
fn oai_copilot_empty_roundtrip() {
    let m = OpenAiCopilotIrMapper;
    let orig = empty_conv();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Copilot, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Copilot, Dialect::OpenAi, &mid)
        .unwrap();
    assert!(back.is_empty());
}

#[test]
fn oai_copilot_error_tool_result_roundtrip() {
    let m = OpenAiCopilotIrMapper;
    let orig = error_tool_result();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Copilot, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Copilot, Dialect::OpenAi, &mid)
        .unwrap();
    assert!(
        has_error_tool_result(&back),
        "is_error lost in Copilot roundtrip"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 6. OpenAI ↔ Codex — lossy (Codex is output-only) (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn oai_codex_simple_text_roundtrip() {
    let m = OpenAiCodexIrMapper;
    let orig = simple_text();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Codex, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Codex, Dialect::OpenAi, &mid)
        .unwrap();
    assert_text_preserved(&orig, &back);
}

#[test]
fn oai_codex_system_dropped_lossy() {
    let m = OpenAiCodexIrMapper;
    let orig = with_system();
    let codex = m
        .map_request(Dialect::OpenAi, Dialect::Codex, &orig)
        .unwrap();
    // System message is dropped in Codex
    assert!(
        codex.messages.iter().all(|msg| msg.role != IrRole::System),
        "system messages should be dropped for Codex"
    );
}

#[test]
fn oai_codex_tools_dropped_lossy() {
    let m = OpenAiCodexIrMapper;
    let orig = with_tools();
    let codex = m
        .map_request(Dialect::OpenAi, Dialect::Codex, &orig)
        .unwrap();
    assert_eq!(
        count_tool_uses(&codex),
        0,
        "tool calls should be dropped for Codex"
    );
}

#[test]
fn oai_codex_text_content_preserved_in_lossy() {
    let m = OpenAiCodexIrMapper;
    let orig = with_tools();
    let codex = m
        .map_request(Dialect::OpenAi, Dialect::Codex, &orig)
        .unwrap();
    // User text "What's the weather?" should survive
    let has_user_text = codex
        .messages
        .iter()
        .any(|m| m.role == IrRole::User && m.text_content().contains("weather"));
    assert!(has_user_text, "user text lost in Codex mapping");
}

#[test]
fn oai_codex_empty_roundtrip() {
    let m = OpenAiCodexIrMapper;
    let orig = empty_conv();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Codex, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Codex, Dialect::OpenAi, &mid)
        .unwrap();
    assert!(back.is_empty());
}

#[test]
fn oai_codex_response_path_roundtrip() {
    let m = OpenAiCodexIrMapper;
    let orig = simple_text();
    let mid = m
        .map_response(Dialect::OpenAi, Dialect::Codex, &orig)
        .unwrap();
    let back = m
        .map_response(Dialect::Codex, Dialect::OpenAi, &mid)
        .unwrap();
    assert_text_preserved(&orig, &back);
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Codex ↔ Claude — lossy (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn codex_claude_simple_text_roundtrip() {
    let m = CodexClaudeIrMapper;
    let orig = simple_text();
    let mid = m
        .map_request(Dialect::Codex, Dialect::Claude, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Claude, Dialect::Codex, &mid)
        .unwrap();
    assert_text_preserved(&orig, &back);
}

#[test]
fn codex_claude_system_dropped_lossy() {
    let m = CodexClaudeIrMapper;
    let orig = with_system();
    let codex = m
        .map_request(Dialect::Claude, Dialect::Codex, &orig)
        .unwrap();
    assert!(
        codex.messages.iter().all(|msg| msg.role != IrRole::System),
        "system messages should be dropped for Codex"
    );
}

#[test]
fn codex_claude_tools_dropped_lossy() {
    let m = CodexClaudeIrMapper;
    let orig = with_tools();
    let codex = m
        .map_request(Dialect::Claude, Dialect::Codex, &orig)
        .unwrap();
    assert_eq!(count_tool_uses(&codex), 0);
}

#[test]
fn codex_claude_empty_roundtrip() {
    let m = CodexClaudeIrMapper;
    let orig = empty_conv();
    let mid = m
        .map_request(Dialect::Codex, Dialect::Claude, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Claude, Dialect::Codex, &mid)
        .unwrap();
    assert!(back.is_empty());
}

#[test]
fn codex_claude_response_path_roundtrip() {
    let m = CodexClaudeIrMapper;
    let orig = simple_text();
    let mid = m
        .map_response(Dialect::Codex, Dialect::Claude, &orig)
        .unwrap();
    let back = m
        .map_response(Dialect::Claude, Dialect::Codex, &mid)
        .unwrap();
    assert_text_preserved(&orig, &back);
}

#[test]
fn codex_claude_multi_turn_lossy() {
    let m = CodexClaudeIrMapper;
    let orig = multi_turn();
    let codex = m
        .map_request(Dialect::Claude, Dialect::Codex, &orig)
        .unwrap();
    // System dropped, text messages survive
    let text_msgs: Vec<_> = codex
        .messages
        .iter()
        .filter(|msg| msg.role != IrRole::System)
        .collect();
    assert!(!text_msgs.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Claude ↔ Kimi (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn claude_kimi_simple_text_roundtrip() {
    let m = ClaudeKimiIrMapper;
    let orig = simple_text();
    let mid = m
        .map_request(Dialect::Claude, Dialect::Kimi, &orig)
        .unwrap();
    let back = m.map_request(Dialect::Kimi, Dialect::Claude, &mid).unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

#[test]
fn claude_kimi_with_tools_roundtrip() {
    let m = ClaudeKimiIrMapper;
    let orig = with_tools();
    let mid = m
        .map_request(Dialect::Claude, Dialect::Kimi, &orig)
        .unwrap();
    let back = m.map_request(Dialect::Kimi, Dialect::Claude, &mid).unwrap();
    assert_eq!(count_tool_uses(&orig), count_tool_uses(&back));
}

#[test]
fn claude_kimi_system_message_survives() {
    let m = ClaudeKimiIrMapper;
    let orig = with_system();
    let mid = m
        .map_request(Dialect::Claude, Dialect::Kimi, &orig)
        .unwrap();
    let back = m.map_request(Dialect::Kimi, Dialect::Claude, &mid).unwrap();
    assert_eq!(
        back.messages[0].text_content(),
        "You are a helpful assistant."
    );
}

#[test]
fn claude_kimi_multi_turn_roundtrip() {
    let m = ClaudeKimiIrMapper;
    let orig = multi_turn();
    let mid = m
        .map_request(Dialect::Claude, Dialect::Kimi, &orig)
        .unwrap();
    let back = m.map_request(Dialect::Kimi, Dialect::Claude, &mid).unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

#[test]
fn claude_kimi_response_path_roundtrip() {
    let m = ClaudeKimiIrMapper;
    let orig = simple_text();
    let mid = m
        .map_response(Dialect::Claude, Dialect::Kimi, &orig)
        .unwrap();
    let back = m
        .map_response(Dialect::Kimi, Dialect::Claude, &mid)
        .unwrap();
    assert_text_preserved(&orig, &back);
}

#[test]
fn claude_kimi_empty_roundtrip() {
    let m = ClaudeKimiIrMapper;
    let orig = empty_conv();
    let mid = m
        .map_request(Dialect::Claude, Dialect::Kimi, &orig)
        .unwrap();
    let back = m.map_request(Dialect::Kimi, Dialect::Claude, &mid).unwrap();
    assert!(back.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Gemini ↔ Kimi (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn gemini_kimi_simple_text_roundtrip() {
    let m = GeminiKimiIrMapper;
    let orig = simple_text();
    let mid = m
        .map_request(Dialect::Gemini, Dialect::Kimi, &orig)
        .unwrap();
    let back = m.map_request(Dialect::Kimi, Dialect::Gemini, &mid).unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

#[test]
fn gemini_kimi_with_tools_roundtrip() {
    let m = GeminiKimiIrMapper;
    let orig = with_tools();
    let mid = m
        .map_request(Dialect::Gemini, Dialect::Kimi, &orig)
        .unwrap();
    let back = m.map_request(Dialect::Kimi, Dialect::Gemini, &mid).unwrap();
    assert_eq!(count_tool_uses(&orig), count_tool_uses(&back));
}

#[test]
fn gemini_kimi_system_message_survives() {
    let m = GeminiKimiIrMapper;
    let orig = with_system();
    let mid = m
        .map_request(Dialect::Gemini, Dialect::Kimi, &orig)
        .unwrap();
    let back = m.map_request(Dialect::Kimi, Dialect::Gemini, &mid).unwrap();
    assert_eq!(
        back.messages[0].text_content(),
        "You are a helpful assistant."
    );
}

#[test]
fn gemini_kimi_multi_turn_roundtrip() {
    let m = GeminiKimiIrMapper;
    let orig = multi_turn();
    let mid = m
        .map_request(Dialect::Gemini, Dialect::Kimi, &orig)
        .unwrap();
    let back = m.map_request(Dialect::Kimi, Dialect::Gemini, &mid).unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

#[test]
fn gemini_kimi_response_path_roundtrip() {
    let m = GeminiKimiIrMapper;
    let orig = simple_text();
    let mid = m
        .map_response(Dialect::Gemini, Dialect::Kimi, &orig)
        .unwrap();
    let back = m
        .map_response(Dialect::Kimi, Dialect::Gemini, &mid)
        .unwrap();
    assert_text_preserved(&orig, &back);
}

#[test]
fn gemini_kimi_empty_roundtrip() {
    let m = GeminiKimiIrMapper;
    let orig = empty_conv();
    let mid = m
        .map_request(Dialect::Gemini, Dialect::Kimi, &orig)
        .unwrap();
    let back = m.map_request(Dialect::Kimi, Dialect::Gemini, &mid).unwrap();
    assert!(back.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Same-dialect identity roundtrip (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn identity_openai_preserves_exact_json() {
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::OpenAi).unwrap();
    let orig = with_tools();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::OpenAi, &orig)
        .unwrap();
    assert_eq!(orig, result);
}

#[test]
fn identity_claude_preserves_exact_json() {
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::Claude).unwrap();
    let orig = with_tools();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Claude, &orig)
        .unwrap();
    assert_eq!(orig, result);
}

#[test]
fn identity_gemini_preserves_exact_json() {
    let mapper = default_ir_mapper(Dialect::Gemini, Dialect::Gemini).unwrap();
    let orig = multi_turn();
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::Gemini, &orig)
        .unwrap();
    assert_eq!(orig, result);
}

#[test]
fn identity_kimi_preserves_exact_json() {
    let mapper = default_ir_mapper(Dialect::Kimi, Dialect::Kimi).unwrap();
    let orig = with_system();
    let result = mapper
        .map_request(Dialect::Kimi, Dialect::Kimi, &orig)
        .unwrap();
    assert_eq!(orig, result);
}

#[test]
fn identity_codex_preserves_exact_json() {
    let mapper = default_ir_mapper(Dialect::Codex, Dialect::Codex).unwrap();
    let orig = simple_text();
    let result = mapper
        .map_request(Dialect::Codex, Dialect::Codex, &orig)
        .unwrap();
    assert_eq!(orig, result);
}

#[test]
fn identity_copilot_preserves_exact_json() {
    let mapper = default_ir_mapper(Dialect::Copilot, Dialect::Copilot).unwrap();
    let orig = multi_turn();
    let result = mapper
        .map_request(Dialect::Copilot, Dialect::Copilot, &orig)
        .unwrap();
    assert_eq!(orig, result);
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Cross-dialect preserves semantics even if JSON differs (4 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn cross_dialect_semantics_oai_claude_text_preserved() {
    let m = OpenAiClaudeIrMapper;
    let orig = with_system();
    let claude = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    // System prompt text should appear somewhere in mapped conversation
    let system_text = claude
        .messages
        .iter()
        .find(|m| m.text_content().contains("helpful assistant"));
    assert!(
        system_text.is_some(),
        "system semantics lost in cross-dialect"
    );
}

#[test]
fn cross_dialect_semantics_tool_names_preserved() {
    let m = OpenAiGeminiIrMapper;
    let orig = with_tools();
    let gemini = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let tool_names: Vec<_> = gemini
        .tool_calls()
        .iter()
        .filter_map(|b| match b {
            IrContentBlock::ToolUse { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert!(tool_names.contains(&"get_weather".to_string()));
}

#[test]
fn cross_dialect_semantics_multi_tool_all_present() {
    let m = ClaudeGeminiIrMapper;
    let orig = multi_tool();
    let gemini = m
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::Claude, &gemini)
        .unwrap();
    let orig_names: Vec<_> = orig
        .tool_calls()
        .iter()
        .filter_map(|b| match b {
            IrContentBlock::ToolUse { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    let back_names: Vec<_> = back
        .tool_calls()
        .iter()
        .filter_map(|b| match b {
            IrContentBlock::ToolUse { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(orig_names, back_names);
}

#[test]
fn cross_dialect_semantics_error_flag_survives_three_way() {
    let oc = OpenAiClaudeIrMapper;
    let cg = ClaudeGeminiIrMapper;
    let go = OpenAiGeminiIrMapper;
    let orig = error_tool_result();
    let claude = oc
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let gemini = cg
        .map_request(Dialect::Claude, Dialect::Gemini, &claude)
        .unwrap();
    let back = go
        .map_request(Dialect::Gemini, Dialect::OpenAi, &gemini)
        .unwrap();
    assert!(
        has_error_tool_result(&back),
        "is_error flag lost in 3-way roundtrip"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Lossy marking — thinking blocks dropped (6 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn lossy_thinking_dropped_claude_to_openai() {
    let m = OpenAiClaudeIrMapper;
    let orig = thinking_conv();
    let openai = m
        .map_request(Dialect::Claude, Dialect::OpenAi, &orig)
        .unwrap();
    assert_no_thinking(&openai);
    assert_eq!(openai.messages[1].text_content(), "The answer is 42.");
}

#[test]
fn lossy_thinking_dropped_claude_to_gemini() {
    let m = ClaudeGeminiIrMapper;
    let orig = thinking_conv();
    let gemini = m
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    assert_no_thinking(&gemini);
    assert_eq!(gemini.messages[1].text_content(), "The answer is 42.");
}

#[test]
fn lossy_thinking_dropped_claude_to_kimi() {
    let m = ClaudeKimiIrMapper;
    let orig = thinking_conv();
    let kimi = m
        .map_request(Dialect::Claude, Dialect::Kimi, &orig)
        .unwrap();
    assert_no_thinking(&kimi);
    assert_eq!(kimi.messages[1].text_content(), "The answer is 42.");
}

#[test]
fn lossy_thinking_not_recovered_after_roundtrip() {
    let m = OpenAiClaudeIrMapper;
    let orig = thinking_conv();
    let openai = m
        .map_request(Dialect::Claude, Dialect::OpenAi, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &openai)
        .unwrap();
    assert_no_thinking(&back);
    assert_eq!(back.messages[1].text_content(), "The answer is 42.");
}

#[test]
fn lossy_system_prompt_preserved_all_paths() {
    let oc = OpenAiClaudeIrMapper;
    let cg = ClaudeGeminiIrMapper;
    let go = OpenAiGeminiIrMapper;
    let orig = with_system();
    let claude = oc
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    assert_eq!(
        claude.messages[0].text_content(),
        "You are a helpful assistant."
    );
    let gemini = cg
        .map_request(Dialect::Claude, Dialect::Gemini, &claude)
        .unwrap();
    assert_eq!(
        gemini.messages[0].text_content(),
        "You are a helpful assistant."
    );
    let back = go
        .map_request(Dialect::Gemini, Dialect::OpenAi, &gemini)
        .unwrap();
    assert_eq!(
        back.messages[0].text_content(),
        "You are a helpful assistant."
    );
}

#[test]
fn lossy_image_data_never_corrupted_three_way() {
    let oc = OpenAiClaudeIrMapper;
    let cg = ClaudeGeminiIrMapper;
    let go = OpenAiGeminiIrMapper;
    let orig = image_conv();
    let claude = oc
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let gemini = cg
        .map_request(Dialect::Claude, Dialect::Gemini, &claude)
        .unwrap();
    let back = go
        .map_request(Dialect::Gemini, Dialect::OpenAi, &gemini)
        .unwrap();
    if let IrContentBlock::Image { data, media_type } = &back.messages[0].content[1] {
        assert_eq!(data, "iVBORw0KGgo=");
        assert_eq!(media_type, "image/png");
    } else {
        panic!("expected Image block in final roundtrip");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Error propagation — unsupported pairs (5 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn error_unsupported_pair_codex_to_gemini() {
    // No direct Codex↔Gemini mapper
    let result = default_ir_mapper(Dialect::Codex, Dialect::Gemini);
    assert!(result.is_none(), "Codex→Gemini should not have a mapper");
}

#[test]
fn error_unsupported_pair_codex_to_kimi() {
    let result = default_ir_mapper(Dialect::Codex, Dialect::Kimi);
    assert!(result.is_none(), "Codex→Kimi should not have a mapper");
}

#[test]
fn error_unsupported_pair_copilot_to_claude() {
    let result = default_ir_mapper(Dialect::Copilot, Dialect::Claude);
    assert!(
        result.is_none(),
        "Copilot→Claude should not have a direct mapper"
    );
}

#[test]
fn error_wrong_pair_on_specific_mapper() {
    let m = ClaudeGeminiIrMapper;
    let conv = simple_text();
    let err = m
        .map_request(Dialect::OpenAi, Dialect::Kimi, &conv)
        .unwrap_err();
    assert!(
        matches!(err, MapError::UnsupportedPair { .. }),
        "expected UnsupportedPair, got {err:?}"
    );
}

#[test]
fn error_map_error_serde_roundtrip() {
    let err = MapError::UnsupportedPair {
        from: Dialect::Codex,
        to: Dialect::Gemini,
    };
    let json = serde_json::to_string(&err).unwrap();
    let back: MapError = serde_json::from_str(&json).unwrap();
    assert_eq!(err, back);
}

// ═══════════════════════════════════════════════════════════════════════
// 14. Factory coverage — all supported pairs have mappers (3 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn factory_all_supported_pairs_resolve() {
    for (from, to) in supported_ir_pairs() {
        assert!(
            default_ir_mapper(from, to).is_some(),
            "no mapper for {from} → {to}"
        );
    }
}

#[test]
fn factory_identity_pairs_for_all_dialects() {
    for &d in Dialect::all() {
        let mapper = default_ir_mapper(d, d);
        assert!(mapper.is_some(), "no identity mapper for {d}");
    }
}

#[test]
fn factory_supported_pairs_count() {
    let pairs = supported_ir_pairs();
    // 6 identity + 18 cross-dialect = 24 total
    assert!(
        pairs.len() >= 24,
        "expected at least 24 supported pairs, got {}",
        pairs.len()
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 15. Three-way roundtrip tests (5 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn three_way_oai_claude_gemini_text() {
    let oc = OpenAiClaudeIrMapper;
    let cg = ClaudeGeminiIrMapper;
    let orig = simple_text();
    let claude = oc
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let gemini = cg
        .map_request(Dialect::Claude, Dialect::Gemini, &claude)
        .unwrap();
    assert_eq!(orig.len(), gemini.len());
    assert_text_preserved(&orig, &gemini);
}

#[test]
fn three_way_oai_claude_gemini_tools() {
    let oc = OpenAiClaudeIrMapper;
    let cg = ClaudeGeminiIrMapper;
    let orig = with_tools();
    let claude = oc
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let gemini = cg
        .map_request(Dialect::Claude, Dialect::Gemini, &claude)
        .unwrap();
    assert_eq!(count_tool_uses(&orig), count_tool_uses(&gemini));
}

#[test]
fn three_way_full_circle_oai_claude_gemini_oai() {
    let oc = OpenAiClaudeIrMapper;
    let cg = ClaudeGeminiIrMapper;
    let go = OpenAiGeminiIrMapper;
    let orig = multi_turn();
    let claude = oc
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let gemini = cg
        .map_request(Dialect::Claude, Dialect::Gemini, &claude)
        .unwrap();
    let back = go
        .map_request(Dialect::Gemini, Dialect::OpenAi, &gemini)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

#[test]
fn three_way_kimi_openai_claude_text() {
    let km = OpenAiKimiIrMapper;
    let oc = OpenAiClaudeIrMapper;
    let orig = simple_text();
    let openai = km
        .map_request(Dialect::Kimi, Dialect::OpenAi, &orig)
        .unwrap();
    let claude = oc
        .map_request(Dialect::OpenAi, Dialect::Claude, &openai)
        .unwrap();
    assert_eq!(orig.len(), claude.len());
    assert_text_preserved(&orig, &claude);
}

#[test]
fn three_way_copilot_openai_gemini_text() {
    let cm = OpenAiCopilotIrMapper;
    let og = OpenAiGeminiIrMapper;
    let orig = with_system();
    let openai = cm
        .map_request(Dialect::Copilot, Dialect::OpenAi, &orig)
        .unwrap();
    let gemini = og
        .map_request(Dialect::OpenAi, Dialect::Gemini, &openai)
        .unwrap();
    assert_eq!(orig.len(), gemini.len());
    assert_text_preserved(&orig, &gemini);
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Parameters — generation config survives (via IR metadata) (3 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn parameters_metadata_temperature_roundtrip() {
    let m = OpenAiClaudeIrMapper;
    let mut conv = simple_text();
    conv.messages[0]
        .metadata
        .insert("temperature".into(), json!(0.7));
    conv.messages[0]
        .metadata
        .insert("max_tokens".into(), json!(1024));
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    let back = m
        .map_request(Dialect::Claude, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(
        back.messages[0].metadata.get("temperature"),
        Some(&json!(0.7))
    );
    assert_eq!(
        back.messages[0].metadata.get("max_tokens"),
        Some(&json!(1024))
    );
}

#[test]
fn parameters_metadata_survives_gemini_roundtrip() {
    let m = OpenAiGeminiIrMapper;
    let mut conv = simple_text();
    conv.messages[0].metadata.insert("top_p".into(), json!(0.9));
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(back.messages[0].metadata.get("top_p"), Some(&json!(0.9)));
}

#[test]
fn parameters_metadata_survives_kimi_roundtrip() {
    let m = OpenAiKimiIrMapper;
    let mut conv = simple_text();
    conv.messages[0]
        .metadata
        .insert("stop".into(), json!(["END"]));
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Kimi, &conv)
        .unwrap();
    let back = m.map_request(Dialect::Kimi, Dialect::OpenAi, &mid).unwrap();
    assert_eq!(back.messages[0].metadata.get("stop"), Some(&json!(["END"])));
}

// ═══════════════════════════════════════════════════════════════════════
// 17. Unsupported features produce clear errors (3 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn unsupported_codex_copilot_no_mapper() {
    assert!(
        default_ir_mapper(Dialect::Codex, Dialect::Copilot).is_none(),
        "Codex→Copilot should have no mapper"
    );
}

#[test]
fn unsupported_kimi_copilot_no_mapper() {
    assert!(
        default_ir_mapper(Dialect::Kimi, Dialect::Copilot).is_none(),
        "Kimi→Copilot should have no mapper"
    );
}

#[test]
fn unsupported_copilot_gemini_no_mapper() {
    assert!(
        default_ir_mapper(Dialect::Copilot, Dialect::Gemini).is_none(),
        "Copilot→Gemini should have no mapper"
    );
}
