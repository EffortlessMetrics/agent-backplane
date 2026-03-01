// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for IR lowering roundtrips across all SDK crates.

use proptest::prelude::*;
use serde_json::json;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};

// ── Strategies ──────────────────────────────────────────────────────────

fn arb_ir_role() -> BoxedStrategy<IrRole> {
    prop_oneof![
        Just(IrRole::System),
        Just(IrRole::User),
        Just(IrRole::Assistant),
        Just(IrRole::Tool),
    ]
    .boxed()
}

/// Roles excluding System — for SDKs that strip system messages in `from_ir`.
fn arb_non_system_role() -> BoxedStrategy<IrRole> {
    prop_oneof![Just(IrRole::User), Just(IrRole::Assistant),].boxed()
}

/// Non-empty text that will not accidentally parse as a JSON array.
fn arb_safe_text() -> BoxedStrategy<String> {
    "[a-zA-Z][a-zA-Z0-9 _.,!?'-]{0,99}".boxed()
}

fn arb_text_block() -> BoxedStrategy<IrContentBlock> {
    arb_safe_text()
        .prop_map(|text| IrContentBlock::Text { text })
        .boxed()
}

fn arb_tool_name() -> BoxedStrategy<String> {
    "[a-z][a-z0-9_]{0,19}".boxed()
}

fn arb_tool_id() -> BoxedStrategy<String> {
    "[a-zA-Z][a-zA-Z0-9_]{4,15}".boxed()
}

fn arb_text_message(role_strategy: BoxedStrategy<IrRole>) -> BoxedStrategy<IrMessage> {
    (role_strategy, arb_text_block())
        .prop_map(|(role, block)| IrMessage::new(role, vec![block]))
        .boxed()
}

fn arb_text_conversation(
    role_strategy: BoxedStrategy<IrRole>,
    max_msgs: usize,
) -> BoxedStrategy<IrConversation> {
    prop::collection::vec(arb_text_message(role_strategy), 1..=max_msgs)
        .prop_map(IrConversation::from_messages)
        .boxed()
}

/// Config tuned for CI speed.
fn fast_config() -> ProptestConfig {
    ProptestConfig {
        cases: 64,
        ..ProptestConfig::default()
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Extract all text content from an IrConversation, concatenated per message.
fn text_contents(conv: &IrConversation) -> Vec<String> {
    conv.messages.iter().map(|m| m.text_content()).collect()
}

/// Extract roles from a conversation.
fn roles(conv: &IrConversation) -> Vec<IrRole> {
    conv.messages.iter().map(|m| m.role).collect()
}

// ═══════════════════════════════════════════════════════════════════════
// §1  Roundtrip invariants — per SDK
// ═══════════════════════════════════════════════════════════════════════

// ── OpenAI ──────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn openai_text_message_count_preserved(
        conv in arb_text_conversation(arb_ir_role(), 8)
    ) {
        let sdk = abp_openai_sdk::lowering::from_ir(&conv);
        let rt = abp_openai_sdk::lowering::to_ir(&sdk);
        prop_assert_eq!(conv.len(), rt.len());
    }

    #[test]
    fn openai_text_content_preserved(
        conv in arb_text_conversation(arb_ir_role(), 8)
    ) {
        let sdk = abp_openai_sdk::lowering::from_ir(&conv);
        let rt = abp_openai_sdk::lowering::to_ir(&sdk);
        prop_assert_eq!(text_contents(&conv), text_contents(&rt));
    }

    #[test]
    fn openai_roles_preserved(
        conv in arb_text_conversation(arb_ir_role(), 8)
    ) {
        let sdk = abp_openai_sdk::lowering::from_ir(&conv);
        let rt = abp_openai_sdk::lowering::to_ir(&sdk);
        prop_assert_eq!(roles(&conv), roles(&rt));
    }
}

// ── Claude ──────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn claude_text_roundtrip_with_system(
        conv in arb_text_conversation(arb_ir_role(), 8)
    ) {
        let sys_prompt = abp_claude_sdk::lowering::extract_system_prompt(&conv);
        let sdk = abp_claude_sdk::lowering::from_ir(&conv);
        let rt = abp_claude_sdk::lowering::to_ir(&sdk, sys_prompt.as_deref());
        // Non-system text content must be equal
        let orig_non_sys: Vec<String> = conv.messages.iter()
            .filter(|m| m.role != IrRole::System)
            .map(|m| m.text_content())
            .collect();
        let rt_non_sys: Vec<String> = rt.messages.iter()
            .filter(|m| m.role != IrRole::System)
            .map(|m| m.text_content())
            .collect();
        prop_assert_eq!(orig_non_sys, rt_non_sys);
    }

    #[test]
    fn claude_system_prompt_preserved(
        conv in arb_text_conversation(arb_ir_role(), 8)
    ) {
        let sys_prompt = abp_claude_sdk::lowering::extract_system_prompt(&conv);
        let sdk = abp_claude_sdk::lowering::from_ir(&conv);
        let rt = abp_claude_sdk::lowering::to_ir(&sdk, sys_prompt.as_deref());
        let rt_sys = abp_claude_sdk::lowering::extract_system_prompt(&rt);
        prop_assert_eq!(sys_prompt, rt_sys);
    }
}

