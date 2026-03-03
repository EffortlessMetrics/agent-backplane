// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for IR roundtrip fidelity.
//!
//! Verifies that all core IR types survive JSON serialization roundtrips
//! and cross-SDK lowering/raising without data loss.

use proptest::prelude::*;
use serde_json::json;
use std::collections::BTreeMap;

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

fn arb_non_system_role() -> BoxedStrategy<IrRole> {
    prop_oneof![Just(IrRole::User), Just(IrRole::Assistant),].boxed()
}

fn arb_safe_text() -> BoxedStrategy<String> {
    "[a-zA-Z][a-zA-Z0-9 ]{0,99}".boxed()
}

fn arb_tool_name() -> BoxedStrategy<String> {
    "[a-z][a-z0-9_]{0,19}".boxed()
}

fn arb_tool_id() -> BoxedStrategy<String> {
    "[a-zA-Z][a-zA-Z0-9_]{4,15}".boxed()
}

fn arb_text_block() -> BoxedStrategy<IrContentBlock> {
    arb_safe_text()
        .prop_map(|text| IrContentBlock::Text { text })
        .boxed()
}

fn arb_thinking_block() -> BoxedStrategy<IrContentBlock> {
    arb_safe_text()
        .prop_map(|text| IrContentBlock::Thinking { text })
        .boxed()
}

fn arb_tool_use_block() -> BoxedStrategy<IrContentBlock> {
    (arb_tool_id(), arb_tool_name())
        .prop_map(|(id, name)| IrContentBlock::ToolUse {
            id,
            name,
            input: json!({"key": "value"}),
        })
        .boxed()
}

fn arb_tool_result_block() -> BoxedStrategy<IrContentBlock> {
    (arb_tool_id(), arb_safe_text(), any::<bool>())
        .prop_map(|(tool_use_id, text, is_error)| IrContentBlock::ToolResult {
            tool_use_id,
            content: vec![IrContentBlock::Text { text }],
            is_error,
        })
        .boxed()
}

fn arb_content_block() -> BoxedStrategy<IrContentBlock> {
    prop_oneof![
        arb_text_block(),
        arb_thinking_block(),
        arb_tool_use_block(),
        arb_tool_result_block(),
    ]
    .boxed()
}

fn arb_metadata() -> BoxedStrategy<BTreeMap<String, serde_json::Value>> {
    prop::collection::btree_map("[a-z]{1,10}", arb_safe_text().prop_map(|s| json!(s)), 0..=3)
        .boxed()
}

fn arb_ir_message() -> BoxedStrategy<IrMessage> {
    (
        arb_ir_role(),
        prop::collection::vec(arb_content_block(), 1..=4),
        arb_metadata(),
    )
        .prop_map(|(role, content, metadata)| IrMessage {
            role,
            content,
            metadata,
        })
        .boxed()
}

fn arb_text_message(role: BoxedStrategy<IrRole>) -> BoxedStrategy<IrMessage> {
    (role, arb_text_block())
        .prop_map(|(role, block)| IrMessage::new(role, vec![block]))
        .boxed()
}

fn arb_text_conversation(
    role: BoxedStrategy<IrRole>,
    max_msgs: usize,
) -> BoxedStrategy<IrConversation> {
    prop::collection::vec(arb_text_message(role), 1..=max_msgs)
        .prop_map(IrConversation::from_messages)
        .boxed()
}

