// SPDX-License-Identifier: MIT OR Apache-2.0
//! Cross-SDK fidelity verification tests.
//!
//! Validates mapping quality between dialect pairs (OpenAI, Claude, Gemini)
//! at the IR level. Each test documents exactly what is preserved, what is
//! lost, and what degrades across round-trips.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;
use abp_mapper::{
    ClaudeGeminiIrMapper, IrIdentityMapper, IrMapper, MapError, OpenAiClaudeIrMapper,
    OpenAiGeminiIrMapper, default_ir_mapper,
};
use serde_json::json;

// ── Helpers ─────────────────────────────────────────────────────────────

fn simple_text_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are a helpful assistant."),
        IrMessage::text(IrRole::User, "Hello!"),
        IrMessage::text(IrRole::Assistant, "Hi there, how can I help?"),
    ])
}

fn user_only_conv() -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "Just a question")])
}

fn multi_turn_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "Be concise."),
        IrMessage::text(IrRole::User, "First turn"),
        IrMessage::text(IrRole::Assistant, "Reply 1"),
        IrMessage::text(IrRole::User, "Second turn"),
        IrMessage::text(IrRole::Assistant, "Reply 2"),
        IrMessage::text(IrRole::User, "Third turn"),
        IrMessage::text(IrRole::Assistant, "Reply 3"),
    ])
}

fn tool_call_conv() -> IrConversation {
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

fn multi_tool_conv() -> IrConversation {
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

fn thinking_conv() -> IrConversation {
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

fn image_conv() -> IrConversation {
    IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "Describe this image.".into(),
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
                    text: "error: command not found".into(),
                }],
                is_error: true,
            }],
        ),
    ])
}

fn metadata_conv() -> IrConversation {
    let mut msg = IrMessage::text(IrRole::User, "with metadata");
    msg.metadata.insert("source".into(), json!("test"));
    msg.metadata
        .insert("timestamp".into(), json!(1_700_000_000));
    IrConversation::from_messages(vec![msg])
}

fn thinking_with_tool_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Complex task"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "I need to call a tool first.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "tc1".into(),
                    name: "analyze".into(),
                    input: json!({"data": [1, 2, 3]}),
                },
            ],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "tc1".into(),
                content: vec![IrContentBlock::Text {
                    text: "analysis done".into(),
                }],
                is_error: false,
            }],
        ),
    ])
}

/// Count Thinking blocks across a conversation.
fn count_thinking(conv: &IrConversation) -> usize {
    conv.messages
        .iter()
        .flat_map(|m| &m.content)
        .filter(|b| matches!(b, IrContentBlock::Thinking { .. }))
        .count()
}

/// Count ToolUse blocks across a conversation.
fn count_tool_uses(conv: &IrConversation) -> usize {
    conv.tool_calls().len()
}

/// Count ToolResult blocks across a conversation.
fn count_tool_results(conv: &IrConversation) -> usize {
    conv.messages
        .iter()
        .flat_map(|m| &m.content)
        .filter(|b| matches!(b, IrContentBlock::ToolResult { .. }))
        .count()
}

/// Concatenate all text across every message.
fn all_text(conv: &IrConversation) -> String {
    conv.messages
        .iter()
        .map(|m| m.text_content())
        .collect::<Vec<_>>()
        .join("|")
}

/// Map via factory, panicking if no mapper exists.
fn map_req(from: Dialect, to: Dialect, conv: &IrConversation) -> IrConversation {
    let mapper = default_ir_mapper(from, to).unwrap_or_else(|| {
        panic!("no mapper for {from:?} -> {to:?}");
    });
    mapper.map_request(from, to, conv).unwrap()
}

/// Supported cross-dialect pairs (non-identity).
const CROSS_PAIRS: [(Dialect, Dialect); 6] = [
    (Dialect::OpenAi, Dialect::Claude),
    (Dialect::Claude, Dialect::OpenAi),
    (Dialect::OpenAi, Dialect::Gemini),
    (Dialect::Gemini, Dialect::OpenAi),
    (Dialect::Claude, Dialect::Gemini),
    (Dialect::Gemini, Dialect::Claude),
];

// ═════════════════════════════════════════════════════════════════════════
// 1. TEXT MESSAGE FIDELITY (15 tests)
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn text_fidelity_openai_to_claude_preserves_all_text() {
    let result = map_req(Dialect::OpenAi, Dialect::Claude, &simple_text_conv());
    assert_eq!(
        result.messages[0].text_content(),
        "You are a helpful assistant."
    );
    assert_eq!(result.messages[1].text_content(), "Hello!");
    assert_eq!(
        result.messages[2].text_content(),
        "Hi there, how can I help?"
    );
}