// ── Gemini ──────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn gemini_text_message_count_preserved(
        conv in arb_text_conversation(arb_non_system_role(), 8)
    ) {
        let sdk = abp_gemini_sdk::lowering::from_ir(&conv);
        let rt = abp_gemini_sdk::lowering::to_ir(&sdk, None);
        prop_assert_eq!(conv.len(), rt.len());
    }

    #[test]
    fn gemini_text_content_preserved(
        conv in arb_text_conversation(arb_non_system_role(), 8)
    ) {
        let sdk = abp_gemini_sdk::lowering::from_ir(&conv);
        let rt = abp_gemini_sdk::lowering::to_ir(&sdk, None);
        prop_assert_eq!(text_contents(&conv), text_contents(&rt));
    }
}

// ── Codex ───────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn codex_assistant_text_roundtrip(
        conv in arb_text_conversation(Just(IrRole::Assistant).boxed(), 8)
    ) {
        let sdk = abp_codex_sdk::lowering::from_ir(&conv);
        let rt = abp_codex_sdk::lowering::to_ir(&sdk);
        prop_assert_eq!(text_contents(&conv), text_contents(&rt));
    }

    /// Codex `from_ir` only emits items for assistant messages (text-only).
    /// System, user, and tool-role text messages produce no output items.
    #[test]
    fn codex_skips_non_assistant_text(
        conv in arb_text_conversation(arb_ir_role(), 8)
    ) {
        let sdk = abp_codex_sdk::lowering::from_ir(&conv);
        let rt = abp_codex_sdk::lowering::to_ir(&sdk);
        let expected_count = conv.messages.iter()
            .filter(|m| m.role == IrRole::Assistant)
            .count();
        prop_assert_eq!(expected_count, rt.len());
    }
}

// ── Kimi ────────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn kimi_text_message_count_preserved(
        conv in arb_text_conversation(arb_ir_role(), 8)
    ) {
        let sdk = abp_kimi_sdk::lowering::from_ir(&conv);
        let rt = abp_kimi_sdk::lowering::to_ir(&sdk);
        prop_assert_eq!(conv.len(), rt.len());
    }

    #[test]
    fn kimi_text_content_preserved(
        conv in arb_text_conversation(arb_ir_role(), 8)
    ) {
        let sdk = abp_kimi_sdk::lowering::from_ir(&conv);
        let rt = abp_kimi_sdk::lowering::to_ir(&sdk);
        prop_assert_eq!(text_contents(&conv), text_contents(&rt));
    }

    #[test]
    fn kimi_roles_preserved(
        conv in arb_text_conversation(arb_ir_role(), 8)
    ) {
        let sdk = abp_kimi_sdk::lowering::from_ir(&conv);
        let rt = abp_kimi_sdk::lowering::to_ir(&sdk);
        prop_assert_eq!(roles(&conv), roles(&rt));
    }
}

