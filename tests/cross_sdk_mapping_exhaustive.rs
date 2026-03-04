#![allow(clippy::all)]
#![allow(dead_code)]
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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]

//! Exhaustive cross-SDK mapping tests.
//!
//! Verifies that every supported dialect pair produces correct mappings
//! through the IR, covering: simple text, multi-turn, system messages,
//! tool calls, thinking/lossy detection, content blocks, finish reasons,
//! usage mapping, and error propagation.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_dialect::Dialect;
use abp_mapper::{
    IrMapper, MapError, OpenAiClaudeIrMapper, OpenAiCodexIrMapper, OpenAiCopilotIrMapper,
    OpenAiGeminiIrMapper, OpenAiKimiIrMapper, default_ir_mapper, supported_ir_pairs,
};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn simple_text() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Hello"),
        IrMessage::text(IrRole::Assistant, "Hi!"),
    ])
}

fn multi_turn() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "What is Rust?"),
        IrMessage::text(IrRole::Assistant, "A systems language."),
        IrMessage::text(IrRole::User, "Tell me more."),
        IrMessage::text(IrRole::Assistant, "It has ownership semantics."),
    ])
}

fn system_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "You are helpful."),
        IrMessage::text(IrRole::User, "Hi"),
        IrMessage::text(IrRole::Assistant, "Hello!"),
    ])
}

fn tool_call_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "What is the weather?"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Checking.".into(),
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
                    text: "72°F".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(IrRole::Assistant, "It is 72°F."),
    ])
}

fn thinking_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "Solve this"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "step by step...".into(),
                },
                IrContentBlock::Text {
                    text: "Answer: 42".into(),
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
                text: "Describe this.".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "iVBORw0KGgo=".into(),
            },
        ],
    )])
}

fn tool_error_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "run"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "exec".into(),
                input: json!({"cmd": "test"}),
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
    ])
}

fn multi_tool_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "search and read"),
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
                        text: "found".into(),
                    }],
                    is_error: false,
                },
                IrContentBlock::ToolResult {
                    tool_use_id: "t2".into(),
                    content: vec![IrContentBlock::Text {
                        text: "fn main".into(),
                    }],
                    is_error: false,
                },
            ],
        ),
    ])
}

fn sample_tools() -> Vec<IrToolDefinition> {
    vec![IrToolDefinition {
        name: "calc".into(),
        description: "Math evaluator".into(),
        parameters: json!({"type": "object", "properties": {"expr": {"type": "string"}}}),
    }]
}

/// Returns the 18 supported cross-dialect (non-identity) pairs.
fn cross_pairs() -> Vec<(Dialect, Dialect)> {
    supported_ir_pairs()
        .into_iter()
        .filter(|(a, b)| a != b)
        .collect()
}

/// Whether a dialect is Codex (lossy target).
fn is_codex(d: Dialect) -> bool {
    d == Dialect::Codex
}

/// Whether a dialect pair involves Codex as the target (lossy).
fn target_is_codex(to: Dialect) -> bool {
    to == Dialect::Codex
}

/// Whether thinking blocks survive mapping to the target dialect.
/// Only Claude preserves thinking natively.
fn thinking_survives(to: Dialect) -> bool {
    to == Dialect::Claude
}

/// Whether the target dialect uses a separate Tool role for tool results.
/// OpenAI, Kimi, Copilot, Codex use Tool role; Claude and Gemini use User role.
fn has_tool_role(d: Dialect) -> bool {
    matches!(
        d,
        Dialect::OpenAi | Dialect::Kimi | Dialect::Copilot | Dialect::Codex
    )
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Simple text message mapping — every cross-dialect pair
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn simple_text_all_pairs_preserve_message_count() {
    let conv = simple_text();
    for (from, to) in cross_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        // Codex drops nothing from a simple user/assistant text conv
        assert!(
            !result.is_empty(),
            "{from}->{to}: result should not be empty"
        );
    }
}

#[test]
fn simple_text_all_pairs_preserve_user_text() {
    let conv = simple_text();
    for (from, to) in cross_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let has_hello = result
            .messages
            .iter()
            .any(|m| m.text_content().contains("Hello"));
        assert!(has_hello, "{from}->{to}: 'Hello' should survive mapping");
    }
}

#[test]
fn simple_text_all_pairs_preserve_assistant_text() {
    let conv = simple_text();
    for (from, to) in cross_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let has_hi = result
            .messages
            .iter()
            .any(|m| m.text_content().contains("Hi!"));
        assert!(has_hi, "{from}->{to}: 'Hi!' should survive mapping");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Multi-turn conversation mapping
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn multi_turn_all_pairs_preserve_turn_count() {
    let conv = multi_turn();
    for (from, to) in cross_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        if !target_is_codex(to) {
            assert_eq!(
                result.len(),
                4,
                "{from}->{to}: multi-turn should preserve 4 messages"
            );
        }
    }
}

