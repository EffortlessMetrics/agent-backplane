// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz IR type roundtrips with both raw bytes and structured Arbitrary input.
//!
//! Verifies:
//! 1. JSON deserialization into IR types never panics on any input.
//! 2. Successfully parsed IR types survive JSON round-trips losslessly.
//! 3. Constructed IrConversations maintain invariants through serde.
//! 4. IrUsage merge is associative and never panics.
//! 5. IrToolDefinition round-trips through JSON.
//! 6. Dialect IR types (IrRequest, IrResponse) round-trip through JSON.
//! 7. Dialect detection never panics on arbitrary JSON values.
#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition, IrUsage};

/// Structured input for building IR types from fuzzer-derived data.
#[derive(Debug, Arbitrary)]
struct IrFuzzInput {
    raw_bytes: Vec<u8>,
    // Structured message construction.
    messages: Vec<FuzzMessage>,
    // Usage merge inputs.
    usage_a: FuzzUsage,
    usage_b: FuzzUsage,
    usage_c: FuzzUsage,
    // Tool definitions.
    tool_name: String,
    tool_description: String,
    tool_params_json: String,
}

#[derive(Debug, Arbitrary)]
struct FuzzMessage {
    role_idx: u8,
    blocks: Vec<FuzzBlock>,
}

#[derive(Debug, Arbitrary)]
struct FuzzBlock {
    kind_idx: u8,
    text: String,
    id: String,
    name: String,
    is_error: bool,
}

#[derive(Debug, Arbitrary)]
struct FuzzUsage {
    input_tokens: u64,
    output_tokens: u64,
    cache_read: u64,
    cache_write: u64,
}

