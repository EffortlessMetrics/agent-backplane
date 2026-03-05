#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Exhaustive property-based tests for the ABP intermediate representation.
//!
//! Covers serde roundtrips, normalization idempotency, dialect lowering
//! roundtrips, degenerate/empty values, merge associativity, and unicode
//! survival for every IR type.

use abp_ir::lower::*;
use abp_ir::normalize::*;
use abp_ir::*;
use abp_sdk_types::Dialect;
use proptest::prelude::*;
use serde_json::json;
use std::collections::BTreeMap;

// ═══════════════════════════════════════════════════════════════════════
// Strategies
// ═══════════════════════════════════════════════════════════════════════

fn arb_role() -> impl Strategy<Value = IrRole> {
    prop_oneof![
        Just(IrRole::System),
        Just(IrRole::User),
        Just(IrRole::Assistant),
        Just(IrRole::Tool),
    ]
}

fn arb_dialect() -> impl Strategy<Value = Dialect> {
    prop_oneof![
        Just(Dialect::OpenAi),
        Just(Dialect::Claude),
        Just(Dialect::Gemini),
        Just(Dialect::Kimi),
        Just(Dialect::Codex),
        Just(Dialect::Copilot),
    ]
}

fn arb_ident() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,15}"
}

fn arb_text() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 _.,!?\\-]{0,200}"
}

/// Non-empty text that won't collapse to empty after trimming.
fn arb_nonempty_text() -> impl Strategy<Value = String> {
    "[a-zA-Z][a-zA-Z0-9 _.,!?\\-]{0,100}"
}

fn arb_base64() -> impl Strategy<Value = String> {
    "[A-Za-z0-9+/=]{0,100}"
}

fn arb_mime() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("image/png".to_string()),
        Just("image/jpeg".to_string()),
        Just("image/gif".to_string()),
        Just("image/webp".to_string()),
    ]
}

fn arb_json_value() -> impl Strategy<Value = serde_json::Value> {
    prop_oneof![
        Just(json!(null)),
        any::<bool>().prop_map(|b| json!(b)),
        any::<i64>().prop_map(|n| json!(n)),
        arb_text().prop_map(|s| json!(s)),
        Just(json!({})),
        Just(json!({"key": "value"})),
        Just(json!({"a": 1, "b": true})),
    ]
}

fn arb_json_schema() -> impl Strategy<Value = serde_json::Value> {
    prop_oneof![
        Just(json!({"type": "object", "properties": {}})),
        Just(json!({"type": "object", "properties": {"q": {"type": "string"}}})),
        Just(json!({"type": "object", "additionalProperties": true})),
    ]
}

fn arb_leaf_content_block() -> impl Strategy<Value = IrContentBlock> {
    prop_oneof![
        arb_text().prop_map(|text| IrContentBlock::Text { text }),
        (arb_mime(), arb_base64())
            .prop_map(|(media_type, data)| IrContentBlock::Image { media_type, data }),
        (arb_ident(), arb_ident(), arb_json_value())
            .prop_map(|(id, name, input)| IrContentBlock::ToolUse { id, name, input }),
        arb_text().prop_map(|text| IrContentBlock::Thinking { text }),
    ]
}

fn arb_content_block() -> impl Strategy<Value = IrContentBlock> {
    prop_oneof![
        3 => arb_leaf_content_block(),
        1 => (
            arb_ident(),
            prop::collection::vec(arb_leaf_content_block(), 0..=3),
            any::<bool>(),
        ).prop_map(|(tool_use_id, content, is_error)| {
            IrContentBlock::ToolResult { tool_use_id, content, is_error }
        }),
    ]
}

fn arb_metadata() -> impl Strategy<Value = BTreeMap<String, serde_json::Value>> {
    prop::collection::btree_map(arb_ident(), arb_json_value(), 0..=4)
}

fn arb_message() -> impl Strategy<Value = IrMessage> {
    (
        arb_role(),
        prop::collection::vec(arb_content_block(), 0..=4),
        arb_metadata(),
    )
        .prop_map(|(role, content, metadata)| IrMessage {
            role,
            content,
            metadata,
        })
}

fn arb_text_message() -> impl Strategy<Value = IrMessage> {
    (arb_role(), prop::collection::vec(arb_text(), 1..=3)).prop_map(|(role, texts)| {
        let content = texts
            .into_iter()
            .map(|text| IrContentBlock::Text { text })
            .collect();
        IrMessage::new(role, content)
    })
}

