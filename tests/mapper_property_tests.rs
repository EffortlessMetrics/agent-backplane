#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
//! Property-based tests for mapper roundtrip correctness.
//!
//! Uses `proptest` to verify that dialect mapping between OpenAI and Claude
//! preserves content, structure, roles, tool IDs, model names, and parameters.

use proptest::prelude::*;
use serde_json::{Value, json};

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;
use abp_mapper::{
    ClaudeToOpenAiMapper, DialectRequest, Mapper, OpenAiToClaudeMapper, default_ir_mapper,
};
use abp_sdk_types::Dialect as SdkDialect;
use abp_sdk_types::convert::RoleMapper;

// ── Strategies ──────────────────────────────────────────────────────────

fn arb_model_name() -> BoxedStrategy<String> {
    "[a-zA-Z0-9._-]{1,50}".boxed()
}

fn arb_nonempty_text() -> BoxedStrategy<String> {
    "[ -~]{1,200}".boxed()
}

fn arb_tool_call_id() -> BoxedStrategy<String> {
    "[a-zA-Z0-9_-]{1,30}".boxed()
}

fn arb_tool_name() -> BoxedStrategy<String> {
    "[a-z][a-z0-9_]{0,29}".boxed()
}

fn arb_temperature() -> BoxedStrategy<f64> {
    (0.0f64..=2.0f64).boxed()
}

fn arb_top_p() -> BoxedStrategy<f64> {
    (0.0f64..=1.0f64).boxed()
}

/// Roles valid in both OpenAI and Claude (shared subset).
fn arb_shared_role() -> BoxedStrategy<&'static str> {
    prop_oneof![Just("user"), Just("assistant"),].boxed()
}

// ── 1. Role mapping roundtrip ───────────────────────────────────────────

proptest! {
    /// OpenAI shared roles (user, assistant) survive OpenAI→Claude→OpenAI roundtrip.
    #[test]
    fn role_roundtrip_openai_claude_openai(role in arb_shared_role()) {
        let to_claude = RoleMapper::map_role(role, SdkDialect::OpenAi, SdkDialect::Claude).unwrap();
        let back = RoleMapper::map_role(&to_claude, SdkDialect::Claude, SdkDialect::OpenAi).unwrap();
        prop_assert_eq!(role, back.as_str());
    }

    /// User/assistant roles are identity-mapped within the same dialect.
    #[test]
    fn role_same_dialect_identity(role in arb_shared_role()) {
        let mapped = RoleMapper::map_role(role, SdkDialect::OpenAi, SdkDialect::OpenAi).unwrap();
        prop_assert_eq!(role, mapped.as_str());
    }

    /// Gemini model↔assistant roundtrip preserves semantics.
    #[test]
    fn role_roundtrip_openai_gemini(role in arb_shared_role()) {
        let to_gemini = RoleMapper::map_role(role, SdkDialect::OpenAi, SdkDialect::Gemini).unwrap();
        let back = RoleMapper::map_role(&to_gemini, SdkDialect::Gemini, SdkDialect::OpenAi).unwrap();
        prop_assert_eq!(role, back.as_str());
    }
}

// ── 2. Message content preservation ─────────────────────────────────────

