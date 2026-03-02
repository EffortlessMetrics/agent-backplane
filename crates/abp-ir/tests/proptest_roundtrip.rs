// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for the ABP intermediate representation layer.
//!
//! Covers serde round-trip invariants for every IR type, nested structures,
//! metadata maps, tool definitions, conversations, and usage records.

use abp_ir::*;
use proptest::prelude::*;
use serde_json::json;
use std::collections::BTreeMap;

// ═══════════════════════════════════════════════════════════════════════
// Strategies
// ═══════════════════════════════════════════════════════════════════════

/// Arbitrary `IrRole`.
fn arb_role() -> impl Strategy<Value = IrRole> {
    prop_oneof![
        Just(IrRole::System),
        Just(IrRole::User),
        Just(IrRole::Assistant),
        Just(IrRole::Tool),
    ]
}

/// Safe identifier strings (tool names, IDs).
fn arb_ident() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,15}"
}

/// Arbitrary non-empty text content.
fn arb_text() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 _.,!?\\-]{0,200}"
}

/// Arbitrary base64 data (safe ASCII subset).
fn arb_base64() -> impl Strategy<Value = String> {
    "[A-Za-z0-9+/=]{0,100}"
}

/// Arbitrary MIME type.
fn arb_mime() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("image/png".to_string()),
        Just("image/jpeg".to_string()),
        Just("image/gif".to_string()),
        Just("image/webp".to_string()),
        Just("image/svg+xml".to_string()),
    ]
}

/// Simple JSON value strategy (no deeply nested objects, for speed).
fn arb_json_value() -> impl Strategy<Value = serde_json::Value> {
    prop_oneof![
        Just(json!(null)),
        any::<bool>().prop_map(|b| json!(b)),
        any::<i64>().prop_map(|n| json!(n)),
        arb_text().prop_map(|s| json!(s)),
        Just(json!({})),
        Just(json!([])),
        Just(json!({"key": "value"})),
        Just(json!({"a": 1, "b": true, "c": "hello"})),
        Just(json!([1, 2, 3])),
        Just(json!({"nested": {"deep": true}})),
    ]
}

/// Arbitrary JSON object for tool parameter schemas.
fn arb_json_schema() -> impl Strategy<Value = serde_json::Value> {
    prop_oneof![
        Just(json!({"type": "object", "properties": {}})),
        Just(
            json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]})
        ),
        Just(json!({"type": "object", "properties": {"n": {"type": "integer", "minimum": 0}}})),
        Just(json!({"type": "object", "properties": {
            "cmd": {"type": "string"},
            "args": {"type": "array", "items": {"type": "string"}}
        }})),
        Just(json!({"type": "object", "additionalProperties": true})),
    ]
}

/// Leaf content block (no ToolResult, to avoid recursion in the strategy).
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

/// Arbitrary content block including ToolResult with nested leaf blocks.
fn arb_content_block() -> impl Strategy<Value = IrContentBlock> {
    prop_oneof![
        3 => arb_leaf_content_block(),
        1 => (
            arb_ident(),
            prop::collection::vec(arb_leaf_content_block(), 0..=3),
            any::<bool>(),
        )
            .prop_map(|(tool_use_id, content, is_error)| {
                IrContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                }
            }),
    ]
}

/// Arbitrary metadata map.
fn arb_metadata() -> impl Strategy<Value = BTreeMap<String, serde_json::Value>> {
    prop::collection::btree_map(arb_ident(), arb_json_value(), 0..=5)
}

/// Arbitrary `IrMessage`.
fn arb_message() -> impl Strategy<Value = IrMessage> {
    (
        arb_role(),
        prop::collection::vec(arb_content_block(), 0..=5),
        arb_metadata(),
    )
        .prop_map(|(role, content, metadata)| IrMessage {
            role,
            content,
            metadata,
        })
}