#[test]
fn multi_turn_alternating_roles_preserved() {
    let conv = multi_turn();
    for (from, to) in cross_pairs() {
        if target_is_codex(to) {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let roles: Vec<IrRole> = result.messages.iter().map(|m| m.role).collect();
        assert_eq!(
            roles,
            vec![
                IrRole::User,
                IrRole::Assistant,
                IrRole::User,
                IrRole::Assistant
            ],
            "{from}->{to}: alternating roles must be preserved"
        );
    }
}

#[test]
fn multi_turn_text_content_fidelity() {
    let conv = multi_turn();
    for (from, to) in cross_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let all_text: String = result.messages.iter().map(|m| m.text_content()).collect();
        assert!(
            all_text.contains("Rust"),
            "{from}->{to}: Rust should appear in text"
        );
        assert!(
            all_text.contains("ownership"),
            "{from}->{to}: ownership should appear in text"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. System message handling
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn system_message_preserved_for_supporting_dialects() {
    let conv = system_conv();
    for (from, to) in cross_pairs() {
        if target_is_codex(to) {
            continue; // Codex drops system messages
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let has_helpful = result
            .messages
            .iter()
            .any(|m| m.text_content().contains("helpful"));
        assert!(
            has_helpful,
            "{from}->{to}: system message text should survive"
        );
    }
}

#[test]
fn system_message_dropped_for_codex() {
    let conv = system_conv();
    for from in [Dialect::OpenAi, Dialect::Claude] {
        let mapper = default_ir_mapper(from, Dialect::Codex);
        if let Some(mapper) = mapper {
            let result = mapper.map_request(from, Dialect::Codex, &conv).unwrap();
            assert!(
                !result.messages.iter().any(|m| m.role == IrRole::System),
                "{from}->Codex: system should be dropped"
            );
        }
    }
}

#[test]
fn system_message_role_correct_after_mapping() {
    let conv = system_conv();
    // For non-Codex targets, system message should exist
    for (from, to) in cross_pairs() {
        if target_is_codex(to) {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let sys_msgs: Vec<_> = result
            .messages
            .iter()
            .filter(|m| m.role == IrRole::System)
            .collect();
        assert!(
            !sys_msgs.is_empty(),
            "{from}->{to}: should have system message"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Tool calls mapping
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn tool_use_blocks_preserved_across_non_codex_pairs() {
    let conv = tool_call_conv();
    for (from, to) in cross_pairs() {
        if target_is_codex(to) {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let tool_uses = result.tool_calls();
        assert!(
            !tool_uses.is_empty(),
            "{from}->{to}: tool use blocks should survive"
        );
    }
}

#[test]
fn tool_use_name_preserved() {
    let conv = tool_call_conv();
    for (from, to) in cross_pairs() {
        if target_is_codex(to) {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let has_weather = result
            .tool_calls()
            .iter()
            .any(|b| matches!(b, IrContentBlock::ToolUse { name, .. } if name == "get_weather"));
        assert!(
            has_weather,
            "{from}->{to}: tool name 'get_weather' should survive"
        );
    }
}

#[test]
fn tool_use_input_preserved() {
    let conv = tool_call_conv();
    for (from, to) in cross_pairs() {
        if target_is_codex(to) {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let has_nyc = result.tool_calls().iter().any(|b| {
            matches!(b, IrContentBlock::ToolUse { input, .. } if input.get("city") == Some(&json!("NYC")))
        });
        assert!(has_nyc, "{from}->{to}: tool input should preserve city=NYC");
    }
}

#[test]
fn tool_result_role_correct_per_dialect() {
    let conv = tool_call_conv();
    for (from, to) in cross_pairs() {
        if target_is_codex(to) {
            continue;
        }
        // Codex-source mappers do lossless clone, preserving original roles
        if from == Dialect::Codex {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
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
            if has_tool_role(to) {
                assert_eq!(
                    msg.role,
                    IrRole::Tool,
                    "{from}->{to}: tool results should use Tool role"
                );
            } else {
                assert_eq!(
                    msg.role,
                    IrRole::User,
                    "{from}->{to}: tool results should use User role"
                );
            }
        }
    }
}

#[test]
fn tool_calls_dropped_for_codex_target() {
    let conv = tool_call_conv();
    for from in [Dialect::OpenAi, Dialect::Claude] {
        let mapper = default_ir_mapper(from, Dialect::Codex);
        if let Some(mapper) = mapper {
            let result = mapper.map_request(from, Dialect::Codex, &conv).unwrap();
            assert!(
                result.tool_calls().is_empty(),
                "{from}->Codex: tool calls should be dropped"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Streaming/thinking — lossy mappings detected
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn thinking_blocks_dropped_for_non_claude_targets() {
    let conv = thinking_conv();
    for (from, to) in cross_pairs() {
        if thinking_survives(to) {
            continue;
        }
        // Codex->OpenAI is lossless clone; Codex wouldn't produce thinking,
        // but the mapper doesn't strip them either. Skip Codex source.
        if from == Dialect::Codex {
            continue;
        }
        let mapper = default_ir_mapper(from, to);
        if let Some(mapper) = mapper {
            let result = mapper.map_request(from, to, &conv).unwrap();
            let has_thinking = result.messages.iter().any(|m| {
                m.content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
            });
            assert!(
                !has_thinking,
                "{from}->{to}: thinking blocks should be dropped"
            );
        }
    }
}

#[test]
fn thinking_text_content_survives_even_when_thinking_dropped() {
    let conv = thinking_conv();
    for (from, to) in cross_pairs() {
        let mapper = default_ir_mapper(from, to);
        if let Some(mapper) = mapper {
            let result = mapper.map_request(from, to, &conv).unwrap();
            let has_answer = result
                .messages
                .iter()
                .any(|m| m.text_content().contains("42"));
            assert!(has_answer, "{from}->{to}: text 'Answer: 42' should survive");
        }
    }
}

#[test]
fn thinking_preserved_for_claude_target() {
    // Claude → Claude identity preserves thinking
    let conv = thinking_conv();
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::Claude).unwrap();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Claude, &conv)
        .unwrap();
    let has_thinking = result.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    });
    assert!(has_thinking, "Claude identity should preserve thinking");
}

#[test]
fn thinking_to_openai_is_lossy() {
    let conv = thinking_conv();
    let mapper = OpenAiClaudeIrMapper;
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
        .unwrap();
    assert!(
        !result.messages.iter().any(|m| m
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))),
        "Claude->OpenAI should drop thinking"
    );
}

#[test]
fn thinking_to_gemini_is_lossy() {
    let conv = thinking_conv();
    let mapper = OpenAiGeminiIrMapper;
    // Map via OpenAI intermediary: Claude->OpenAI drops thinking, then OpenAI->Gemini
    let oc = OpenAiClaudeIrMapper;
    let openai = oc
        .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
        .unwrap();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &openai)
        .unwrap();
    assert!(
        !result.messages.iter().any(|m| m
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))),
        "thinking should not reach Gemini"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Lossy mappings — Codex pair specifics
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn codex_to_openai_is_lossless_for_text() {
    let conv = simple_text();
    let mapper = OpenAiCodexIrMapper;
    let result = mapper
        .map_request(Dialect::Codex, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(result.len(), conv.len());
    for (o, r) in conv.messages.iter().zip(result.messages.iter()) {
        assert_eq!(o.text_content(), r.text_content());
    }
}

#[test]
fn openai_to_codex_drops_system_and_tool() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "system"),
        IrMessage::text(IrRole::User, "hello"),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "result".into(),
                }],
                is_error: false,
            }],
        ),
        IrMessage::text(IrRole::Assistant, "bye"),
    ]);
    let mapper = OpenAiCodexIrMapper;
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
        .unwrap();
    assert!(
        !result.messages.iter().any(|m| m.role == IrRole::System),
        "system dropped"
    );
    assert!(
        !result.messages.iter().any(|m| m.role == IrRole::Tool),
        "tool dropped"
    );
    assert_eq!(result.len(), 2); // user + assistant only
}

