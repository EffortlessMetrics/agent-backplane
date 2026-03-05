// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(unknown_lints)]

//! Exhaustive cross-SDK parity tests (60+ tests).
//!
//! Verifies translation fidelity across all SDK pairs:
//! 1. Basic text message roundtrip (15 pairs)
//! 2. Tool call roundtrip (9 supported pairs)
//! 3. Streaming event mapping (response path, 9 pairs)
//! 4. Error propagation for tool results (9 pairs)
//! 5. Feature matrix verification — native/emulated/unsupported per SDK
//! 6. Loss detection — thinking, images, tools lost in translation
//! 7. Metadata preservation — vendor-specific metadata survives in `extra`
//! 8. Role mapping correctness — system/user/assistant/tool roles

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;
use abp_mapper::{
    capabilities::{dialect_capabilities, Support},
    default_ir_mapper, supported_ir_pairs, ClaudeGeminiIrMapper, ClaudeKimiIrMapper,
    CodexClaudeIrMapper, GeminiKimiIrMapper, IrMapper, MapError, OpenAiClaudeIrMapper,
    OpenAiCodexIrMapper, OpenAiCopilotIrMapper, OpenAiGeminiIrMapper, OpenAiKimiIrMapper,
};
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

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
    msg.metadata
        .insert("vendor_field".into(), json!("vendor_value"));
    msg.metadata.insert("priority".into(), json!(42));
    msg.metadata.insert("tags".into(), json!(["alpha", "beta"]));
    IrConversation::from_messages(vec![msg])
}

fn all_roles_conv() -> IrConversation {
    IrConversation::from_messages(vec![
        IrMessage::text(IrRole::System, "System prompt"),
        IrMessage::text(IrRole::User, "User turn"),
        IrMessage::text(IrRole::Assistant, "Assistant turn"),
        IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "r1".into(),
                name: "test_tool".into(),
                input: json!({}),
            }],
        ),
        IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "r1".into(),
                content: vec![IrContentBlock::Text {
                    text: "tool output".into(),
                }],
                is_error: false,
            }],
        ),
    ])
}

// ── Assertion helpers ──────────────────────────────────────────────────

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

fn count_tool_uses(conv: &IrConversation) -> usize {
    conv.tool_calls().len()
}

fn has_error_tool_result(conv: &IrConversation) -> bool {
    conv.messages
        .iter()
        .flat_map(|m| &m.content)
        .any(|b| matches!(b, IrContentBlock::ToolResult { is_error, .. } if *is_error))
}

fn has_thinking_block(conv: &IrConversation) -> bool {
    conv.messages
        .iter()
        .flat_map(|m| &m.content)
        .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
}

fn has_image_block(conv: &IrConversation) -> bool {
    conv.messages
        .iter()
        .flat_map(|m| &m.content)
        .any(|b| matches!(b, IrContentBlock::Image { .. }))
}

fn has_role(conv: &IrConversation, role: IrRole) -> bool {
    conv.messages.iter().any(|m| m.role == role)
}