#[test]
fn text_fidelity_claude_to_openai_preserves_all_text() {
    let result = map_req(Dialect::Claude, Dialect::OpenAi, &simple_text_conv());
    assert_eq!(
        result.messages[0].text_content(),
        "You are a helpful assistant."
    );
    assert_eq!(result.messages[1].text_content(), "Hello!");
    assert_eq!(
        result.messages[2].text_content(),
        "Hi there, how can I help?"
    );
}

#[test]
fn text_fidelity_openai_to_gemini_preserves_all_text() {
    let result = map_req(Dialect::OpenAi, Dialect::Gemini, &simple_text_conv());
    assert_eq!(
        result.messages[0].text_content(),
        "You are a helpful assistant."
    );
    assert_eq!(result.messages[1].text_content(), "Hello!");
    assert_eq!(
        result.messages[2].text_content(),
        "Hi there, how can I help?"
    );
}

#[test]
fn text_fidelity_gemini_to_openai_preserves_all_text() {
    let result = map_req(Dialect::Gemini, Dialect::OpenAi, &simple_text_conv());
    assert_eq!(
        result.messages[0].text_content(),
        "You are a helpful assistant."
    );
    assert_eq!(result.messages[1].text_content(), "Hello!");
    assert_eq!(
        result.messages[2].text_content(),
        "Hi there, how can I help?"
    );
}

#[test]
fn text_fidelity_claude_to_gemini_preserves_all_text() {
    let result = map_req(Dialect::Claude, Dialect::Gemini, &simple_text_conv());
    assert_eq!(
        result.messages[0].text_content(),
        "You are a helpful assistant."
    );
    assert_eq!(result.messages[1].text_content(), "Hello!");
    assert_eq!(
        result.messages[2].text_content(),
        "Hi there, how can I help?"
    );
}

#[test]
fn text_fidelity_gemini_to_claude_preserves_all_text() {
    let result = map_req(Dialect::Gemini, Dialect::Claude, &simple_text_conv());
    assert_eq!(
        result.messages[0].text_content(),
        "You are a helpful assistant."
    );
    assert_eq!(result.messages[1].text_content(), "Hello!");
    assert_eq!(
        result.messages[2].text_content(),
        "Hi there, how can I help?"
    );
}

#[test]
fn text_fidelity_system_role_preserved_all_pairs() {
    let conv = simple_text_conv();
    for (from, to) in &CROSS_PAIRS {
        let result = map_req(*from, *to, &conv);
        assert_eq!(
            result.messages[0].text_content(),
            "You are a helpful assistant.",
            "system text lost for {from:?} -> {to:?}"
        );
    }
}

#[test]
fn text_fidelity_user_role_preserved_all_cross_pairs() {
    let conv = user_only_conv();
    for (from, to) in &CROSS_PAIRS {
        let result = map_req(*from, *to, &conv);
        assert_eq!(result.messages[0].role, IrRole::User);
        assert_eq!(
            result.messages[0].text_content(),
            "Just a question",
            "user text lost for {from:?} -> {to:?}"
        );
    }
}

#[test]
fn text_fidelity_multi_turn_message_count_preserved_across_pairs() {
    let conv = multi_turn_conv();
    for (from, to) in &CROSS_PAIRS {
        let result = map_req(*from, *to, &conv);
        assert_eq!(
            result.len(),
            conv.len(),
            "message count changed for {from:?} -> {to:?}"
        );
    }
}

#[test]
fn text_fidelity_multi_turn_text_ordering_preserved() {
    let conv = multi_turn_conv();
    let expected_texts: Vec<String> = conv.messages.iter().map(|m| m.text_content()).collect();
    for (from, to) in &CROSS_PAIRS {
        let result = map_req(*from, *to, &conv);
        let result_texts: Vec<String> = result.messages.iter().map(|m| m.text_content()).collect();
        assert_eq!(
            expected_texts, result_texts,
            "text ordering broken for {from:?} -> {to:?}"
        );
    }
}

#[test]
fn text_fidelity_empty_conversation_stays_empty() {
    let conv = IrConversation::new();
    for (from, to) in &CROSS_PAIRS {
        let result = map_req(*from, *to, &conv);
        assert!(
            result.is_empty(),
            "empty conv not empty for {from:?} -> {to:?}"
        );
    }
}

#[test]
fn text_fidelity_unicode_preserved() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "こんにちは 🌍 émojis «quotes»"),
        IrMessage::text(IrRole::Assistant, "Ответ: ∑∏∫ — ñ ü ö"),
    ]);
    for (from, to) in &CROSS_PAIRS {
        let result = map_req(*from, *to, &conv);
        assert_eq!(
            result.messages[0].text_content(),
            "こんにちは 🌍 émojis «quotes»"
        );
        assert_eq!(result.messages[1].text_content(), "Ответ: ∑∏∫ — ñ ü ö");
    }
}