proptest! {
    /// User text content survives OpenAI→Claude mapping.
    #[test]
    fn content_preserved_openai_to_claude(text in arb_nonempty_text()) {
        let o2c = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": text.clone()}],
                "max_tokens": 1024
            }),
        };
        let result = o2c.map_request(&req).unwrap();
        let mapped_content = result["messages"][0]["content"].as_str().unwrap();
        prop_assert_eq!(&text, mapped_content);
    }

    /// User text content survives Claude→OpenAI mapping.
    #[test]
    fn content_preserved_claude_to_openai(text in arb_nonempty_text()) {
        let c2o = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({
                "model": "claude-3",
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": text.clone()}]
            }),
        };
        let result = c2o.map_request(&req).unwrap();
        let mapped_content = result["messages"][0]["content"].as_str().unwrap();
        prop_assert_eq!(&text, mapped_content);
    }

    /// Text content roundtrips through OpenAI→Claude→OpenAI unchanged.
    #[test]
    fn content_roundtrip_openai_claude_openai(text in arb_nonempty_text()) {
        let o2c = OpenAiToClaudeMapper;
        let c2o = ClaudeToOpenAiMapper;

        let openai_req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": text.clone()}],
            "max_tokens": 1024
        });

        let claude_body = o2c.map_request(&DialectRequest {
            dialect: Dialect::OpenAi,
            body: openai_req,
        }).unwrap();

        let back = c2o.map_request(&DialectRequest {
            dialect: Dialect::Claude,
            body: claude_body,
        }).unwrap();

        let back_content = back["messages"][0]["content"].as_str().unwrap();
        prop_assert_eq!(&text, back_content);
    }

    /// System message content is preserved through OpenAI→Claude mapping.
    #[test]
    fn system_content_preserved_openai_to_claude(text in arb_nonempty_text()) {
        let o2c = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "system", "content": text.clone()},
                    {"role": "user", "content": "hi"}
                ]
            }),
        };
        let result = o2c.map_request(&req).unwrap();
        let system = result["system"].as_str().unwrap();
        prop_assert_eq!(&text, system);
    }
}

// ── 3. Tool call ID preservation ────────────────────────────────────────

proptest! {
    /// Tool call IDs survive OpenAI→Claude mapping.
    #[test]
    fn tool_call_id_preserved_openai_to_claude(id in arb_tool_call_id()) {
        let o2c = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "hi"},
                    {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{
                            "id": id.clone(),
                            "type": "function",
                            "function": {
                                "name": "get_weather",
                                "arguments": "{}"
                            }
                        }]
                    }
                ],
                "max_tokens": 1024
            }),
        };
        let result = o2c.map_request(&req).unwrap();
        let blocks = result["messages"][1]["content"].as_array().unwrap();
        let tool_use = &blocks[0];
        prop_assert_eq!(id.as_str(), tool_use["id"].as_str().unwrap());
    }

    /// Tool result IDs survive OpenAI→Claude mapping.
    #[test]
    fn tool_result_id_preserved_openai_to_claude(id in arb_tool_call_id()) {
        let o2c = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "hi"},
                    {"role": "tool", "tool_call_id": id.clone(), "content": "result"}
                ],
                "max_tokens": 1024
            }),
        };
        let result = o2c.map_request(&req).unwrap();
        let tool_result = &result["messages"][1]["content"][0];
        prop_assert_eq!(id.as_str(), tool_result["tool_use_id"].as_str().unwrap());
    }

    /// Tool use IDs survive Claude→OpenAI mapping.
    #[test]
    fn tool_use_id_preserved_claude_to_openai(id in arb_tool_call_id()) {
        let c2o = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({
                "model": "claude-3",
                "max_tokens": 1024,
                "messages": [
                    {"role": "user", "content": "hi"},
                    {
                        "role": "assistant",
                        "content": [{
                            "type": "tool_use",
                            "id": id.clone(),
                            "name": "read_file",
                            "input": {}
                        }]
                    }
                ]
            }),
        };
        let result = c2o.map_request(&req).unwrap();
        let tool_calls = result["messages"][1]["tool_calls"].as_array().unwrap();
        prop_assert_eq!(id.as_str(), tool_calls[0]["id"].as_str().unwrap());
    }

    /// Tool call IDs roundtrip OpenAI→Claude→OpenAI.
    #[test]
    fn tool_call_id_roundtrip(id in arb_tool_call_id()) {
        let o2c = OpenAiToClaudeMapper;
        let c2o = ClaudeToOpenAiMapper;

        let openai_req = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "hi"},
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": id.clone(),
                        "type": "function",
                        "function": {"name": "f", "arguments": "{}"}
                    }]
                }
            ],
            "max_tokens": 1024
        });

        let claude_body = o2c.map_request(&DialectRequest {
            dialect: Dialect::OpenAi,
            body: openai_req,
        }).unwrap();

        let back = c2o.map_request(&DialectRequest {
            dialect: Dialect::Claude,
            body: claude_body,
        }).unwrap();

        let back_id = back["messages"][1]["tool_calls"][0]["id"].as_str().unwrap();
        prop_assert_eq!(id.as_str(), back_id);
    }
}