/// Roundtrip: A→B→A through default_ir_mapper.
fn roundtrip(
    from: Dialect,
    to: Dialect,
    conv: &IrConversation,
) -> Result<(IrConversation, IrConversation), MapError> {
    let mapper = default_ir_mapper(from, to).expect("mapper must exist");
    let mid = mapper.map_request(from, to, conv)?;
    let back_mapper = default_ir_mapper(to, from).expect("reverse mapper must exist");
    let back = back_mapper.map_request(to, from, &mid)?;
    Ok((mid, back))
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 1: Basic text message roundtrip — all 15 cross-dialect pairs
// ═══════════════════════════════════════════════════════════════════════════

macro_rules! text_roundtrip_test {
    ($name:ident, $from:expr, $to:expr) => {
        #[test]
        fn $name() {
            let orig = simple_text();
            let (mid, back) = roundtrip($from, $to, &orig).unwrap();
            assert_eq!(
                mid.len(),
                orig.len(),
                "message count changed in forward map"
            );
            assert_eq!(back.len(), orig.len(), "message count changed in roundtrip");
            assert_text_preserved(&orig, &back);
        }
    };
}

// 15 directional pairs (excluding identity)
text_roundtrip_test!(text_rt_openai_claude, Dialect::OpenAi, Dialect::Claude);
text_roundtrip_test!(text_rt_openai_gemini, Dialect::OpenAi, Dialect::Gemini);
text_roundtrip_test!(text_rt_openai_codex, Dialect::OpenAi, Dialect::Codex);
text_roundtrip_test!(text_rt_openai_kimi, Dialect::OpenAi, Dialect::Kimi);
text_roundtrip_test!(text_rt_openai_copilot, Dialect::OpenAi, Dialect::Copilot);
text_roundtrip_test!(text_rt_claude_gemini, Dialect::Claude, Dialect::Gemini);
text_roundtrip_test!(text_rt_claude_kimi, Dialect::Claude, Dialect::Kimi);
text_roundtrip_test!(text_rt_claude_codex, Dialect::Claude, Dialect::Codex);
text_roundtrip_test!(text_rt_gemini_kimi, Dialect::Gemini, Dialect::Kimi);

// Reverse directions for pairs with known asymmetry
text_roundtrip_test!(text_rt_claude_openai, Dialect::Claude, Dialect::OpenAi);
text_roundtrip_test!(text_rt_gemini_openai, Dialect::Gemini, Dialect::OpenAi);
text_roundtrip_test!(text_rt_codex_openai, Dialect::Codex, Dialect::OpenAi);
text_roundtrip_test!(text_rt_kimi_openai, Dialect::Kimi, Dialect::OpenAi);
text_roundtrip_test!(text_rt_copilot_openai, Dialect::Copilot, Dialect::OpenAi);
text_roundtrip_test!(text_rt_kimi_gemini, Dialect::Kimi, Dialect::Gemini);

// ═══════════════════════════════════════════════════════════════════════════
// Section 2: Tool call roundtrip — pairs that support tools
// ═══════════════════════════════════════════════════════════════════════════

macro_rules! tool_roundtrip_test {
    ($name:ident, $from:expr, $to:expr) => {
        #[test]
        fn $name() {
            let caps_from = dialect_capabilities($from);
            let caps_to = dialect_capabilities($to);
            if !caps_from.tool_use.is_native() || !caps_to.tool_use.is_native() {
                // Skip pairs where either side lacks native tool support.
                // Codex has no tool support so these pairs are lossy by design.
                return;
            }
            let orig = with_tools();
            let (mid, back) = roundtrip($from, $to, &orig).unwrap();
            assert_eq!(
                count_tool_uses(&orig),
                count_tool_uses(&mid),
                "tool use count changed in forward map"
            );
            assert_eq!(
                count_tool_uses(&orig),
                count_tool_uses(&back),
                "tool use count changed in roundtrip"
            );
            // Verify tool input JSON is preserved
            let orig_tools = orig.tool_calls();
            let back_tools = back.tool_calls();
            for (ot, bt) in orig_tools.iter().zip(back_tools.iter()) {
                if let (
                    IrContentBlock::ToolUse { input: oi, .. },
                    IrContentBlock::ToolUse { input: bi, .. },
                ) = (ot, bt)
                {
                    assert_eq!(oi, bi, "tool input JSON diverged");
                }
            }
        }
    };
}

tool_roundtrip_test!(tool_rt_openai_claude, Dialect::OpenAi, Dialect::Claude);
tool_roundtrip_test!(tool_rt_openai_gemini, Dialect::OpenAi, Dialect::Gemini);
tool_roundtrip_test!(tool_rt_openai_kimi, Dialect::OpenAi, Dialect::Kimi);
tool_roundtrip_test!(tool_rt_openai_copilot, Dialect::OpenAi, Dialect::Copilot);
tool_roundtrip_test!(tool_rt_claude_gemini, Dialect::Claude, Dialect::Gemini);
tool_roundtrip_test!(tool_rt_claude_kimi, Dialect::Claude, Dialect::Kimi);
tool_roundtrip_test!(tool_rt_gemini_kimi, Dialect::Gemini, Dialect::Kimi);
tool_roundtrip_test!(tool_rt_openai_codex, Dialect::OpenAi, Dialect::Codex);
tool_roundtrip_test!(tool_rt_claude_codex, Dialect::Claude, Dialect::Codex);

// ═══════════════════════════════════════════════════════════════════════════
// Section 3: Streaming event mapping (response path roundtrip)
// ═══════════════════════════════════════════════════════════════════════════

macro_rules! stream_response_test {
    ($name:ident, $from:expr, $to:expr) => {
        #[test]
        fn $name() {
            let mapper = default_ir_mapper($from, $to).expect("mapper must exist");
            let orig = simple_text();
            let mid = mapper.map_response($from, $to, &orig).unwrap();
            let back_mapper = default_ir_mapper($to, $from).expect("reverse mapper must exist");
            let back = back_mapper.map_response($to, $from, &mid).unwrap();
            assert_eq!(
                orig.len(),
                back.len(),
                "response roundtrip message count diverged"
            );
            assert_text_preserved(&orig, &back);
        }
    };
}

stream_response_test!(stream_rt_openai_claude, Dialect::OpenAi, Dialect::Claude);
stream_response_test!(stream_rt_openai_gemini, Dialect::OpenAi, Dialect::Gemini);
stream_response_test!(stream_rt_openai_kimi, Dialect::OpenAi, Dialect::Kimi);
stream_response_test!(stream_rt_openai_copilot, Dialect::OpenAi, Dialect::Copilot);
stream_response_test!(stream_rt_claude_gemini, Dialect::Claude, Dialect::Gemini);
stream_response_test!(stream_rt_claude_kimi, Dialect::Claude, Dialect::Kimi);
stream_response_test!(stream_rt_openai_codex, Dialect::OpenAi, Dialect::Codex);
stream_response_test!(stream_rt_gemini_kimi, Dialect::Gemini, Dialect::Kimi);
stream_response_test!(stream_rt_codex_claude, Dialect::Codex, Dialect::Claude);

// ═══════════════════════════════════════════════════════════════════════════
// Section 4: Error propagation — is_error flag on tool results
// ═══════════════════════════════════════════════════════════════════════════

macro_rules! error_propagation_test {
    ($name:ident, $from:expr, $to:expr) => {
        #[test]
        fn $name() {
            let caps_to = dialect_capabilities($to);
            if !caps_to.tool_use.is_native() {
                return; // target doesn't support tools, skip
            }
            let orig = error_tool_result();
            let mapper = default_ir_mapper($from, $to).expect("mapper");
            let mid = mapper.map_request($from, $to, &orig).unwrap();
            assert!(
                has_error_tool_result(&mid),
                "is_error flag lost in {from:?} -> {to:?} mapping",
                from = $from,
                to = $to,
            );
        }
    };
}

error_propagation_test!(err_prop_openai_claude, Dialect::OpenAi, Dialect::Claude);
error_propagation_test!(err_prop_openai_gemini, Dialect::OpenAi, Dialect::Gemini);
error_propagation_test!(err_prop_openai_kimi, Dialect::OpenAi, Dialect::Kimi);
error_propagation_test!(err_prop_openai_copilot, Dialect::OpenAi, Dialect::Copilot);
error_propagation_test!(err_prop_claude_gemini, Dialect::Claude, Dialect::Gemini);
error_propagation_test!(err_prop_claude_kimi, Dialect::Claude, Dialect::Kimi);
error_propagation_test!(err_prop_gemini_kimi, Dialect::Gemini, Dialect::Kimi);
error_propagation_test!(err_prop_claude_openai, Dialect::Claude, Dialect::OpenAi);
error_propagation_test!(err_prop_gemini_openai, Dialect::Gemini, Dialect::OpenAi);

// ═══════════════════════════════════════════════════════════════════════════
// Section 5: Feature matrix verification
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn feature_matrix_openai() {
    let caps = dialect_capabilities(Dialect::OpenAi);
    assert_eq!(caps.system_prompt, Support::Native);
    assert_eq!(caps.thinking, Support::None);
    assert_eq!(caps.images, Support::Native);
    assert_eq!(caps.tool_use, Support::Native);
    assert_eq!(caps.tool_role, Support::Native);
    assert_eq!(caps.streaming, Support::Native);
}

#[test]
fn feature_matrix_claude() {
    let caps = dialect_capabilities(Dialect::Claude);
    assert_eq!(caps.system_prompt, Support::Native);
    assert_eq!(caps.thinking, Support::Native);
    assert_eq!(caps.images, Support::Native);
    assert_eq!(caps.tool_use, Support::Native);
    assert_eq!(caps.tool_role, Support::None); // Claude uses User+ToolResult blocks
    assert_eq!(caps.streaming, Support::Native);
}

#[test]
fn feature_matrix_gemini() {
    let caps = dialect_capabilities(Dialect::Gemini);
    assert_eq!(caps.system_prompt, Support::Native);
    assert_eq!(caps.thinking, Support::None);
    assert_eq!(caps.images, Support::Native);
    assert_eq!(caps.tool_use, Support::Native);
    assert_eq!(caps.tool_role, Support::None); // Gemini uses functionResponse in user turns
    assert_eq!(caps.streaming, Support::Native);
}

#[test]
fn feature_matrix_codex() {
    let caps = dialect_capabilities(Dialect::Codex);
    assert_eq!(caps.system_prompt, Support::None);
    assert_eq!(caps.thinking, Support::None);
    assert_eq!(caps.images, Support::None);
    assert_eq!(caps.tool_use, Support::None);
    assert_eq!(caps.tool_role, Support::None);
    assert_eq!(caps.streaming, Support::None);
}

#[test]
fn feature_matrix_kimi() {
    let caps = dialect_capabilities(Dialect::Kimi);
    assert_eq!(caps.system_prompt, Support::Native);
    assert_eq!(caps.thinking, Support::None);
    assert_eq!(caps.images, Support::None);
    assert_eq!(caps.tool_use, Support::Native);
    assert_eq!(caps.tool_role, Support::Native); // OpenAI-compatible
    assert_eq!(caps.streaming, Support::Native);
}

#[test]
fn feature_matrix_copilot() {
    let caps = dialect_capabilities(Dialect::Copilot);
    assert_eq!(caps.system_prompt, Support::Native);
    assert_eq!(caps.thinking, Support::None);
    assert_eq!(caps.images, Support::None);
    assert_eq!(caps.tool_use, Support::Native);
    assert_eq!(caps.tool_role, Support::Native); // OpenAI-compatible
    assert_eq!(caps.streaming, Support::Native);
}

#[test]
fn feature_matrix_all_dialects_covered() {
    for &d in Dialect::all() {
        let caps = dialect_capabilities(d);
        assert_eq!(caps.dialect, d, "capability struct has wrong dialect tag");
    }
}

#[test]
fn feature_matrix_only_claude_has_thinking() {
    for &d in Dialect::all() {
        let caps = dialect_capabilities(d);
        if d == Dialect::Claude {
            assert!(caps.thinking.is_native(), "Claude should support thinking");
        } else {
            assert!(
                !caps.thinking.is_native(),
                "{d:?} should NOT support thinking"
            );
        }
    }
}

#[test]
fn feature_matrix_image_support_subset() {
    // Only OpenAI, Claude, Gemini support images natively
    let image_sdks = [Dialect::OpenAi, Dialect::Claude, Dialect::Gemini];
    for &d in Dialect::all() {
        let caps = dialect_capabilities(d);
        if image_sdks.contains(&d) {
            assert!(caps.images.is_native(), "{d:?} should support images");
        } else {
            assert!(!caps.images.is_native(), "{d:?} should NOT support images");
        }
    }
}

#[test]
fn feature_matrix_no_sdk_supports_system_images() {
    for &d in Dialect::all() {
        let caps = dialect_capabilities(d);
        assert!(
            !caps.system_images.is_native(),
            "{d:?} should NOT support system images"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 6: Loss detection — what's lost in each translation direction
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn loss_thinking_openai_drops_thinking() {
    let orig = thinking_conv();
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();
    let mapped = mapper
        .map_request(Dialect::Claude, Dialect::OpenAi, &orig)
        .unwrap();
    // OpenAI does not support thinking blocks; they should be stripped or
    // collapsed. The text answer must survive.
    assert!(
        !mapped.is_empty(),
        "mapped conversation should not be empty"
    );
    let text = mapped
        .messages
        .iter()
        .map(|m| m.text_content())
        .collect::<Vec<_>>()
        .join("");
    assert!(
        text.contains("42"),
        "text answer should survive thinking strip"
    );
}

#[test]
fn loss_thinking_gemini_drops_thinking() {
    let orig = thinking_conv();
    let mapper = default_ir_mapper(Dialect::Claude, Dialect::Gemini).unwrap();
    let mapped = mapper
        .map_request(Dialect::Claude, Dialect::Gemini, &orig)
        .unwrap();
    assert!(!mapped.is_empty());
    let text = mapped
        .messages
        .iter()
        .map(|m| m.text_content())
        .collect::<Vec<_>>()
        .join("");
    assert!(text.contains("42"), "text answer lost in Gemini mapping");
}

#[test]
fn loss_images_kimi_cannot_carry_images() {
    let caps = dialect_capabilities(Dialect::Kimi);
    assert!(!caps.images.is_native(), "Kimi should not support images");
}

#[test]
fn loss_images_copilot_cannot_carry_images() {
    let caps = dialect_capabilities(Dialect::Copilot);
    assert!(
        !caps.images.is_native(),
        "Copilot should not support images"
    );
}

#[test]
fn loss_images_codex_cannot_carry_images() {
    let caps = dialect_capabilities(Dialect::Codex);
    assert!(!caps.images.is_native(), "Codex should not support images");
}

#[test]
fn loss_codex_has_no_system_prompt() {
    let caps = dialect_capabilities(Dialect::Codex);
    assert!(
        !caps.system_prompt.is_native(),
        "Codex should not support system prompts"
    );
}

#[test]
fn loss_codex_has_no_tool_use() {
    let caps = dialect_capabilities(Dialect::Codex);
    assert!(
        !caps.tool_use.is_native(),
        "Codex should not support tool use"
    );
}

#[test]
fn loss_tool_role_claude_uses_user_role() {
    // Claude does not have a dedicated "tool" role — tool results are sent
    // as User messages with ToolResult content blocks.
    let caps = dialect_capabilities(Dialect::Claude);
    assert!(!caps.tool_role.is_native());
}

#[test]
fn loss_tool_role_gemini_uses_user_role() {
    let caps = dialect_capabilities(Dialect::Gemini);
    assert!(!caps.tool_role.is_native());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 7: Metadata preservation
// ═══════════════════════════════════════════════════════════════════════════

macro_rules! metadata_roundtrip_test {
    ($name:ident, $from:expr, $to:expr) => {
        #[test]
        fn $name() {
            let orig = metadata_conv();
            let (_, back) = roundtrip($from, $to, &orig).unwrap();
            // Metadata stored in IrMessage.metadata should survive roundtrip
            // through the IR layer (mappers pass it through unmodified).
            assert_eq!(back.len(), orig.len(), "message count diverged");
            let orig_meta = &orig.messages[0].metadata;
            let back_meta = &back.messages[0].metadata;
            for (key, value) in orig_meta {
                assert_eq!(
                    back_meta.get(key),
                    Some(value),
                    "metadata key `{key}` lost in {from:?} -> {to:?} roundtrip",
                    from = $from,
                    to = $to,
                );
            }
        }
    };
}

metadata_roundtrip_test!(meta_rt_openai_claude, Dialect::OpenAi, Dialect::Claude);
metadata_roundtrip_test!(meta_rt_openai_gemini, Dialect::OpenAi, Dialect::Gemini);
metadata_roundtrip_test!(meta_rt_openai_kimi, Dialect::OpenAi, Dialect::Kimi);
metadata_roundtrip_test!(meta_rt_openai_copilot, Dialect::OpenAi, Dialect::Copilot);
metadata_roundtrip_test!(meta_rt_claude_gemini, Dialect::Claude, Dialect::Gemini);
metadata_roundtrip_test!(meta_rt_openai_codex, Dialect::OpenAi, Dialect::Codex);

// ═══════════════════════════════════════════════════════════════════════════
// Section 8: Role mapping correctness
// ═══════════════════════════════════════════════════════════════════════════

macro_rules! role_mapping_test {
    ($name:ident, $from:expr, $to:expr) => {
        #[test]
        fn $name() {
            let orig = with_system();
            let mapper = default_ir_mapper($from, $to).expect("mapper");
            let mapped = mapper.map_request($from, $to, &orig).unwrap();

            // System role: should survive if target supports it
            let target_caps = dialect_capabilities($to);
            if target_caps.system_prompt.is_native() {
                assert!(
                    has_role(&mapped, IrRole::System),
                    "system role lost in {from:?} -> {to:?}",
                    from = $from,
                    to = $to,
                );
            }
            // User role must always survive
            assert!(
                has_role(&mapped, IrRole::User),
                "user role lost in {from:?} -> {to:?}",
                from = $from,
                to = $to,
            );
            // Assistant role must always survive
            assert!(
                has_role(&mapped, IrRole::Assistant),
                "assistant role lost in {from:?} -> {to:?}",
                from = $from,
                to = $to,
            );
        }
    };
}

role_mapping_test!(role_map_openai_claude, Dialect::OpenAi, Dialect::Claude);
role_mapping_test!(role_map_openai_gemini, Dialect::OpenAi, Dialect::Gemini);
role_mapping_test!(role_map_openai_kimi, Dialect::OpenAi, Dialect::Kimi);
role_mapping_test!(role_map_openai_copilot, Dialect::OpenAi, Dialect::Copilot);
role_mapping_test!(role_map_claude_openai, Dialect::Claude, Dialect::OpenAi);
role_mapping_test!(role_map_claude_gemini, Dialect::Claude, Dialect::Gemini);
role_mapping_test!(role_map_claude_kimi, Dialect::Claude, Dialect::Kimi);
role_mapping_test!(role_map_gemini_openai, Dialect::Gemini, Dialect::OpenAi);
role_mapping_test!(role_map_gemini_kimi, Dialect::Gemini, Dialect::Kimi);
role_mapping_test!(role_map_kimi_openai, Dialect::Kimi, Dialect::OpenAi);
role_mapping_test!(role_map_codex_openai, Dialect::Codex, Dialect::OpenAi);
role_mapping_test!(role_map_codex_claude, Dialect::Codex, Dialect::Claude);
role_mapping_test!(role_map_copilot_openai, Dialect::Copilot, Dialect::OpenAi);

// ═══════════════════════════════════════════════════════════════════════════
// Section 9: Multi-tool parity across pairs
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn multi_tool_openai_claude_both_tools_survive() {
    let orig = multi_tool();
    let (mid, _) = roundtrip(Dialect::OpenAi, Dialect::Claude, &orig).unwrap();
    assert_eq!(count_tool_uses(&mid), 2, "expected both tool calls");
}

#[test]
fn multi_tool_openai_gemini_both_tools_survive() {
    let orig = multi_tool();
    let (mid, _) = roundtrip(Dialect::OpenAi, Dialect::Gemini, &orig).unwrap();
    assert_eq!(count_tool_uses(&mid), 2);
}

#[test]
fn multi_tool_claude_gemini_both_tools_survive() {
    let orig = multi_tool();
    let (mid, _) = roundtrip(Dialect::Claude, Dialect::Gemini, &orig).unwrap();
    assert_eq!(count_tool_uses(&mid), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 10: Factory coverage — every supported_ir_pairs entry works
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn factory_all_supported_pairs_resolve() {
    let pairs = supported_ir_pairs();
    assert!(
        pairs.len() >= 24,
        "expected at least 24 supported pairs (6 identity + 18 cross)"
    );
    for (from, to) in &pairs {
        let mapper = default_ir_mapper(*from, *to);
        assert!(
            mapper.is_some(),
            "no mapper for {from:?} -> {to:?}",
            from = from,
            to = to,
        );
    }
}

#[test]
fn factory_identity_pairs_for_all_dialects() {
    for &d in Dialect::all() {
        let mapper = default_ir_mapper(d, d).expect("identity mapper must exist");
        let orig = simple_text();
        let mapped = mapper.map_request(d, d, &orig).unwrap();
        assert_eq!(orig.len(), mapped.len());
        assert_text_preserved(&orig, &mapped);
    }
}

#[test]
fn factory_unsupported_pair_returns_none() {
    // Codex ↔ Copilot has no direct mapper
    let m = default_ir_mapper(Dialect::Codex, Dialect::Copilot);
    assert!(m.is_none(), "Codex->Copilot should have no direct mapper");
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 11: Multi-turn conversation fidelity
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn multi_turn_openai_claude_preserves_turn_count() {
    let orig = multi_turn();
    let (_, back) = roundtrip(Dialect::OpenAi, Dialect::Claude, &orig).unwrap();
    assert_eq!(orig.len(), back.len(), "turn count changed");
    assert_text_preserved(&orig, &back);
}

#[test]
fn multi_turn_openai_gemini_preserves_turn_count() {
    let orig = multi_turn();
    let (_, back) = roundtrip(Dialect::OpenAi, Dialect::Gemini, &orig).unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

#[test]
fn multi_turn_claude_kimi_preserves_turn_count() {
    let orig = multi_turn();
    let (_, back) = roundtrip(Dialect::Claude, Dialect::Kimi, &orig).unwrap();
    assert_eq!(orig.len(), back.len());
    assert_text_preserved(&orig, &back);
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 12: All-roles conversation across pairs
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn all_roles_openai_claude_roundtrip() {
    let orig = all_roles_conv();
    let caps = dialect_capabilities(Dialect::Claude);
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
    let mid = mapper
        .map_request(Dialect::OpenAi, Dialect::Claude, &orig)
        .unwrap();
    // System, User, Assistant must survive
    assert!(has_role(&mid, IrRole::System));
    assert!(has_role(&mid, IrRole::User));
    assert!(has_role(&mid, IrRole::Assistant));
    // Claude doesn't have tool_role, so Tool role may become User
    if !caps.tool_role.is_native() {
        // Either Tool role is preserved in IR or remapped — both are valid
        assert!(mid.len() >= 4, "messages should not be lost");
    }
}

#[test]
fn all_roles_openai_gemini_roundtrip() {
    let orig = all_roles_conv();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Gemini).unwrap();
    let mid = mapper
        .map_request(Dialect::OpenAi, Dialect::Gemini, &orig)
        .unwrap();
    assert!(has_role(&mid, IrRole::System));
    assert!(has_role(&mid, IrRole::User));
    assert!(has_role(&mid, IrRole::Assistant));
}

#[test]
fn all_roles_openai_kimi_roundtrip() {
    let orig = all_roles_conv();
    let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Kimi).unwrap();
    let mid = mapper
        .map_request(Dialect::OpenAi, Dialect::Kimi, &orig)
        .unwrap();
    assert!(has_role(&mid, IrRole::System));
    assert!(has_role(&mid, IrRole::User));
    assert!(has_role(&mid, IrRole::Assistant));
    // Kimi has tool_role, so Tool role should survive
    let caps = dialect_capabilities(Dialect::Kimi);
    if caps.tool_role.is_native() {
        assert!(has_role(&mid, IrRole::Tool));
    }
}