// ── Copilot ─────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(fast_config())]

    #[test]
    fn copilot_text_message_count_preserved(
        conv in arb_text_conversation(arb_non_system_role(), 8)
    ) {
        let sdk = abp_copilot_sdk::lowering::from_ir(&conv);
        let rt = abp_copilot_sdk::lowering::to_ir(&sdk);
        prop_assert_eq!(conv.len(), rt.len());
    }

    #[test]
    fn copilot_text_content_preserved(
        conv in arb_text_conversation(arb_non_system_role(), 8)
    ) {
        let sdk = abp_copilot_sdk::lowering::from_ir(&conv);
        let rt = abp_copilot_sdk::lowering::to_ir(&sdk);
        prop_assert_eq!(text_contents(&conv), text_contents(&rt));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §2  Cross-SDK parity
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// OpenAI and Kimi share identical wire formats — lowering must agree.
    #[test]
    fn cross_sdk_openai_kimi_text_parity(
        conv in arb_text_conversation(arb_ir_role(), 8)
    ) {
        let openai_rt = {
            let sdk = abp_openai_sdk::lowering::from_ir(&conv);
            abp_openai_sdk::lowering::to_ir(&sdk)
        };
        let kimi_rt = {
            let sdk = abp_kimi_sdk::lowering::from_ir(&conv);
            abp_kimi_sdk::lowering::to_ir(&sdk)
        };
        prop_assert_eq!(text_contents(&openai_rt), text_contents(&kimi_rt));
        prop_assert_eq!(roles(&openai_rt), roles(&kimi_rt));
    }

    /// All SDKs that preserve non-system messages must extract the same text.
    #[test]
    fn cross_sdk_non_system_text_parity(
        conv in arb_text_conversation(arb_non_system_role(), 6)
    ) {
        let openai_texts = {
            let sdk = abp_openai_sdk::lowering::from_ir(&conv);
            text_contents(&abp_openai_sdk::lowering::to_ir(&sdk))
        };
        let gemini_texts = {
            let sdk = abp_gemini_sdk::lowering::from_ir(&conv);
            text_contents(&abp_gemini_sdk::lowering::to_ir(&sdk, None))
        };
        let copilot_texts = {
            let sdk = abp_copilot_sdk::lowering::from_ir(&conv);
            text_contents(&abp_copilot_sdk::lowering::to_ir(&sdk))
        };
        prop_assert_eq!(&openai_texts, &gemini_texts);
        prop_assert_eq!(&openai_texts, &copilot_texts);
    }

    /// Tool names survive roundtrip through both OpenAI and Kimi identically.
    #[test]
    fn cross_sdk_tool_name_parity(
        name in arb_tool_name(),
        id in arb_tool_id(),
    ) {
        let conv = IrConversation::from_messages(vec![
            IrMessage::new(IrRole::Assistant, vec![IrContentBlock::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: json!({"key": "val"}),
            }]),
        ]);
        let openai_rt = {
            let sdk = abp_openai_sdk::lowering::from_ir(&conv);
            abp_openai_sdk::lowering::to_ir(&sdk)
        };
        let kimi_rt = {
            let sdk = abp_kimi_sdk::lowering::from_ir(&conv);
            abp_kimi_sdk::lowering::to_ir(&sdk)
        };
        // Both must recover the same tool name
        let openai_names: Vec<&str> = openai_rt.tool_calls().iter().filter_map(|b| match b {
            IrContentBlock::ToolUse { name, .. } => Some(name.as_str()),
            _ => None,
        }).collect();
        let kimi_names: Vec<&str> = kimi_rt.tool_calls().iter().filter_map(|b| match b {
            IrContentBlock::ToolUse { name, .. } => Some(name.as_str()),
            _ => None,
        }).collect();
        prop_assert_eq!(&openai_names, &kimi_names);
        prop_assert_eq!(openai_names, vec![name.as_str()]);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §3  Edge cases
// ═══════════════════════════════════════════════════════════════════════

/// Empty conversations roundtrip through every SDK without error.
#[test]
fn edge_empty_conversation_all_sdks() {
    let empty = IrConversation::new();

    let oai = abp_openai_sdk::lowering::from_ir(&empty);
    assert!(abp_openai_sdk::lowering::to_ir(&oai).is_empty());

    let claude = abp_claude_sdk::lowering::from_ir(&empty);
    assert!(abp_claude_sdk::lowering::to_ir(&claude, None).is_empty());

    let gemini = abp_gemini_sdk::lowering::from_ir(&empty);
    assert!(abp_gemini_sdk::lowering::to_ir(&gemini, None).is_empty());

    let codex = abp_codex_sdk::lowering::from_ir(&empty);
    assert!(abp_codex_sdk::lowering::to_ir(&codex).is_empty());

    let kimi = abp_kimi_sdk::lowering::from_ir(&empty);
    assert!(abp_kimi_sdk::lowering::to_ir(&kimi).is_empty());

    let copilot = abp_copilot_sdk::lowering::from_ir(&empty);
    assert!(abp_copilot_sdk::lowering::to_ir(&copilot).is_empty());
}

proptest! {
    #![proptest_config(fast_config())]

    /// Very long text messages survive OpenAI roundtrip.
    #[test]
    fn edge_long_text_roundtrip(
        text in "[a-zA-Z0-9 ]{500,2000}"
    ) {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, &text),
        ]);
        let sdk = abp_openai_sdk::lowering::from_ir(&conv);
        let rt = abp_openai_sdk::lowering::to_ir(&sdk);
        prop_assert_eq!(rt.messages[0].text_content(), text);
    }

    /// Unicode and special characters survive roundtrip.
    #[test]
    fn edge_special_characters_roundtrip(
        text in prop::string::string_regex("[a-zA-Z0-9àéîöü∑πΩ★♦♠♣ ]{1,100}")
            .unwrap()
    ) {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, &text),
        ]);
        // OpenAI
        let oai_rt = {
            let sdk = abp_openai_sdk::lowering::from_ir(&conv);
            abp_openai_sdk::lowering::to_ir(&sdk)
        };
        prop_assert_eq!(oai_rt.messages[0].text_content(), text.clone());
        // Kimi
        let kimi_rt = {
            let sdk = abp_kimi_sdk::lowering::from_ir(&conv);
            abp_kimi_sdk::lowering::to_ir(&sdk)
        };
        prop_assert_eq!(kimi_rt.messages[0].text_content(), text);
    }

    /// Single-message conversations roundtrip correctly through every SDK.
    #[test]
    fn edge_single_message_roundtrip(
        text in arb_safe_text()
    ) {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, &text),
        ]);
        // OpenAI
        let oai = abp_openai_sdk::lowering::from_ir(&conv);
        let oai_rt = abp_openai_sdk::lowering::to_ir(&oai);
        prop_assert_eq!(oai_rt.messages[0].text_content(), text.clone());
        // Gemini
        let gem = abp_gemini_sdk::lowering::from_ir(&conv);
        let gem_rt = abp_gemini_sdk::lowering::to_ir(&gem, None);
        prop_assert_eq!(gem_rt.messages[0].text_content(), text.clone());
        // Copilot
        let cop = abp_copilot_sdk::lowering::from_ir(&conv);
        let cop_rt = abp_copilot_sdk::lowering::to_ir(&cop);
        prop_assert_eq!(cop_rt.messages[0].text_content(), text);
    }

    /// Nested tool results (ToolResult containing text) survive roundtrip.
    #[test]
    fn edge_nested_tool_result_roundtrip(
        tool_id in arb_tool_id(),
        result_text in arb_safe_text(),
    ) {
        let conv = IrConversation::from_messages(vec![
            IrMessage::new(IrRole::Tool, vec![IrContentBlock::ToolResult {
                tool_use_id: tool_id.clone(),
                content: vec![IrContentBlock::Text { text: result_text.clone() }],
                is_error: false,
            }]),
        ]);
        // OpenAI roundtrip
        let oai = abp_openai_sdk::lowering::from_ir(&conv);
        let oai_rt = abp_openai_sdk::lowering::to_ir(&oai);
        match &oai_rt.messages[0].content[0] {
            IrContentBlock::ToolResult { tool_use_id, content, .. } => {
                prop_assert_eq!(tool_use_id, &tool_id);
                prop_assert_eq!(content[0].clone(), IrContentBlock::Text { text: result_text.clone() });
            }
            other => prop_assert!(false, "expected ToolResult, got {:?}", other),
        }
        // Kimi roundtrip
        let kimi = abp_kimi_sdk::lowering::from_ir(&conv);
        let kimi_rt = abp_kimi_sdk::lowering::to_ir(&kimi);
        match &kimi_rt.messages[0].content[0] {
            IrContentBlock::ToolResult { tool_use_id, content, .. } => {
                prop_assert_eq!(tool_use_id, &tool_id);
                prop_assert_eq!(content[0].clone(), IrContentBlock::Text { text: result_text });
            }
            other => prop_assert!(false, "expected ToolResult, got {:?}", other),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §4  Content preservation
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Tool names survive OpenAI roundtrip.
    #[test]
    fn content_openai_tool_name_survives(
        name in arb_tool_name(),
        id in arb_tool_id(),
    ) {
        let conv = IrConversation::from_messages(vec![
            IrMessage::new(IrRole::Assistant, vec![IrContentBlock::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: json!({"a": 1}),
            }]),
        ]);
        let sdk = abp_openai_sdk::lowering::from_ir(&conv);
        let rt = abp_openai_sdk::lowering::to_ir(&sdk);
        match &rt.messages[0].content[0] {
            IrContentBlock::ToolUse { id: rt_id, name: rt_name, input } => {
                prop_assert_eq!(rt_id, &id);
                prop_assert_eq!(rt_name, &name);
                prop_assert_eq!(input, &json!({"a": 1}));
            }
            other => prop_assert!(false, "expected ToolUse, got {:?}", other),
        }
    }

    /// Tool names survive Codex roundtrip.
    #[test]
    fn content_codex_tool_name_survives(
        name in arb_tool_name(),
        id in arb_tool_id(),
    ) {
        let conv = IrConversation::from_messages(vec![
            IrMessage::new(IrRole::Assistant, vec![IrContentBlock::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: json!({"b": 2}),
            }]),
        ]);
        let sdk = abp_codex_sdk::lowering::from_ir(&conv);
        let rt = abp_codex_sdk::lowering::to_ir(&sdk);
        match &rt.messages[0].content[0] {
            IrContentBlock::ToolUse { id: rt_id, name: rt_name, input } => {
                prop_assert_eq!(rt_id, &id);
                prop_assert_eq!(rt_name, &name);
                prop_assert_eq!(input, &json!({"b": 2}));
            }
            other => prop_assert!(false, "expected ToolUse, got {:?}", other),
        }
    }

    /// Kimi tool result IDs survive roundtrip.
    #[test]
    fn content_kimi_tool_result_id_survives(
        tool_id in arb_tool_id(),
        text in arb_safe_text(),
    ) {
        let conv = IrConversation::from_messages(vec![
            IrMessage::new(IrRole::Tool, vec![IrContentBlock::ToolResult {
                tool_use_id: tool_id.clone(),
                content: vec![IrContentBlock::Text { text }],
                is_error: false,
            }]),
        ]);
        let sdk = abp_kimi_sdk::lowering::from_ir(&conv);
        let rt = abp_kimi_sdk::lowering::to_ir(&sdk);
        match &rt.messages[0].content[0] {
            IrContentBlock::ToolResult { tool_use_id, .. } => {
                prop_assert_eq!(tool_use_id, &tool_id);
            }
            other => prop_assert!(false, "expected ToolResult, got {:?}", other),
        }
    }

    /// Role mapping is consistent: same IR role always maps to the same SDK
    /// role string and back.
    #[test]
    fn content_role_mapping_consistency(
        text in arb_safe_text(),
        role in arb_ir_role(),
    ) {
        // OpenAI roundtrip: all 4 roles should map back to themselves
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(role, &text),
        ]);
        let oai = abp_openai_sdk::lowering::from_ir(&conv);
        let oai_rt = abp_openai_sdk::lowering::to_ir(&oai);
        prop_assert_eq!(oai_rt.messages[0].role, role);
        // Kimi roundtrip: same property
        let kimi = abp_kimi_sdk::lowering::from_ir(&conv);
        let kimi_rt = abp_kimi_sdk::lowering::to_ir(&kimi);
        prop_assert_eq!(kimi_rt.messages[0].role, role);
    }

    /// Gemini function call names survive roundtrip (IDs are synthesized).
    #[test]
    fn content_gemini_function_call_name_survives(
        name in arb_tool_name(),
    ) {
        let conv = IrConversation::from_messages(vec![
            IrMessage::new(IrRole::Assistant, vec![IrContentBlock::ToolUse {
                id: format!("gemini_{name}"),
                name: name.clone(),
                input: json!({"x": true}),
            }]),
        ]);
        let sdk = abp_gemini_sdk::lowering::from_ir(&conv);
        let rt = abp_gemini_sdk::lowering::to_ir(&sdk, None);
        match &rt.messages[0].content[0] {
            IrContentBlock::ToolUse { name: rt_name, input, .. } => {
                prop_assert_eq!(rt_name, &name);
                prop_assert_eq!(input, &json!({"x": true}));
            }
            other => prop_assert!(false, "expected ToolUse, got {:?}", other),
        }
    }

    /// Claude structured blocks (tool use) survive roundtrip.
    #[test]
    fn content_claude_tool_use_roundtrip(
        name in arb_tool_name(),
        id in arb_tool_id(),
    ) {
        let conv = IrConversation::from_messages(vec![
            IrMessage::new(IrRole::Assistant, vec![IrContentBlock::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: json!({"p": "v"}),
            }]),
        ]);
        let sdk = abp_claude_sdk::lowering::from_ir(&conv);
        let rt = abp_claude_sdk::lowering::to_ir(&sdk, None);
        match &rt.messages[0].content[0] {
            IrContentBlock::ToolUse { id: rt_id, name: rt_name, input } => {
                prop_assert_eq!(rt_id, &id);
                prop_assert_eq!(rt_name, &name);
                prop_assert_eq!(input, &json!({"p": "v"}));
            }
            other => prop_assert!(false, "expected ToolUse, got {:?}", other),
        }
    }
}