fn arb_tool_definition() -> impl Strategy<Value = IrToolDefinition> {
    (arb_ident(), arb_text(), arb_json_schema()).prop_map(|(name, description, parameters)| {
        IrToolDefinition {
            name,
            description,
            parameters,
        }
    })
}

fn arb_conversation() -> impl Strategy<Value = IrConversation> {
    prop::collection::vec(arb_message(), 0..=6).prop_map(IrConversation::from_messages)
}

/// Conversation with at least one non-empty text message (for normalization tests).
fn arb_nonempty_text_conversation() -> impl Strategy<Value = IrConversation> {
    prop::collection::vec(
        (arb_role(), arb_nonempty_text()).prop_map(|(role, text)| IrMessage::text(role, text)),
        1..=5,
    )
    .prop_map(IrConversation::from_messages)
}

fn arb_usage() -> impl Strategy<Value = IrUsage> {
    (any::<u32>(), any::<u32>(), any::<u32>(), any::<u32>()).prop_map(|(i, o, cr, cw)| {
        let input_tokens = i as u64;
        let output_tokens = o as u64;
        IrUsage {
            input_tokens,
            output_tokens,
            total_tokens: input_tokens + output_tokens,
            cache_read_tokens: cr as u64,
            cache_write_tokens: cw as u64,
        }
    })
}

/// Unicode string strategy including multi-byte characters.
fn arb_unicode() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("héllo wörld".to_string()),
        Just("日本語テスト".to_string()),
        Just("Ω≈ç√∫".to_string()),
        Just("emoji: 🦀🔥✅".to_string()),
        Just("mixed: abc-123-αβγ".to_string()),
        Just("中文Chinese混合".to_string()),
        Just("العربية".to_string()),
        Just("Привет мир".to_string()),
        Just("🏳️‍🌈🏴‍☠️".to_string()),
        Just("null\0byte".to_string()),
        Just("tabs\there\tthere".to_string()),
        Just("newlines\nare\nfun".to_string()),
        arb_text(),
    ]
}

