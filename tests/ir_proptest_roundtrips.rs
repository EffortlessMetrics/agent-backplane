#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Property-based tests for IR roundtrip invariants.
//!
//! Each test verifies that lowering an [`IrConversation`] to a vendor-specific
//! format and parsing it back preserves the semantic content (text, tool calls,
//! roles, etc.).

#![allow(dead_code)]

use abp_ir::lower::*;
use abp_ir::normalize::normalize_role;
use abp_ir::*;
use proptest::prelude::*;
use serde_json::Value;
use std::collections::BTreeMap;

// ── Proptest strategies ────────────────────────────────────────────────

/// Strategy for non-empty printable text (avoids control chars that break JSON).
fn arb_text() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 _.,!?]{1,80}"
}

/// Strategy for identifier-like strings (tool names, IDs).
fn arb_ident() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,19}"
}

fn arb_role() -> impl Strategy<Value = IrRole> {
    prop_oneof![
        Just(IrRole::System),
        Just(IrRole::User),
        Just(IrRole::Assistant),
        Just(IrRole::Tool),
    ]
}

fn arb_non_system_role() -> impl Strategy<Value = IrRole> {
    prop_oneof![
        Just(IrRole::User),
        Just(IrRole::Assistant),
        Just(IrRole::Tool),
    ]
}

fn arb_text_block() -> impl Strategy<Value = IrContentBlock> {
    arb_text().prop_map(|text| IrContentBlock::Text { text })
}

fn arb_thinking_block() -> impl Strategy<Value = IrContentBlock> {
    arb_text().prop_map(|text| IrContentBlock::Thinking { text })
}

fn arb_image_block() -> impl Strategy<Value = IrContentBlock> {
    (
        prop_oneof![Just("image/png"), Just("image/jpeg")].prop_map(String::from),
        "[A-Za-z0-9+/]{4,40}",
    )
        .prop_map(|(media_type, data)| IrContentBlock::Image { media_type, data })
}

fn arb_tool_use_block() -> impl Strategy<Value = IrContentBlock> {
    (arb_ident(), arb_ident(), arb_text()).prop_map(|(id, name, arg)| IrContentBlock::ToolUse {
        id,
        name,
        input: serde_json::json!({"arg": arg}),
    })
}

fn arb_tool_result_block() -> impl Strategy<Value = IrContentBlock> {
    (arb_ident(), arb_text(), any::<bool>()).prop_map(|(tool_use_id, text, is_error)| {
        IrContentBlock::ToolResult {
            tool_use_id,
            content: vec![IrContentBlock::Text { text }],
            is_error,
        }
    })
}

fn arb_content_block() -> impl Strategy<Value = IrContentBlock> {
    prop_oneof![
        4 => arb_text_block(),
        1 => arb_thinking_block(),
        1 => arb_image_block(),
        2 => arb_tool_use_block(),
        2 => arb_tool_result_block(),
    ]
}

/// Strategy for a text-only message with the given role.
fn arb_text_message(role: IrRole) -> impl Strategy<Value = IrMessage> {
    arb_text().prop_map(move |text| IrMessage::text(role, text))
}

/// Strategy for a message with arbitrary content blocks.
fn arb_message() -> impl Strategy<Value = IrMessage> {
    (arb_role(), prop::collection::vec(arb_content_block(), 1..4)).prop_map(|(role, content)| {
        IrMessage {
            role,
            content,
            metadata: BTreeMap::new(),
        }
    })
}

/// Strategy for a message with only text blocks (simplifies roundtrip assertions).
fn arb_text_only_message() -> impl Strategy<Value = IrMessage> {
    (arb_role(), prop::collection::vec(arb_text_block(), 1..3)).prop_map(|(role, content)| {
        IrMessage {
            role,
            content,
            metadata: BTreeMap::new(),
        }
    })
}