// ── 4. JSON structure preservation ──────────────────────────────────────

proptest! {
    /// OpenAI→Claude→OpenAI preserves the overall message count (user messages).
    #[test]
    fn structure_message_count_roundtrip(
        msg_count in 1usize..5,
        text in arb_nonempty_text(),
    ) {
        let o2c = OpenAiToClaudeMapper;
        let c2o = ClaudeToOpenAiMapper;

        let messages: Vec<Value> = (0..msg_count)
            .map(|_| json!({"role": "user", "content": text.clone()}))
            .collect();

        let openai_req = json!({
            "model": "gpt-4",
            "messages": messages,
            "max_tokens": 1024
        });

        let claude_body = o2c.map_request(&DialectRequest {
            dialect: Dialect::OpenAi,
            body: openai_req,
        }).unwrap();

        let back = c2o.map_request(&DialectRequest {
            dialect: Dialect::Claude,
            body: claude_body,
        }).unwrap();

        let back_msgs = back["messages"].as_array().unwrap();
        prop_assert_eq!(back_msgs.len(), msg_count);
    }

    /// Mapped Claude request always has required `max_tokens` field.
    #[test]
    fn structure_claude_always_has_max_tokens(text in arb_nonempty_text()) {
        let o2c = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": text}]
            }),
        };
        let result = o2c.map_request(&req).unwrap();
        prop_assert!(result.get("max_tokens").is_some());
    }

    /// Mapped OpenAI request always has `messages` array.
    #[test]
    fn structure_openai_always_has_messages(text in arb_nonempty_text()) {
        let c2o = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({
                "model": "claude-3",
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": text}]
            }),
        };
        let result = c2o.map_request(&req).unwrap();
        prop_assert!(result["messages"].is_array());
    }

    /// System message extraction creates valid Claude structure (no system in messages).
    #[test]
    fn structure_system_extracted_from_claude_messages(
        sys_text in arb_nonempty_text(),
        user_text in arb_nonempty_text(),
    ) {
        let o2c = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "system", "content": sys_text},
                    {"role": "user", "content": user_text}
                ]
            }),
        };
        let result = o2c.map_request(&req).unwrap();
        let messages = result["messages"].as_array().unwrap();
        // No message should have role "system" in Claude format
        for msg in messages {
            prop_assert_ne!(msg["role"].as_str().unwrap_or(""), "system");
        }
    }
}

// ── 5. Content block conversion ─────────────────────────────────────────

proptest! {
    /// OpenAI user text → Claude always produces a valid message with content.
    #[test]
    fn content_block_user_text_produces_valid_claude(text in arb_nonempty_text()) {
        let o2c = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": text}],
                "max_tokens": 1024
            }),
        };
        let result = o2c.map_request(&req).unwrap();
        let msg = &result["messages"][0];
        prop_assert_eq!(msg["role"].as_str().unwrap(), "user");
        // Content should be present (string or array)
        prop_assert!(!msg["content"].is_null());
    }

    /// OpenAI assistant tool_calls → Claude produces tool_use content blocks.
    #[test]
    fn content_block_tool_calls_become_tool_use(
        id in arb_tool_call_id(),
        name in arb_tool_name(),
    ) {
        let o2c = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "hi"},
                    {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{
                            "id": id,
                            "type": "function",
                            "function": {"name": name.clone(), "arguments": "{}"}
                        }]
                    }
                ],
                "max_tokens": 1024
            }),
        };
        let result = o2c.map_request(&req).unwrap();
        let blocks = result["messages"][1]["content"].as_array().unwrap();
        prop_assert!(!blocks.is_empty());
        prop_assert_eq!(blocks[0]["type"].as_str().unwrap(), "tool_use");
        prop_assert_eq!(blocks[0]["name"].as_str().unwrap(), name.as_str());
    }

    /// Claude text blocks → OpenAI produces string content.
    #[test]
    fn content_block_claude_text_to_openai_string(text in arb_nonempty_text()) {
        let c2o = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({
                "model": "claude-3",
                "max_tokens": 1024,
                "messages": [{
                    "role": "assistant",
                    "content": [{"type": "text", "text": text.clone()}]
                }]
            }),
        };
        let result = c2o.map_request(&req).unwrap();
        let content = result["messages"][0]["content"].as_str().unwrap();
        prop_assert_eq!(&text, content);
    }
}