#[test]
fn text_fidelity_empty_string_text_preserved() {
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "")]);
    for (from, to) in &CROSS_PAIRS {
        let result = map_req(*from, *to, &conv);
        assert_eq!(result.messages[0].text_content(), "");
    }
}

#[test]
fn text_fidelity_very_long_text_preserved() {
    let long_text = "a".repeat(100_000);
    let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, &long_text)]);
    for (from, to) in &CROSS_PAIRS {
        let result = map_req(*from, *to, &conv);
        assert_eq!(result.messages[0].text_content().len(), 100_000);
    }
}

#[test]
fn text_fidelity_identity_mapper_is_exact_clone() {
    let mapper = IrIdentityMapper;
    let conv = simple_text_conv();
    for &d in Dialect::all() {
        let result = mapper.map_request(d, d, &conv).unwrap();
        assert_eq!(conv, result, "identity not exact for {d:?}");
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 2. TOOL CALL FIDELITY (15 tests)
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn tool_fidelity_openai_to_claude_tool_use_name_preserved() {
    let result = map_req(Dialect::OpenAi, Dialect::Claude, &tool_call_conv());
    let tools = result.tool_calls();
    assert_eq!(tools.len(), 1);
    if let IrContentBlock::ToolUse { name, .. } = tools[0] {
        assert_eq!(name, "get_weather");
    } else {
        panic!("expected ToolUse");
    }
}

#[test]
fn tool_fidelity_openai_to_claude_tool_use_input_preserved() {
    let result = map_req(Dialect::OpenAi, Dialect::Claude, &tool_call_conv());
    let tools = result.tool_calls();
    if let IrContentBlock::ToolUse { input, .. } = tools[0] {
        assert_eq!(input, &json!({"city": "NYC"}));
    } else {
        panic!("expected ToolUse");
    }
}

#[test]
fn tool_fidelity_openai_to_claude_tool_use_id_preserved() {
    let result = map_req(Dialect::OpenAi, Dialect::Claude, &tool_call_conv());
    let tools = result.tool_calls();
    if let IrContentBlock::ToolUse { id, .. } = tools[0] {
        assert_eq!(id, "call_1");
    } else {
        panic!("expected ToolUse");
    }
}

#[test]
fn tool_fidelity_openai_to_claude_tool_result_role_becomes_user() {
    let result = map_req(Dialect::OpenAi, Dialect::Claude, &tool_call_conv());
    let tool_result_msg = &result.messages[2];
    assert_eq!(tool_result_msg.role, IrRole::User);
    assert!(matches!(
        &tool_result_msg.content[0],
        IrContentBlock::ToolResult { .. }
    ));
}

#[test]
fn tool_fidelity_openai_to_gemini_tool_result_role_becomes_user() {
    let result = map_req(Dialect::OpenAi, Dialect::Gemini, &tool_call_conv());
    let tool_result_msg = &result.messages[2];
    assert_eq!(tool_result_msg.role, IrRole::User);
}

#[test]
fn tool_fidelity_claude_to_openai_user_tool_results_become_tool_role() {
    let result = map_req(Dialect::Claude, Dialect::OpenAi, &multi_tool_conv());
    let tool_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert_eq!(tool_msgs.len(), 2, "expected two Tool-role messages");
}

#[test]
fn tool_fidelity_gemini_to_openai_user_tool_results_become_tool_role() {
    let result = map_req(Dialect::Gemini, Dialect::OpenAi, &multi_tool_conv());
    let tool_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert_eq!(tool_msgs.len(), 2);
}

#[test]
fn tool_fidelity_tool_use_count_preserved_all_pairs() {
    let conv = tool_call_conv();
    let orig_count = count_tool_uses(&conv);
    for (from, to) in &CROSS_PAIRS {
        let result = map_req(*from, *to, &conv);
        assert_eq!(
            count_tool_uses(&result),
            orig_count,
            "tool use count changed for {from:?} -> {to:?}"
        );
    }
}

#[test]
fn tool_fidelity_tool_result_count_preserved_all_pairs() {
    let conv = tool_call_conv();
    let orig_count = count_tool_results(&conv);
    for (from, to) in &CROSS_PAIRS {
        let result = map_req(*from, *to, &conv);
        assert_eq!(
            count_tool_results(&result),
            orig_count,
            "tool result count changed for {from:?} -> {to:?}"
        );
    }
}

#[test]
fn tool_fidelity_multi_tool_use_names_preserved() {
    let conv = multi_tool_conv();
    for (from, to) in &CROSS_PAIRS {
        let result = map_req(*from, *to, &conv);
        let names: Vec<&str> = result
            .tool_calls()
            .iter()
            .filter_map(|b| {
                if let IrContentBlock::ToolUse { name, .. } = b {
                    Some(name.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            names,
            vec!["search", "read_file"],
            "tool names differ for {from:?} -> {to:?}"
        );
    }
}

#[test]
fn tool_fidelity_error_flag_preserved_openai_to_claude() {
    let result = map_req(Dialect::OpenAi, Dialect::Claude, &error_tool_result_conv());
    let tr_blocks: Vec<_> = result
        .messages
        .iter()
        .flat_map(|m| &m.content)
        .filter(|b| matches!(b, IrContentBlock::ToolResult { .. }))
        .collect();
    assert_eq!(tr_blocks.len(), 1);
    if let IrContentBlock::ToolResult { is_error, .. } = tr_blocks[0] {
        assert!(is_error, "is_error flag lost");
    }
}

#[test]
fn tool_fidelity_error_flag_preserved_openai_to_gemini() {
    let result = map_req(Dialect::OpenAi, Dialect::Gemini, &error_tool_result_conv());
    let tr_blocks: Vec<_> = result
        .messages
        .iter()
        .flat_map(|m| &m.content)
        .filter(|b| matches!(b, IrContentBlock::ToolResult { .. }))
        .collect();
    if let IrContentBlock::ToolResult { is_error, .. } = tr_blocks[0] {
        assert!(is_error, "is_error flag lost in OpenAI -> Gemini");
    }
}

#[test]
fn tool_fidelity_error_flag_preserved_claude_to_gemini() {
    let result = map_req(Dialect::Claude, Dialect::Gemini, &error_tool_result_conv());
    let tr_blocks: Vec<_> = result
        .messages
        .iter()
        .flat_map(|m| &m.content)
        .filter(|b| matches!(b, IrContentBlock::ToolResult { .. }))
        .collect();
    if let IrContentBlock::ToolResult { is_error, .. } = tr_blocks[0] {
        assert!(is_error, "is_error flag lost in Claude -> Gemini");
    }
}

#[test]
fn tool_fidelity_tool_result_content_text_preserved() {
    let conv = tool_call_conv();
    for (from, to) in &CROSS_PAIRS {
        let result = map_req(*from, *to, &conv);
        let found = result.messages.iter().flat_map(|m| &m.content).any(|b| {
            if let IrContentBlock::ToolResult { content, .. } = b {
                content
                    .iter()
                    .any(|c| matches!(c, IrContentBlock::Text { text } if text == "72°F, sunny"))
            } else {
                false
            }
        });
        assert!(
            found,
            "tool result content text lost for {from:?} -> {to:?}"
        );
    }
}

#[test]
fn tool_fidelity_mixed_text_and_tool_use_in_assistant_preserved() {
    let conv = tool_call_conv();
    for (from, to) in &CROSS_PAIRS {
        let result = map_req(*from, *to, &conv);
        let asst_msg = result.messages.iter().find(|m| {
            m.role == IrRole::Assistant
                && m.content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::ToolUse { .. }))
        });
        assert!(
            asst_msg.is_some(),
            "assistant with tool_use vanished for {from:?} -> {to:?}"
        );
        let asst = asst_msg.unwrap();
        assert!(
            asst.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Text { text } if text == "Let me check.")),
            "text alongside tool_use lost for {from:?} -> {to:?}"
        );
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 3. STREAMING FIDELITY (10 tests)
//    Simulated by mapping incrementally growing conversations and
//    verifying order, content, and partial-state correctness.
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn streaming_fidelity_incremental_messages_preserve_order_openai_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let msgs = [
        IrMessage::text(IrRole::User, "msg1"),
        IrMessage::text(IrRole::Assistant, "msg2"),
        IrMessage::text(IrRole::User, "msg3"),
    ];
    for i in 1..=msgs.len() {
        let conv = IrConversation::from_messages(msgs[..i].to_vec());
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
            .unwrap();
        assert_eq!(result.len(), i);
        for (mapped, orig) in result.messages.iter().zip(msgs[..i].iter()) {
            assert_eq!(mapped.text_content(), orig.text_content());
        }
    }
}

#[test]
fn streaming_fidelity_incremental_messages_preserve_order_openai_gemini() {
    let mapper = OpenAiGeminiIrMapper;
    let msgs = [
        IrMessage::text(IrRole::User, "msg1"),
        IrMessage::text(IrRole::Assistant, "msg2"),
        IrMessage::text(IrRole::User, "msg3"),
    ];
    for i in 1..=msgs.len() {
        let conv = IrConversation::from_messages(msgs[..i].to_vec());
        let result = mapper
            .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
            .unwrap();
        assert_eq!(result.len(), i);
    }
}

#[test]
fn streaming_fidelity_incremental_messages_preserve_order_claude_gemini() {
    let mapper = ClaudeGeminiIrMapper;
    let msgs = [
        IrMessage::text(IrRole::User, "msg1"),
        IrMessage::text(IrRole::Assistant, "msg2"),
        IrMessage::text(IrRole::User, "msg3"),
    ];
    for i in 1..=msgs.len() {
        let conv = IrConversation::from_messages(msgs[..i].to_vec());
        let result = mapper
            .map_request(Dialect::Claude, Dialect::Gemini, &conv)
            .unwrap();
        assert_eq!(result.len(), i);
    }
}

#[test]
fn streaming_fidelity_partial_tool_flow_maps_consistently() {
    let mapper = OpenAiClaudeIrMapper;
    let step1 = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "question")]);
    let r1 = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &step1)
        .unwrap();
    assert_eq!(r1.len(), 1);

    let step2 = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "question"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "x".into(),
                name: "fn".into(),
                input: json!({}),
            }],
        ),
    ]);
    let r2 = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &step2)
        .unwrap();
    assert_eq!(r2.len(), 2);
    assert_eq!(count_tool_uses(&r2), 1);
}