/// Strategy for a non-system text message.
fn arb_non_system_text_message() -> impl Strategy<Value = IrMessage> {
    (arb_non_system_role(), arb_text()).prop_map(|(role, text)| IrMessage::text(role, text))
}

/// Strategy for a conversation with an optional system message followed by user/assistant turns.
fn arb_conversation() -> impl Strategy<Value = IrConversation> {
    (
        prop::option::of(arb_text()),
        prop::collection::vec(arb_non_system_text_message(), 1..6),
    )
        .prop_map(|(sys, mut msgs)| {
            let mut all = Vec::new();
            if let Some(sys_text) = sys {
                all.push(IrMessage::text(IrRole::System, sys_text));
            }
            all.append(&mut msgs);
            IrConversation::from_messages(all)
        })
}

/// Strategy for a conversation containing tool calls.
fn arb_tool_conversation() -> impl Strategy<Value = IrConversation> {
    (arb_ident(), arb_ident(), arb_text(), arb_text()).prop_map(
        |(call_id, tool_name, arg, result)| {
            IrConversation::from_messages(vec![
                IrMessage::text(IrRole::User, "do something"),
                IrMessage::new(
                    IrRole::Assistant,
                    vec![IrContentBlock::ToolUse {
                        id: call_id.clone(),
                        name: tool_name,
                        input: serde_json::json!({"arg": arg}),
                    }],
                ),
                IrMessage::new(
                    IrRole::Tool,
                    vec![IrContentBlock::ToolResult {
                        tool_use_id: call_id,
                        content: vec![IrContentBlock::Text { text: result }],
                        is_error: false,
                    }],
                ),
            ])
        },
    )
}

fn arb_tool_definition() -> impl Strategy<Value = IrToolDefinition> {
    (arb_ident(), arb_text()).prop_map(|(name, description)| IrToolDefinition {
        name,
        description,
        parameters: serde_json::json!({"type": "object", "properties": {"x": {"type": "string"}}}),
    })
}

fn arb_usage() -> impl Strategy<Value = IrUsage> {
    (0u64..10000, 0u64..10000, 0u64..5000, 0u64..5000).prop_map(
        |(input, output, cache_read, cache_write)| {
            IrUsage::with_cache(input, output, cache_read, cache_write)
        },
    )
}

// ── Helpers ────────────────────────────────────────────────────────────

/// Extract all text strings from an OpenAI-lowered JSON value.
fn extract_openai_texts(lowered: &Value) -> Vec<String> {
    let mut texts = Vec::new();
    if let Some(msgs) = lowered["messages"].as_array() {
        for msg in msgs {
            if let Some(s) = msg["content"].as_str() {
                if !s.is_empty() {
                    texts.push(s.to_string());
                }
            }
        }
    }
    texts
}

/// Extract all text strings from a Claude-lowered JSON value.
fn extract_claude_texts(lowered: &Value) -> Vec<String> {
    let mut texts = Vec::new();
    if let Some(sys) = lowered["system"].as_str() {
        if !sys.is_empty() {
            texts.push(sys.to_string());
        }
    }
    if let Some(msgs) = lowered["messages"].as_array() {
        for msg in msgs {
            if let Some(content) = msg["content"].as_array() {
                for block in content {
                    if block["type"] == "text" {
                        if let Some(t) = block["text"].as_str() {
                            texts.push(t.to_string());
                        }
                    }
                }
            }
        }
    }
    texts
}

/// Extract all text strings from a Gemini-lowered JSON value.
fn extract_gemini_texts(lowered: &Value) -> Vec<String> {
    let mut texts = Vec::new();
    if let Some(si) = lowered.get("system_instruction") {
        if let Some(parts) = si["parts"].as_array() {
            for p in parts {
                if let Some(t) = p["text"].as_str() {
                    texts.push(t.to_string());
                }
            }
        }
    }
    if let Some(contents) = lowered["contents"].as_array() {
        for c in contents {
            if let Some(parts) = c["parts"].as_array() {
                for p in parts {
                    if let Some(t) = p["text"].as_str() {
                        texts.push(t.to_string());
                    }
                }
            }
        }
    }
    texts
}

