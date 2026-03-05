// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fuzz IR conversation deserialization with random JSON values.
//!
//! Exercises the full IR type surface with random JSON, focusing on:
//! 1. `IrConversation` deserialization never panics.
//! 2. `IrRequest` and `IrResponse` deserialization never panics.
//! 3. Normalization passes never panic on arbitrary conversations.
//! 4. All accessor methods are safe on deserialized conversations.
//! 5. `normalize_role()` never panics on arbitrary strings.
//! 6. `sort_tools()` and `normalize_tool_schemas()` never panic.
#![no_main]
use libfuzzer_sys::fuzz_target;

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};

fuzz_target!(|data: &[u8]| {
    let s = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    // --- Property 1: IrConversation deserialization ---
    if let Ok(conv) = serde_json::from_str::<IrConversation>(s) {
        exercise_conversation(&conv);

        // --- Property 3: normalization passes ---
        let deduped = abp_ir::normalize::dedup_system(&conv);
        let _ = deduped.len();

        let trimmed = abp_ir::normalize::trim_text(&conv);
        let _ = trimmed.len();

        let stripped = abp_ir::normalize::strip_empty(&conv);
        let _ = stripped.len();

        let merged = abp_ir::normalize::merge_adjacent_text(&conv);
        let _ = merged.len();

        let cleaned = abp_ir::normalize::strip_metadata(&conv, &["keep_this"]);
        let _ = cleaned.len();

        let (sys, rest) = abp_ir::normalize::extract_system(&conv);
        let _ = sys;
        let _ = rest.len();

        let normalized = abp_ir::normalize::normalize(&conv);
        let _ = normalized.len();

        // Round-trip the normalized result.
        if let Ok(json) = serde_json::to_string(&normalized) {
            let _ = serde_json::from_str::<IrConversation>(&json);
        }
    }

    // --- Property 2: IrRequest / IrResponse ---
    if let Ok(req) = serde_json::from_str::<abp_dialect::ir::IrRequest>(s) {
        let _ = req.system_message();
        let _ = req.all_tool_calls();
        if let Ok(json) = serde_json::to_string(&req) {
            let _ = serde_json::from_str::<abp_dialect::ir::IrRequest>(&json);
        }
    }
    if let Ok(resp) = serde_json::from_str::<abp_dialect::ir::IrResponse>(s) {
        let _ = resp.text_content();
        let _ = resp.tool_calls();
        let _ = resp.has_tool_calls();
    }

    // Individual types.
    let _ = serde_json::from_str::<IrMessage>(s);
    let _ = serde_json::from_str::<IrContentBlock>(s);
    let _ = serde_json::from_str::<IrToolDefinition>(s);
    let _ = serde_json::from_str::<abp_dialect::ir::IrGenerationConfig>(s);
    let _ = serde_json::from_str::<abp_dialect::ir::IrStopReason>(s);

    // --- Property 5: normalize_role on arbitrary strings ---
    let _ = abp_ir::normalize::normalize_role(s);
    // Try common role-like prefixes.
    for line in s.lines().take(10) {
        let _ = abp_ir::normalize::normalize_role(line.trim());
    }

    // --- Property 6: sort_tools and normalize_tool_schemas ---
    if let Ok(tools) = serde_json::from_str::<Vec<IrToolDefinition>>(s) {
        let mut tools_clone = tools.clone();
        abp_ir::normalize::sort_tools(&mut tools_clone);
        let _ = abp_ir::normalize::normalize_tool_schemas(&tools);
    }
});

fn exercise_conversation(conv: &IrConversation) {
    let _ = conv.len();
    let _ = conv.is_empty();
    let _ = conv.system_message();
    let _ = conv.last_assistant();
    let _ = conv.last_message();
    let _ = conv.tool_calls();
    let _ = conv.messages_by_role(IrRole::System);
    let _ = conv.messages_by_role(IrRole::User);
    let _ = conv.messages_by_role(IrRole::Assistant);
    let _ = conv.messages_by_role(IrRole::Tool);

    for msg in &conv.messages {
        let _ = msg.is_text_only();
        let _ = msg.text_content();
        let _ = msg.tool_use_blocks();
    }
}