/// Arbitrary `IrMessage` with only text blocks.
fn arb_text_message() -> impl Strategy<Value = IrMessage> {
    (arb_role(), prop::collection::vec(arb_text(), 1..=3)).prop_map(|(role, texts)| {
        let content = texts
            .into_iter()
            .map(|text| IrContentBlock::Text { text })
            .collect();
        IrMessage::new(role, content)
    })
}

/// Arbitrary system instruction message.
fn arb_system_message() -> impl Strategy<Value = IrMessage> {
    arb_text().prop_map(|text| IrMessage::text(IrRole::System, text))
}

/// Arbitrary `IrToolDefinition`.
fn arb_tool_definition() -> impl Strategy<Value = IrToolDefinition> {
    (arb_ident(), arb_text(), arb_json_schema()).prop_map(|(name, description, parameters)| {
        IrToolDefinition {
            name,
            description,
            parameters,
        }
    })
}

/// Arbitrary `IrConversation`.
fn arb_conversation() -> impl Strategy<Value = IrConversation> {
    prop::collection::vec(arb_message(), 0..=8).prop_map(IrConversation::from_messages)
}

/// Arbitrary `IrUsage`.
fn arb_usage() -> impl Strategy<Value = IrUsage> {
    (any::<u32>(), any::<u32>(), any::<u32>(), any::<u32>()).prop_map(|(inp, out, cr, cw)| {
        let input_tokens = inp as u64;
        let output_tokens = out as u64;
        IrUsage {
            input_tokens,
            output_tokens,
            total_tokens: input_tokens + output_tokens,
            cache_read_tokens: cr as u64,
            cache_write_tokens: cw as u64,
        }
    })
}