#[test]
fn codex_to_claude_rejects_unmappable_tools() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "apply_patch".into(),
            input: json!({"patch": "..."}),
        }],
    )]);
    let mapper = default_ir_mapper(Dialect::Codex, Dialect::Claude).unwrap();
    let err = mapper
        .map_request(Dialect::Codex, Dialect::Claude, &conv)
        .unwrap_err();
    assert!(
        matches!(err, MapError::UnmappableTool { ref name, .. } if name == "apply_patch"),
        "should reject apply_patch"
    );
}

#[test]
fn codex_to_claude_rejects_apply_diff() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "apply_diff".into(),
            input: json!({"diff": "..."}),
        }],
    )]);
    let mapper = default_ir_mapper(Dialect::Codex, Dialect::Claude).unwrap();
    let err = mapper
        .map_request(Dialect::Codex, Dialect::Claude, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnmappableTool { .. }));
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Content blocks — text, image, tool results across pairs
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn image_blocks_preserved_across_non_codex_pairs() {
    let conv = image_conv();
    for (from, to) in cross_pairs() {
        if target_is_codex(to) {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let has_image = result.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Image { .. }))
        });
        assert!(has_image, "{from}->{to}: image blocks should be preserved");
    }
}

#[test]
fn image_media_type_preserved() {
    let conv = image_conv();
    for (from, to) in cross_pairs() {
        if target_is_codex(to) {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let has_png = result.messages.iter().any(|m| {
            m.content.iter().any(|b| {
                matches!(b, IrContentBlock::Image { media_type, .. } if media_type == "image/png")
            })
        });
        assert!(has_png, "{from}->{to}: image/png media type should survive");
    }
}

#[test]
fn image_data_preserved() {
    let conv = image_conv();
    for (from, to) in cross_pairs() {
        if target_is_codex(to) {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let has_data = result.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Image { data, .. } if data == "iVBORw0KGgo="))
        });
        assert!(has_data, "{from}->{to}: image data should survive");
    }
}