// ═══════════════════════════════════════════════════════════════════════
// 1. Serde roundtrip tests
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn rt_ir_role(role in arb_role()) {
        let json = serde_json::to_string(&role).unwrap();
        let back: IrRole = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(role, back);
    }

    #[test]
    fn rt_ir_content_block(block in arb_content_block()) {
        let json = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&block, &back);
    }

    #[test]
    fn rt_ir_message(msg in arb_message()) {
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&msg, &back);
    }

    #[test]
    fn rt_ir_conversation(conv in arb_conversation()) {
        let json = serde_json::to_string(&conv).unwrap();
        let back: IrConversation = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&conv, &back);
    }

    #[test]
    fn rt_ir_tool_definition(td in arb_tool_definition()) {
        let json = serde_json::to_string(&td).unwrap();
        let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&td, &back);
    }

    #[test]
    fn rt_ir_usage(usage in arb_usage()) {
        let json = serde_json::to_string(&usage).unwrap();
        let back: IrUsage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&usage, &back);
    }

    #[test]
    fn rt_tool_definition_vec(defs in prop::collection::vec(arb_tool_definition(), 0..=5)) {
        let json = serde_json::to_string(&defs).unwrap();
        let back: Vec<IrToolDefinition> = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&defs, &back);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Serde roundtrip via Value (JSON Value intermediary)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(150))]

    #[test]
    fn rt_message_via_value(msg in arb_message()) {
        let value = serde_json::to_value(&msg).unwrap();
        let back: IrMessage = serde_json::from_value(value).unwrap();
        prop_assert_eq!(&msg, &back);
    }

    #[test]
    fn rt_conversation_via_value(conv in arb_conversation()) {
        let value = serde_json::to_value(&conv).unwrap();
        let back: IrConversation = serde_json::from_value(value).unwrap();
        prop_assert_eq!(&conv, &back);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Content block type discriminator
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn content_block_has_type_tag(block in arb_content_block()) {
        let v = serde_json::to_value(&block).unwrap();
        let t = v.get("type").expect("missing 'type'");
        let valid = ["text", "image", "tool_use", "tool_result", "thinking"];
        prop_assert!(valid.contains(&t.as_str().unwrap()));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Normalization idempotency
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(150))]

    #[test]
    fn normalize_is_idempotent(conv in arb_conversation()) {
        let once = normalize(&conv);
        let twice = normalize(&once);
        prop_assert_eq!(&once, &twice);
    }

    #[test]
    fn dedup_system_is_idempotent(conv in arb_conversation()) {
        let once = dedup_system(&conv);
        let twice = dedup_system(&once);
        prop_assert_eq!(&once, &twice);
    }

    #[test]
    fn trim_text_is_idempotent(conv in arb_conversation()) {
        let once = trim_text(&conv);
        let twice = trim_text(&once);
        prop_assert_eq!(&once, &twice);
    }

    #[test]
    fn merge_adjacent_text_is_idempotent(conv in arb_conversation()) {
        let once = merge_adjacent_text(&conv);
        let twice = merge_adjacent_text(&once);
        prop_assert_eq!(&once, &twice);
    }

    #[test]
    fn strip_empty_is_idempotent(conv in arb_conversation()) {
        let once = strip_empty(&conv);
        let twice = strip_empty(&once);
        prop_assert_eq!(&once, &twice);
    }

    #[test]
    fn strip_metadata_all_is_idempotent(conv in arb_conversation()) {
        let once = strip_metadata(&conv, &[]);
        let twice = strip_metadata(&once, &[]);
        prop_assert_eq!(&once, &twice);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Normalization preserves non-system message count
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(150))]

    #[test]
    fn dedup_system_preserves_non_system(conv in arb_nonempty_text_conversation()) {
        let non_sys_before: Vec<_> = conv.messages.iter()
            .filter(|m| m.role != IrRole::System)
            .cloned()
            .collect();
        let result = dedup_system(&conv);
        let non_sys_after: Vec<_> = result.messages.iter()
            .filter(|m| m.role != IrRole::System)
            .cloned()
            .collect();
        prop_assert_eq!(&non_sys_before, &non_sys_after);
    }

    #[test]
    fn dedup_system_at_most_one_system(conv in arb_conversation()) {
        let result = dedup_system(&conv);
        let sys_count = result.messages.iter().filter(|m| m.role == IrRole::System).count();
        prop_assert!(sys_count <= 1);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. extract_system roundtrip
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(150))]

    #[test]
    fn extract_system_removes_all_system(conv in arb_conversation()) {
        let (_sys, rest) = extract_system(&conv);
        prop_assert!(rest.messages.iter().all(|m| m.role != IrRole::System));
    }

    #[test]
    fn extract_system_preserves_non_system_count(conv in arb_conversation()) {
        let non_sys_count = conv.messages.iter().filter(|m| m.role != IrRole::System).count();
        let (_sys, rest) = extract_system(&conv);
        prop_assert_eq!(rest.len(), non_sys_count);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. IR → Dialect lowering produces valid JSON
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn lower_to_all_dialects_is_object(
        conv in arb_nonempty_text_conversation(),
        tools in prop::collection::vec(arb_tool_definition(), 0..=3),
        dialect in arb_dialect(),
    ) {
        let lowered = lower_for_dialect(dialect, &conv, &tools);
        prop_assert!(lowered.is_object(), "lowered must be JSON object");
    }

    #[test]
    fn lower_openai_has_messages(conv in arb_nonempty_text_conversation()) {
        let lowered = lower_to_openai(&conv, &[]);
        prop_assert!(lowered.get("messages").is_some());
        prop_assert!(lowered["messages"].is_array());
    }

    #[test]
    fn lower_claude_extracts_system(
        sys_text in arb_nonempty_text(),
        user_text in arb_nonempty_text(),
    ) {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, &sys_text))
            .push(IrMessage::text(IrRole::User, &user_text));
        let lowered = lower_to_claude(&conv, &[]);
        // Claude puts system at top level
        prop_assert_eq!(lowered["system"].as_str().unwrap(), sys_text);
        // No system role in messages array
        let msgs = lowered["messages"].as_array().unwrap();
        prop_assert!(msgs.iter().all(|m| m["role"] != "system"));
    }

    #[test]
    fn lower_gemini_uses_model_role(
        user_text in arb_nonempty_text(),
        asst_text in arb_nonempty_text(),
    ) {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, &user_text))
            .push(IrMessage::text(IrRole::Assistant, &asst_text));
        let lowered = lower_to_gemini(&conv, &[]);
        let contents = lowered["contents"].as_array().unwrap();
        prop_assert_eq!(contents[1]["role"].as_str().unwrap(), "model");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Kimi/Codex/Copilot match OpenAI output
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn kimi_codex_copilot_match_openai(
        conv in arb_nonempty_text_conversation(),
        tools in prop::collection::vec(arb_tool_definition(), 0..=2),
    ) {
        let openai = lower_to_openai(&conv, &tools);
        prop_assert_eq!(&lower_to_kimi(&conv, &tools), &openai);
        prop_assert_eq!(&lower_to_codex(&conv, &tools), &openai);
        prop_assert_eq!(&lower_to_copilot(&conv, &tools), &openai);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Empty/degenerate IR values
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn empty_conversation_roundtrip(_x in 0..1u8) {
        let conv = IrConversation::new();
        let json = serde_json::to_string(&conv).unwrap();
        let back: IrConversation = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&conv, &back);
        prop_assert!(back.is_empty());
    }

    #[test]
    fn empty_content_message_roundtrip(role in arb_role()) {
        let msg = IrMessage::new(role, vec![]);
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&msg, &back);
        prop_assert!(back.content.is_empty());
    }

    #[test]
    fn empty_text_block_roundtrip(_x in 0..1u8) {
        let block = IrContentBlock::Text { text: String::new() };
        let json = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&block, &back);
    }

    #[test]
    fn empty_tool_result_roundtrip(id in arb_ident(), is_err in any::<bool>()) {
        let block = IrContentBlock::ToolResult {
            tool_use_id: id,
            content: vec![],
            is_error: is_err,
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&block, &back);
    }

    #[test]
    fn default_usage_roundtrip(_x in 0..1u8) {
        let usage = IrUsage::default();
        let json = serde_json::to_string(&usage).unwrap();
        let back: IrUsage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&usage, &back);
        prop_assert_eq!(back.input_tokens, 0);
        prop_assert_eq!(back.output_tokens, 0);
        prop_assert_eq!(back.total_tokens, 0);
    }

    #[test]
    fn empty_metadata_omitted(msg in arb_text_message()) {
        let json = serde_json::to_string(&msg).unwrap();
        if msg.metadata.is_empty() {
            prop_assert!(!json.contains("\"metadata\""));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Normalize pipeline on empty/degenerate
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn normalize_empty_conversation(_x in 0..1u8) {
        let conv = IrConversation::new();
        let result = normalize(&conv);
        prop_assert!(result.is_empty());
    }

    #[test]
    fn strip_empty_removes_contentless(role in arb_role()) {
        let conv = IrConversation::from_messages(vec![
            IrMessage::new(role, vec![]),
        ]);
        let result = strip_empty(&conv);
        prop_assert!(result.is_empty());
    }

    #[test]
    fn normalize_single_whitespace_system(_x in 0..1u8) {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "   "));
        let result = normalize(&conv);
        // After trim, system text is empty string, but the message still has a text
        // block with empty text, so strip_empty won't remove it (it has content).
        // The important thing is idempotency.
        let twice = normalize(&result);
        prop_assert_eq!(&result, &twice);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 11. IrUsage merge associativity
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn usage_merge_associative(a in arb_usage(), b in arb_usage(), c in arb_usage()) {
        let ab_c = a.merge(b).merge(c);
        let a_bc = a.merge(b.merge(c));
        prop_assert_eq!(ab_c, a_bc);
    }

    #[test]
    fn usage_merge_commutative(a in arb_usage(), b in arb_usage()) {
        let ab = a.merge(b);
        let ba = b.merge(a);
        prop_assert_eq!(ab, ba);
    }

    #[test]
    fn usage_merge_identity(a in arb_usage()) {
        let zero = IrUsage::default();
        prop_assert_eq!(a.merge(zero), a);
        prop_assert_eq!(zero.merge(a), a);
    }

    #[test]
    fn usage_from_io_total(inp in 0u32..100_000, out in 0u32..100_000) {
        let usage = IrUsage::from_io(inp as u64, out as u64);
        prop_assert_eq!(usage.total_tokens, usage.input_tokens + usage.output_tokens);
    }

    #[test]
    fn usage_with_cache_total(
        inp in 0u32..100_000,
        out in 0u32..100_000,
        cr in 0u32..100_000,
        cw in 0u32..100_000,
    ) {
        let usage = IrUsage::with_cache(inp as u64, out as u64, cr as u64, cw as u64);
        prop_assert_eq!(usage.total_tokens, usage.input_tokens + usage.output_tokens);
        prop_assert_eq!(usage.cache_read_tokens, cr as u64);
        prop_assert_eq!(usage.cache_write_tokens, cw as u64);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Unicode survival
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn unicode_text_block_roundtrip(text in arb_unicode()) {
        let block = IrContentBlock::Text { text: text.clone() };
        let json = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json).unwrap();
        if let IrContentBlock::Text { text: recovered } = &back {
            prop_assert_eq!(&text, recovered);
        } else {
            prop_assert!(false, "expected Text block");
        }
    }

    #[test]
    fn unicode_thinking_block_roundtrip(text in arb_unicode()) {
        let block = IrContentBlock::Thinking { text: text.clone() };
        let json = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json).unwrap();
        if let IrContentBlock::Thinking { text: recovered } = &back {
            prop_assert_eq!(&text, recovered);
        } else {
            prop_assert!(false, "expected Thinking block");
        }
    }

    #[test]
    fn unicode_message_text_content(text in arb_unicode()) {
        let msg = IrMessage::text(IrRole::User, &text);
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back.text_content(), text);
    }

    #[test]
    fn unicode_tool_name_roundtrip(name in arb_ident(), desc in arb_unicode()) {
        let td = IrToolDefinition {
            name: name.clone(),
            description: desc.clone(),
            parameters: json!({}),
        };
        let json = serde_json::to_string(&td).unwrap();
        let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&td, &back);
    }

    #[test]
    fn unicode_metadata_value(key in arb_ident(), val in arb_unicode()) {
        let mut meta = BTreeMap::new();
        meta.insert(key, json!(val.clone()));
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text { text: "hi".into() }],
            metadata: meta,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&msg.metadata, &back.metadata);
    }

    #[test]
    fn unicode_conversation_roundtrip(text in arb_unicode()) {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, &text));
        let json = serde_json::to_string(&conv).unwrap();
        let back: IrConversation = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&conv, &back);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 13. Normalize role mapping
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn normalize_role_known_always_some(
        alias in prop_oneof![
            Just("system"), Just("developer"),
            Just("user"), Just("human"),
            Just("assistant"), Just("model"), Just("bot"),
            Just("tool"), Just("function"),
        ]
    ) {
        prop_assert!(normalize_role(alias).is_some());
    }

    #[test]
    fn normalize_role_unknown_is_none(s in "[A-Z][A-Z]{3,10}") {
        prop_assert!(normalize_role(&s).is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 14. sort_tools stability and idempotency
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn sort_tools_is_idempotent(tools in prop::collection::vec(arb_tool_definition(), 0..=6)) {
        let mut once = tools.clone();
        sort_tools(&mut once);
        let mut twice = once.clone();
        sort_tools(&mut twice);
        prop_assert_eq!(&once, &twice);
    }

    #[test]
    fn sort_tools_preserves_elements(tools in prop::collection::vec(arb_tool_definition(), 0..=6)) {
        let mut sorted = tools.clone();
        sort_tools(&mut sorted);
        prop_assert_eq!(sorted.len(), tools.len());
        // Every original tool is in the sorted list
        for t in &tools {
            prop_assert!(sorted.contains(t));
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 15. normalize_tool_schemas idempotency
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn normalize_tool_schemas_is_idempotent(
        tools in prop::collection::vec(arb_tool_definition(), 0..=4),
    ) {
        let once = normalize_tool_schemas(&tools);
        let twice = normalize_tool_schemas(&once);
        prop_assert_eq!(&once, &twice);
    }

    #[test]
    fn normalize_tool_schemas_adds_type_object(
        tools in prop::collection::vec(arb_tool_definition(), 1..=4),
    ) {
        let normalized = normalize_tool_schemas(&tools);
        for t in &normalized {
            if t.parameters.is_object() {
                prop_assert_eq!(t.parameters.get("type").unwrap(), "object");
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 16. Conversation accessors
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn conversation_len_matches(msgs in prop::collection::vec(arb_message(), 0..=8)) {
        let conv = IrConversation::from_messages(msgs.clone());
        prop_assert_eq!(conv.len(), msgs.len());
        prop_assert_eq!(conv.is_empty(), msgs.is_empty());
    }

    #[test]
    fn conversation_push_increments_len(conv in arb_conversation(), msg in arb_message()) {
        let old_len = conv.len();
        let new_conv = conv.push(msg);
        prop_assert_eq!(new_conv.len(), old_len + 1);
    }

    #[test]
    fn messages_by_role_correct(conv in arb_conversation(), role in arb_role()) {
        let expected = conv.messages.iter().filter(|m| m.role == role).count();
        let actual = conv.messages_by_role(role).len();
        prop_assert_eq!(expected, actual);
    }

    #[test]
    fn last_message_is_last(conv in arb_conversation()) {
        if conv.is_empty() {
            prop_assert!(conv.last_message().is_none());
        } else {
            prop_assert_eq!(
                conv.last_message().unwrap(),
                conv.messages.last().unwrap()
            );
        }
    }

    #[test]
    fn tool_calls_count(conv in arb_conversation()) {
        let expected: usize = conv.messages.iter()
            .flat_map(|m| &m.content)
            .filter(|b| matches!(b, IrContentBlock::ToolUse { .. }))
            .count();
        prop_assert_eq!(conv.tool_calls().len(), expected);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 17. IrMessage helpers
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn text_only_message_is_text_only(role in arb_role(), texts in prop::collection::vec(arb_text(), 1..=3)) {
        let content: Vec<IrContentBlock> = texts.into_iter()
            .map(|t| IrContentBlock::Text { text: t })
            .collect();
        let msg = IrMessage::new(role, content);
        prop_assert!(msg.is_text_only());
    }

    #[test]
    fn non_text_message_not_text_only(role in arb_role(), name in arb_ident(), id in arb_ident()) {
        let msg = IrMessage::new(role, vec![
            IrContentBlock::ToolUse { id, name, input: json!({}) },
        ]);
        prop_assert!(!msg.is_text_only());
    }

    #[test]
    fn text_content_concatenates(role in arb_role(), a in arb_text(), b in arb_text()) {
        let msg = IrMessage::new(role, vec![
            IrContentBlock::Text { text: a.clone() },
            IrContentBlock::Text { text: b.clone() },
        ]);
        prop_assert_eq!(msg.text_content(), format!("{}{}", a, b));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 18. Dialect role mapping exhaustive
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn ir_role_to_dialect_never_empty(role in arb_role(), dialect in arb_dialect()) {
        let s = ir_role_to_dialect(role, dialect);
        prop_assert!(!s.is_empty());
    }

    #[test]
    fn user_role_always_user(dialect in arb_dialect()) {
        prop_assert_eq!(ir_role_to_dialect(IrRole::User, dialect), "user");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 19. Lower with tools vs without
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(80))]

    #[test]
    fn no_tools_omits_tools_field(
        conv in arb_nonempty_text_conversation(),
        dialect in arb_dialect(),
    ) {
        let lowered = lower_for_dialect(dialect, &conv, &[]);
        prop_assert!(lowered.get("tools").is_none());
    }

    #[test]
    fn with_tools_includes_tools_field(
        conv in arb_nonempty_text_conversation(),
        tools in prop::collection::vec(arb_tool_definition(), 1..=3),
        dialect in arb_dialect(),
    ) {
        let lowered = lower_for_dialect(dialect, &conv, &tools);
        prop_assert!(lowered.get("tools").is_some());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 20. IrRole serializes to snake_case
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn role_serde_snake_case(role in arb_role()) {
        let json = serde_json::to_string(&role).unwrap();
        let expected = match role {
            IrRole::System => "\"system\"",
            IrRole::User => "\"user\"",
            IrRole::Assistant => "\"assistant\"",
            IrRole::Tool => "\"tool\"",
        };
        prop_assert_eq!(json.as_str(), expected);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 21. merge_adjacent_text reduces block count
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn merge_adjacent_text_does_not_increase_blocks(conv in arb_conversation()) {
        let merged = merge_adjacent_text(&conv);
        for (orig, m) in conv.messages.iter().zip(merged.messages.iter()) {
            prop_assert!(m.content.len() <= orig.content.len());
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 22. Lower roundtrip: normalize → lower → JSON parses
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(80))]

    #[test]
    fn normalize_then_lower_produces_valid_json(
        conv in arb_nonempty_text_conversation(),
        dialect in arb_dialect(),
    ) {
        let normalized = normalize(&conv);
        let lowered = lower_for_dialect(dialect, &normalized, &[]);
        let json_str = serde_json::to_string(&lowered).unwrap();
        let reparsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        prop_assert_eq!(&lowered, &reparsed);
    }
}