#[test]
fn streaming_fidelity_response_mapping_preserves_order() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = simple_text_conv();
    let result = mapper
        .map_response(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    for (orig, mapped) in conv.messages.iter().zip(result.messages.iter()) {
        assert_eq!(orig.text_content(), mapped.text_content());
    }
}

#[test]
fn streaming_fidelity_multi_content_block_order_preserved() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "First".into(),
            },
            IrContentBlock::Text {
                text: "Second".into(),
            },
            IrContentBlock::Text {
                text: "Third".into(),
            },
        ],
    )]);
    for (from, to) in &CROSS_PAIRS {
        let result = map_req(*from, *to, &conv);
        let texts: Vec<&str> = result.messages[0]
            .content
            .iter()
            .filter_map(|b| match b {
                IrContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["First", "Second", "Third"]);
    }
}

#[test]
fn streaming_fidelity_tool_use_order_in_assistant_preserved() {
    let conv = multi_tool_conv();
    for (from, to) in &CROSS_PAIRS {
        let result = map_req(*from, *to, &conv);
        let tool_names: Vec<&str> = result
            .tool_calls()
            .iter()
            .filter_map(|b| {
                if let IrContentBlock::ToolUse { name, .. } = b {
                    Some(name.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(tool_names, vec!["search", "read_file"]);
    }
}

#[test]
fn streaming_fidelity_metadata_carried_through_mapping() {
    let conv = metadata_conv();
    for (from, to) in &CROSS_PAIRS {
        let result = map_req(*from, *to, &conv);
        assert_eq!(
            result.messages[0].metadata.get("source"),
            Some(&json!("test")),
            "metadata lost for {from:?} -> {to:?}"
        );
    }
}

#[test]
fn streaming_fidelity_image_blocks_preserved_all_pairs() {
    let conv = image_conv();
    for (from, to) in &CROSS_PAIRS {
        let result = map_req(*from, *to, &conv);
        let has_image = result.messages[0].content.iter().any(|b| {
            matches!(
                b,
                IrContentBlock::Image { media_type, .. } if media_type == "image/png"
            )
        });
        assert!(has_image, "image block lost for {from:?} -> {to:?}");
    }
}

#[test]
fn streaming_fidelity_request_and_response_produce_same_text() {
    let conv = simple_text_conv();
    for (from, to) in &CROSS_PAIRS {
        let mapper = default_ir_mapper(*from, *to).unwrap();
        let req_result = mapper.map_request(*from, *to, &conv).unwrap();
        let resp_result = mapper.map_response(*from, *to, &conv).unwrap();
        assert_eq!(
            all_text(&req_result),
            all_text(&resp_result),
            "request/response diverge for {from:?} -> {to:?}"
        );
    }
}

// ═════════════════════════════════════════════════════════════════════════
// 4. LOSSY MAPPING AWARENESS (15 tests)
//    Explicitly documents what information is lost in each direction.
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn lossy_thinking_dropped_claude_to_openai() {
    let result = map_req(Dialect::Claude, Dialect::OpenAi, &thinking_conv());
    assert_eq!(count_thinking(&result), 0, "thinking should be dropped");
    assert_eq!(result.messages[1].text_content(), "The answer is 42.");
}

#[test]
fn lossy_thinking_dropped_claude_to_gemini() {
    let result = map_req(Dialect::Claude, Dialect::Gemini, &thinking_conv());
    assert_eq!(count_thinking(&result), 0);
    assert_eq!(result.messages[1].text_content(), "The answer is 42.");
}

#[test]
fn lossy_thinking_dropped_openai_to_gemini() {
    let result = map_req(Dialect::OpenAi, Dialect::Gemini, &thinking_conv());
    assert_eq!(count_thinking(&result), 0);
}

#[test]
fn lossy_thinking_preserved_openai_to_claude() {
    // Claude supports thinking natively, so this direction is lossless.
    let result = map_req(Dialect::OpenAi, Dialect::Claude, &thinking_conv());
    assert_eq!(count_thinking(&result), 1);
}

#[test]
fn lossy_thinking_preserved_gemini_to_claude() {
    // Gemini→Claude mapper does not filter thinking.
    let result = map_req(Dialect::Gemini, Dialect::Claude, &thinking_conv());
    assert_eq!(count_thinking(&result), 1);
}

#[test]
fn lossy_tool_role_remapped_to_user_for_claude() {
    let conv = tool_call_conv();
    let result = map_req(Dialect::OpenAi, Dialect::Claude, &conv);
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
            "Claude should use User role for tool results"
        );
    }
}

#[test]
fn lossy_tool_role_remapped_to_user_for_gemini() {
    let conv = tool_call_conv();
    let result = map_req(Dialect::OpenAi, Dialect::Gemini, &conv);
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
            "Gemini should use User role for tool results"
        );
    }
}

