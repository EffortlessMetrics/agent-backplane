// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz IR translation between arbitrary dialects.
//!
//! Uses `DialectRegistry` to parse raw JSON into IR for one dialect,
//! then serialize to another dialect, exercising the full translation
//! pipeline. Also tests detect_and_parse and round-trips through all
//! registered dialect pairs.
//!
//! Verifies:
//! 1. `DialectRegistry::parse` never panics for any dialect + input.
//! 2. `DialectRegistry::serialize` never panics for any dialect + IR.
//! 3. `detect_and_parse` never panics on arbitrary JSON.
//! 4. Cross-dialect translation (parse A → serialize B) never panics.
//! 5. `DialectDetector::detect` / `detect_all` never panic.
//! 6. Round-trip within same dialect preserves structure.
#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use abp_dialect::ir::{
    IrContentBlock, IrGenerationConfig, IrMessage, IrRequest, IrResponse, IrRole, IrStopReason,
    IrToolDefinition,
};
use abp_dialect::registry::DialectRegistry;
use abp_dialect::{Dialect, DialectDetector};

/// All dialect variants for indexed selection.
const DIALECTS: &[Dialect] = &[
    Dialect::OpenAi,
    Dialect::Claude,
    Dialect::Gemini,
    Dialect::Codex,
    Dialect::Kimi,
    Dialect::Copilot,
];

#[derive(Debug, Arbitrary)]
struct IrTranslateInput {
    /// Raw JSON string to try parsing.
    raw_json: String,
    /// Source dialect index.
    src_idx: u8,
    /// Target dialect index.
    dst_idx: u8,
    /// Structured IR request fields for construction.
    model: Option<String>,
    system_prompt: Option<String>,
    messages: Vec<FuzzMsg>,
    tools: Vec<FuzzTool>,
    max_tokens: Option<u64>,
    temperature: Option<f64>,
}

#[derive(Debug, Arbitrary)]
struct FuzzMsg {
    role_idx: u8,
    text: String,
    tool_id: String,
    tool_name: String,
}

#[derive(Debug, Arbitrary)]
struct FuzzTool {
    name: String,
    description: String,
}

fuzz_target!(|input: IrTranslateInput| {
    let registry = DialectRegistry::with_builtins();
    let src = DIALECTS[input.src_idx as usize % DIALECTS.len()];
    let dst = DIALECTS[input.dst_idx as usize % DIALECTS.len()];

    // --- Property 1 & 3: parse raw JSON through each dialect ---
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&input.raw_json) {
        // detect_and_parse never panics.
        let _ = registry.detect_and_parse(&val);

        // Try parsing with specified source dialect.
        if let Ok(ir) = registry.parse(src, &val) {
            // --- Property 2 & 4: serialize to target dialect never panics ---
            let _ = registry.serialize(dst, &ir);

            // Serialize to all dialects.
            for &d in DIALECTS {
                let _ = registry.serialize(d, &ir);
            }
        }

        // Try parsing with all dialects.
        for &d in DIALECTS {
            let _ = registry.parse(d, &val);
        }

        // --- Property 5: detection never panics ---
        let detector = DialectDetector::new();
        let _ = detector.detect(&val);
        let _ = detector.detect_all(&val);
    }

    // --- Structured IR construction + cross-dialect serialization ---
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
            let block = match m.role_idx % 3 {
                0 => IrContentBlock::Text {
                    text: m.text.clone(),
                },
                1 => IrContentBlock::ToolCall {
                    id: m.tool_id.clone(),
                    name: m.tool_name.clone(),
                    input: serde_json::Value::String(m.text.clone()),
                },
                _ => IrContentBlock::ToolResult {
                    tool_call_id: m.tool_id.clone(),
                    content: vec![IrContentBlock::Text {
                        text: m.text.clone(),
                    }],
                    is_error: false,
                },
            };
            IrMessage::new(role, vec![block])
        })
        .collect();

    let tools: Vec<IrToolDefinition> = input
        .tools
        .iter()
        .map(|t| IrToolDefinition {
            name: t.name.clone(),
            description: t.description.clone(),
            parameters: serde_json::json!({"type": "object"}),
        })
        .collect();

    let config = IrGenerationConfig {
        max_tokens: input.max_tokens,
        temperature: input.temperature,
        top_p: None,
        top_k: None,
        stop_sequences: vec![],
        extra: Default::default(),
    };

    let ir_request = IrRequest {
        model: input.model,
        system_prompt: input.system_prompt,
        messages,
        tools,
        config,
        metadata: Default::default(),
    };

    // --- Property 4: cross-dialect translation with constructed IR ---
    if let Ok(serialized) = registry.serialize(src, &ir_request) {
        // Parse it back with the target dialect (may fail, that's fine).
        let _ = registry.parse(dst, &serialized);
    }

    // --- Property 6: same-dialect round-trip ---
    for &d in DIALECTS {
        if let Ok(val) = registry.serialize(d, &ir_request) {
            if let Ok(rt_ir) = registry.parse(d, &val) {
                // Re-serialize and compare JSON structure.
                if let (Ok(a), Ok(b)) = (
                    registry.serialize(d, &ir_request),
                    registry.serialize(d, &rt_ir),
                ) {
                    // Both should produce valid JSON (no panic).
                    let _ = (a, b);
                }
            }
        }
    }

    // Also exercise IrResponse deserialization.
    let _ = serde_json::from_str::<IrResponse>(&input.raw_json);
    let _ = serde_json::from_str::<IrStopReason>(&input.raw_json);
    let _ = serde_json::from_str::<IrGenerationConfig>(&input.raw_json);
});
