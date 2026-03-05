// SPDX-License-Identifier: MIT OR Apache-2.0

//! Comprehensive roundtrip tests for all IR mapper combinations.
//!
//! Verifies that data survives cross-dialect mapping without corruption,
//! and explicitly documents what information is lost in lossy conversions.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;
use serde_json::json;

use crate::factory::default_ir_mapper;
use crate::ir_claude_gemini::ClaudeGeminiIrMapper;
use crate::ir_mapper::IrMapper;
use crate::ir_openai_claude::OpenAiClaudeIrMapper;
use crate::ir_openai_gemini::OpenAiGeminiIrMapper;

// ── Helpers ─────────────────────────────────────────────────────────────

fn simple_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are helpful."),
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi!"),
    ])
}

fn tool_call_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Weather?"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Checking.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "get_weather".into(),
                    input: json!({"city": "SF"}),
                },
            ],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![IrContentBlock::Text {
                    text: "68°F".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(IrRole::Assistant, "It's 68°F in SF."),
    ])
}

fn thinking_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Hard problem"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "Step 1: consider…".into(),
                },
                IrContentBlock::Text {
                    text: "Answer: 42".into(),
                },
            ],
        ),
    ])
}

fn multi_tool_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Do two things"),
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
                        text: "found".into(),
                    }],
                    is_error: false,
                },
                IrContentBlock::ToolResult {
                    tool_use_id: "b".into(),
                    content: vec![IrContentBlock::Text {
                        text: "contents".into(),
                    }],
                    is_error: false,
                },
            ],
        ),
    ])
}

fn image_conv() -> IrConversation {
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

fn error_tool_result_conv() -> IrConversation {
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
                    text: "error: not found".into(),
                }],
                is_error: true,
            }],
        ),
    ])
}

fn multi_turn_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be concise."),
        IrMessage::text(IrRole::User, "Hi"),
        IrMessage::text(IrRole::Assistant, "Hello!"),
        IrMessage::text(IrRole::User, "How are you?"),
        IrMessage::text(IrRole::Assistant, "Good, thanks!"),
        IrMessage::text(IrRole::User, "Bye"),
        IrMessage::text(IrRole::Assistant, "Goodbye!"),
    ])
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

/// Helper: assert text content survives roundtrip.
fn assert_text_preserved(orig: &IrConversation, result: &IrConversation) {
    for (o, r) in orig.messages.iter().zip(result.messages.iter()) {
        assert_eq!(
            o.text_content(),
            r.text_content(),
            "text mismatch: role={:?}",
            o.role
        );
    }
}

/// Helper: count tool-use blocks.
fn count_tool_uses(conv: &IrConversation) -> usize {
    conv.tool_calls().len()
}

/// Helper: check no thinking blocks remain.
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

// ═══════════════════════════════════════════════════════════════════════
// 1. OpenAI → Claude → OpenAI roundtrip (10 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn rt_oai_claude_oai_simple_text() {
    let m = OpenAiClaudeIrMapper;
    let orig = simple_conv();
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
fn rt_oai_claude_oai_roles_preserved() {
    let m = OpenAiClaudeIrMapper;
    let orig = simple_conv();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Claude, Dialect::OpenAi, &mid)
        .unwrap();
    for (o, b) in orig.messages.iter().zip(back.messages.iter()) {
        assert_eq!(o.role, b.role);
    }
}

#[test]
fn rt_oai_claude_oai_tool_call_names() {
    let m = OpenAiClaudeIrMapper;
    let orig = tool_call_conv();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Claude, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(count_tool_uses(&orig), count_tool_uses(&back));
}