fn fast_config() -> ProptestConfig {
    ProptestConfig {
        cases: 64,
        ..ProptestConfig::default()
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn serde_roundtrip<T: serde::Serialize + serde::de::DeserializeOwned>(val: &T) -> T {
    let json = serde_json::to_string(val).expect("serialize");
    serde_json::from_str(&json).expect("deserialize")
}

fn text_contents(conv: &IrConversation) -> Vec<String> {
    conv.messages.iter().map(|m| m.text_content()).collect()
}

fn roles(conv: &IrConversation) -> Vec<IrRole> {
    conv.messages.iter().map(|m| m.role).collect()
}

// ═══════════════════════════════════════════════════════════════════════
// §1  IrMessage roundtrip (4 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Any IrMessage serializes and deserializes identically.
    #[test]
    fn ir_message_serde_roundtrip(msg in arb_ir_message()) {
        let rt = serde_roundtrip(&msg);
        prop_assert_eq!(msg, rt);
    }

    /// IrRole is preserved through JSON roundtrip.
    #[test]
    fn ir_role_preserved_through_roundtrip(role in arb_ir_role()) {
        let rt: IrRole = serde_roundtrip(&role);
        prop_assert_eq!(role, rt);
    }

    /// Content blocks are preserved through JSON roundtrip.
    #[test]
    fn ir_content_blocks_preserved(blocks in prop::collection::vec(arb_content_block(), 1..=6)) {
        let rt: Vec<IrContentBlock> = serde_roundtrip(&blocks);
        prop_assert_eq!(blocks, rt);
    }

    /// Metadata is preserved through JSON roundtrip.
    #[test]
    fn ir_metadata_preserved(
        role in arb_ir_role(),
        text in arb_safe_text(),
        metadata in arb_metadata(),
    ) {
        let msg = IrMessage {
            role,
            content: vec![IrContentBlock::Text { text }],
            metadata: metadata.clone(),
        };
        let rt = serde_roundtrip(&msg);
        prop_assert_eq!(msg.metadata, rt.metadata);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §2  IrConversation roundtrip (4 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// Any conversation with 1–20 messages roundtrips through JSON.
    #[test]
    fn ir_conversation_roundtrip(
        msgs in prop::collection::vec(arb_text_message(arb_ir_role()), 1..=20)
    ) {
        let conv = IrConversation::from_messages(msgs);
        let rt = serde_roundtrip(&conv);
        prop_assert_eq!(conv, rt);
    }

    /// System message position is preserved through roundtrip.
    #[test]
    fn ir_system_message_position_preserved(
        pre in prop::collection::vec(arb_text_message(arb_non_system_role()), 0..=3),
        sys_text in arb_safe_text(),
        post in prop::collection::vec(arb_text_message(arb_non_system_role()), 1..=5),
    ) {
        let mut msgs = pre;
        msgs.push(IrMessage::text(IrRole::System, &sys_text));
        msgs.extend(post);
        let conv = IrConversation::from_messages(msgs);
        let rt = serde_roundtrip(&conv);
        // Find system message positions
        let orig_pos = conv.messages.iter().position(|m| m.role == IrRole::System);
        let rt_pos = rt.messages.iter().position(|m| m.role == IrRole::System);
        prop_assert_eq!(orig_pos, rt_pos);
        prop_assert_eq!(conv.system_message().map(|m| m.text_content()),
                        rt.system_message().map(|m| m.text_content()));
    }

    /// Tool calls and results pair correctly through roundtrip.
    #[test]
    fn ir_tool_call_result_pairing(
        tool_id in arb_tool_id(),
        tool_name in arb_tool_name(),
        result_text in arb_safe_text(),
    ) {
        let conv = IrConversation::from_messages(vec![
            IrMessage::new(IrRole::Assistant, vec![IrContentBlock::ToolUse {
                id: tool_id.clone(),
                name: tool_name.clone(),
                input: json!({"arg": 1}),
            }]),
            IrMessage::new(IrRole::Tool, vec![IrContentBlock::ToolResult {
                tool_use_id: tool_id.clone(),
                content: vec![IrContentBlock::Text { text: result_text.clone() }],
                is_error: false,
            }]),
        ]);
        let rt = serde_roundtrip(&conv);
        // Extract tool_use id and tool_result tool_use_id and verify they match
        let use_id = match &rt.messages[0].content[0] {
            IrContentBlock::ToolUse { id, .. } => id.clone(),
            other => panic!("expected ToolUse, got {other:?}"),
        };
        let result_ref = match &rt.messages[1].content[0] {
            IrContentBlock::ToolResult { tool_use_id, .. } => tool_use_id.clone(),
            other => panic!("expected ToolResult, got {other:?}"),
        };
        prop_assert_eq!(&use_id, &tool_id);
        prop_assert_eq!(&result_ref, &tool_id);
        prop_assert_eq!(use_id, result_ref);
    }
}

/// Empty conversation roundtrips through JSON.
#[test]
fn ir_empty_conversation_roundtrip() {
    let conv = IrConversation::new();
    let rt = serde_roundtrip(&conv);
    assert_eq!(conv, rt);
    assert!(rt.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// §3  Cross-SDK roundtrip (4 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// OpenAI→IR→OpenAI preserves user messages.
    #[test]
    fn cross_sdk_openai_user_messages_preserved(
        conv in arb_text_conversation(Just(IrRole::User).boxed(), 8)
    ) {
        let sdk = abp_openai_sdk::lowering::from_ir(&conv);
        let rt = abp_openai_sdk::lowering::to_ir(&sdk);
        prop_assert_eq!(text_contents(&conv), text_contents(&rt));
        prop_assert_eq!(roles(&conv), roles(&rt));
    }

    /// Claude→IR→Claude preserves assistant messages.
    #[test]
    fn cross_sdk_claude_assistant_messages_preserved(
        conv in arb_text_conversation(Just(IrRole::Assistant).boxed(), 8)
    ) {
        let sdk = abp_claude_sdk::lowering::from_ir(&conv);
        let rt = abp_claude_sdk::lowering::to_ir(&sdk, None);
        prop_assert_eq!(text_contents(&conv), text_contents(&rt));
    }

    /// Gemini→IR→Gemini preserves multi-turn conversations.
    #[test]
    fn cross_sdk_gemini_multi_turn_preserved(
        conv in arb_text_conversation(arb_non_system_role(), 10)
    ) {
        let sdk = abp_gemini_sdk::lowering::from_ir(&conv);
        let rt = abp_gemini_sdk::lowering::to_ir(&sdk, None);
        prop_assert_eq!(text_contents(&conv), text_contents(&rt));
        prop_assert_eq!(conv.len(), rt.len());
    }

    /// System message handled across all SDKs that support it.
    #[test]
    fn cross_sdk_system_message_handled(
        sys_text in arb_safe_text(),
        user_text in arb_safe_text(),
    ) {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, &sys_text),
            IrMessage::text(IrRole::User, &user_text),
        ]);

        // OpenAI preserves system messages
        let oai = abp_openai_sdk::lowering::from_ir(&conv);
        let oai_rt = abp_openai_sdk::lowering::to_ir(&oai);
        prop_assert_eq!(oai_rt.system_message().map(|m| m.text_content()), Some(sys_text.clone()));

        // Claude extracts system prompt separately
        let sys_prompt = abp_claude_sdk::lowering::extract_system_prompt(&conv);
        prop_assert_eq!(sys_prompt.as_deref(), Some(sys_text.as_str()));

        // Kimi preserves system messages
        let kimi = abp_kimi_sdk::lowering::from_ir(&conv);
        let kimi_rt = abp_kimi_sdk::lowering::to_ir(&kimi);
        prop_assert_eq!(kimi_rt.system_message().map(|m| m.text_content()), Some(sys_text));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §4  Invariants (3 tests)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(fast_config())]

    /// IR conversation message count is preserved through any serde roundtrip.
    #[test]
    fn invariant_message_count_preserved(
        conv in arb_text_conversation(arb_ir_role(), 15)
    ) {
        let rt = serde_roundtrip(&conv);
        prop_assert_eq!(conv.len(), rt.len());

        // Also through SDK roundtrips
        let oai = abp_openai_sdk::lowering::from_ir(&conv);
        let oai_rt = abp_openai_sdk::lowering::to_ir(&oai);
        prop_assert_eq!(conv.len(), oai_rt.len());

        let kimi = abp_kimi_sdk::lowering::from_ir(&conv);
        let kimi_rt = abp_kimi_sdk::lowering::to_ir(&kimi);
        prop_assert_eq!(conv.len(), kimi_rt.len());
    }

    /// Role ordering is preserved through roundtrip.
    #[test]
    fn invariant_role_ordering_preserved(
        conv in arb_text_conversation(arb_ir_role(), 12)
    ) {
        // JSON roundtrip
        let rt = serde_roundtrip(&conv);
        prop_assert_eq!(roles(&conv), roles(&rt));

        // OpenAI roundtrip
        let oai = abp_openai_sdk::lowering::from_ir(&conv);
        let oai_rt = abp_openai_sdk::lowering::to_ir(&oai);
        prop_assert_eq!(roles(&conv), roles(&oai_rt));
    }

    /// Content type (text vs tool_use etc.) is preserved through roundtrip.
    #[test]
    fn invariant_content_type_preserved(
        blocks in prop::collection::vec(arb_content_block(), 1..=6)
    ) {
        let msg = IrMessage::new(IrRole::Assistant, blocks);
        let rt = serde_roundtrip(&msg);
        prop_assert_eq!(msg.content.len(), rt.content.len());
        for (orig, round) in msg.content.iter().zip(rt.content.iter()) {
            let orig_type = std::mem::discriminant(orig);
            let round_type = std::mem::discriminant(round);
            prop_assert_eq!(orig_type, round_type);
        }
    }
}