// ── 6. Temperature / top_p passthrough ──────────────────────────────────

proptest! {
    /// Temperature values pass through OpenAI→Claude unchanged.
    #[test]
    fn temperature_passthrough_openai_to_claude(temp in arb_temperature()) {
        let o2c = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "temperature": temp
            }),
        };
        let result = o2c.map_request(&req).unwrap();
        let mapped_temp = result["temperature"].as_f64().unwrap();
        prop_assert!((mapped_temp - temp).abs() < f64::EPSILON);
    }

    /// Temperature values pass through Claude→OpenAI unchanged.
    #[test]
    fn temperature_passthrough_claude_to_openai(temp in arb_temperature()) {
        let c2o = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({
                "model": "claude-3",
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": "hi"}],
                "temperature": temp
            }),
        };
        let result = c2o.map_request(&req).unwrap();
        let mapped_temp = result["temperature"].as_f64().unwrap();
        prop_assert!((mapped_temp - temp).abs() < f64::EPSILON);
    }

    /// Temperature roundtrips OpenAI→Claude→OpenAI.
    #[test]
    fn temperature_roundtrip(temp in arb_temperature()) {
        let o2c = OpenAiToClaudeMapper;
        let c2o = ClaudeToOpenAiMapper;

        let claude_body = o2c.map_request(&DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "max_tokens": 1024,
                "temperature": temp
            }),
        }).unwrap();

        let back = c2o.map_request(&DialectRequest {
            dialect: Dialect::Claude,
            body: claude_body,
        }).unwrap();

        let back_temp = back["temperature"].as_f64().unwrap();
        prop_assert!((back_temp - temp).abs() < f64::EPSILON);
    }

    /// top_p values pass through OpenAI→Claude unchanged.
    #[test]
    fn top_p_passthrough_openai_to_claude(top_p in arb_top_p()) {
        let o2c = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "top_p": top_p
            }),
        };
        let result = o2c.map_request(&req).unwrap();
        let mapped = result["top_p"].as_f64().unwrap();
        prop_assert!((mapped - top_p).abs() < f64::EPSILON);
    }

    /// top_p roundtrips OpenAI→Claude→OpenAI.
    #[test]
    fn top_p_roundtrip(top_p in arb_top_p()) {
        let o2c = OpenAiToClaudeMapper;
        let c2o = ClaudeToOpenAiMapper;

        let claude_body = o2c.map_request(&DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "max_tokens": 1024,
                "top_p": top_p
            }),
        }).unwrap();

        let back = c2o.map_request(&DialectRequest {
            dialect: Dialect::Claude,
            body: claude_body,
        }).unwrap();

        let back_top_p = back["top_p"].as_f64().unwrap();
        prop_assert!((back_top_p - top_p).abs() < f64::EPSILON);
    }
}

// ── 7. Model name passthrough ───────────────────────────────────────────