#[test]
fn rt_oai_claude_oai_tool_call_input() {
    let m = OpenAiClaudeIrMapper;
    let orig = tool_call_conv();
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
fn rt_oai_claude_oai_multi_turn() {
    let m = OpenAiClaudeIrMapper;
    let orig = multi_turn_conv();
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
fn rt_oai_claude_oai_empty() {
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
fn rt_oai_claude_oai_image_preserved() {
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

#[test]
fn rt_oai_claude_oai_error_tool_result() {
    let m = OpenAiClaudeIrMapper;
    let orig = error_tool_result_conv();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Claude, Dialect::OpenAi, &mid)
        .unwrap();
    // is_error flag should survive
    let tr = back
        .messages
        .iter()
        .flat_map(|m| &m.content)
        .find(|b| matches!(b, IrContentBlock::ToolResult { is_error, .. } if *is_error));
    assert!(tr.is_some(), "is_error flag lost in roundtrip");
}

#[test]
fn rt_oai_claude_oai_metadata() {
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
fn rt_oai_claude_oai_response_path() {
    let m = OpenAiClaudeIrMapper;
    let orig = simple_conv();
    let mid = m
        .map_response(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let back = m
        .map_response(Dialect::Claude, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

// ═══════════════════════════════════════════════════════════════════════
// 2. OpenAI → Gemini → OpenAI roundtrip (10 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn rt_oai_gemini_oai_simple_text() {
    let m = OpenAiGeminiIrMapper;
    let orig = simple_conv();
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
fn rt_oai_gemini_oai_roles_preserved() {
    let m = OpenAiGeminiIrMapper;
    let orig = simple_conv();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::OpenAi, &mid)
        .unwrap();
    for (o, b) in orig.messages.iter().zip(back.messages.iter()) {
        assert_eq!(o.role, b.role);
    }
}

#[test]
fn rt_oai_gemini_oai_tool_call_count() {
    let m = OpenAiGeminiIrMapper;
    let orig = tool_call_conv();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(count_tool_uses(&orig), count_tool_uses(&back));
}

#[test]
fn rt_oai_gemini_oai_function_input() {
    let m = OpenAiGeminiIrMapper;
    let orig = tool_call_conv();
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
fn rt_oai_gemini_oai_multi_turn() {
    let m = OpenAiGeminiIrMapper;
    let orig = multi_turn_conv();
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
fn rt_oai_gemini_oai_empty() {
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
fn rt_oai_gemini_oai_image_preserved() {
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

#[test]
fn rt_oai_gemini_oai_error_tool_result() {
    let m = OpenAiGeminiIrMapper;
    let orig = error_tool_result_conv();
    let mid = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::OpenAi, &mid)
        .unwrap();
    let tr = back
        .messages
        .iter()
        .flat_map(|m| &m.content)
        .find(|b| matches!(b, IrContentBlock::ToolResult { is_error, .. } if *is_error));
    assert!(tr.is_some(), "is_error flag lost in roundtrip");
}

#[test]
fn rt_oai_gemini_oai_metadata() {
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
fn rt_oai_gemini_oai_response_path() {
    let m = OpenAiGeminiIrMapper;
    let orig = simple_conv();
    let mid = m
        .map_response(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_response(Dialect::Gemini, Dialect::OpenAi, &mid)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Claude → Gemini → Claude roundtrip (10 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn rt_claude_gemini_claude_simple_text() {
    let m = ClaudeGeminiIrMapper;
    let orig = simple_conv();
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
fn rt_claude_gemini_claude_roles_preserved() {
    let m = ClaudeGeminiIrMapper;
    let orig = simple_conv();
    let mid = m
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::Claude, &mid)
        .unwrap();
    for (o, b) in orig.messages.iter().zip(back.messages.iter()) {
        assert_eq!(o.role, b.role);
    }
}

#[test]
fn rt_claude_gemini_claude_tool_use_count() {
    let m = ClaudeGeminiIrMapper;
    let orig = tool_call_conv();
    let mid = m
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::Claude, &mid)
        .unwrap();
    assert_eq!(count_tool_uses(&orig), count_tool_uses(&back));
}

#[test]
fn rt_claude_gemini_claude_tool_use_input() {
    let m = ClaudeGeminiIrMapper;
    let orig = tool_call_conv();
    let mid = m
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::Claude, &mid)
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
fn rt_claude_gemini_claude_multi_turn() {
    let m = ClaudeGeminiIrMapper;
    let orig = multi_turn_conv();
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
fn rt_claude_gemini_claude_empty() {
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
fn rt_claude_gemini_claude_image_preserved() {
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

#[test]
fn rt_claude_gemini_claude_error_tool_result() {
    let m = ClaudeGeminiIrMapper;
    let orig = error_tool_result_conv();
    let mid = m
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::Claude, &mid)
        .unwrap();
    let tr = back
        .messages
        .iter()
        .flat_map(|m| &m.content)
        .find(|b| matches!(b, IrContentBlock::ToolResult { is_error, .. } if *is_error));
    assert!(tr.is_some(), "is_error flag lost in roundtrip");
}

#[test]
fn rt_claude_gemini_claude_metadata() {
    let m = ClaudeGeminiIrMapper;
    let orig = metadata_conv();
    let mid = m
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::Claude, &mid)
        .unwrap();
    assert_eq!(
        back.messages[0].metadata.get("source"),
        Some(&json!("test"))
    );
}

#[test]
fn rt_claude_gemini_claude_response_path() {
    let m = ClaudeGeminiIrMapper;
    let orig = simple_conv();
    let mid = m
        .map_response(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_response(Dialect::Gemini, Dialect::Claude, &mid)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Three-way roundtrip tests (10 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn rt_three_way_oai_claude_gemini_text() {
    let oc = OpenAiClaudeIrMapper;
    let cg = ClaudeGeminiIrMapper;
    let orig = simple_conv();
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
fn rt_three_way_oai_claude_gemini_tools() {
    let oc = OpenAiClaudeIrMapper;
    let cg = ClaudeGeminiIrMapper;
    let orig = tool_call_conv();
    let claude = oc
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let gemini = cg
        .map_request(Dialect::Claude, Dialect::Gemini, &claude)
        .unwrap();
    assert_eq!(count_tool_uses(&orig), count_tool_uses(&gemini));
}

#[test]
fn rt_three_way_claude_gemini_oai_text() {
    let cg = ClaudeGeminiIrMapper;
    let go = OpenAiGeminiIrMapper;
    let orig = simple_conv();
    let gemini = cg
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let openai = go
        .map_request(Dialect::Gemini, Dialect::OpenAi, &gemini)
        .unwrap();
    assert_eq!(orig.len(), openai.len());
    assert_text_preserved(&orig, &openai);
}

#[test]
fn rt_three_way_gemini_oai_claude_text() {
    let go = OpenAiGeminiIrMapper;
    let oc = OpenAiClaudeIrMapper;
    let orig = simple_conv();
    let openai = go
        .map_request(Dialect::Gemini, Dialect::OpenAi, &orig)
        .unwrap();
    let claude = oc
        .map_request(Dialect::OpenAi, Dialect::Claude, &openai)
        .unwrap();
    assert_eq!(orig.len(), claude.len());
    assert_text_preserved(&orig, &claude);
}

#[test]
fn rt_three_way_full_circle_oai_claude_gemini_oai() {
    let oc = OpenAiClaudeIrMapper;
    let cg = ClaudeGeminiIrMapper;
    let go = OpenAiGeminiIrMapper;
    let orig = simple_conv();
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
fn rt_three_way_full_circle_tools() {
    let oc = OpenAiClaudeIrMapper;
    let cg = ClaudeGeminiIrMapper;
    let go = OpenAiGeminiIrMapper;
    let orig = tool_call_conv();
    let claude = oc
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let gemini = cg
        .map_request(Dialect::Claude, Dialect::Gemini, &claude)
        .unwrap();
    let back = go
        .map_request(Dialect::Gemini, Dialect::OpenAi, &gemini)
        .unwrap();
    assert_eq!(count_tool_uses(&orig), count_tool_uses(&back));
}

#[test]
fn rt_three_way_reverse_circle_gemini_claude_oai_gemini() {
    let cg = ClaudeGeminiIrMapper;
    let oc = OpenAiClaudeIrMapper;
    let go = OpenAiGeminiIrMapper;
    let orig = simple_conv();
    let claude = cg
        .map_request(Dialect::Gemini, Dialect::Claude, &orig)
        .unwrap();
    let openai = oc
        .map_request(Dialect::Claude, Dialect::OpenAi, &claude)
        .unwrap();
    let back = go
        .map_request(Dialect::OpenAi, Dialect::Gemini, &openai)
        .unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

#[test]
fn rt_three_way_image_survives() {
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
    assert!(back.messages[0].content.iter().any(
        |b| matches!(b, IrContentBlock::Image { media_type, .. } if media_type == "image/png")
    ));
}

#[test]
fn rt_three_way_factory_based() {
    let orig = simple_conv();
    let m1 = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let m2 = default_ir_mapper(Dialect::Claude, Dialect::Gemini).unwrap();
    let m3 = default_ir_mapper(Dialect::Gemini, Dialect::OpenAi).unwrap();
    let step1 = m1
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let step2 = m2
        .map_request(Dialect::Claude, Dialect::Gemini, &step1)
        .unwrap();
    let step3 = m3
        .map_request(Dialect::Gemini, Dialect::OpenAi, &step2)
        .unwrap();
    assert_eq!(orig.len(), step3.len());
    assert_text_preserved(&orig, &step3);
}

#[test]
fn rt_three_way_multi_turn_survives() {
    let oc = OpenAiClaudeIrMapper;
    let cg = ClaudeGeminiIrMapper;
    let go = OpenAiGeminiIrMapper;
    let orig = multi_turn_conv();
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

// ═══════════════════════════════════════════════════════════════════════
// 5. Lossy roundtrip verification (10 tests)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn lossy_thinking_dropped_claude_to_gemini() {
    let m = ClaudeGeminiIrMapper;
    let orig = thinking_conv();
    let gemini = m
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    // Thinking block should be gone
    assert_no_thinking(&gemini);
    // Text content should survive
    assert_eq!(gemini.messages[1].text_content(), "Answer: 42");
}

#[test]
fn lossy_thinking_not_recovered_gemini_to_claude() {
    let m = ClaudeGeminiIrMapper;
    let orig = thinking_conv();
    let gemini = m
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::Claude, &gemini)
        .unwrap();
    // Thinking block cannot be recovered
    assert_no_thinking(&back);
    // But text still correct
    assert_eq!(back.messages[1].text_content(), "Answer: 42");
}

#[test]
fn lossy_thinking_dropped_claude_openai_roundtrip() {
    let m = OpenAiClaudeIrMapper;
    let orig = thinking_conv();
    let openai = m
        .map_request(Dialect::Claude, Dialect::OpenAi, &orig)
        .unwrap();
    assert_no_thinking(&openai);
    let back = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &openai)
        .unwrap();
    assert_no_thinking(&back);
    assert_eq!(back.messages[1].text_content(), "Answer: 42");
}

#[test]
fn lossy_thinking_dropped_three_way() {
    let oc = OpenAiClaudeIrMapper;
    let cg = ClaudeGeminiIrMapper;
    let orig = thinking_conv();
    let gemini = cg
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    assert_no_thinking(&gemini);
    let go = OpenAiGeminiIrMapper;
    let openai = go
        .map_request(Dialect::Gemini, Dialect::OpenAi, &gemini)
        .unwrap();
    assert_no_thinking(&openai);
    let back = oc
        .map_request(Dialect::OpenAi, Dialect::Claude, &openai)
        .unwrap();
    assert_no_thinking(&back);
    assert_eq!(back.messages[1].text_content(), "Answer: 42");
}

#[test]
fn lossy_tool_role_changes_but_data_preserved_oai_claude() {
    let m = OpenAiClaudeIrMapper;
    let orig = tool_call_conv();
    let claude = m
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    // Tool-role → User-role in Claude
    let tool_result_msgs: Vec<_> = claude
        .messages
        .iter()
        .filter(|msg| {
            msg.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
        })
        .collect();
    assert!(!tool_result_msgs.is_empty());
    for msg in &tool_result_msgs {
        assert_eq!(msg.role, IrRole::User);
    }
    // Data inside ToolResult is intact
    if let IrContentBlock::ToolResult { content, .. } = &tool_result_msgs[0].content[0] {
        assert_eq!(content.len(), 1);
        if let IrContentBlock::Text { text } = &content[0] {
            assert_eq!(text, "68°F");
        }
    }
}

#[test]
fn lossy_tool_role_changes_but_data_preserved_oai_gemini() {
    let m = OpenAiGeminiIrMapper;
    let orig = tool_call_conv();
    let gemini = m
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    // Tool-role → User-role in Gemini
    let tool_result_msgs: Vec<_> = gemini
        .messages
        .iter()
        .filter(|msg| {
            msg.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
        })
        .collect();
    assert!(!tool_result_msgs.is_empty());
    for msg in &tool_result_msgs {
        assert_eq!(msg.role, IrRole::User);
    }
}

#[test]
fn lossy_no_data_corruption_multi_tool() {
    let m = ClaudeGeminiIrMapper;
    let orig = multi_tool_conv();
    let gemini = m
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    let back = m
        .map_request(Dialect::Gemini, Dialect::Claude, &gemini)
        .unwrap();
    // Tool use names survive
    let orig_names: Vec<_> = orig
        .tool_calls()
        .iter()
        .filter_map(|b| {
            if let IrContentBlock::ToolUse { name, .. } = b {
                Some(name.clone())
            } else {
                None
            }
        })
        .collect();
    let back_names: Vec<_> = back
        .tool_calls()
        .iter()
        .filter_map(|b| {
            if let IrContentBlock::ToolUse { name, .. } = b {
                Some(name.clone())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(orig_names, back_names);
}

#[test]
fn lossy_image_data_never_corrupted() {
    let oc = OpenAiClaudeIrMapper;
    let cg = ClaudeGeminiIrMapper;
    let go = OpenAiGeminiIrMapper;
    let orig = image_conv();
    // Full circle: OpenAI → Claude → Gemini → OpenAI
    let claude = oc
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let gemini = cg
        .map_request(Dialect::Claude, Dialect::Gemini, &claude)
        .unwrap();
    let back = go
        .map_request(Dialect::Gemini, Dialect::OpenAi, &gemini)
        .unwrap();
    // Verify exact data
    if let IrContentBlock::Image { data, media_type } = &back.messages[0].content[1] {
        assert_eq!(data, "iVBORw0KGgo=");
        assert_eq!(media_type, "image/png");
    } else {
        panic!("expected Image block");
    }
}

#[test]
fn lossy_system_prompt_preserved_all_paths() {
    let oc = OpenAiClaudeIrMapper;
    let cg = ClaudeGeminiIrMapper;
    let go = OpenAiGeminiIrMapper;
    let orig = simple_conv();
    // OpenAI → Claude
    let r1 = oc
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    assert_eq!(r1.messages[0].text_content(), "You are helpful.");
    // Claude → Gemini
    let r2 = cg
        .map_request(Dialect::Claude, Dialect::Gemini, &r1)
        .unwrap();
    assert_eq!(r2.messages[0].text_content(), "You are helpful.");
    // Gemini → OpenAI
    let r3 = go
        .map_request(Dialect::Gemini, Dialect::OpenAi, &r2)
        .unwrap();
    assert_eq!(r3.messages[0].text_content(), "You are helpful.");
    // All system roles preserved
    assert_eq!(r1.messages[0].role, IrRole::System);
    assert_eq!(r2.messages[0].role, IrRole::System);
    assert_eq!(r3.messages[0].role, IrRole::System);
}

#[test]
fn lossy_error_flag_survives_all_paths() {
    let oc = OpenAiClaudeIrMapper;
    let cg = ClaudeGeminiIrMapper;
    let go = OpenAiGeminiIrMapper;
    let orig = error_tool_result_conv();
    // OpenAI → Claude → Gemini → OpenAI
    let claude = oc
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    let gemini = cg
        .map_request(Dialect::Claude, Dialect::Gemini, &claude)
        .unwrap();
    let back = go
        .map_request(Dialect::Gemini, Dialect::OpenAi, &gemini)
        .unwrap();
    let has_error = back
        .messages
        .iter()
        .flat_map(|m| &m.content)
        .any(|b| matches!(b, IrContentBlock::ToolResult { is_error, .. } if *is_error));
    assert!(has_error, "is_error flag lost in three-way roundtrip");
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Claude↔Gemini mapper unit tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn claude_gemini_supported_pairs() {
    let m = ClaudeGeminiIrMapper;
    let pairs = m.supported_pairs();
    assert!(pairs.contains(&(Dialect::Claude, Dialect::Gemini)));
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::Claude)));
    assert_eq!(pairs.len(), 2);
}

#[test]
fn claude_gemini_unsupported_pair() {
    let m = ClaudeGeminiIrMapper;
    let conv = simple_conv();
    let err = m
        .map_request(Dialect::OpenAi, Dialect::Kimi, &conv)
        .unwrap_err();
    assert!(matches!(err, crate::MapError::UnsupportedPair { .. }));
}

#[test]
fn claude_gemini_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ClaudeGeminiIrMapper>();
}

#[test]
fn claude_gemini_factory_registered() {
    assert!(default_ir_mapper(Dialect::Claude, Dialect::Gemini).is_some());
    assert!(default_ir_mapper(Dialect::Gemini, Dialect::Claude).is_some());
}

#[test]
fn claude_gemini_factory_supported_pairs() {
    let pairs = crate::factory::supported_ir_pairs();
    assert!(pairs.contains(&(Dialect::Claude, Dialect::Gemini)));
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::Claude)));
}

#[test]
fn claude_to_gemini_tool_role_becomes_user() {
    let m = ClaudeGeminiIrMapper;
    let conv = tool_call_conv();
    let result = m
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap();
    // Tool-role msg should become User
    let tool_result_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|msg| {
            msg.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
        })
        .collect();
    for msg in tool_result_msgs {
        assert_eq!(msg.role, IrRole::User);
    }
}

#[test]
fn gemini_to_claude_tool_role_becomes_user() {
    let m = ClaudeGeminiIrMapper;
    // Simulate Gemini producing a Tool-role message (rare but possible in IR)
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "call tool"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "g1".into(),
                name: "fetch".into(),
                input: json!({"url": "https://example.com"}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "g1".into(),
                content: vec![IrContentBlock::Text { text: "OK".into() }],
                is_error: false,
            }],
        ),
    ]);
    let result = m
        .map_request(Dialect::Gemini, Dialect::Claude, &conv)
        .unwrap();
    // Tool-role → User in Claude
    assert_eq!(result.messages[2].role, IrRole::User);
}