// ═══════════════════════════════════════════════════════════════════════
// 1. IrMessage (request-equivalent) roundtrip
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn ir_message_json_roundtrip(msg in arb_message()) {
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&msg, &back);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. IrConversation (response-equivalent) roundtrip
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn ir_conversation_json_roundtrip(conv in arb_conversation()) {
        let json = serde_json::to_string(&conv).unwrap();
        let back: IrConversation = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&conv, &back);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. All IrRole variants roundtrip through serde
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn ir_role_roundtrip(role in arb_role()) {
        let json = serde_json::to_string(&role).unwrap();
        let back: IrRole = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(role, back);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn ir_role_serializes_to_snake_case(role in arb_role()) {
        let json = serde_json::to_string(&role).unwrap();
        let expected = match role {
            IrRole::System => r#""system""#,
            IrRole::User => r#""user""#,
            IrRole::Assistant => r#""assistant""#,
            IrRole::Tool => r#""tool""#,
        };
        prop_assert_eq!(json.as_str(), expected);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. All IrContentBlock variants roundtrip through serde
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn ir_content_block_roundtrip(block in arb_content_block()) {
        let json = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&block, &back);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn ir_content_block_has_type_discriminator(block in arb_content_block()) {
        let v: serde_json::Value = serde_json::to_value(&block).unwrap();
        let type_field = v.get("type").expect("missing 'type' discriminator");
        let valid = ["text", "image", "tool_use", "tool_result", "thinking"];
        prop_assert!(
            valid.contains(&type_field.as_str().unwrap()),
            "unexpected type: {:?}",
            type_field
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn ir_text_block_roundtrip(text in arb_text()) {
        let block = IrContentBlock::Text { text: text.clone() };
        let json = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(IrContentBlock::Text { text }, back);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn ir_image_block_roundtrip(mime in arb_mime(), data in arb_base64()) {
        let block = IrContentBlock::Image {
            media_type: mime.clone(),
            data: data.clone(),
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(block, back);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn ir_thinking_block_roundtrip(text in arb_text()) {
        let block = IrContentBlock::Thinking { text: text.clone() };
        let json = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(IrContentBlock::Thinking { text }, back);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Arbitrary nested messages roundtrip
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn nested_tool_result_roundtrip(
        tool_use_id in arb_ident(),
        inner_blocks in prop::collection::vec(arb_leaf_content_block(), 1..=4),
        is_error in any::<bool>(),
    ) {
        let block = IrContentBlock::ToolResult {
            tool_use_id,
            content: inner_blocks,
            is_error,
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&block, &back);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn deeply_nested_message_roundtrip(
        role in arb_role(),
        tool_use_id in arb_ident(),
        leaf_text in arb_text(),
        is_error in any::<bool>(),
        meta_key in arb_ident(),
        meta_val in arb_json_value(),
    ) {
        let nested = IrContentBlock::ToolResult {
            tool_use_id,
            content: vec![
                IrContentBlock::Text { text: leaf_text.clone() },
                IrContentBlock::Thinking { text: leaf_text },
            ],
            is_error,
        };
        let mut metadata = BTreeMap::new();
        metadata.insert(meta_key, meta_val);
        let msg = IrMessage {
            role,
            content: vec![nested],
            metadata,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&msg, &back);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn tool_result_preserves_is_error_flag(
        tool_use_id in arb_ident(),
        is_error in any::<bool>(),
    ) {
        let block = IrContentBlock::ToolResult {
            tool_use_id,
            content: vec![IrContentBlock::Text { text: "result".into() }],
            is_error,
        };
        let v: serde_json::Value = serde_json::to_value(&block).unwrap();
        prop_assert_eq!(v["is_error"].as_bool().unwrap(), is_error);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Tool definitions roundtrip
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn ir_tool_definition_roundtrip(td in arb_tool_definition()) {
        let json = serde_json::to_string(&td).unwrap();
        let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&td, &back);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn ir_tool_definition_preserves_schema(td in arb_tool_definition()) {
        let json = serde_json::to_string(&td).unwrap();
        let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&td.parameters, &back.parameters);
        prop_assert_eq!(&td.name, &back.name);
        prop_assert_eq!(&td.description, &back.description);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn tool_definition_vec_roundtrip(
        defs in prop::collection::vec(arb_tool_definition(), 0..=6),
    ) {
        let json = serde_json::to_string(&defs).unwrap();
        let back: Vec<IrToolDefinition> = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&defs, &back);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. System instructions roundtrip
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn system_message_roundtrip(msg in arb_system_message()) {
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&msg, &back);
        prop_assert_eq!(back.role, IrRole::System);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn system_message_text_content_preserved(text in arb_text()) {
        let msg = IrMessage::text(IrRole::System, &text);
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back.text_content(), text);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn conversation_system_message_survives_roundtrip(
        system_text in arb_text(),
        user_text in arb_text(),
    ) {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, &system_text))
            .push(IrMessage::text(IrRole::User, &user_text));
        let json = serde_json::to_string(&conv).unwrap();
        let back: IrConversation = serde_json::from_str(&json).unwrap();
        let sys = back.system_message().expect("system message missing after roundtrip");
        prop_assert_eq!(sys.text_content(), system_text);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Temperature / max_tokens constraints preserved (via metadata)
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn temperature_in_metadata_preserved(temp in 0.0f64..=2.0) {
        let mut metadata = BTreeMap::new();
        metadata.insert("temperature".into(), json!(temp));
        let msg = IrMessage {
            role: IrRole::System,
            content: vec![IrContentBlock::Text { text: "cfg".into() }],
            metadata,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        let back_temp = back.metadata["temperature"].as_f64().unwrap();
        // JSON f64 roundtrip may lose ULP precision; allow tiny delta.
        prop_assert!((back_temp - temp).abs() < 1e-10);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn max_tokens_in_metadata_preserved(max_tokens in 1u64..=100_000) {
        let mut metadata = BTreeMap::new();
        metadata.insert("max_tokens".into(), json!(max_tokens));
        let msg = IrMessage {
            role: IrRole::System,
            content: vec![IrContentBlock::Text { text: "cfg".into() }],
            metadata,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back.metadata["max_tokens"].as_u64().unwrap(), max_tokens);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn combined_constraints_in_metadata(
        temp in 0.0f64..=2.0,
        max_tokens in 1u64..=100_000,
        top_p in 0.0f64..=1.0,
    ) {
        let mut metadata = BTreeMap::new();
        metadata.insert("temperature".into(), json!(temp));
        metadata.insert("max_tokens".into(), json!(max_tokens));
        metadata.insert("top_p".into(), json!(top_p));
        let msg = IrMessage {
            role: IrRole::System,
            content: vec![],
            metadata,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back.metadata["max_tokens"].as_u64().unwrap(), max_tokens);
        prop_assert!((back.metadata["temperature"].as_f64().unwrap() - temp).abs() < 1e-10);
        prop_assert!((back.metadata["top_p"].as_f64().unwrap() - top_p).abs() < 1e-10);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Metadata maps with arbitrary keys/values roundtrip
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn metadata_map_roundtrip(meta in arb_metadata()) {
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text { text: "hi".into() }],
            metadata: meta.clone(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&meta, &back.metadata);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn empty_metadata_omitted_in_json(msg in arb_text_message()) {
        // IrMessage uses skip_serializing_if = "BTreeMap::is_empty"
        let json = serde_json::to_string(&msg).unwrap();
        if msg.metadata.is_empty() {
            prop_assert!(
                !json.contains("metadata"),
                "empty metadata should be omitted"
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn metadata_with_nested_json_roundtrip(key in arb_ident()) {
        let nested = json!({
            "config": {"retry": 3, "timeout_ms": 5000},
            "tags": ["alpha", "beta"],
            "enabled": true
        });
        let mut metadata = BTreeMap::new();
        metadata.insert(key.clone(), nested.clone());
        let msg = IrMessage {
            role: IrRole::Assistant,
            content: vec![],
            metadata,
        };
        let json_str = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json_str).unwrap();
        prop_assert_eq!(&back.metadata[&key], &nested);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn metadata_keys_are_deterministically_ordered(
        meta in arb_metadata(),
    ) {
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![],
            metadata: meta,
        };
        let json1 = serde_json::to_string(&msg).unwrap();
        let json2 = serde_json::to_string(&msg).unwrap();
        // BTreeMap guarantees deterministic key ordering
        prop_assert_eq!(&json1, &json2);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Large message lists roundtrip correctly
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn large_conversation_roundtrip(
        msgs in prop::collection::vec(arb_message(), 20..=50),
    ) {
        let conv = IrConversation::from_messages(msgs);
        let json = serde_json::to_string(&conv).unwrap();
        let back: IrConversation = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&conv, &back);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn large_conversation_preserves_length(
        msgs in prop::collection::vec(arb_message(), 20..=50),
    ) {
        let len = msgs.len();
        let conv = IrConversation::from_messages(msgs);
        let json = serde_json::to_string(&conv).unwrap();
        let back: IrConversation = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back.len(), len);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn large_tool_definition_list_roundtrip(
        defs in prop::collection::vec(arb_tool_definition(), 10..=30),
    ) {
        let json = serde_json::to_string(&defs).unwrap();
        let back: Vec<IrToolDefinition> = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&defs, &back);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    #[test]
    fn large_message_many_content_blocks(
        blocks in prop::collection::vec(arb_content_block(), 15..=30),
    ) {
        let msg = IrMessage {
            role: IrRole::Assistant,
            content: blocks,
            metadata: BTreeMap::new(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&msg, &back);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Additional roundtrip and invariant tests
// ═══════════════════════════════════════════════════════════════════════

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn ir_usage_roundtrip(usage in arb_usage()) {
        let json = serde_json::to_string(&usage).unwrap();
        let back: IrUsage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(usage, back);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn ir_usage_total_tokens_invariant(
        input_tokens in any::<u32>(),
        output_tokens in any::<u32>(),
    ) {
        let u = IrUsage::from_io(input_tokens as u64, output_tokens as u64);
        prop_assert_eq!(u.total_tokens, u.input_tokens + u.output_tokens);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn ir_usage_merge_is_commutative(a in arb_usage(), b in arb_usage()) {
        let ab = a.merge(b);
        let ba = b.merge(a);
        prop_assert_eq!(ab.input_tokens, ba.input_tokens);
        prop_assert_eq!(ab.output_tokens, ba.output_tokens);
        prop_assert_eq!(ab.total_tokens, ba.total_tokens);
        prop_assert_eq!(ab.cache_read_tokens, ba.cache_read_tokens);
        prop_assert_eq!(ab.cache_write_tokens, ba.cache_write_tokens);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn ir_usage_merge_identity(u in arb_usage()) {
        let zero = IrUsage::default();
        let merged = u.merge(zero);
        prop_assert_eq!(u, merged);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn ir_message_text_only_predicate(msg in arb_text_message()) {
        prop_assert!(msg.is_text_only());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn ir_message_pretty_json_roundtrip(msg in arb_message()) {
        let json = serde_json::to_string_pretty(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&msg, &back);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn ir_conversation_value_roundtrip(conv in arb_conversation()) {
        // Round-trip through serde_json::Value intermediate representation.
        let val = serde_json::to_value(&conv).unwrap();
        let back: IrConversation = serde_json::from_value(val).unwrap();
        prop_assert_eq!(&conv, &back);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn ir_tool_use_block_preserves_input(
        id in arb_ident(),
        name in arb_ident(),
        input in arb_json_value(),
    ) {
        let block = IrContentBlock::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: IrContentBlock = serde_json::from_str(&json).unwrap();
        if let IrContentBlock::ToolUse {
            id: back_id,
            name: back_name,
            input: back_input,
        } = back
        {
            prop_assert_eq!(&id, &back_id);
            prop_assert_eq!(&name, &back_name);
            prop_assert_eq!(&input, &back_input);
        } else {
            prop_assert!(false, "deserialized to wrong variant");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn ir_conversation_push_preserves_order(
        msgs in prop::collection::vec(arb_text_message(), 2..=10),
    ) {
        let mut conv = IrConversation::new();
        for m in &msgs {
            conv = conv.push(m.clone());
        }
        let json = serde_json::to_string(&conv).unwrap();
        let back: IrConversation = serde_json::from_str(&json).unwrap();
        for (i, (orig, roundtripped)) in msgs.iter().zip(back.messages.iter()).enumerate() {
            prop_assert_eq!(
                orig, roundtripped,
                "mismatch at index {}", i
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn ir_conversation_messages_by_role_count_preserved(conv in arb_conversation()) {
        let json = serde_json::to_string(&conv).unwrap();
        let back: IrConversation = serde_json::from_str(&json).unwrap();
        for role in [IrRole::System, IrRole::User, IrRole::Assistant, IrRole::Tool] {
            prop_assert_eq!(
                conv.messages_by_role(role).len(),
                back.messages_by_role(role).len(),
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn ir_message_double_roundtrip(msg in arb_message()) {
        let json1 = serde_json::to_string(&msg).unwrap();
        let mid: IrMessage = serde_json::from_str(&json1).unwrap();
        let json2 = serde_json::to_string(&mid).unwrap();
        // Deterministic serialization: both JSON strings must be identical.
        prop_assert_eq!(&json1, &json2);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn ir_conversation_double_roundtrip(conv in arb_conversation()) {
        let json1 = serde_json::to_string(&conv).unwrap();
        let mid: IrConversation = serde_json::from_str(&json1).unwrap();
        let json2 = serde_json::to_string(&mid).unwrap();
        prop_assert_eq!(&json1, &json2);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn ir_tool_definition_double_roundtrip(td in arb_tool_definition()) {
        let json1 = serde_json::to_string(&td).unwrap();
        let mid: IrToolDefinition = serde_json::from_str(&json1).unwrap();
        let json2 = serde_json::to_string(&mid).unwrap();
        prop_assert_eq!(&json1, &json2);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn ir_usage_with_cache_roundtrip(
        inp in 0u32..100_000,
        out in 0u32..100_000,
        cr in 0u32..50_000,
        cw in 0u32..50_000,
    ) {
        let u = IrUsage::with_cache(inp as u64, out as u64, cr as u64, cw as u64);
        let json = serde_json::to_string(&u).unwrap();
        let back: IrUsage = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(u, back);
    }
}