/// Collect all text from an IrConversation.
fn ir_texts(conv: &IrConversation) -> Vec<String> {
    conv.messages
        .iter()
        .filter_map(|m| {
            let t = m.text_content();
            if t.is_empty() { None } else { Some(t) }
        })
        .collect()
}

/// Extract role strings from OpenAI-lowered JSON.
fn extract_openai_roles(lowered: &Value) -> Vec<String> {
    lowered["messages"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|m| m["role"].as_str().map(String::from))
        .collect()
}

/// Extract tool call names from OpenAI-lowered JSON.
fn extract_openai_tool_names(lowered: &Value) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(msgs) = lowered["messages"].as_array() {
        for msg in msgs {
            if let Some(calls) = msg["tool_calls"].as_array() {
                for call in calls {
                    if let Some(n) = call["function"]["name"].as_str() {
                        names.push(n.to_string());
                    }
                }
            }
        }
    }
    names
}

// ── 1. OpenAI roundtrip: text preserved ────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn openai_roundtrip_text_preserved(conv in arb_conversation()) {
        let lowered = lower_to_openai(&conv, &[]);
        let original_texts = ir_texts(&conv);
        let lowered_texts = extract_openai_texts(&lowered);
        for t in &original_texts {
            prop_assert!(
                lowered_texts.iter().any(|lt| lt.contains(t.as_str())),
                "text {:?} missing in OpenAI output", t
            );
        }
    }

    // ── 2. Claude roundtrip: text preserved ────────────────────────────

    #[test]
    fn claude_roundtrip_text_preserved(conv in arb_conversation()) {
        let lowered = lower_to_claude(&conv, &[]);
        let original_texts = ir_texts(&conv);
        let lowered_texts = extract_claude_texts(&lowered);
        for t in &original_texts {
            prop_assert!(
                lowered_texts.iter().any(|lt| lt.contains(t.as_str())),
                "text {:?} missing in Claude output", t
            );
        }
    }

    // ── 3. Gemini roundtrip: text preserved ────────────────────────────

    #[test]
    fn gemini_roundtrip_text_preserved(conv in arb_conversation()) {
        let lowered = lower_to_gemini(&conv, &[]);
        let original_texts = ir_texts(&conv);
        let lowered_texts = extract_gemini_texts(&lowered);
        for t in &original_texts {
            prop_assert!(
                lowered_texts.iter().any(|lt| lt.contains(t.as_str())),
                "text {:?} missing in Gemini output", t
            );
        }
    }

    // ── 4. Role roundtrips through all dialects ────────────────────────

    #[test]
    fn openai_role_roundtrip(role in arb_role()) {
        let role_str = ir_role_to_dialect(role, abp_sdk_types::Dialect::OpenAi);
        let parsed = normalize_role(role_str);
        prop_assert_eq!(parsed, Some(role));
    }

    #[test]
    fn claude_role_roundtrip(role in arb_non_system_role()) {
        let role_str = ir_role_to_dialect(role, abp_sdk_types::Dialect::Claude);
        let parsed = normalize_role(role_str);
        // Claude maps Tool → "user", so parsed will be User
        if role == IrRole::Tool {
            prop_assert_eq!(parsed, Some(IrRole::User));
        } else {
            prop_assert_eq!(parsed, Some(role));
        }
    }

    #[test]
    fn gemini_role_roundtrip(role in arb_non_system_role()) {
        let role_str = ir_role_to_dialect(role, abp_sdk_types::Dialect::Gemini);
        let parsed = normalize_role(role_str);
        // Gemini maps Assistant → "model", Tool → "user"
        match role {
            IrRole::Assistant => prop_assert_eq!(parsed, Some(IrRole::Assistant)),
            IrRole::Tool => prop_assert_eq!(parsed, Some(IrRole::User)),
            _ => prop_assert_eq!(parsed, Some(role)),
        }
    }

    #[test]
    fn kimi_role_roundtrip(role in arb_role()) {
        let role_str = ir_role_to_dialect(role, abp_sdk_types::Dialect::Kimi);
        let parsed = normalize_role(role_str);
        prop_assert_eq!(parsed, Some(role));
    }

    #[test]
    fn codex_role_roundtrip(role in arb_role()) {
        let role_str = ir_role_to_dialect(role, abp_sdk_types::Dialect::Codex);
        let parsed = normalize_role(role_str);
        prop_assert_eq!(parsed, Some(role));
    }

    #[test]
    fn copilot_role_roundtrip(role in arb_role()) {
        let role_str = ir_role_to_dialect(role, abp_sdk_types::Dialect::Copilot);
        let parsed = normalize_role(role_str);
        prop_assert_eq!(parsed, Some(role));
    }

    // ── 5. Text block survives all lowering paths ──────────────────────

    #[test]
    fn text_block_survives_openai(text in arb_text()) {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, &text));
        let lowered = lower_to_openai(&conv, &[]);
        let content = lowered["messages"][0]["content"].as_str().unwrap_or("");
        prop_assert_eq!(content, text.as_str());
    }

    #[test]
    fn text_block_survives_claude(text in arb_text()) {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, &text));
        let lowered = lower_to_claude(&conv, &[]);
        let block = &lowered["messages"][0]["content"][0];
        prop_assert_eq!(block["text"].as_str().unwrap(), text.as_str());
    }

    #[test]
    fn text_block_survives_gemini(text in arb_text()) {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, &text));
        let lowered = lower_to_gemini(&conv, &[]);
        let part = &lowered["contents"][0]["parts"][0];
        prop_assert_eq!(part["text"].as_str().unwrap(), text.as_str());
    }

    #[test]
    fn text_block_survives_kimi(text in arb_text()) {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, &text));
        let lowered = lower_to_kimi(&conv, &[]);
        let content = lowered["messages"][0]["content"].as_str().unwrap_or("");
        prop_assert_eq!(content, text.as_str());
    }

    #[test]
    fn text_block_survives_codex(text in arb_text()) {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, &text));
        let lowered = lower_to_codex(&conv, &[]);
        let content = lowered["messages"][0]["content"].as_str().unwrap_or("");
        prop_assert_eq!(content, text.as_str());
    }

    #[test]
    fn text_block_survives_copilot(text in arb_text()) {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, &text));
        let lowered = lower_to_copilot(&conv, &[]);
        let content = lowered["messages"][0]["content"].as_str().unwrap_or("");
        prop_assert_eq!(content, text.as_str());
    }

    // ── 6. IrUsage fields preserved across roundtrips ──────────────────

    #[test]
    fn usage_serde_roundtrip(usage in arb_usage()) {
        let json = serde_json::to_value(&usage).unwrap();
        let parsed: IrUsage = serde_json::from_value(json).unwrap();
        prop_assert_eq!(parsed, usage);
    }

    #[test]
    fn usage_from_io_total_is_sum(input in 0u64..100000, output in 0u64..100000) {
        let usage = IrUsage::from_io(input, output);
        prop_assert_eq!(usage.total_tokens, input + output);
        prop_assert_eq!(usage.cache_read_tokens, 0);
        prop_assert_eq!(usage.cache_write_tokens, 0);
    }

    #[test]
    fn usage_merge_is_additive(a in arb_usage(), b in arb_usage()) {
        let merged = a.merge(b);
        prop_assert_eq!(merged.input_tokens, a.input_tokens + b.input_tokens);
        prop_assert_eq!(merged.output_tokens, a.output_tokens + b.output_tokens);
        prop_assert_eq!(merged.cache_read_tokens, a.cache_read_tokens + b.cache_read_tokens);
        prop_assert_eq!(merged.cache_write_tokens, a.cache_write_tokens + b.cache_write_tokens);
    }

    // ── 7. Tool call names/arguments survive across dialects ───────────

    #[test]
    fn tool_call_name_survives_openai(conv in arb_tool_conversation()) {
        let lowered = lower_to_openai(&conv, &[]);
        let names = extract_openai_tool_names(&lowered);
        let ir_names: Vec<String> = conv.tool_calls().iter().filter_map(|b| match b {
            IrContentBlock::ToolUse { name, .. } => Some(name.clone()),
            _ => None,
        }).collect();
        for n in &ir_names {
            prop_assert!(names.contains(n), "tool name {:?} missing from OpenAI", n);
        }
    }

    #[test]
    fn tool_call_name_survives_claude(conv in arb_tool_conversation()) {
        let lowered = lower_to_claude(&conv, &[]);
        let lowered_str = lowered.to_string();
        let ir_names: Vec<String> = conv.tool_calls().iter().filter_map(|b| match b {
            IrContentBlock::ToolUse { name, .. } => Some(name.clone()),
            _ => None,
        }).collect();
        for n in &ir_names {
            prop_assert!(lowered_str.contains(n.as_str()), "tool name {:?} missing from Claude", n);
        }
    }

    #[test]
    fn tool_call_name_survives_gemini(conv in arb_tool_conversation()) {
        let lowered = lower_to_gemini(&conv, &[]);
        let lowered_str = lowered.to_string();
        let ir_names: Vec<String> = conv.tool_calls().iter().filter_map(|b| match b {
            IrContentBlock::ToolUse { name, .. } => Some(name.clone()),
            _ => None,
        }).collect();
        for n in &ir_names {
            prop_assert!(lowered_str.contains(n.as_str()), "tool name {:?} missing from Gemini", n);
        }
    }

    #[test]
    fn tool_call_arguments_survive_openai(conv in arb_tool_conversation()) {
        let lowered = lower_to_openai(&conv, &[]);
        if let Some(msgs) = lowered["messages"].as_array() {
            for msg in msgs {
                if let Some(calls) = msg["tool_calls"].as_array() {
                    for call in calls {
                        let args_str = call["function"]["arguments"].as_str().unwrap_or("{}");
                        let args: Value = serde_json::from_str(args_str).unwrap();
                        prop_assert!(args.is_object(), "arguments should be valid JSON object");
                    }
                }
            }
        }
    }

    #[test]
    fn tool_call_arguments_survive_claude(conv in arb_tool_conversation()) {
        let lowered = lower_to_claude(&conv, &[]);
        if let Some(msgs) = lowered["messages"].as_array() {
            for msg in msgs {
                if let Some(content) = msg["content"].as_array() {
                    for block in content {
                        if block["type"] == "tool_use" {
                            prop_assert!(block["input"].is_object(), "Claude tool input should be object");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn tool_call_arguments_survive_gemini(conv in arb_tool_conversation()) {
        let lowered = lower_to_gemini(&conv, &[]);
        if let Some(contents) = lowered["contents"].as_array() {
            for c in contents {
                if let Some(parts) = c["parts"].as_array() {
                    for part in parts {
                        if let Some(fc) = part.get("functionCall") {
                            prop_assert!(fc["args"].is_object(), "Gemini functionCall args should be object");
                        }
                    }
                }
            }
        }
    }

    // ── 8. System messages handled correctly ───────────────────────────

    #[test]
    fn system_msg_inline_for_openai(sys_text in arb_text(), user_text in arb_text()) {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, &sys_text))
            .push(IrMessage::text(IrRole::User, &user_text));
        let lowered = lower_to_openai(&conv, &[]);
        let msgs = lowered["messages"].as_array().unwrap();
        prop_assert_eq!(&msgs[0]["role"], "system");
        prop_assert_eq!(msgs[0]["content"].as_str().unwrap(), sys_text.as_str());
    }

    #[test]
    fn system_msg_extracted_for_claude(sys_text in arb_text(), user_text in arb_text()) {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, &sys_text))
            .push(IrMessage::text(IrRole::User, &user_text));
        let lowered = lower_to_claude(&conv, &[]);
        prop_assert_eq!(lowered["system"].as_str().unwrap(), sys_text.as_str());
        let msgs = lowered["messages"].as_array().unwrap();
        prop_assert!(msgs.iter().all(|m| m["role"] != "system"), "Claude should not have inline system");
    }

    #[test]
    fn system_msg_extracted_for_gemini(sys_text in arb_text(), user_text in arb_text()) {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, &sys_text))
            .push(IrMessage::text(IrRole::User, &user_text));
        let lowered = lower_to_gemini(&conv, &[]);
        let si = &lowered["system_instruction"]["parts"][0]["text"];
        prop_assert_eq!(si.as_str().unwrap(), sys_text.as_str());
    }

    #[test]
    fn no_system_msg_omits_system_field_claude(conv_msgs in prop::collection::vec(arb_non_system_text_message(), 1..4)) {
        let conv = IrConversation::from_messages(conv_msgs);
        let lowered = lower_to_claude(&conv, &[]);
        prop_assert!(lowered.get("system").is_none(), "Claude should omit system field when absent");
    }

    #[test]
    fn no_system_msg_omits_system_instruction_gemini(conv_msgs in prop::collection::vec(arb_non_system_text_message(), 1..4)) {
        let conv = IrConversation::from_messages(conv_msgs);
        let lowered = lower_to_gemini(&conv, &[]);
        prop_assert!(lowered.get("system_instruction").is_none(), "Gemini should omit system_instruction when absent");
    }

    // ── 9. Empty conversations handled gracefully ──────────────────────

    #[test]
    fn empty_conv_openai_does_not_panic(_dummy in 0..1i32) {
        let conv = IrConversation::new();
        let lowered = lower_to_openai(&conv, &[]);
        prop_assert!(lowered["messages"].as_array().unwrap().is_empty());
    }

    #[test]
    fn empty_conv_claude_does_not_panic(_dummy in 0..1i32) {
        let conv = IrConversation::new();
        let lowered = lower_to_claude(&conv, &[]);
        prop_assert!(lowered["messages"].as_array().unwrap().is_empty());
    }

    #[test]
    fn empty_conv_gemini_does_not_panic(_dummy in 0..1i32) {
        let conv = IrConversation::new();
        let lowered = lower_to_gemini(&conv, &[]);
        prop_assert!(lowered["contents"].as_array().unwrap().is_empty());
    }

    // ── 10. Large conversations don't panic ────────────────────────────

    #[test]
    fn large_conv_openai_no_panic(msgs in prop::collection::vec(arb_non_system_text_message(), 100..130)) {
        let conv = IrConversation::from_messages(msgs);
        let lowered = lower_to_openai(&conv, &[]);
        let arr = lowered["messages"].as_array().unwrap();
        prop_assert!(arr.len() >= 100);
    }

    #[test]
    fn large_conv_claude_no_panic(msgs in prop::collection::vec(arb_non_system_text_message(), 100..130)) {
        let conv = IrConversation::from_messages(msgs);
        let lowered = lower_to_claude(&conv, &[]);
        let arr = lowered["messages"].as_array().unwrap();
        prop_assert!(arr.len() >= 100);
    }

    #[test]
    fn large_conv_gemini_no_panic(msgs in prop::collection::vec(arb_non_system_text_message(), 100..130)) {
        let conv = IrConversation::from_messages(msgs);
        let lowered = lower_to_gemini(&conv, &[]);
        let arr = lowered["contents"].as_array().unwrap();
        prop_assert!(arr.len() >= 100);
    }

    // ── Additional roundtrip invariants ────────────────────────────────

    #[test]
    fn all_dialects_produce_valid_json_objects(conv in arb_conversation()) {
        for dialect in abp_sdk_types::Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &[]);
            prop_assert!(lowered.is_object(), "{:?} did not produce a JSON object", dialect);
        }
    }

    #[test]
    fn openai_message_count_matches(msgs in prop::collection::vec(arb_non_system_text_message(), 1..10)) {
        let conv = IrConversation::from_messages(msgs.clone());
        let lowered = lower_to_openai(&conv, &[]);
        let arr = lowered["messages"].as_array().unwrap();
        prop_assert_eq!(arr.len(), msgs.len());
    }

    #[test]
    fn claude_non_system_message_count_matches(conv in arb_conversation()) {
        let non_sys = conv.messages.iter().filter(|m| m.role != IrRole::System).count();
        let lowered = lower_to_claude(&conv, &[]);
        let arr = lowered["messages"].as_array().unwrap();
        prop_assert_eq!(arr.len(), non_sys);
    }

    #[test]
    fn gemini_non_system_message_count_matches(conv in arb_conversation()) {
        let non_sys = conv.messages.iter().filter(|m| m.role != IrRole::System).count();
        let lowered = lower_to_gemini(&conv, &[]);
        let arr = lowered["contents"].as_array().unwrap();
        prop_assert_eq!(arr.len(), non_sys);
    }

    #[test]
    fn tool_definitions_roundtrip_openai(tools in prop::collection::vec(arb_tool_definition(), 1..5)) {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let lowered = lower_to_openai(&conv, &tools);
        let arr = lowered["tools"].as_array().unwrap();
        prop_assert_eq!(arr.len(), tools.len());
        for (i, t) in tools.iter().enumerate() {
            prop_assert_eq!(arr[i]["function"]["name"].as_str().unwrap(), t.name.as_str());
        }
    }

    #[test]
    fn tool_definitions_roundtrip_claude(tools in prop::collection::vec(arb_tool_definition(), 1..5)) {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let lowered = lower_to_claude(&conv, &tools);
        let arr = lowered["tools"].as_array().unwrap();
        prop_assert_eq!(arr.len(), tools.len());
        for (i, t) in tools.iter().enumerate() {
            prop_assert_eq!(arr[i]["name"].as_str().unwrap(), t.name.as_str());
            prop_assert!(arr[i].get("input_schema").is_some());
        }
    }

    #[test]
    fn tool_definitions_roundtrip_gemini(tools in prop::collection::vec(arb_tool_definition(), 1..5)) {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let lowered = lower_to_gemini(&conv, &tools);
        let decls = lowered["tools"][0]["function_declarations"].as_array().unwrap();
        prop_assert_eq!(decls.len(), tools.len());
        for (i, t) in tools.iter().enumerate() {
            prop_assert_eq!(decls[i]["name"].as_str().unwrap(), t.name.as_str());
        }
    }

    #[test]
    fn kimi_matches_openai(conv in arb_conversation(), tools in prop::collection::vec(arb_tool_definition(), 0..3)) {
        let openai = lower_to_openai(&conv, &tools);
        let kimi = lower_to_kimi(&conv, &tools);
        prop_assert_eq!(openai, kimi);
    }

    #[test]
    fn codex_matches_openai(conv in arb_conversation(), tools in prop::collection::vec(arb_tool_definition(), 0..3)) {
        let openai = lower_to_openai(&conv, &tools);
        let codex = lower_to_codex(&conv, &tools);
        prop_assert_eq!(openai, codex);
    }

    #[test]
    fn copilot_matches_openai(conv in arb_conversation(), tools in prop::collection::vec(arb_tool_definition(), 0..3)) {
        let openai = lower_to_openai(&conv, &tools);
        let copilot = lower_to_copilot(&conv, &tools);
        prop_assert_eq!(openai, copilot);
    }

    // ── IR serde roundtrip tests ───────────────────────────────────────

    #[test]
    fn ir_conversation_serde_roundtrip(conv in arb_conversation()) {
        let json = serde_json::to_value(&conv).unwrap();
        let parsed: IrConversation = serde_json::from_value(json).unwrap();
        prop_assert_eq!(parsed, conv);
    }

    #[test]
    fn ir_message_serde_roundtrip(msg in arb_text_only_message()) {
        let json = serde_json::to_value(&msg).unwrap();
        let parsed: IrMessage = serde_json::from_value(json).unwrap();
        prop_assert_eq!(parsed, msg);
    }

    #[test]
    fn ir_content_block_serde_roundtrip(block in arb_text_block()) {
        let json = serde_json::to_value(&block).unwrap();
        let parsed: IrContentBlock = serde_json::from_value(json).unwrap();
        prop_assert_eq!(parsed, block);
    }

    #[test]
    fn ir_tool_def_serde_roundtrip(tool in arb_tool_definition()) {
        let json = serde_json::to_value(&tool).unwrap();
        let parsed: IrToolDefinition = serde_json::from_value(json).unwrap();
        prop_assert_eq!(parsed, tool);
    }

    // ── Thinking block dialect handling ─────────────────────────────────

    #[test]
    fn thinking_block_preserved_in_claude(text in arb_text()) {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking { text: text.clone() },
                IrContentBlock::Text { text: "answer".into() },
            ],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        let content = lowered["messages"][0]["content"].as_array().unwrap();
        let thinking = content.iter().find(|b| b["type"] == "thinking");
        prop_assert!(thinking.is_some(), "Claude should preserve thinking blocks");
        prop_assert_eq!(thinking.unwrap()["thinking"].as_str().unwrap(), text.as_str());
    }

    #[test]
    fn thinking_block_dropped_in_gemini(text in arb_text()) {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking { text },
                IrContentBlock::Text { text: "answer".into() },
            ],
        ));
        let lowered = lower_to_gemini(&conv, &[]);
        let parts = lowered["contents"][0]["parts"].as_array().unwrap();
        prop_assert_eq!(parts.len(), 1, "Gemini should drop thinking blocks");
        prop_assert_eq!(parts[0]["text"].as_str().unwrap(), "answer");
    }

    // ── Image block dialect handling ───────────────────────────────────

    #[test]
    fn image_block_preserved_claude(block in arb_image_block()) {
        let (media_type, data) = match &block {
            IrContentBlock::Image { media_type, data } => (media_type.clone(), data.clone()),
            _ => unreachable!(),
        };
        let conv = IrConversation::new().push(IrMessage::new(IrRole::User, vec![block]));
        let lowered = lower_to_claude(&conv, &[]);
        let cb = &lowered["messages"][0]["content"][0];
        prop_assert_eq!(&cb["type"], "image");
        prop_assert_eq!(cb["source"]["media_type"].as_str().unwrap(), media_type.as_str());
        prop_assert_eq!(cb["source"]["data"].as_str().unwrap(), data.as_str());
    }

    #[test]
    fn image_block_preserved_gemini(block in arb_image_block()) {
        let (media_type, data) = match &block {
            IrContentBlock::Image { media_type, data } => (media_type.clone(), data.clone()),
            _ => unreachable!(),
        };
        let conv = IrConversation::new().push(IrMessage::new(IrRole::User, vec![block]));
        let lowered = lower_to_gemini(&conv, &[]);
        let part = &lowered["contents"][0]["parts"][0];
        prop_assert_eq!(part["inline_data"]["mime_type"].as_str().unwrap(), media_type.as_str());
        prop_assert_eq!(part["inline_data"]["data"].as_str().unwrap(), data.as_str());
    }

    // ── No tools → no tools field ──────────────────────────────────────

    #[test]
    fn no_tools_omits_tools_all_dialects(conv in arb_conversation()) {
        for dialect in abp_sdk_types::Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &[]);
            prop_assert!(lowered.get("tools").is_none(), "{:?} should omit tools field", dialect);
        }
    }
}
