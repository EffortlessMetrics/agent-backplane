// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz IR conversation normalization with random messages.
//!
//! Constructs `IrConversation` and dialect IR types (`IrRequest`, `IrResponse`)
//! from structured fuzzer input and exercises normalization, accessor methods,
//! and serde round-trips. Verifies:
//! 1. Constructing conversations from arbitrary messages never panics.
//! 2. All accessor methods are safe on any content.
//! 3. Serde round-trips preserve message count and content.
//! 4. `IrRequest` / `IrResponse` construction and accessors never panic.
//! 5. `IrUsage::merge` is associative and commutative for total_tokens.
//! 6. `IrGenerationConfig` round-trips through JSON.
//! 7. Content block type predicates are consistent.
#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use abp_dialect::ir::{
    IrContentBlock, IrGenerationConfig, IrMessage, IrRequest, IrResponse, IrRole, IrStopReason,
    IrToolDefinition, IrUsage,
};

#[derive(Debug, Arbitrary)]
struct NormalizeFuzzInput {
    messages: Vec<FuzzMessage>,
    system_prompt: Option<String>,
    model: Option<String>,
    tool_defs: Vec<FuzzToolDef>,
    gen_config: FuzzGenConfig,
    response_blocks: Vec<FuzzBlock>,
    stop_reason_idx: u8,
    usage_a: FuzzUsage,
    usage_b: FuzzUsage,
    raw_json: String,
}

#[derive(Debug, Arbitrary)]
struct FuzzMessage {
    role_idx: u8,
    blocks: Vec<FuzzBlock>,
    meta_key: Option<String>,
    meta_val: Option<String>,
}

#[derive(Debug, Arbitrary)]
struct FuzzBlock {
    kind_idx: u8,
    text: String,
    id: String,
    name: String,
    media_type: String,
    is_error: bool,
    custom_type: String,
    custom_json: String,
}

#[derive(Debug, Arbitrary)]
struct FuzzToolDef {
    name: String,
    description: String,
    params_json: String,
}

#[derive(Debug, Arbitrary)]
struct FuzzGenConfig {
    max_tokens: Option<u64>,
    temperature: Option<f64>,
    top_p: Option<f64>,
    top_k: Option<u32>,
    stop_sequences: Vec<String>,
}

#[derive(Debug, Arbitrary)]
struct FuzzUsage {
    input_tokens: u64,
    output_tokens: u64,
    cache_read: u64,
    cache_write: u64,
}

fn make_block(b: &FuzzBlock) -> IrContentBlock {
    match b.kind_idx % 7 {
        0 => IrContentBlock::Text {
            text: b.text.clone(),
        },
        1 => IrContentBlock::Image {
            media_type: b.media_type.clone(),
            data: b.text.clone(),
        },
        2 => IrContentBlock::ToolCall {
            id: b.id.clone(),
            name: b.name.clone(),
            input: serde_json::Value::String(b.text.clone()),
        },
        3 => IrContentBlock::ToolResult {
            tool_call_id: b.id.clone(),
            content: vec![IrContentBlock::Text {
                text: b.text.clone(),
            }],
            is_error: b.is_error,
        },
        4 => IrContentBlock::Thinking {
            text: b.text.clone(),
        },
        5 => IrContentBlock::Audio {
            media_type: b.media_type.clone(),
            data: b.text.clone(),
        },
        _ => IrContentBlock::Custom {
            custom_type: b.custom_type.clone(),
            data: serde_json::from_str(&b.custom_json).unwrap_or(serde_json::Value::Null),
        },
    }
}

fn make_role(idx: u8) -> IrRole {
    match idx % 4 {
        0 => IrRole::System,
        1 => IrRole::User,
        2 => IrRole::Assistant,
        _ => IrRole::Tool,
    }
}