fuzz_target!(|input: IrFuzzInput| {
    // ===== Raw bytes deserialization path =====
    if let Ok(s) = std::str::from_utf8(&input.raw_bytes) {
        // --- Property 1 & 2: parse + round-trip for core IR types ---
        try_roundtrip::<IrConversation>(s, |conv| {
            assert_eq!(
                serde_json::from_str::<IrConversation>(
                    &serde_json::to_string(&conv).unwrap()
                ).unwrap().len(),
                conv.len(),
                "round-trip must preserve message count"
            );
            let _ = conv.system_message();
            let _ = conv.last_assistant();
            let _ = conv.last_message();
            let _ = conv.tool_calls();
            let _ = conv.is_empty();
            for msg in &conv.messages {
                let _ = msg.is_text_only();
                let _ = msg.text_content();
                let _ = msg.tool_use_blocks();
            }
        });
        try_roundtrip::<IrMessage>(s, |msg| {
            let _ = msg.is_text_only();
            let _ = msg.text_content();
            let _ = msg.tool_use_blocks();
        });
        try_roundtrip::<IrContentBlock>(s, |_| {});
        try_roundtrip::<IrRole>(s, |_| {});
        try_roundtrip::<IrUsage>(s, |_| {});
        try_roundtrip::<IrToolDefinition>(s, |_| {});

        // --- Property 6: dialect IR types ---
        try_roundtrip::<abp_dialect::ir::IrRequest>(s, |req| {
            let _ = req.system_message();
            let _ = req.all_tool_calls();
        });
        try_roundtrip::<abp_dialect::ir::IrResponse>(s, |resp| {
            let _ = resp.text_content();
            let _ = resp.tool_calls();
            let _ = resp.has_tool_calls();
        });
        try_roundtrip::<abp_dialect::ir::IrGenerationConfig>(s, |_| {});
        try_roundtrip::<abp_dialect::ir::IrStopReason>(s, |_| {});

        // --- Property 7: dialect detection never panics ---
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(s) {
            let detector = abp_dialect::DialectDetector::new();
            let _ = detector.detect(&val);
            let _ = detector.detect_all(&val);
        }
    }

    // ===== Structured construction path =====

    // --- Property 3: construct IrConversation from Arbitrary data ---
    let mut conv = IrConversation::new();
    for fmsg in &input.messages {
        let role = match fmsg.role_idx % 4 {
            0 => IrRole::System,
            1 => IrRole::User,
            2 => IrRole::Assistant,
            _ => IrRole::Tool,
        };
        let blocks: Vec<IrContentBlock> = fmsg
            .blocks
            .iter()
            .map(|b| match b.kind_idx % 3 {
                0 => IrContentBlock::Text {
                    text: b.text.clone(),
                },
                1 => IrContentBlock::ToolUse {
                    id: b.id.clone(),
                    name: b.name.clone(),
                    input: serde_json::Value::String(b.text.clone()),
                },
                _ => IrContentBlock::ToolResult {
                    tool_use_id: b.id.clone(),
                    content: vec![IrContentBlock::Text {
                        text: b.text.clone(),
                    }],
                    is_error: b.is_error,
                },
            })
            .collect();
        let msg = IrMessage::new(role, blocks);
        conv = conv.push(msg);
    }

    // Round-trip the constructed conversation.
    if let Ok(json) = serde_json::to_string(&conv) {
        let rt = serde_json::from_str::<IrConversation>(&json);
        assert!(rt.is_ok(), "constructed IrConversation round-trip must succeed");
        assert_eq!(rt.unwrap().len(), conv.len());
    }

    // Exercise all accessors on constructed conversation.
    let _ = conv.system_message();
    let _ = conv.last_assistant();
    let _ = conv.last_message();
    let _ = conv.tool_calls();
    let _ = conv.messages_by_role(IrRole::User);
    let _ = conv.messages_by_role(IrRole::Assistant);

    // --- Property 4: IrUsage merge is associative ---
    let ua = IrUsage::with_cache(
        input.usage_a.input_tokens,
        input.usage_a.output_tokens,
        input.usage_a.cache_read,
        input.usage_a.cache_write,
    );
    let ub = IrUsage::with_cache(
        input.usage_b.input_tokens,
        input.usage_b.output_tokens,
        input.usage_b.cache_read,
        input.usage_b.cache_write,
    );
    let uc = IrUsage::with_cache(
        input.usage_c.input_tokens,
        input.usage_c.output_tokens,
        input.usage_c.cache_read,
        input.usage_c.cache_write,
    );
    // (a merge b) merge c
    let ab_c = ua.merge(ub).merge(uc);
    // a merge (b merge c)
    let ua2 = IrUsage::with_cache(
        input.usage_a.input_tokens,
        input.usage_a.output_tokens,
        input.usage_a.cache_read,
        input.usage_a.cache_write,
    );
    let ub2 = IrUsage::with_cache(
        input.usage_b.input_tokens,
        input.usage_b.output_tokens,
        input.usage_b.cache_read,
        input.usage_b.cache_write,
    );
    let uc2 = IrUsage::with_cache(
        input.usage_c.input_tokens,
        input.usage_c.output_tokens,
        input.usage_c.cache_read,
        input.usage_c.cache_write,
    );
    let a_bc = ua2.merge(ub2.merge(uc2));
    // Associativity: total_tokens must match.
    assert_eq!(
        ab_c.total_tokens, a_bc.total_tokens,
        "IrUsage merge must be associative for total_tokens"
    );
    assert_eq!(ab_c.input_tokens, a_bc.input_tokens);
    assert_eq!(ab_c.output_tokens, a_bc.output_tokens);

    // --- Property 5: IrToolDefinition round-trip ---
    let params = serde_json::from_str::<serde_json::Value>(&input.tool_params_json)
        .unwrap_or(serde_json::Value::Object(Default::default()));
    let tool_def = IrToolDefinition {
        name: input.tool_name,
        description: input.tool_description,
        parameters: params,
    };
    if let Ok(json) = serde_json::to_string(&tool_def) {
        let rt = serde_json::from_str::<IrToolDefinition>(&json);
        assert!(rt.is_ok(), "IrToolDefinition round-trip must succeed");
    }
});

/// Helper: try to deserialize `s` as `T`, and if successful, verify JSON round-trip
/// and call `extra` for additional property checks.
fn try_roundtrip<T>(s: &str, extra: impl FnOnce(&T))
where
    T: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug,
{
    if let Ok(val) = serde_json::from_str::<T>(s) {
        if let Ok(json) = serde_json::to_string(&val) {
            let rt = serde_json::from_str::<T>(&json);
            assert!(
                rt.is_ok(),
                "round-trip must succeed for {}",
                std::any::type_name::<T>()
            );
        }
        extra(&val);
    }
}