proptest! {
    /// Model name survives OpenAI→Claude mapping.
    #[test]
    fn model_passthrough_openai_to_claude(model in arb_model_name()) {
        let o2c = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": model.clone(),
                "messages": [{"role": "user", "content": "hi"}],
                "max_tokens": 1024
            }),
        };
        let result = o2c.map_request(&req).unwrap();
        prop_assert_eq!(model.as_str(), result["model"].as_str().unwrap());
    }

    /// Model name survives Claude→OpenAI mapping.
    #[test]
    fn model_passthrough_claude_to_openai(model in arb_model_name()) {
        let c2o = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({
                "model": model.clone(),
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": "hi"}]
            }),
        };
        let result = c2o.map_request(&req).unwrap();
        prop_assert_eq!(model.as_str(), result["model"].as_str().unwrap());
    }

    /// Model name roundtrips OpenAI→Claude→OpenAI.
    #[test]
    fn model_roundtrip(model in arb_model_name()) {
        let o2c = OpenAiToClaudeMapper;
        let c2o = ClaudeToOpenAiMapper;

        let claude_body = o2c.map_request(&DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": model.clone(),
                "messages": [{"role": "user", "content": "hi"}],
                "max_tokens": 1024
            }),
        }).unwrap();

        let back = c2o.map_request(&DialectRequest {
            dialect: Dialect::Claude,
            body: claude_body,
        }).unwrap();

        prop_assert_eq!(model.as_str(), back["model"].as_str().unwrap());
    }
}

// ── IR-level roundtrip properties ───────────────────────────────────────

proptest! {
    /// IR text content survives OpenAI→Claude→OpenAI roundtrip.
    #[test]
    fn ir_text_content_roundtrip(text in arb_nonempty_text()) {
        let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
        let back_mapper = default_ir_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();

        let ir = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, text.clone()),
        ]);

        let mapped = mapper.map_request(Dialect::OpenAi, Dialect::Claude, &ir).unwrap();
        let back = back_mapper.map_request(Dialect::Claude, Dialect::OpenAi, &mapped).unwrap();

        prop_assert!(!back.messages.is_empty());
        prop_assert_eq!(back.messages[0].text_content(), text);
    }

    /// IR tool use IDs survive roundtrip.
    #[test]
    fn ir_tool_use_id_roundtrip(
        id in arb_tool_call_id(),
        name in arb_tool_name(),
    ) {
        let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
        let back_mapper = default_ir_mapper(Dialect::Claude, Dialect::OpenAi).unwrap();

        let ir = IrConversation::from_messages(vec![
            IrMessage::new(IrRole::Assistant, vec![
                IrContentBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: json!({}),
                },
            ]),
        ]);

        let mapped = mapper.map_request(Dialect::OpenAi, Dialect::Claude, &ir).unwrap();
        let back = back_mapper.map_request(Dialect::Claude, Dialect::OpenAi, &mapped).unwrap();

        let tool_blocks = back.tool_calls();
        prop_assert!(!tool_blocks.is_empty());
        if let IrContentBlock::ToolUse { id: back_id, name: back_name, .. } = tool_blocks[0] {
            prop_assert_eq!(&id, back_id);
            prop_assert_eq!(&name, back_name);
        } else {
            prop_assert!(false, "expected ToolUse block");
        }
    }

    /// Identity IR mapper preserves all content exactly.
    #[test]
    fn ir_identity_preserves_content(text in arb_nonempty_text()) {
        let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::OpenAi).unwrap();
        let ir = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, text.clone()),
        ]);
        let mapped = mapper.map_request(Dialect::OpenAi, Dialect::OpenAi, &ir).unwrap();
        prop_assert_eq!(ir, mapped);
    }

    /// IR message count is preserved or expanded predictably.
    #[test]
    fn ir_message_count_preserved(
        count in 1usize..5,
        text in arb_nonempty_text(),
    ) {
        let mapper = default_ir_mapper(Dialect::OpenAi, Dialect::Claude).unwrap();
        let messages: Vec<IrMessage> = (0..count)
            .map(|_| IrMessage::text(IrRole::User, text.clone()))
            .collect();
        let ir = IrConversation::from_messages(messages);
        let mapped = mapper.map_request(Dialect::OpenAi, Dialect::Claude, &ir).unwrap();
        // User-only messages should have same count
        prop_assert_eq!(mapped.messages.len(), count);
    }
}

// ── Tool definition roundtrip ───────────────────────────────────────────