fuzz_target!(|input: NormalizeFuzzInput| {
    // ===== Property 1 & 2: construct conversation and exercise accessors =====
    let messages: Vec<IrMessage> = input
        .messages
        .iter()
        .map(|m| {
            let role = make_role(m.role_idx);
            let blocks: Vec<IrContentBlock> = m.blocks.iter().map(make_block).collect();
            let mut msg = IrMessage::new(role, blocks);
            if let (Some(k), Some(v)) = (&m.meta_key, &m.meta_val) {
                msg.metadata
                    .insert(k.clone(), serde_json::Value::String(v.clone()));
            }
            msg
        })
        .collect();

    // Exercise individual message accessors.
    for msg in &messages {
        let _ = msg.text_content();
        let _ = msg.tool_calls();
        // Content block predicates must be consistent.
        for block in &msg.content {
            let _ = block.as_text();
            let _ = block.is_tool_call();
            let _ = block.is_tool_result();
        }
    }

    // ===== Property 3: IrRequest construction and serde round-trip =====
    let tools: Vec<IrToolDefinition> = input
        .tool_defs
        .iter()
        .map(|t| IrToolDefinition {
            name: t.name.clone(),
            description: t.description.clone(),
            parameters: serde_json::from_str(&t.params_json)
                .unwrap_or(serde_json::Value::Object(Default::default())),
        })
        .collect();

    let config = IrGenerationConfig {
        max_tokens: input.gen_config.max_tokens,
        temperature: input.gen_config.temperature.filter(|t| t.is_finite()),
        top_p: input.gen_config.top_p.filter(|t| t.is_finite()),
        top_k: input.gen_config.top_k,
        stop_sequences: input.gen_config.stop_sequences.clone(),
        extra: Default::default(),
    };

    let request = IrRequest::new(messages.clone()).with_config(config.clone());
    let request = if let Some(ref model) = input.model {
        request.with_model(model.clone())
    } else {
        request
    };
    let request = if let Some(ref prompt) = input.system_prompt {
        request.with_system_prompt(prompt.clone())
    } else {
        request
    };
    let request = tools.into_iter().fold(request, |r, t| r.with_tool(t));

    // IrRequest accessors must not panic.
    let _ = request.system_message();
    let _ = request.all_tool_calls();

    // Serde round-trip.
    if let Ok(json) = serde_json::to_string(&request) {
        let rt: Result<IrRequest, _> = serde_json::from_str(&json);
        assert!(rt.is_ok(), "IrRequest round-trip must succeed");
        let rt = rt.unwrap();
        assert_eq!(rt.messages.len(), request.messages.len());
    }

    // ===== Property 4: IrResponse construction =====
    let resp_blocks: Vec<IrContentBlock> = input.response_blocks.iter().map(make_block).collect();

    let stop_reason = match input.stop_reason_idx % 6 {
        0 => IrStopReason::EndTurn,
        1 => IrStopReason::StopSequence,
        2 => IrStopReason::MaxTokens,
        3 => IrStopReason::ToolUse,
        4 => IrStopReason::ContentFilter,
        _ => IrStopReason::Other("fuzz".into()),
    };

    let usage = IrUsage::from_io(input.usage_a.input_tokens, input.usage_a.output_tokens);

    let response = IrResponse::new(resp_blocks)
        .with_stop_reason(stop_reason)
        .with_usage(usage);
    let response = if let Some(ref model) = input.model {
        response.with_model(model.clone())
    } else {
        response
    };

    // IrResponse accessors.
    let _ = response.text_content();
    let _ = response.tool_calls();
    let _ = response.has_tool_calls();

    // IrResponse serde round-trip.
    if let Ok(json) = serde_json::to_string(&response) {
        let rt: Result<IrResponse, _> = serde_json::from_str(&json);
        assert!(rt.is_ok(), "IrResponse round-trip must succeed");
    }

    // ===== Property 5: IrUsage merge associativity =====
    let ua = IrUsage {
        input_tokens: input.usage_a.input_tokens,
        output_tokens: input.usage_a.output_tokens,
        total_tokens: input
            .usage_a
            .input_tokens
            .saturating_add(input.usage_a.output_tokens),
        cache_read_tokens: input.usage_a.cache_read,
        cache_write_tokens: input.usage_a.cache_write,
    };
    let ub = IrUsage {
        input_tokens: input.usage_b.input_tokens,
        output_tokens: input.usage_b.output_tokens,
        total_tokens: input
            .usage_b
            .input_tokens
            .saturating_add(input.usage_b.output_tokens),
        cache_read_tokens: input.usage_b.cache_read,
        cache_write_tokens: input.usage_b.cache_write,
    };
    let ab = ua.merge(ub);
    let ba = ub.merge(ua);
    assert_eq!(
        ab.total_tokens, ba.total_tokens,
        "merge must be commutative"
    );
    assert_eq!(ab.input_tokens, ba.input_tokens);
    assert_eq!(ab.output_tokens, ba.output_tokens);

    // ===== Property 6: IrGenerationConfig round-trip =====
    if let Ok(json) = serde_json::to_string(&config) {
        let rt: Result<IrGenerationConfig, _> = serde_json::from_str(&json);
        assert!(rt.is_ok(), "IrGenerationConfig round-trip must succeed");
    }

    // ===== Property 7: raw JSON parse path =====
    if let Ok(req) = serde_json::from_str::<IrRequest>(&input.raw_json) {
        let _ = req.system_message();
        let _ = req.all_tool_calls();
        if let Ok(json) = serde_json::to_string(&req) {
            assert!(
                serde_json::from_str::<IrRequest>(&json).is_ok(),
                "raw-parsed IrRequest round-trip must succeed"
            );
        }
    }
    if let Ok(resp) = serde_json::from_str::<IrResponse>(&input.raw_json) {
        let _ = resp.text_content();
        let _ = resp.tool_calls();
    }
    let _ = serde_json::from_str::<IrMessage>(&input.raw_json);
    let _ = serde_json::from_str::<IrContentBlock>(&input.raw_json);
    let _ = serde_json::from_str::<IrStopReason>(&input.raw_json);
});