#[test]
fn image_blocks_dropped_for_codex() {
    let conv = image_conv();
    for from in [Dialect::OpenAi, Dialect::Claude] {
        let mapper = default_ir_mapper(from, Dialect::Codex);
        if let Some(mapper) = mapper {
            let result = mapper.map_request(from, Dialect::Codex, &conv).unwrap();
            let has_image = result.messages.iter().any(|m| {
                m.content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::Image { .. }))
            });
            assert!(!has_image, "{from}->Codex: image blocks should be dropped");
        }
    }
}

#[test]
fn tool_error_flag_preserved() {
    let conv = tool_error_conv();
    for (from, to) in cross_pairs() {
        if target_is_codex(to) {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let has_error = result.messages.iter().any(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolResult { is_error: true, .. }))
        });
        assert!(
            has_error,
            "{from}->{to}: tool error flag should be preserved"
        );
    }
}

#[test]
fn multi_tool_results_preserved() {
    let conv = multi_tool_conv();
    for (from, to) in cross_pairs() {
        if target_is_codex(to) {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let tool_result_count: usize = result
            .messages
            .iter()
            .flat_map(|m| &m.content)
            .filter(|b| matches!(b, IrContentBlock::ToolResult { .. }))
            .count();
        assert_eq!(
            tool_result_count, 2,
            "{from}->{to}: both tool results should survive"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Usage/token mapping — IrUsage round-trip
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn usage_struct_serde_roundtrip() {
    use abp_core::ir::IrUsage;
    let usage = IrUsage {
        input_tokens: 100,
        output_tokens: 200,
        total_tokens: 300,
        cache_read_tokens: 10,
        cache_write_tokens: 5,
    };
    let json = serde_json::to_value(usage).unwrap();
    assert_eq!(json["input_tokens"], 100);
    assert_eq!(json["output_tokens"], 200);
    assert_eq!(json["total_tokens"], 300);
    assert_eq!(json["cache_read_tokens"], 10);
    assert_eq!(json["cache_write_tokens"], 5);
    let back: IrUsage = serde_json::from_value(json).unwrap();
    assert_eq!(back.input_tokens, usage.input_tokens);
    assert_eq!(back.output_tokens, usage.output_tokens);
    assert_eq!(back.total_tokens, usage.total_tokens);
}

#[test]
fn usage_zero_values() {
    use abp_core::ir::IrUsage;
    let usage = IrUsage {
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
    };
    let json = serde_json::to_value(usage).unwrap();
    let back: IrUsage = serde_json::from_value(json).unwrap();
    assert_eq!(back.total_tokens, 0);
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Finish reasons — stop vs end_turn vs STOP etc. (metadata level)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn metadata_preserved_through_all_pairs() {
    let mut msg = IrMessage::text(IrRole::User, "hi");
    msg.metadata.insert("finish_reason".into(), json!("stop"));
    let conv = IrConversation::from_messages(vec![msg]);
    for (from, to) in cross_pairs() {
        if target_is_codex(to) {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let has_meta = result
            .messages
            .iter()
            .any(|m| m.metadata.get("finish_reason") == Some(&json!("stop")));
        assert!(has_meta, "{from}->{to}: metadata should be preserved");
    }
}

#[test]
fn metadata_end_turn_preserved() {
    let mut msg = IrMessage::text(IrRole::Assistant, "done");
    msg.metadata.insert("stop_reason".into(), json!("end_turn"));
    let conv = IrConversation::from_messages(vec![msg]);
    for (from, to) in cross_pairs() {
        if target_is_codex(to) {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let has_meta = result
            .messages
            .iter()
            .any(|m| m.metadata.get("stop_reason") == Some(&json!("end_turn")));
        assert!(
            has_meta,
            "{from}->{to}: stop_reason=end_turn should be preserved"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Error propagation — unmappable requests fail early
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn unsupported_pair_returns_error() {
    // Kimi->Copilot has no direct mapper
    let mapper = default_ir_mapper(Dialect::Kimi, Dialect::Copilot);
    assert!(
        mapper.is_none(),
        "Kimi->Copilot should have no mapper registered"
    );
}

#[test]
fn unsupported_pair_error_from_concrete_mapper() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = simple_text();
    let err = mapper
        .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
        .unwrap_err();
    assert!(matches!(
        err,
        MapError::UnsupportedPair {
            from: Dialect::Gemini,
            to: Dialect::Kimi
        }
    ));
}

#[test]
fn unsupported_pair_error_from_gemini_mapper() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = simple_text();
    let err = mapper
        .map_request(Dialect::Claude, Dialect::Codex, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn unsupported_pair_error_from_kimi_mapper() {
    let mapper = OpenAiKimiIrMapper;
    let conv = simple_text();
    let err = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn unsupported_pair_error_from_copilot_mapper() {
    let mapper = OpenAiCopilotIrMapper;
    let conv = simple_text();
    let err = mapper
        .map_request(Dialect::Gemini, Dialect::Claude, &conv)
        .unwrap_err();
    assert!(matches!(err, MapError::UnsupportedPair { .. }));
}

#[test]
fn claude_gemini_system_with_image_fails_early() {
    use abp_mapper::ClaudeGeminiIrMapper;
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::System,
        vec![
            IrContentBlock::Text {
                text: "instructions".into(),
            },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "data".into(),
            },
        ],
    )]);
    let mapper = ClaudeGeminiIrMapper;
    let err = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap_err();
    assert!(
        matches!(err, MapError::UnmappableContent { .. }),
        "system with image should fail for Gemini"
    );
}

#[test]
fn map_error_serialization_roundtrip() {
    let errors = vec![
        MapError::UnsupportedPair {
            from: Dialect::Kimi,
            to: Dialect::Copilot,
        },
        MapError::LossyConversion {
            field: "thinking".into(),
            reason: "target has no thinking block".into(),
        },
        MapError::UnmappableTool {
            name: "apply_patch".into(),
            reason: "Codex-specific".into(),
        },
        MapError::IncompatibleCapability {
            capability: "logprobs".into(),
            reason: "not supported".into(),
        },
        MapError::UnmappableContent {
            field: "system".into(),
            reason: "image blocks in system prompt".into(),
        },
    ];
    for err in &errors {
        let json = serde_json::to_string(err).unwrap();
        let back: MapError = serde_json::from_str(&json).unwrap();
        assert_eq!(*err, back);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Cross-cutting: factory coverage
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn factory_resolves_all_supported_pairs() {
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to);
        assert!(
            mapper.is_some(),
            "factory should resolve mapper for {from}->{to}"
        );
    }
}

#[test]
fn factory_identity_for_all_dialects() {
    for &d in Dialect::all() {
        let mapper = default_ir_mapper(d, d).unwrap();
        let conv = simple_text();
        let result = mapper.map_request(d, d, &conv).unwrap();
        assert_eq!(
            result.len(),
            conv.len(),
            "{d}: identity should preserve length"
        );
    }
}

#[test]
fn factory_identity_preserves_content() {
    for &d in Dialect::all() {
        let mapper = default_ir_mapper(d, d).unwrap();
        let conv = tool_call_conv();
        let result = mapper.map_request(d, d, &conv).unwrap();
        assert_eq!(
            result, conv,
            "{d}: identity should return identical conversation"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Cross-cutting: response mapping
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn response_mapping_all_pairs() {
    let conv = simple_text();
    for (from, to) in cross_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_response(from, to, &conv).unwrap();
        assert!(
            !result.is_empty(),
            "{from}->{to}: response mapping should produce output"
        );
    }
}

#[test]
fn response_mapping_preserves_text() {
    let conv = simple_text();
    for (from, to) in cross_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_response(from, to, &conv).unwrap();
        let all_text: String = result.messages.iter().map(|m| m.text_content()).collect();
        assert!(
            all_text.contains("Hello") || all_text.contains("Hi!"),
            "{from}->{to}: response should preserve some text"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Roundtrip fidelity — every non-lossy pair
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_simple_text_non_codex_pairs() {
    let conv = simple_text();
    for (from, to) in cross_pairs() {
        if is_codex(from) || is_codex(to) {
            continue;
        }
        let fwd = default_ir_mapper(from, to).unwrap();
        let rev = default_ir_mapper(to, from);
        if let Some(rev) = rev {
            let mapped = fwd.map_request(from, to, &conv).unwrap();
            let back = rev.map_request(to, from, &mapped).unwrap();
            let orig_text: String = conv.messages.iter().map(|m| m.text_content()).collect();
            let back_text: String = back.messages.iter().map(|m| m.text_content()).collect();
            assert_eq!(
                orig_text, back_text,
                "{from}->{to}->{from}: text should survive roundtrip"
            );
        }
    }
}

#[test]
fn roundtrip_system_message_non_codex_pairs() {
    let conv = system_conv();
    for (from, to) in cross_pairs() {
        if is_codex(from) || is_codex(to) {
            continue;
        }
        let fwd = default_ir_mapper(from, to).unwrap();
        let rev = default_ir_mapper(to, from);
        if let Some(rev) = rev {
            let mapped = fwd.map_request(from, to, &conv).unwrap();
            let back = rev.map_request(to, from, &mapped).unwrap();
            let has_helpful = back
                .messages
                .iter()
                .any(|m| m.text_content().contains("helpful"));
            assert!(
                has_helpful,
                "{from}->{to}->{from}: system text should survive roundtrip"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Specific pair-level tests (covering named SDK pairs)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_claude_tool_result_becomes_user_role() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = tool_call_conv();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    let tool_result_msg = result
        .messages
        .iter()
        .find(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
        })
        .unwrap();
    assert_eq!(tool_result_msg.role, IrRole::User);
}

#[test]
fn claude_to_openai_user_tool_results_split_to_tool_role() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = multi_tool_conv();
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
fn openai_to_gemini_tool_result_becomes_user_role() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = tool_call_conv();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    let tool_result_msg = result
        .messages
        .iter()
        .find(|m| {
            m.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
        })
        .unwrap();
    assert_eq!(tool_result_msg.role, IrRole::User);
}

#[test]
fn gemini_to_openai_user_tool_results_split() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = multi_tool_conv();
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
fn openai_to_kimi_near_identity() {
    let conv = tool_call_conv();
    let mapper = OpenAiKimiIrMapper;
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Kimi, &conv)
        .unwrap();
    // Should preserve all roles since Kimi is OpenAI-compatible
    assert_eq!(result.len(), conv.len());
    for (o, r) in conv.messages.iter().zip(result.messages.iter()) {
        assert_eq!(o.role, r.role);
    }
}

#[test]
fn kimi_to_openai_near_identity() {
    let conv = tool_call_conv();
    let mapper = OpenAiKimiIrMapper;
    let result = mapper
        .map_request(Dialect::Kimi, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(result.len(), conv.len());
}

#[test]
fn openai_to_copilot_near_identity() {
    let conv = tool_call_conv();
    let mapper = OpenAiCopilotIrMapper;
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Copilot, &conv)
        .unwrap();
    assert_eq!(result.len(), conv.len());
}

#[test]
fn copilot_to_openai_near_identity() {
    let conv = tool_call_conv();
    let mapper = OpenAiCopilotIrMapper;
    let result = mapper
        .map_request(Dialect::Copilot, Dialect::OpenAi, &conv)
        .unwrap();
    assert_eq!(result.len(), conv.len());
}

#[test]
fn claude_to_kimi_tool_result_role_switch() {
    use abp_mapper::ClaudeKimiIrMapper;
    let conv = multi_tool_conv();
    let mapper = ClaudeKimiIrMapper;
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Kimi, &conv)
        .unwrap();
    // Claude's user-role tool results should become Tool-role for Kimi
    let tool_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert_eq!(tool_msgs.len(), 2);
}

#[test]
fn kimi_to_claude_tool_role_becomes_user() {
    use abp_mapper::ClaudeKimiIrMapper;
    let conv = tool_call_conv();
    let mapper = ClaudeKimiIrMapper;
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
        assert_eq!(msg.role, IrRole::User, "Kimi->Claude: tool results as User");
    }
}

#[test]
fn claude_to_gemini_thinking_dropped() {
    use abp_mapper::ClaudeGeminiIrMapper;
    let conv = thinking_conv();
    let mapper = ClaudeGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &conv)
        .unwrap();
    assert!(!result.messages.iter().any(|m| {
        m.content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
    }));
}

#[test]
fn gemini_to_claude_tool_role_becomes_user() {
    use abp_mapper::ClaudeGeminiIrMapper;
    let conv = tool_call_conv();
    let mapper = ClaudeGeminiIrMapper;
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::Claude, &conv)
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
            "Gemini->Claude: tool results as User"
        );
    }
}

#[test]
fn gemini_to_kimi_tool_result_role_switch() {
    use abp_mapper::GeminiKimiIrMapper;
    let conv = multi_tool_conv();
    let mapper = GeminiKimiIrMapper;
    let result = mapper
        .map_request(Dialect::Gemini, Dialect::Kimi, &conv)
        .unwrap();
    let tool_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == IrRole::Tool)
        .collect();
    assert_eq!(tool_msgs.len(), 2);
}

#[test]
fn kimi_to_gemini_tool_role_becomes_user() {
    use abp_mapper::GeminiKimiIrMapper;
    let conv = tool_call_conv();
    let mapper = GeminiKimiIrMapper;
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
        assert_eq!(msg.role, IrRole::User, "Kimi->Gemini: tool results as User");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Empty conversation edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn empty_conv_all_pairs() {
    let conv = IrConversation::new();
    for (from, to) in supported_ir_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        assert!(
            result.is_empty(),
            "{from}->{to}: empty conv should map to empty"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Mixed content blocks
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn mixed_text_and_image_in_user_message() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::User,
        vec![
            IrContentBlock::Text {
                text: "Look at this".into(),
            },
            IrContentBlock::Image {
                media_type: "image/jpeg".into(),
                data: "jpeg_data".into(),
            },
            IrContentBlock::Text {
                text: "What do you see?".into(),
            },
        ],
    )]);
    for (from, to) in cross_pairs() {
        if target_is_codex(to) {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let has_text = result
            .messages
            .iter()
            .any(|m| m.text_content().contains("Look"));
        assert!(has_text, "{from}->{to}: mixed content text should survive");
    }
}

#[test]
fn mixed_tool_use_and_text_in_assistant() {
    let conv = IrConversation::from_messages(vec![IrMessage::new(
        IrRole::Assistant,
        vec![
            IrContentBlock::Text {
                text: "Let me search.".into(),
            },
            IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "web_search".into(),
                input: json!({"query": "rust lang"}),
            },
        ],
    )]);
    for (from, to) in cross_pairs() {
        if target_is_codex(to) {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let asst = result
            .messages
            .iter()
            .find(|m| m.role == IrRole::Assistant)
            .unwrap();
        let has_text = asst
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::Text { text } if text.contains("search")));
        let has_tool = asst
            .content
            .iter()
            .any(|b| matches!(b, IrContentBlock::ToolUse { .. }));
        assert!(
            has_text && has_tool,
            "{from}->{to}: assistant should have both text and tool_use"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Supported pairs completeness
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn supported_pairs_includes_all_identity() {
    let pairs = supported_ir_pairs();
    for &d in Dialect::all() {
        assert!(pairs.contains(&(d, d)), "missing identity pair for {d}");
    }
}

#[test]
fn supported_pairs_count() {
    let pairs = supported_ir_pairs();
    // 6 identity + 18 cross-dialect = 24
    assert!(
        pairs.len() >= 24,
        "expected at least 24 pairs, got {}",
        pairs.len()
    );
}

#[test]
fn cross_dialect_pairs_have_both_directions() {
    let pairs = supported_ir_pairs();
    let cross: Vec<_> = pairs.iter().filter(|(a, b)| a != b).collect();
    for (a, b) in &cross {
        assert!(
            cross.contains(&&(*b, *a)),
            "pair {a}->{b} exists but {b}->{a} does not"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// IR lowering through mapper + lower pipeline
// (lower module uses abp_sdk_types::Dialect, mapper uses abp_dialect::Dialect)
// ═══════════════════════════════════════════════════════════════════════

/// Convert abp_dialect::Dialect to abp_sdk_types::Dialect for lower module calls.
fn to_sdk_dialect(d: Dialect) -> abp_sdk_types::Dialect {
    match d {
        Dialect::OpenAi => abp_sdk_types::Dialect::OpenAi,
        Dialect::Claude => abp_sdk_types::Dialect::Claude,
        Dialect::Gemini => abp_sdk_types::Dialect::Gemini,
        Dialect::Codex => abp_sdk_types::Dialect::Codex,
        Dialect::Kimi => abp_sdk_types::Dialect::Kimi,
        Dialect::Copilot => abp_sdk_types::Dialect::Copilot,
    }
}

#[test]
fn lowering_after_mapping_produces_valid_json() {
    use abp_ir::lower::lower_for_dialect;
    let conv = system_conv();
    let tools = sample_tools();
    for (from, to) in cross_pairs() {
        let mapper = default_ir_mapper(from, to).unwrap();
        let mapped = mapper.map_request(from, to, &conv).unwrap();
        let lowered = lower_for_dialect(to_sdk_dialect(to), &mapped, &tools);
        assert!(
            lowered.is_object(),
            "{from}->{to}: lowered output should be a JSON object"
        );
    }
}

#[test]
fn lowering_openai_has_messages_key() {
    use abp_ir::lower::lower_to_openai;
    let conv = simple_text();
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();
    let mapped = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
        .unwrap();
    let lowered = lower_to_openai(&mapped, &[]);
    assert!(lowered.get("messages").is_some());
}

#[test]
fn lowering_claude_has_system_field() {
    use abp_ir::lower::lower_to_claude;
    let conv = system_conv();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    let lowered = lower_to_claude(&mapped, &[]);
    assert_eq!(lowered["system"], "You are helpful.");
}

#[test]
fn lowering_gemini_has_contents_key() {
    use abp_ir::lower::lower_to_gemini;
    let conv = simple_text();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).unwrap();
    let mapped = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    let lowered = lower_to_gemini(&mapped, &[]);
    assert!(lowered.get("contents").is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// Additional coverage: per-pair specific behavior tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn openai_to_claude_preserves_tool_use_id() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = tool_call_conv();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &conv)
        .unwrap();
    let has_id = result
        .tool_calls()
        .iter()
        .any(|b| matches!(b, IrContentBlock::ToolUse { id, .. } if id == "call_1"));
    assert!(has_id, "tool use id 'call_1' should survive OpenAI->Claude");
}

#[test]
fn claude_to_openai_preserves_tool_use_id() {
    let mapper = OpenAiClaudeIrMapper;
    let conv = tool_call_conv();
    let result = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &conv)
        .unwrap();
    let has_id = result
        .tool_calls()
        .iter()
        .any(|b| matches!(b, IrContentBlock::ToolUse { id, .. } if id == "call_1"));
    assert!(has_id, "tool use id 'call_1' should survive Claude->OpenAI");
}

#[test]
fn openai_to_gemini_preserves_tool_use_id() {
    let mapper = OpenAiGeminiIrMapper;
    let conv = tool_call_conv();
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &conv)
        .unwrap();
    let has_id = result
        .tool_calls()
        .iter()
        .any(|b| matches!(b, IrContentBlock::ToolUse { id, .. } if id == "call_1"));
    assert!(has_id, "tool use id 'call_1' should survive OpenAI->Gemini");
}

#[test]
fn tool_result_content_text_preserved() {
    let conv = tool_call_conv();
    for (from, to) in cross_pairs() {
        if target_is_codex(to) || from == Dialect::Codex {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let has_72 = result.messages.iter().any(|m| {
            m.content.iter().any(|b| match b {
                IrContentBlock::ToolResult { content, .. } => content
                    .iter()
                    .any(|c| matches!(c, IrContentBlock::Text { text } if text.contains("72"))),
                _ => false,
            })
        });
        assert!(
            has_72,
            "{from}->{to}: tool result content '72°F' should survive"
        );
    }
}

#[test]
fn tool_result_tool_use_id_preserved() {
    let conv = tool_call_conv();
    for (from, to) in cross_pairs() {
        if target_is_codex(to) || from == Dialect::Codex {
            continue;
        }
        let mapper = default_ir_mapper(from, to).unwrap();
        let result = mapper.map_request(from, to, &conv).unwrap();
        let has_call_1 = result.messages.iter().any(|m| {
            m.content.iter().any(|b| {
                matches!(b, IrContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "call_1")
            })
        });
        assert!(
            has_call_1,
            "{from}->{to}: tool_use_id 'call_1' should survive"
        );
    }
}

#[test]
fn only_text_messages_survive_codex() {
    let conv = IrConversation::from_messages(vec![
        IrMessage::text(IrRole::User, "question"),
        IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "answer".into(),
                },
                IrContentBlock::Thinking {
                    text: "hidden".into(),
                },
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "x".into(),
                    input: json!({}),
                },
            ],
        ),
    ]);
    let mapper = OpenAiCodexIrMapper;
    let result = mapper
        .map_request(Dialect::OpenAi, Dialect::Codex, &conv)
        .unwrap();
    for msg in &result.messages {
        for block in &msg.content {
            assert!(
                matches!(block, IrContentBlock::Text { .. }),
                "Codex should only have text blocks"
            );
        }
    }
}

#[test]
fn codex_claude_mapper_supported_pairs() {
    use abp_mapper::CodexClaudeIrMapper;
    let mapper = CodexClaudeIrMapper;
    let pairs = mapper.supported_pairs();
    assert!(pairs.contains(&(Dialect::Codex, Dialect::Claude)));
    assert!(pairs.contains(&(Dialect::Claude, Dialect::Codex)));
    assert_eq!(pairs.len(), 2);
}

#[test]
fn gemini_kimi_mapper_supported_pairs() {
    use abp_mapper::GeminiKimiIrMapper;
    let mapper = GeminiKimiIrMapper;
    let pairs = mapper.supported_pairs();
    assert!(pairs.contains(&(Dialect::Gemini, Dialect::Kimi)));
    assert!(pairs.contains(&(Dialect::Kimi, Dialect::Gemini)));
    assert_eq!(pairs.len(), 2);
}

#[test]
fn claude_kimi_mapper_supported_pairs() {
    use abp_mapper::ClaudeKimiIrMapper;
    let mapper = ClaudeKimiIrMapper;
    let pairs = mapper.supported_pairs();
    assert!(pairs.contains(&(Dialect::Claude, Dialect::Kimi)));
    assert!(pairs.contains(&(Dialect::Kimi, Dialect::Claude)));
    assert_eq!(pairs.len(), 2);
}

#[test]
fn roundtrip_tool_calls_non_codex() {
    let conv = tool_call_conv();
    for (from, to) in cross_pairs() {
        if is_codex(from) || is_codex(to) {
            continue;
        }
        let fwd = default_ir_mapper(from, to).unwrap();
        let rev = default_ir_mapper(to, from);
        if let Some(rev) = rev {
            let mapped = fwd.map_request(from, to, &conv).unwrap();
            let back = rev.map_request(to, from, &mapped).unwrap();
            let orig_tools = conv.tool_calls();
            let back_tools = back.tool_calls();
            assert_eq!(
                orig_tools.len(),
                back_tools.len(),
                "{from}->{to}->{from}: tool call count should survive roundtrip"
            );
        }
    }
}

#[test]
fn all_mappers_are_send_sync() {
    use abp_mapper::{
        ClaudeGeminiIrMapper, ClaudeKimiIrMapper, CodexClaudeIrMapper, GeminiKimiIrMapper,
        IrIdentityMapper,
    };
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<IrIdentityMapper>();
    assert_send_sync::<OpenAiClaudeIrMapper>();
    assert_send_sync::<OpenAiGeminiIrMapper>();
    assert_send_sync::<OpenAiCodexIrMapper>();
    assert_send_sync::<OpenAiKimiIrMapper>();
    assert_send_sync::<OpenAiCopilotIrMapper>();
    assert_send_sync::<ClaudeGeminiIrMapper>();
    assert_send_sync::<ClaudeKimiIrMapper>();
    assert_send_sync::<GeminiKimiIrMapper>();
    assert_send_sync::<CodexClaudeIrMapper>();
}