#[test]
fn lossy_claude_user_tool_results_split_to_openai_tool_messages() {
    let conv = multi_tool_conv();
    let result = map_req(Dialect::Claude, Dialect::OpenAi, &conv);
    let user_with_tool_results = result
        .messages
        .iter()
        .filter(|m| {
            m.role == IrRole::User
                && m.content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
        })
        .count();
    assert_eq!(
        user_with_tool_results, 0,
        "tool results should not remain in User messages"
    );
    let tool_msgs = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .count();
    assert_eq!(tool_msgs, 2);
}

#[test]
fn lossy_gemini_user_tool_results_split_to_openai_tool_messages() {
    let conv = multi_tool_conv();
    let result = map_req(Dialect::Gemini, Dialect::OpenAi, &conv);
    let tool_msgs = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .count();
    assert_eq!(tool_msgs, 2);
}

#[test]
fn lossy_message_count_may_change_claude_to_openai_with_tool_results() {
    let conv = multi_tool_conv();
    let result = map_req(Dialect::Claude, Dialect::OpenAi, &conv);
    assert!(
        result.len() >= conv.len(),
        "message count should not decrease"
    );
}

#[test]
fn lossy_thinking_with_tool_use_drops_thinking_keeps_tool() {
    let result = map_req(Dialect::Claude, Dialect::OpenAi, &thinking_with_tool_conv());
    assert_eq!(count_thinking(&result), 0);
    assert_eq!(count_tool_uses(&result), 1);
}

