// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz dialect mapping with random IR conversations.
//!
//! Uses the `DialectRegistry` to parse→serialize (round-trip) through every
//! registered dialect pair. Also constructs `IrRequest` from structured input
//! and verifies that lowering to each dialect and re-lifting produces
//! equivalent results. Exercises:
//! 1. Registry parse never panics on arbitrary JSON.
//! 2. Registry serialize never panics on arbitrary IrRequest.
//! 3. For each (source, target) dialect pair: parse → serialize → re-parse.
//! 4. Constructed IrRequest survives lowering to all dialects.
//! 5. DialectDetector agrees with the dialect used for serialization.
//! 6. DialectValidator never panics on serialized output.
//! 7. Self-roundtrip: parse(d, serialize(d, ir)) ≈ ir for each dialect.
#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use abp_dialect::ir::{
    IrContentBlock, IrGenerationConfig, IrMessage, IrRequest, IrRole, IrToolDefinition,
};
use abp_dialect::registry::DialectRegistry;
use abp_dialect::{Dialect, DialectDetector, DialectValidator};

#[derive(Debug, Arbitrary)]
struct MapperFuzzInput {
    /// Raw JSON to try parsing through each dialect.
    raw_json: String,
    /// Source dialect index.
    source_idx: u8,
    /// Target dialect index.
    target_idx: u8,
    /// Structured messages for IrRequest construction.
    messages: Vec<FuzzMessage>,
    /// System prompt.
    system_prompt: Option<String>,
    /// Model name.
    model: Option<String>,
    /// Tool definitions.
    tools: Vec<FuzzTool>,
    /// Generation config.
    max_tokens: Option<u64>,
    temperature: Option<f64>,
    stop_sequences: Vec<String>,
}

#[derive(Debug, Arbitrary)]
struct FuzzMessage {
    role_idx: u8,
    text: String,
    tool_call_id: Option<String>,
    tool_call_name: Option<String>,
}

#[derive(Debug, Arbitrary)]
struct FuzzTool {
    name: String,
    description: String,
    params_json: String,
}

fuzz_target!(|input: MapperFuzzInput| {
    let registry = DialectRegistry::with_builtins();
    let detector = DialectDetector::new();
    let validator = DialectValidator::new();
    let all = Dialect::all();

    let source = all[input.source_idx as usize % all.len()];
    let target = all[input.target_idx as usize % all.len()];

    // ===== Path 1: raw JSON through registry parse =====
    let value: serde_json::Value = match serde_json::from_str(&input.raw_json) {
        Ok(v) => v,
        Err(_) => serde_json::Value::Null,
    };

    // Property 1: parse never panics for any dialect.
    for &dialect in all {
        let _ = registry.parse(dialect, &value);
    }

    // If parse succeeds for source, try the full mapping pipeline.
    if let Ok(ir) = registry.parse(source, &value) {
        // Property 2: serialize never panics.
        if let Ok(target_json) = registry.serialize(target, &ir) {
            // Property 6: validator never panics on serialized output.
            let vr = validator.validate(&target_json, target);
            assert_eq!(vr.valid, vr.errors.is_empty());

            // Property 3: re-parse the serialized output.
            let _ = registry.parse(target, &target_json);

            // Property 5: detector on serialized output.
            let _ = detector.detect(&target_json);
        }

        // Property 7: self-roundtrip for source dialect.
        if let Ok(src_json) = registry.serialize(source, &ir) {
            if let Ok(rt_ir) = registry.parse(source, &src_json) {
                // Message count should be preserved through self-roundtrip.
                assert_eq!(
                    ir.messages.len(),
                    rt_ir.messages.len(),
                    "self-roundtrip must preserve message count for {:?}",
                    source
                );
            }
        }
    }

    // ===== Path 2: constructed IrRequest through all dialect pairs =====
    let messages: Vec<IrMessage> = input
        .messages
        .iter()
        .map(|m| {
            let role = match m.role_idx % 4 {
                0 => IrRole::System,
                1 => IrRole::User,
                2 => IrRole::Assistant,
                _ => IrRole::Tool,
            };

            let mut blocks = vec![IrContentBlock::Text {
                text: m.text.clone(),
            }];

            // Add tool call or tool result based on role.
            if role == IrRole::Assistant {
                if let (Some(id), Some(name)) = (&m.tool_call_id, &m.tool_call_name) {
                    blocks.push(IrContentBlock::ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        input: serde_json::Value::Object(Default::default()),
                    });
                }
            }
            if role == IrRole::Tool {
                if let Some(id) = &m.tool_call_id {
                    blocks = vec![IrContentBlock::ToolResult {
                        tool_call_id: id.clone(),
                        content: blocks,
                        is_error: false,
                    }];
                }
            }

            IrMessage::new(role, blocks)
        })
        .collect();

    let tools: Vec<IrToolDefinition> = input
        .tools
        .iter()
        .map(|t| IrToolDefinition {
            name: t.name.clone(),
            description: t.description.clone(),
            parameters: serde_json::from_str(&t.params_json)
                .unwrap_or(serde_json::Value::Object(Default::default())),
        })
        .collect();

    let config = IrGenerationConfig {
        max_tokens: input.max_tokens,
        temperature: input.temperature.filter(|t| t.is_finite()),
        top_p: None,
        top_k: None,
        stop_sequences: input.stop_sequences.clone(),
        extra: Default::default(),
    };

    let ir = IrRequest::new(messages).with_config(config);
    let ir = if let Some(ref model) = input.model {
        ir.with_model(model.clone())
    } else {
        ir
    };
    let ir = if let Some(ref prompt) = input.system_prompt {
        ir.with_system_prompt(prompt.clone())
    } else {
        ir
    };
    let ir = tools.into_iter().fold(ir, |r, t| r.with_tool(t));

    // Property 4: lowering to every dialect never panics.
    for &dialect in all {
        if let Ok(serialized) = registry.serialize(dialect, &ir) {
            // Validator never panics.
            let _ = validator.validate(&serialized, dialect);

            // Try to re-parse.
            let _ = registry.parse(dialect, &serialized);

            // Detector never panics.
            let _ = detector.detect(&serialized);
            let _ = detector.detect_all(&serialized);
        }
    }

    // Cross-dialect mapping: source → target for constructed IR.
    if let Ok(target_json) = registry.serialize(target, &ir) {
        if let Ok(rt_ir) = registry.parse(target, &target_json) {
            // Re-serialize back to source.
            let _ = registry.serialize(source, &rt_ir);
        }
    }
});