proptest! {
    /// Tool name survives OpenAI→Claude→OpenAI roundtrip.
    #[test]
    fn tool_def_name_roundtrip(name in arb_tool_name()) {
        let o2c = OpenAiToClaudeMapper;
        let c2o = ClaudeToOpenAiMapper;

        let openai_req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 1024,
            "tools": [{
                "type": "function",
                "function": {
                    "name": name.clone(),
                    "description": "A tool",
                    "parameters": {"type": "object", "properties": {}}
                }
            }]
        });

        let claude_body = o2c.map_request(&DialectRequest {
            dialect: Dialect::OpenAi,
            body: openai_req,
        }).unwrap();

        let back = c2o.map_request(&DialectRequest {
            dialect: Dialect::Claude,
            body: claude_body,
        }).unwrap();

        let back_name = back["tools"][0]["function"]["name"].as_str().unwrap();
        prop_assert_eq!(name.as_str(), back_name);
    }

    /// Tool description survives roundtrip.
    #[test]
    fn tool_def_description_roundtrip(desc in arb_nonempty_text()) {
        let o2c = OpenAiToClaudeMapper;
        let c2o = ClaudeToOpenAiMapper;

        let openai_req = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 1024,
            "tools": [{
                "type": "function",
                "function": {
                    "name": "my_tool",
                    "description": desc.clone(),
                    "parameters": {"type": "object", "properties": {}}
                }
            }]
        });

        let claude_body = o2c.map_request(&DialectRequest {
            dialect: Dialect::OpenAi,
            body: openai_req,
        }).unwrap();

        let back = c2o.map_request(&DialectRequest {
            dialect: Dialect::Claude,
            body: claude_body,
        }).unwrap();

        let back_desc = back["tools"][0]["function"]["description"].as_str().unwrap();
        prop_assert_eq!(desc.as_str(), back_desc);
    }
}

// ── Dialect source/target correctness ───────────────────────────────────

proptest! {
    /// OpenAiToClaudeMapper rejects non-OpenAI dialect requests.
    #[test]
    fn wrong_dialect_rejected_o2c(text in arb_nonempty_text()) {
        let o2c = OpenAiToClaudeMapper;
        let req = DialectRequest {
            dialect: Dialect::Claude,
            body: json!({"model": "x", "messages": [{"role": "user", "content": text}]}),
        };
        prop_assert!(o2c.map_request(&req).is_err());
    }

    /// ClaudeToOpenAiMapper rejects non-Claude dialect requests.
    #[test]
    fn wrong_dialect_rejected_c2o(text in arb_nonempty_text()) {
        let c2o = ClaudeToOpenAiMapper;
        let req = DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({"model": "x", "messages": [{"role": "user", "content": text}]}),
        };
        prop_assert!(c2o.map_request(&req).is_err());
    }
}

// ── Stream flag preservation ────────────────────────────────────────────

proptest! {
    /// Stream flag survives roundtrip.
    #[test]
    fn stream_flag_roundtrip(stream in prop::bool::ANY) {
        let o2c = OpenAiToClaudeMapper;
        let c2o = ClaudeToOpenAiMapper;

        let claude_body = o2c.map_request(&DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "max_tokens": 1024,
                "stream": stream
            }),
        }).unwrap();

        let back = c2o.map_request(&DialectRequest {
            dialect: Dialect::Claude,
            body: claude_body,
        }).unwrap();

        prop_assert_eq!(stream, back["stream"].as_bool().unwrap());
    }
}

// ── max_tokens preservation ─────────────────────────────────────────────

proptest! {
    /// max_tokens roundtrips OpenAI→Claude→OpenAI.
    #[test]
    fn max_tokens_roundtrip(max_tokens in 1u32..100_000) {
        let o2c = OpenAiToClaudeMapper;
        let c2o = ClaudeToOpenAiMapper;

        let claude_body = o2c.map_request(&DialectRequest {
            dialect: Dialect::OpenAi,
            body: json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}],
                "max_tokens": max_tokens
            }),
        }).unwrap();

        let back = c2o.map_request(&DialectRequest {
            dialect: Dialect::Claude,
            body: claude_body,
        }).unwrap();

        prop_assert_eq!(
            max_tokens as u64,
            back["max_tokens"].as_u64().unwrap()
        );
    }
}