#[test]
fn lossy_thinking_with_tool_use_drops_thinking_for_gemini() {
    let result = map_req(Dialect::Claude, Dialect::Gemini, &thinking_with_tool_conv());
    assert_eq!(count_thinking(&result), 0);
    assert_eq!(count_tool_uses(&result), 1);
}

#[test]
fn lossy_content_block_count_decreases_when_thinking_dropped() {
    let conv = thinking_conv();
    let orig_blocks = conv.messages[1].content.len();
    assert_eq!(orig_blocks, 2); // Thinking + Text

    let result = map_req(Dialect::Claude, Dialect::OpenAi, &conv);
    let mapped_blocks = result.messages[1].content.len();
    assert_eq!(mapped_blocks, 1); // Only Text
}

#[test]
fn lossy_unsupported_pair_returns_error() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = simple_text_conv();
    let err = mapper
        .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn lossy_factory_returns_none_for_unsupported_pair() {
    assert!(default_ir_mapper(Dialect::Kimi, Dialect::Copilot).is_none());
    assert!(default_ir_mapper(Dialect::Codex, Dialect::Gemini).is_none());
}

// ═════════════════════════════════════════════════════════════════════════
// 5. ROUND-TRIP DEGRADATION (15 tests)
//    Map A→B→A and verify what survives and what is lost.
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_openai_claude_openai_simple_text_lossless() {
    let conv = simple_text_conv();
    let mid = map_req(Dialect::OpenAi, Dialect::Claude, &conv);
    let back = map_req(Dialect::Claude, Dialect::OpenAi, &mid);
    assert_eq!(conv.len(), back.len());
    for (o, b) in conv.messages.iter().zip(back.messages.iter()) {
        assert_eq!(o.role, b.role);
        assert_eq!(o.text_content(), b.text_content());
    }
}

#[test]
fn roundtrip_openai_gemini_openai_simple_text_lossless() {
    let conv = simple_text_conv();
    let mid = map_req(Dialect::OpenAi, Dialect::Gemini, &conv);
    let back = map_req(Dialect::Gemini, Dialect::OpenAi, &mid);
    assert_eq!(conv.len(), back.len());
    for (o, b) in conv.messages.iter().zip(back.messages.iter()) {
        assert_eq!(o.role, b.role);
        assert_eq!(o.text_content(), b.text_content());
    }
}

#[test]
fn roundtrip_claude_gemini_claude_simple_text_lossless() {
    let conv = simple_text_conv();
    let mid = map_req(Dialect::Claude, Dialect::Gemini, &conv);
    let back = map_req(Dialect::Gemini, Dialect::Claude, &mid);
    assert_eq!(conv.len(), back.len());
    for (o, b) in conv.messages.iter().zip(back.messages.iter()) {
        assert_eq!(o.role, b.role);
        assert_eq!(o.text_content(), b.text_content());
    }
}

#[test]
fn roundtrip_openai_claude_openai_tool_use_names_preserved() {
    let conv = tool_call_conv();
    let mid = map_req(Dialect::OpenAi, Dialect::Claude, &conv);
    let back = map_req(Dialect::Claude, Dialect::OpenAi, &mid);
    assert_eq!(count_tool_uses(&conv), count_tool_uses(&back));
    let orig_names: Vec<&str> = conv
        .tool_calls()
        .iter()
        .filter_map(|b| {
            if let IrContentBlock::ToolUse { name, .. } = b {
                Some(name.as_str())
            } else {
                None
            }
        })
        .collect();
    let back_names: Vec<&str> = back
        .tool_calls()
        .iter()
        .filter_map(|b| {
            if let IrContentBlock::ToolUse { name, .. } = b {
                Some(name.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(orig_names, back_names);
}

#[test]
fn roundtrip_openai_gemini_openai_tool_use_names_preserved() {
    let conv = tool_call_conv();
    let mid = map_req(Dialect::OpenAi, Dialect::Gemini, &conv);
    let back = map_req(Dialect::Gemini, Dialect::OpenAi, &mid);
    assert_eq!(count_tool_uses(&conv), count_tool_uses(&back));
}

#[test]
fn roundtrip_claude_gemini_claude_tool_use_preserved() {
    let conv = tool_call_conv();
    let mid = map_req(Dialect::Claude, Dialect::Gemini, &conv);
    let back = map_req(Dialect::Gemini, Dialect::Claude, &mid);
    assert_eq!(count_tool_uses(&conv), count_tool_uses(&back));
}

#[test]
fn roundtrip_thinking_lost_claude_openai_claude() {
    let conv = thinking_conv();
    assert_eq!(count_thinking(&conv), 1);
    let mid = map_req(Dialect::Claude, Dialect::OpenAi, &conv);
    assert_eq!(count_thinking(&mid), 0);
    let back = map_req(Dialect::OpenAi, Dialect::Claude, &mid);
    // Thinking is permanently lost after the OpenAI hop.
    assert_eq!(count_thinking(&back), 0);
    // But text survives.
    assert_eq!(back.messages[1].text_content(), "The answer is 42.");
}

#[test]
fn roundtrip_thinking_lost_claude_gemini_claude() {
    let conv = thinking_conv();
    let mid = map_req(Dialect::Claude, Dialect::Gemini, &conv);
    assert_eq!(count_thinking(&mid), 0);
    let back = map_req(Dialect::Gemini, Dialect::Claude, &mid);
    assert_eq!(count_thinking(&back), 0);
    assert_eq!(back.messages[1].text_content(), "The answer is 42.");
}

#[test]
fn roundtrip_tool_role_recovers_openai_claude_openai() {
    let conv = tool_call_conv();
    let mid = map_req(Dialect::OpenAi, Dialect::Claude, &conv);
    // In Claude form, tool result should be User-role.
    assert!(mid.messages.iter().any(|m| {
        m.role == IrRole::User
            && m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
    }));
    let back = map_req(Dialect::Claude, Dialect::OpenAi, &mid);
    // Should recover Tool-role.
    assert!(back.messages.iter().any(|m| m.role == IrRole::Tool));
}

#[test]
fn roundtrip_tool_role_recovers_openai_gemini_openai() {
    let conv = tool_call_conv();
    let mid = map_req(Dialect::OpenAi, Dialect::Gemini, &conv);
    assert!(!mid.messages.iter().any(|m| m.role == IrRole::Tool));
    let back = map_req(Dialect::Gemini, Dialect::OpenAi, &mid);
    assert!(back.messages.iter().any(|m| m.role == IrRole::Tool));
}

#[test]
fn roundtrip_multi_turn_text_all_pairs_lossless() {
    let conv = multi_turn_conv();
    let pairs = [
        (Dialect::OpenAi, Dialect::Claude),
        (Dialect::OpenAi, Dialect::Gemini),
        (Dialect::Claude, Dialect::Gemini),
    ];
    for (a, b) in &pairs {
        let mid = map_req(*a, *b, &conv);
        let back = map_req(*b, *a, &mid);
        assert_eq!(
            conv.len(),
            back.len(),
            "message count diverges for {a:?}→{b:?}→{a:?}"
        );
        for (o, r) in conv.messages.iter().zip(back.messages.iter()) {
            assert_eq!(o.text_content(), r.text_content());
        }
    }
}

#[test]
fn roundtrip_image_preserved_openai_claude_openai() {
    let conv = image_conv();
    let mid = map_req(Dialect::OpenAi, Dialect::Claude, &conv);
    let back = map_req(Dialect::Claude, Dialect::OpenAi, &mid);
    let has_image = back.messages[0].content.iter().any(|b| {
        matches!(
            b,
            IrContentBlock::Image {
                media_type,
                data,
                ..
            } if media_type == "image/png" && data == "iVBORw0KGgo="
        )
    });
    assert!(has_image, "image lost in round-trip");
}

#[test]
fn roundtrip_image_preserved_openai_gemini_openai() {
    let conv = image_conv();
    let mid = map_req(Dialect::OpenAi, Dialect::Gemini, &conv);
    let back = map_req(Dialect::Gemini, Dialect::OpenAi, &mid);
    let has_image = back.messages[0].content.iter().any(|b| {
        matches!(
            b,
            IrContentBlock::Image { media_type, .. } if media_type == "image/png"
        )
    });
    assert!(has_image, "image lost in OpenAI→Gemini→OpenAI round-trip");
}

#[test]
fn roundtrip_error_tool_result_preserved_openai_claude_openai() {
    let conv = error_tool_result_conv();
    let mid = map_req(Dialect::OpenAi, Dialect::Claude, &conv);
    let back = map_req(Dialect::Claude, Dialect::OpenAi, &mid);
    let tr = back
        .messages
        .iter()
        .flat_map(|m| &m.content)
        .find(|b| matches!(b, IrContentBlock::ToolResult { .. }));
    assert!(tr.is_some());
    if let Some(IrContentBlock::ToolResult {
        is_error, content, ..
    }) = tr
    {
        assert!(is_error, "is_error flag lost in round-trip");
        assert!(content.iter().any(
            |c| matches!(c, IrContentBlock::Text { text } if text == "error: command not found")
        ));
    }
}

#[test]
fn roundtrip_metadata_survives_all_pairs() {
    let conv = metadata_conv();
    let pairs = [
        (Dialect::OpenAi, Dialect::Claude),
        (Dialect::OpenAi, Dialect::Gemini),
        (Dialect::Claude, Dialect::Gemini),
    ];
    for (a, b) in &pairs {
        let mid = map_req(*a, *b, &conv);
        let back = map_req(*b, *a, &mid);
        assert_eq!(
            back.messages[0].metadata.get("source"),
            Some(&json!("test")),
            "metadata lost in roundtrip {a:?}→{b:?}→{a:?}"
        );
        assert_eq!(
            back.messages[0].metadata.get("timestamp"),
            Some(&json!(1_700_000_000)),
            "metadata value lost in roundtrip {a:?}→{b:?}→{a:?}"
        );
    }
}

// ═════════════════════════════════════════════════════════════════════════
// BONUS: Factory & structural tests (5 tests)
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn factory_all_cross_pairs_have_mappers() {
    for (from, to) in &CROSS_PAIRS {
        assert!(
            default_ir_mapper(*from, *to).is_some(),
            "no mapper for {from:?} -> {to:?}"
        );
    }
}

#[test]
fn factory_identity_mappers_exist_for_all_dialects() {
    for &d in Dialect::all() {
        assert!(
            default_ir_mapper(d, d).is_some(),
            "no identity mapper for {d:?}"
        );
    }
}

#[test]
fn factory_supported_pairs_consistent_with_cross_pairs() {
    let pairs = abp_mapper::supported_ir_pairs();
    for (from, to) in &CROSS_PAIRS {
        assert!(
            pairs.contains(&(*from, *to)),
            "supported_ir_pairs missing ({from:?}, {to:?})"
        );
    }
}

#[test]
fn mapper_supported_pairs_match_declared_openai_claude() {
    let mapper = OpenAiClaudeIrMapper;
    let pairs = mapper.supported_pairs();
    assert!(pairs.contains(&(Dialect::OpenAi, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::OpenAi)));
    assert_eq!(pairs.len(), 2);
}

#[test]
fn mapper_supported_pairs_match_declared_claude_gemini() {
    let mapper = ClaudeGeminiIrMapper;
    let pairs = mapper.supported_pairs();
    assert!(pairs.contains(&(Dialect::Claude, Dialect::Gemini)));
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::Claude)));
    assert_eq!(pairs.len(), 2);
}
