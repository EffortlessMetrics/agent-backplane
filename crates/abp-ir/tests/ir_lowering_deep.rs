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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_vec)]
//! Deep tests for IR lowering and lifting between vendor-specific types and
//! the ABP intermediate representation.
//!
//! Covers OpenAI → IR, IR → OpenAI, Claude → IR, IR → Claude, Gemini → IR,
//! IR → Gemini, Codex, Kimi, Copilot lowering, cross-dialect roundtrips,
//! information-loss tracking, and error / boundary cases.

use abp_ir::lower::*;
use abp_ir::normalize;
use abp_ir::*;
use abp_sdk_types::Dialect;
use serde_json::json;
use std::collections::BTreeMap;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn td(name: &str, desc: &str) -> IrToolDefinition {
    IrToolDefinition {
        name: name.into(),
        description: desc.into(),
        parameters: json!({"type": "object", "properties": {}}),
    }
}

fn td_params(name: &str, desc: &str, params: serde_json::Value) -> IrToolDefinition {
    IrToolDefinition {
        name: name.into(),
        description: desc.into(),
        parameters: params,
    }
}

fn tu(id: &str, name: &str, input: serde_json::Value) -> IrContentBlock {
    IrContentBlock::ToolUse {
        id: id.into(),
        name: name.into(),
        input,
    }
}

fn tr(tool_use_id: &str, text: &str, is_error: bool) -> IrContentBlock {
    IrContentBlock::ToolResult {
        tool_use_id: tool_use_id.into(),
        content: vec![IrContentBlock::Text { text: text.into() }],
        is_error,
    }
}

fn base_conv() -> IrConversation {
    IrConversation::new()
        .push(IrMessage::text(
            IrRole::System,
            "You are a helpful assistant.",
        ))
        .push(IrMessage::text(IrRole::User, "Hello"))
        .push(IrMessage::text(IrRole::Assistant, "Hi there!"))
}

/// Lift OpenAI JSON back into IR.
fn lift_openai(v: &serde_json::Value) -> (IrConversation, Vec<IrToolDefinition>) {
    let mut conv = IrConversation::new();
    if let Some(msgs) = v["messages"].as_array() {
        for m in msgs {
            let role = match m["role"].as_str().unwrap_or("user") {
                "system" => IrRole::System,
                "assistant" => IrRole::Assistant,
                "tool" => IrRole::Tool,
                _ => IrRole::User,
            };
            let mut blocks = Vec::new();
            if let Some(text) = m["content"].as_str() {
                blocks.push(IrContentBlock::Text {
                    text: text.to_string(),
                });
            }
            if let Some(calls) = m["tool_calls"].as_array() {
                for tc in calls {
                    blocks.push(IrContentBlock::ToolUse {
                        id: tc["id"].as_str().unwrap_or("").to_string(),
                        name: tc["function"]["name"].as_str().unwrap_or("").to_string(),
                        input: serde_json::from_str(
                            tc["function"]["arguments"].as_str().unwrap_or("{}"),
                        )
                        .unwrap_or(json!({})),
                    });
                }
            }
            if role == IrRole::Tool {
                if let Some(tid) = m["tool_call_id"].as_str() {
                    let text = m["content"].as_str().unwrap_or("");
                    blocks = vec![IrContentBlock::ToolResult {
                        tool_use_id: tid.to_string(),
                        content: vec![IrContentBlock::Text {
                            text: text.to_string(),
                        }],
                        is_error: false,
                    }];
                }
            }
            conv = conv.push(IrMessage::new(role, blocks));
        }
    }
    let tools = v["tools"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|t| IrToolDefinition {
                    name: t["function"]["name"].as_str().unwrap_or("").to_string(),
                    description: t["function"]["description"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                    parameters: t["function"]["parameters"].clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    (conv, tools)
}

/// Lift Claude JSON back into IR.
fn lift_claude(v: &serde_json::Value) -> (IrConversation, Vec<IrToolDefinition>) {
    let mut conv = IrConversation::new();
    if let Some(sys) = v["system"].as_str() {
        conv = conv.push(IrMessage::text(IrRole::System, sys));
    }
    if let Some(msgs) = v["messages"].as_array() {
        for m in msgs {
            let role = match m["role"].as_str().unwrap_or("user") {
                "assistant" => IrRole::Assistant,
                _ => IrRole::User,
            };
            let mut blocks = Vec::new();
            if let Some(content) = m["content"].as_array() {
                for b in content {
                    match b["type"].as_str().unwrap_or("") {
                        "text" => blocks.push(IrContentBlock::Text {
                            text: b["text"].as_str().unwrap_or("").to_string(),
                        }),
                        "tool_use" => blocks.push(IrContentBlock::ToolUse {
                            id: b["id"].as_str().unwrap_or("").to_string(),
                            name: b["name"].as_str().unwrap_or("").to_string(),
                            input: b["input"].clone(),
                        }),
                        "tool_result" => {
                            let inner: Vec<IrContentBlock> = b["content"]
                                .as_array()
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|c| {
                                            c["text"].as_str().map(|t| IrContentBlock::Text {
                                                text: t.to_string(),
                                            })
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            blocks.push(IrContentBlock::ToolResult {
                                tool_use_id: b["tool_use_id"].as_str().unwrap_or("").to_string(),
                                content: inner,
                                is_error: b["is_error"].as_bool().unwrap_or(false),
                            });
                        }
                        "thinking" => blocks.push(IrContentBlock::Thinking {
                            text: b["thinking"].as_str().unwrap_or("").to_string(),
                        }),
                        "image" => blocks.push(IrContentBlock::Image {
                            media_type: b["source"]["media_type"]
                                .as_str()
                                .unwrap_or("")
                                .to_string(),
                            data: b["source"]["data"].as_str().unwrap_or("").to_string(),
                        }),
                        _ => {}
                    }
                }
            }
            conv = conv.push(IrMessage::new(role, blocks));
        }
    }
    let tools = v["tools"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|t| IrToolDefinition {
                    name: t["name"].as_str().unwrap_or("").to_string(),
                    description: t["description"].as_str().unwrap_or("").to_string(),
                    parameters: t["input_schema"].clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    (conv, tools)
}

/// Lift Gemini JSON back into IR.
fn lift_gemini(v: &serde_json::Value) -> (IrConversation, Vec<IrToolDefinition>) {
    let mut conv = IrConversation::new();
    if let Some(sys) = v.get("system_instruction") {
        let text: String = sys["parts"]
            .as_array()
            .map(|p| {
                p.iter()
                    .filter_map(|x| x["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        if !text.is_empty() {
            conv = conv.push(IrMessage::text(IrRole::System, &text));
        }
    }
    if let Some(contents) = v["contents"].as_array() {
        for c in contents {
            let role = match c["role"].as_str().unwrap_or("user") {
                "model" => IrRole::Assistant,
                _ => IrRole::User,
            };
            let mut blocks = Vec::new();
            if let Some(parts) = c["parts"].as_array() {
                for p in parts {
                    if let Some(t) = p["text"].as_str() {
                        blocks.push(IrContentBlock::Text {
                            text: t.to_string(),
                        });
                    } else if p.get("functionCall").is_some() {
                        blocks.push(IrContentBlock::ToolUse {
                            id: String::new(),
                            name: p["functionCall"]["name"].as_str().unwrap_or("").to_string(),
                            input: p["functionCall"]["args"].clone(),
                        });
                    } else if p.get("functionResponse").is_some() {
                        let rt = p["functionResponse"]["response"]["result"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();
                        blocks.push(IrContentBlock::ToolResult {
                            tool_use_id: p["functionResponse"]["name"]
                                .as_str()
                                .unwrap_or("")
                                .to_string(),
                            content: vec![IrContentBlock::Text { text: rt }],
                            is_error: false,
                        });
                    }
                }
            }
            conv = conv.push(IrMessage::new(role, blocks));
        }
    }
    let tools = v["tools"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|t| t["function_declarations"].as_array())
        .map(|decls| {
            decls
                .iter()
                .map(|d| IrToolDefinition {
                    name: d["name"].as_str().unwrap_or("").to_string(),
                    description: d["description"].as_str().unwrap_or("").to_string(),
                    parameters: d["parameters"].clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    (conv, tools)
}

// ═══════════════════════════════════════════════════════════════════════
// 1. OpenAI → IR lowering
// ═══════════════════════════════════════════════════════════════════════

mod openai_to_ir {
    use super::*;

    #[test]
    fn basic_messages_lift() {
        let req = json!({
            "messages": [
                {"role": "system", "content": "Be concise."},
                {"role": "user", "content": "ping"},
                {"role": "assistant", "content": "pong"}
            ]
        });
        let (conv, _) = lift_openai(&req);
        assert_eq!(conv.len(), 3);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "Be concise.");
        assert_eq!(conv.messages[1].role, IrRole::User);
        assert_eq!(conv.messages[2].role, IrRole::Assistant);
    }

    #[test]
    fn tool_definitions_lift() {
        let req = json!({
            "messages": [],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "read_file",
                    "description": "Read file contents",
                    "parameters": {"type": "object", "properties": {"path": {"type": "string"}}}
                }
            }]
        });
        let (_, tools) = lift_openai(&req);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "read_file");
        assert_eq!(tools[0].description, "Read file contents");
        assert_eq!(tools[0].parameters["type"], "object");
    }

    #[test]
    fn tool_calls_lift_from_assistant() {
        let req = json!({
            "messages": [{
                "role": "assistant",
                "content": "Checking...",
                "tool_calls": [{
                    "id": "call_1",
                    "function": {"name": "search", "arguments": "{\"q\":\"rust\"}"}
                }]
            }]
        });
        let (conv, _) = lift_openai(&req);
        let blocks = &conv.messages[0].content;
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], IrContentBlock::Text { text } if text == "Checking..."));
        assert!(
            matches!(&blocks[1], IrContentBlock::ToolUse { id, name, input }
                if id == "call_1" && name == "search" && input["q"] == "rust")
        );
    }

    #[test]
    fn tool_result_lift() {
        let req = json!({
            "messages": [{"role": "tool", "tool_call_id": "call_1", "content": "42"}]
        });
        let (conv, _) = lift_openai(&req);
        assert_eq!(conv.messages[0].role, IrRole::Tool);
        assert!(matches!(
            &conv.messages[0].content[0],
            IrContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "call_1"
        ));
    }

    #[test]
    fn model_param_not_in_ir_conversation() {
        let req = json!({
            "model": "gpt-4o",
            "temperature": 0.7,
            "top_p": 0.9,
            "messages": [{"role": "user", "content": "hi"}]
        });
        let (conv, _) = lift_openai(&req);
        // Model params are not part of IrConversation
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].text_content(), "hi");
    }

    #[test]
    fn multiple_tool_calls_in_one_message() {
        let req = json!({
            "messages": [{
                "role": "assistant",
                "tool_calls": [
                    {"id": "c1", "function": {"name": "search", "arguments": "{}"}},
                    {"id": "c2", "function": {"name": "read", "arguments": "{}"}}
                ]
            }]
        });
        let (conv, _) = lift_openai(&req);
        let tool_blocks: Vec<_> = conv.messages[0]
            .content
            .iter()
            .filter(|b| matches!(b, IrContentBlock::ToolUse { .. }))
            .collect();
        assert_eq!(tool_blocks.len(), 2);
    }

    #[test]
    fn empty_messages_array() {
        let req = json!({"messages": []});
        let (conv, _) = lift_openai(&req);
        assert!(conv.is_empty());
    }

    #[test]
    fn no_tools_field() {
        let req = json!({"messages": [{"role": "user", "content": "hi"}]});
        let (_, tools) = lift_openai(&req);
        assert!(tools.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. IR → OpenAI lifting (roundtrip fidelity)
// ═══════════════════════════════════════════════════════════════════════

mod ir_to_openai {
    use super::*;

    #[test]
    fn system_message_inline() {
        let lowered = lower_to_openai(&base_conv(), &[]);
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "You are a helpful assistant.");
    }

    #[test]
    fn user_assistant_roles() {
        let lowered = lower_to_openai(&base_conv(), &[]);
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[1]["content"], "Hello");
        assert_eq!(msgs[2]["role"], "assistant");
        assert_eq!(msgs[2]["content"], "Hi there!");
    }

    #[test]
    fn tool_calls_have_function_wrapper() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![tu("c1", "run_sql", json!({"query": "SELECT 1"}))],
        ));
        let lowered = lower_to_openai(&conv, &[]);
        let tc = &lowered["messages"][0]["tool_calls"][0];
        assert_eq!(tc["type"], "function");
        assert_eq!(tc["function"]["name"], "run_sql");
        // arguments is a JSON string, not an object
        assert!(tc["function"]["arguments"].is_string());
    }

    #[test]
    fn roundtrip_text_messages() {
        let conv = base_conv();
        let lowered = lower_to_openai(&conv, &[]);
        let (rebuilt, _) = lift_openai(&lowered);
        assert_eq!(rebuilt.len(), conv.len());
        for (a, b) in rebuilt.messages.iter().zip(conv.messages.iter()) {
            assert_eq!(a.role, b.role);
            assert_eq!(a.text_content(), b.text_content());
        }
    }

    #[test]
    fn roundtrip_tool_definitions() {
        let tools = vec![
            td_params(
                "search",
                "Search",
                json!({"type": "object", "properties": {"q": {"type": "string"}}}),
            ),
            td("calc", "Calculator"),
        ];
        let lowered = lower_to_openai(&IrConversation::new(), &tools);
        let (_, rebuilt_tools) = lift_openai(&lowered);
        assert_eq!(rebuilt_tools.len(), 2);
        assert_eq!(rebuilt_tools[0].name, "search");
        assert_eq!(rebuilt_tools[1].name, "calc");
    }

    #[test]
    fn roundtrip_tool_call_and_result() {
        let conv = IrConversation::new()
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "Let me check.".into(),
                    },
                    tu("c1", "lookup", json!({"key": "abc"})),
                ],
            ))
            .push(IrMessage::new(IrRole::Tool, vec![tr("c1", "found", false)]));
        let lowered = lower_to_openai(&conv, &[]);
        let (rebuilt, _) = lift_openai(&lowered);
        assert_eq!(rebuilt.len(), 2);
        assert_eq!(rebuilt.messages[0].role, IrRole::Assistant);
        assert_eq!(rebuilt.messages[1].role, IrRole::Tool);
    }

    #[test]
    fn arguments_is_json_string() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![tu("c1", "fn1", json!({"nested": {"a": [1, 2]}}))],
        ));
        let lowered = lower_to_openai(&conv, &[]);
        let args = lowered["messages"][0]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(args).unwrap();
        assert_eq!(parsed["nested"]["a"][0], 1);
    }

    #[test]
    fn multiple_tool_results_expand() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![tr("c1", "r1", false), tr("c2", "r2", false)],
        ));
        let lowered = lower_to_openai(&conv, &[]);
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["tool_call_id"], "c1");
        assert_eq!(msgs[1]["tool_call_id"], "c2");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. Claude → IR lowering
// ═══════════════════════════════════════════════════════════════════════

mod claude_to_ir {
    use super::*;

    #[test]
    fn system_prompt_lifts() {
        let req = json!({
            "system": "You are a coding assistant.",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "Hi"}]}]
        });
        let (conv, _) = lift_claude(&req);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(
            conv.messages[0].text_content(),
            "You are a coding assistant."
        );
    }

    #[test]
    fn text_content_blocks_lift() {
        let req = json!({
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Hello!"},
                    {"type": "text", "text": " How are you?"}
                ]
            }]
        });
        let (conv, _) = lift_claude(&req);
        let blocks = &conv.messages[0].content;
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], IrContentBlock::Text { text } if text == "Hello!"));
    }

    #[test]
    fn tool_use_blocks_lift() {
        let req = json!({
            "messages": [{
                "role": "assistant",
                "content": [{"type": "tool_use", "id": "tu1", "name": "search", "input": {"q": "test"}}]
            }]
        });
        let (conv, _) = lift_claude(&req);
        assert!(matches!(
            &conv.messages[0].content[0],
            IrContentBlock::ToolUse { id, name, input }
                if id == "tu1" && name == "search" && input["q"] == "test"
        ));
    }

    #[test]
    fn tool_result_blocks_lift() {
        let req = json!({
            "messages": [{
                "role": "user",
                "content": [{"type": "tool_result", "tool_use_id": "tu1",
                             "content": [{"type": "text", "text": "ok"}], "is_error": false}]
            }]
        });
        let (conv, _) = lift_claude(&req);
        assert!(matches!(
            &conv.messages[0].content[0],
            IrContentBlock::ToolResult { tool_use_id, is_error, .. }
                if tool_use_id == "tu1" && !is_error
        ));
    }

    #[test]
    fn tools_use_input_schema_field() {
        let req = json!({
            "messages": [],
            "tools": [{
                "name": "compile",
                "description": "Compile code",
                "input_schema": {"type": "object", "properties": {"lang": {"type": "string"}}}
            }]
        });
        let (_, tools) = lift_claude(&req);
        assert_eq!(tools[0].name, "compile");
        assert_eq!(tools[0].parameters["type"], "object");
    }

    #[test]
    fn max_tokens_not_in_conversation() {
        let req = json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 4096,
            "system": "Be brief.",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
        });
        let (conv, _) = lift_claude(&req);
        // max_tokens is a model param, not in IR conversation
        assert_eq!(conv.len(), 2); // system + user
    }

    #[test]
    fn thinking_block_lifts() {
        let req = json!({
            "messages": [{
                "role": "assistant",
                "content": [{"type": "thinking", "thinking": "Let me reason..."}]
            }]
        });
        let (conv, _) = lift_claude(&req);
        assert!(matches!(
            &conv.messages[0].content[0],
            IrContentBlock::Thinking { text } if text == "Let me reason..."
        ));
    }

    #[test]
    fn image_block_lifts() {
        let req = json!({
            "messages": [{
                "role": "user",
                "content": [{"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "abc123"}}]
            }]
        });
        let (conv, _) = lift_claude(&req);
        assert!(matches!(
            &conv.messages[0].content[0],
            IrContentBlock::Image { media_type, data }
                if media_type == "image/png" && data == "abc123"
        ));
    }

    #[test]
    fn no_system_field() {
        let req = json!({
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
        });
        let (conv, _) = lift_claude(&req);
        assert!(conv.system_message().is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. IR → Claude lifting (roundtrip, content blocks)
// ═══════════════════════════════════════════════════════════════════════

mod ir_to_claude {
    use super::*;

    #[test]
    fn system_extracted_to_top_level() {
        let lowered = lower_to_claude(&base_conv(), &[]);
        assert_eq!(lowered["system"], "You are a helpful assistant.");
        let msgs = lowered["messages"].as_array().unwrap();
        assert!(msgs.iter().all(|m| m["role"] != "system"));
    }

    #[test]
    fn content_blocks_are_typed_objects() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hello"));
        let lowered = lower_to_claude(&conv, &[]);
        let block = &lowered["messages"][0]["content"][0];
        assert_eq!(block["type"], "text");
        assert_eq!(block["text"], "hello");
    }

    #[test]
    fn tool_use_block_format() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![tu("tu_1", "search", json!({"q": "rust"}))],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        let block = &lowered["messages"][0]["content"][0];
        assert_eq!(block["type"], "tool_use");
        assert_eq!(block["id"], "tu_1");
        assert_eq!(block["name"], "search");
        assert_eq!(block["input"]["q"], "rust");
    }

    #[test]
    fn tool_result_block_with_error() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![tr("tu_1", "Error: not found", true)],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        let block = &lowered["messages"][0]["content"][0];
        assert_eq!(block["type"], "tool_result");
        assert_eq!(block["is_error"], true);
    }

    #[test]
    fn thinking_block_preserved() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "step 1...".into(),
            }],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        assert_eq!(lowered["messages"][0]["content"][0]["type"], "thinking");
    }

    #[test]
    fn image_block_base64_source() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/jpeg".into(),
                data: "dGVzdA==".into(),
            }],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        let block = &lowered["messages"][0]["content"][0];
        assert_eq!(block["source"]["type"], "base64");
        assert_eq!(block["source"]["media_type"], "image/jpeg");
    }

    #[test]
    fn roundtrip_text_messages() {
        let conv = base_conv();
        let lowered = lower_to_claude(&conv, &[]);
        let (rebuilt, _) = lift_claude(&lowered);
        // system + user + assistant
        assert_eq!(rebuilt.len(), conv.len());
        assert_eq!(
            rebuilt.system_message().unwrap().text_content(),
            conv.system_message().unwrap().text_content()
        );
    }

    #[test]
    fn roundtrip_tool_use() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Looking up...".into(),
                },
                tu("tu1", "db_query", json!({"sql": "SELECT 1"})),
            ],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        let (rebuilt, _) = lift_claude(&lowered);
        assert_eq!(rebuilt.messages[0].content.len(), 2);
        assert!(matches!(
            &rebuilt.messages[0].content[1],
            IrContentBlock::ToolUse { name, .. } if name == "db_query"
        ));
    }

    #[test]
    fn tools_use_input_schema_key() {
        let tools = vec![td("fn1", "A function")];
        let lowered = lower_to_claude(&IrConversation::new(), &tools);
        assert!(lowered["tools"][0].get("input_schema").is_some());
        assert!(lowered["tools"][0].get("parameters").is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Gemini → IR lowering
// ═══════════════════════════════════════════════════════════════════════

mod gemini_to_ir {
    use super::*;

    #[test]
    fn contents_and_system_instruction_lift() {
        let req = json!({
            "system_instruction": {"parts": [{"text": "You are an expert."}]},
            "contents": [
                {"role": "user", "parts": [{"text": "Explain Rust."}]},
                {"role": "model", "parts": [{"text": "Rust is..."}]}
            ]
        });
        let (conv, _) = lift_gemini(&req);
        assert_eq!(conv.len(), 3);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "You are an expert.");
        assert_eq!(conv.messages[2].role, IrRole::Assistant);
    }

    #[test]
    fn function_call_parts_lift() {
        let req = json!({
            "contents": [{
                "role": "model",
                "parts": [{"functionCall": {"name": "calc", "args": {"x": 5}}}]
            }]
        });
        let (conv, _) = lift_gemini(&req);
        assert!(matches!(
            &conv.messages[0].content[0],
            IrContentBlock::ToolUse { name, input, .. }
                if name == "calc" && input["x"] == 5
        ));
    }

    #[test]
    fn function_response_parts_lift() {
        let req = json!({
            "contents": [{
                "role": "user",
                "parts": [{"functionResponse": {"name": "calc", "response": {"result": "42"}}}]
            }]
        });
        let (conv, _) = lift_gemini(&req);
        assert!(matches!(
            &conv.messages[0].content[0],
            IrContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "calc"
        ));
    }

    #[test]
    fn tool_declarations_lift() {
        let req = json!({
            "contents": [],
            "tools": [{"function_declarations": [
                {"name": "search", "description": "Web search", "parameters": {"type": "object"}},
                {"name": "calc", "description": "Math", "parameters": {"type": "object"}}
            ]}]
        });
        let (_, tools) = lift_gemini(&req);
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "search");
        assert_eq!(tools[1].name, "calc");
    }

    #[test]
    fn generation_config_not_in_conversation() {
        let req = json!({
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "generationConfig": {"temperature": 0.5, "topP": 0.8, "maxOutputTokens": 1024}
        });
        let (conv, _) = lift_gemini(&req);
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].text_content(), "hi");
    }

    #[test]
    fn no_system_instruction() {
        let req = json!({"contents": [{"role": "user", "parts": [{"text": "hi"}]}]});
        let (conv, _) = lift_gemini(&req);
        assert!(conv.system_message().is_none());
    }

    #[test]
    fn model_role_becomes_assistant() {
        let req = json!({
            "contents": [{"role": "model", "parts": [{"text": "response"}]}]
        });
        let (conv, _) = lift_gemini(&req);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. IR → Gemini lifting (roundtrip, parts)
// ═══════════════════════════════════════════════════════════════════════

mod ir_to_gemini {
    use super::*;

    #[test]
    fn system_becomes_system_instruction() {
        let lowered = lower_to_gemini(&base_conv(), &[]);
        assert_eq!(
            lowered["system_instruction"]["parts"][0]["text"],
            "You are a helpful assistant."
        );
    }

    #[test]
    fn assistant_role_becomes_model() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::Assistant, "hi"));
        let lowered = lower_to_gemini(&conv, &[]);
        assert_eq!(lowered["contents"][0]["role"], "model");
    }

    #[test]
    fn text_becomes_text_part() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "query"));
        let lowered = lower_to_gemini(&conv, &[]);
        assert_eq!(lowered["contents"][0]["parts"][0]["text"], "query");
    }

    #[test]
    fn tool_use_becomes_function_call() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![tu("c1", "search", json!({"q": "test"}))],
        ));
        let lowered = lower_to_gemini(&conv, &[]);
        let part = &lowered["contents"][0]["parts"][0];
        assert_eq!(part["functionCall"]["name"], "search");
        assert_eq!(part["functionCall"]["args"]["q"], "test");
    }

    #[test]
    fn tool_result_becomes_function_response() {
        let conv =
            IrConversation::new().push(IrMessage::new(IrRole::Tool, vec![tr("calc", "42", false)]));
        let lowered = lower_to_gemini(&conv, &[]);
        let part = &lowered["contents"][0]["parts"][0];
        assert_eq!(part["functionResponse"]["name"], "calc");
        assert_eq!(part["functionResponse"]["response"]["result"], "42");
    }

    #[test]
    fn thinking_blocks_dropped() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking { text: "hmm".into() },
                IrContentBlock::Text {
                    text: "result".into(),
                },
            ],
        ));
        let lowered = lower_to_gemini(&conv, &[]);
        let parts = lowered["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], "result");
    }

    #[test]
    fn image_becomes_inline_data() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "base64data".into(),
            }],
        ));
        let lowered = lower_to_gemini(&conv, &[]);
        let part = &lowered["contents"][0]["parts"][0];
        assert_eq!(part["inline_data"]["mime_type"], "image/png");
        assert_eq!(part["inline_data"]["data"], "base64data");
    }

    #[test]
    fn roundtrip_text_messages() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Be brief."))
            .push(IrMessage::text(IrRole::User, "What is 2+2?"))
            .push(IrMessage::text(IrRole::Assistant, "4"));
        let lowered = lower_to_gemini(&conv, &[]);
        let (rebuilt, _) = lift_gemini(&lowered);
        assert_eq!(rebuilt.len(), conv.len());
        assert_eq!(
            rebuilt.system_message().unwrap().text_content(),
            "Be brief."
        );
    }

    #[test]
    fn tools_use_function_declarations_wrapper() {
        let tools = vec![td("fn1", "A function"), td("fn2", "Another")];
        let lowered = lower_to_gemini(&IrConversation::new(), &tools);
        let decls = lowered["tools"][0]["function_declarations"]
            .as_array()
            .unwrap();
        assert_eq!(decls.len(), 2);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Codex → IR lowering
// ═══════════════════════════════════════════════════════════════════════

mod codex_lowering {
    use super::*;

    #[test]
    fn codex_uses_openai_format() {
        let conv = base_conv();
        let tools = vec![td("compile", "Compile")];
        assert_eq!(
            lower_to_codex(&conv, &tools),
            lower_to_openai(&conv, &tools)
        );
    }

    #[test]
    fn codex_instructions_as_system_message() {
        // Codex uses a system message for instructions
        let conv = IrConversation::new()
            .push(IrMessage::text(
                IrRole::System,
                "Write code in a sandbox. No network access.",
            ))
            .push(IrMessage::text(IrRole::User, "Build a web server"));
        let lowered = lower_to_codex(&conv, &[]);
        assert_eq!(
            lowered["messages"][0]["content"],
            "Write code in a sandbox. No network access."
        );
    }

    #[test]
    fn codex_sandbox_mode_not_in_lowered_output() {
        // Sandbox mode is a Codex-specific config, not part of lowered messages
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let lowered = lower_to_codex(&conv, &[]);
        let serialized = serde_json::to_string(&lowered).unwrap();
        assert!(!serialized.contains("sandbox"));
    }

    #[test]
    fn codex_role_mapping_matches_openai() {
        assert_eq!(
            ir_role_to_dialect(IrRole::System, Dialect::Codex),
            ir_role_to_dialect(IrRole::System, Dialect::OpenAi)
        );
        assert_eq!(
            ir_role_to_dialect(IrRole::Tool, Dialect::Codex),
            ir_role_to_dialect(IrRole::Tool, Dialect::OpenAi)
        );
    }

    #[test]
    fn codex_tool_calls_same_as_openai() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![tu("c1", "exec", json!({"cmd": "ls"}))],
        ));
        assert_eq!(lower_to_codex(&conv, &[]), lower_to_openai(&conv, &[]));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Kimi → IR lowering
// ═══════════════════════════════════════════════════════════════════════

mod kimi_lowering {
    use super::*;

    #[test]
    fn kimi_uses_openai_format() {
        let conv = base_conv();
        assert_eq!(lower_to_kimi(&conv, &[]), lower_to_openai(&conv, &[]));
    }

    #[test]
    fn kimi_context_window_as_system_message() {
        // Kimi's context window / k1 reasoning info would be in system prompt
        let conv = IrConversation::new()
            .push(IrMessage::text(
                IrRole::System,
                "Use k1 reasoning mode. Context window: 128k tokens.",
            ))
            .push(IrMessage::text(IrRole::User, "Analyze this codebase"));
        let lowered = lower_to_kimi(&conv, &[]);
        let sys = lowered["messages"][0]["content"].as_str().unwrap();
        assert!(sys.contains("k1 reasoning"));
        assert!(sys.contains("128k tokens"));
    }

    #[test]
    fn kimi_role_mapping() {
        assert_eq!(ir_role_to_dialect(IrRole::System, Dialect::Kimi), "system");
        assert_eq!(ir_role_to_dialect(IrRole::User, Dialect::Kimi), "user");
        assert_eq!(
            ir_role_to_dialect(IrRole::Assistant, Dialect::Kimi),
            "assistant"
        );
        assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::Kimi), "tool");
    }

    #[test]
    fn kimi_empty_conversation() {
        let lowered = lower_to_kimi(&IrConversation::new(), &[]);
        assert_eq!(lowered["messages"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn kimi_with_tools() {
        let tools = vec![td("web_search", "Search the internet")];
        let lowered = lower_to_kimi(&IrConversation::new(), &tools);
        assert_eq!(lowered["tools"][0]["function"]["name"], "web_search");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Copilot → IR lowering
// ═══════════════════════════════════════════════════════════════════════

mod copilot_lowering {
    use super::*;

    #[test]
    fn copilot_uses_openai_format() {
        let conv = base_conv();
        assert_eq!(lower_to_copilot(&conv, &[]), lower_to_openai(&conv, &[]));
    }

    #[test]
    fn copilot_agent_system_prompt() {
        let conv = IrConversation::new()
            .push(IrMessage::text(
                IrRole::System,
                "You are GitHub Copilot, an AI programming assistant.",
            ))
            .push(IrMessage::text(IrRole::User, "Fix the bug in main.rs"));
        let lowered = lower_to_copilot(&conv, &[]);
        assert_eq!(
            lowered["messages"][0]["content"],
            "You are GitHub Copilot, an AI programming assistant."
        );
    }

    #[test]
    fn copilot_role_mapping() {
        assert_eq!(
            ir_role_to_dialect(IrRole::System, Dialect::Copilot),
            "system"
        );
        assert_eq!(ir_role_to_dialect(IrRole::User, Dialect::Copilot), "user");
        assert_eq!(
            ir_role_to_dialect(IrRole::Assistant, Dialect::Copilot),
            "assistant"
        );
        assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::Copilot), "tool");
    }

    #[test]
    fn copilot_tool_calls() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![tu(
                "c1",
                "edit_file",
                json!({"path": "src/main.rs", "content": "fn main() {}"}),
            )],
        ));
        let lowered = lower_to_copilot(&conv, &[]);
        let tc = &lowered["messages"][0]["tool_calls"][0];
        assert_eq!(tc["function"]["name"], "edit_file");
    }

    #[test]
    fn copilot_matches_openai_with_tools() {
        let conv = base_conv();
        let tools = vec![td("edit", "Edit file"), td("run", "Run command")];
        assert_eq!(
            lower_to_copilot(&conv, &tools),
            lower_to_openai(&conv, &tools)
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Cross-dialect roundtrip
// ═══════════════════════════════════════════════════════════════════════

mod cross_dialect_roundtrip {
    use super::*;

    #[test]
    fn openai_to_claude_text_fidelity() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Be helpful."))
            .push(IrMessage::text(IrRole::User, "What is Rust?"))
            .push(IrMessage::text(IrRole::Assistant, "Rust is a language."));
        let oai = lower_to_openai(&conv, &[]);
        let (ir_from_oai, _) = lift_openai(&oai);
        let claude = lower_to_claude(&ir_from_oai, &[]);
        assert_eq!(claude["system"], "Be helpful.");
        assert_eq!(claude["messages"][0]["content"][0]["text"], "What is Rust?");
    }

    #[test]
    fn claude_to_gemini_text_fidelity() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Expert mode."))
            .push(IrMessage::text(IrRole::User, "Explain ownership."));
        let claude = lower_to_claude(&conv, &[]);
        let (ir_from_claude, _) = lift_claude(&claude);
        let gemini = lower_to_gemini(&ir_from_claude, &[]);
        assert_eq!(
            gemini["system_instruction"]["parts"][0]["text"],
            "Expert mode."
        );
        assert_eq!(
            gemini["contents"][0]["parts"][0]["text"],
            "Explain ownership."
        );
    }

    #[test]
    fn gemini_to_openai_text_fidelity() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Sys prompt"))
            .push(IrMessage::text(IrRole::User, "Hello"))
            .push(IrMessage::text(IrRole::Assistant, "Hi!"));
        let gemini = lower_to_gemini(&conv, &[]);
        let (ir_from_gem, _) = lift_gemini(&gemini);
        let oai = lower_to_openai(&ir_from_gem, &[]);
        assert_eq!(oai["messages"][0]["role"], "system");
        assert_eq!(oai["messages"][0]["content"], "Sys prompt");
        assert_eq!(oai["messages"][2]["content"], "Hi!");
    }

    #[test]
    fn openai_to_gemini_tool_definitions() {
        let tools = vec![td_params(
            "search",
            "Search web",
            json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        )];
        let oai = lower_to_openai(&IrConversation::new(), &tools);
        let (_, tools_from_oai) = lift_openai(&oai);
        let gemini = lower_to_gemini(&IrConversation::new(), &tools_from_oai);
        let decl = &gemini["tools"][0]["function_declarations"][0];
        assert_eq!(decl["name"], "search");
        assert_eq!(decl["parameters"]["type"], "object");
    }

    #[test]
    fn claude_to_openai_tool_use_roundtrip() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Searching.".into(),
                },
                tu("tu1", "search", json!({"q": "rust"})),
            ],
        ));
        let claude = lower_to_claude(&conv, &[]);
        let (ir_from_claude, _) = lift_claude(&claude);
        let oai = lower_to_openai(&ir_from_claude, &[]);
        assert_eq!(oai["messages"][0]["content"], "Searching.");
        assert_eq!(
            oai["messages"][0]["tool_calls"][0]["function"]["name"],
            "search"
        );
    }

    #[test]
    fn all_dialects_preserve_user_message_text() {
        let text = "The quick brown fox jumps over the lazy dog.";
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, text));
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &[]);
            let (rebuilt, _) = match *dialect {
                Dialect::OpenAi | Dialect::Codex | Dialect::Copilot | Dialect::Kimi => {
                    lift_openai(&lowered)
                }
                Dialect::Claude => lift_claude(&lowered),
                Dialect::Gemini => lift_gemini(&lowered),
            };
            assert_eq!(
                rebuilt.messages[0].text_content(),
                text,
                "{dialect}: user text should survive roundtrip"
            );
        }
    }

    #[test]
    fn openai_to_kimi_identity() {
        let conv = base_conv();
        let tools = vec![td("t1", "test")];
        assert_eq!(lower_to_openai(&conv, &tools), lower_to_kimi(&conv, &tools));
    }

    #[test]
    fn openai_to_codex_identity() {
        let conv = base_conv();
        let tools = vec![td("t1", "test")];
        assert_eq!(
            lower_to_openai(&conv, &tools),
            lower_to_codex(&conv, &tools)
        );
    }

    #[test]
    fn lower_for_dialect_consistent_dispatch() {
        let conv = base_conv();
        let tools = vec![td("t", "d")];
        for dialect in Dialect::all() {
            let via_dispatch = lower_for_dialect(*dialect, &conv, &tools);
            let via_direct = match *dialect {
                Dialect::OpenAi => lower_to_openai(&conv, &tools),
                Dialect::Claude => lower_to_claude(&conv, &tools),
                Dialect::Gemini => lower_to_gemini(&conv, &tools),
                Dialect::Kimi => lower_to_kimi(&conv, &tools),
                Dialect::Codex => lower_to_codex(&conv, &tools),
                Dialect::Copilot => lower_to_copilot(&conv, &tools),
            };
            assert_eq!(via_dispatch, via_direct, "{dialect}: dispatch mismatch");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Information loss tracking
// ═══════════════════════════════════════════════════════════════════════

mod information_loss {
    use super::*;

    #[test]
    fn thinking_blocks_lost_in_gemini() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "I should analyze...".into(),
                },
                IrContentBlock::Text {
                    text: "The answer is 42.".into(),
                },
            ],
        ));
        let lowered = lower_to_gemini(&conv, &[]);
        let parts = lowered["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1, "thinking block should be dropped in Gemini");
        assert_eq!(parts[0]["text"], "The answer is 42.");
    }

    #[test]
    fn thinking_blocks_preserved_in_claude() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "reasoning".into(),
                },
                IrContentBlock::Text {
                    text: "answer".into(),
                },
            ],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        let content = lowered["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "thinking");
    }

    #[test]
    fn thinking_blocks_lost_in_openai() {
        // OpenAI lowering treats thinking as text_content()
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "deep thought".into(),
            }],
        ));
        let lowered = lower_to_openai(&conv, &[]);
        // Thinking is not a text block, so text_content() returns ""
        let msg = &lowered["messages"][0];
        assert_eq!(msg["content"], "");
    }

    #[test]
    fn tool_call_id_lost_in_gemini_function_call() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![tu("unique_call_id_123", "search", json!({"q": "test"}))],
        ));
        let lowered = lower_to_gemini(&conv, &[]);
        let part = &lowered["contents"][0]["parts"][0];
        // Gemini functionCall does not have an "id" field
        assert!(part["functionCall"].get("id").is_none());
    }

    #[test]
    fn tool_result_is_error_lost_in_gemini() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![tr("fn1", "Error occurred", true)],
        ));
        let lowered = lower_to_gemini(&conv, &[]);
        let part = &lowered["contents"][0]["parts"][0];
        // Gemini functionResponse has no is_error field
        assert!(part["functionResponse"].get("is_error").is_none());
    }

    #[test]
    fn tool_result_is_error_preserved_in_claude() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![tr("fn1", "Error occurred", true)],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        assert_eq!(lowered["messages"][0]["content"][0]["is_error"], true);
    }

    #[test]
    fn metadata_lost_in_all_dialects() {
        let mut meta = BTreeMap::new();
        meta.insert("custom_key".to_string(), json!("custom_value"));
        meta.insert("vendor_trace".to_string(), json!(12345));
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text {
                text: "hello".into(),
            }],
            metadata: meta,
        };
        let conv = IrConversation::from_messages(vec![msg]);
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &[]);
            let serialized = serde_json::to_string(&lowered).unwrap();
            assert!(
                !serialized.contains("custom_key"),
                "{dialect}: metadata should not leak"
            );
            assert!(
                !serialized.contains("vendor_trace"),
                "{dialect}: metadata should not leak"
            );
        }
    }

    #[test]
    fn openai_content_is_string_not_blocks() {
        // Claude uses content blocks; OpenAI uses a flat string for text
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hello"));
        let oai = lower_to_openai(&conv, &[]);
        assert!(
            oai["messages"][0]["content"].is_string(),
            "OpenAI text should be flat string"
        );
        let claude = lower_to_claude(&conv, &[]);
        assert!(
            claude["messages"][0]["content"].is_array(),
            "Claude text should be content blocks array"
        );
    }

    #[test]
    fn gemini_uses_parts_not_content() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hello"));
        let gemini = lower_to_gemini(&conv, &[]);
        assert!(gemini["contents"][0].get("parts").is_some());
        assert!(gemini["contents"][0].get("content").is_none());
    }

    #[test]
    fn image_lost_in_openai_text() {
        // OpenAI lowering extracts text_content() which ignores images
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Image {
                    media_type: "image/png".into(),
                    data: "abc".into(),
                },
                IrContentBlock::Text {
                    text: "describe this".into(),
                },
            ],
        ));
        let lowered = lower_to_openai(&conv, &[]);
        // Only text is preserved in OpenAI format
        assert_eq!(lowered["messages"][0]["content"], "describe this");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Error cases / boundary conditions
// ═══════════════════════════════════════════════════════════════════════

mod error_cases {
    use super::*;

    #[test]
    fn empty_conversation_all_dialects() {
        let conv = IrConversation::new();
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &[]);
            assert!(lowered.is_object(), "{dialect}: must produce JSON object");
            let key = if *dialect == Dialect::Gemini {
                "contents"
            } else {
                "messages"
            };
            assert_eq!(
                lowered[key].as_array().unwrap().len(),
                0,
                "{dialect}: empty conv must produce empty array"
            );
        }
    }

    #[test]
    fn empty_tools_omits_tools_field() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &[]);
            assert!(
                lowered.get("tools").is_none(),
                "{dialect}: tools should be absent when empty"
            );
        }
    }

    #[test]
    fn only_system_messages_openai() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "a"))
            .push(IrMessage::text(IrRole::System, "b"));
        let lowered = lower_to_openai(&conv, &[]);
        assert_eq!(lowered["messages"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn only_system_messages_claude() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "a"))
            .push(IrMessage::text(IrRole::System, "b"));
        let lowered = lower_to_claude(&conv, &[]);
        // System extracted; only first system message is captured
        assert_eq!(lowered["messages"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn only_system_messages_gemini() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "a"))
            .push(IrMessage::text(IrRole::System, "b"));
        let lowered = lower_to_gemini(&conv, &[]);
        assert_eq!(lowered["contents"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn empty_text_messages() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, ""));
        let oai = lower_to_openai(&conv, &[]);
        assert_eq!(oai["messages"][0]["content"], "");
        let claude = lower_to_claude(&conv, &[]);
        assert_eq!(claude["messages"][0]["content"][0]["text"], "");
        let gemini = lower_to_gemini(&conv, &[]);
        assert_eq!(gemini["contents"][0]["parts"][0]["text"], "");
    }

    #[test]
    fn message_with_no_content_blocks() {
        let conv = IrConversation::new().push(IrMessage::new(IrRole::User, vec![]));
        let oai = lower_to_openai(&conv, &[]);
        assert_eq!(oai["messages"][0]["role"], "user");
        assert_eq!(oai["messages"][0]["content"], "");
    }

    #[test]
    fn tool_result_with_empty_content() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![],
                is_error: false,
            }],
        ));
        let oai = lower_to_openai(&conv, &[]);
        assert_eq!(oai["messages"][0]["content"], "");
        let claude = lower_to_claude(&conv, &[]);
        assert_eq!(
            claude["messages"][0]["content"][0]["content"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn unicode_preservation() {
        let text = "こんにちは 🌍 مرحبا Ñ Привет 中文 🎉";
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, text));
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &[]);
            let (rebuilt, _) = match *dialect {
                Dialect::OpenAi | Dialect::Codex | Dialect::Copilot | Dialect::Kimi => {
                    lift_openai(&lowered)
                }
                Dialect::Claude => lift_claude(&lowered),
                Dialect::Gemini => lift_gemini(&lowered),
            };
            assert_eq!(
                rebuilt.messages[0].text_content(),
                text,
                "{dialect}: unicode should be preserved"
            );
        }
    }

    #[test]
    fn very_long_message_preserved() {
        let long_text = "x".repeat(100_000);
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, &long_text));
        let lowered = lower_to_openai(&conv, &[]);
        assert_eq!(
            lowered["messages"][0]["content"].as_str().unwrap().len(),
            100_000
        );
    }

    #[test]
    fn special_chars_in_tool_names() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![tu("c1", "my-tool_v2.0@beta", json!({}))],
        ));
        let oai = lower_to_openai(&conv, &[]);
        assert_eq!(
            oai["messages"][0]["tool_calls"][0]["function"]["name"],
            "my-tool_v2.0@beta"
        );
        let claude = lower_to_claude(&conv, &[]);
        assert_eq!(
            claude["messages"][0]["content"][0]["name"],
            "my-tool_v2.0@beta"
        );
    }

    #[test]
    fn json_special_chars_in_text() {
        let text = r#"She said "hello" \n\t {'key': 'value'}"#;
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, text));
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &[]);
            let serialized = serde_json::to_string(&lowered).unwrap();
            let _: serde_json::Value =
                serde_json::from_str(&serialized).expect("must be valid JSON");
        }
    }

    #[test]
    fn complex_nested_tool_input() {
        let input = json!({
            "query": "SELECT *",
            "options": {"limit": 10, "nested": {"deep": [1, [2, 3]], "map": {"a": "b"}}},
            "tags": ["alpha", "beta"]
        });
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![tu("c1", "db", input.clone())],
        ));
        // Claude preserves as object
        let claude = lower_to_claude(&conv, &[]);
        assert_eq!(claude["messages"][0]["content"][0]["input"], input);
        // OpenAI stringifies then can be parsed back
        let oai = lower_to_openai(&conv, &[]);
        let args_str = oai["messages"][0]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(args_str).unwrap();
        assert_eq!(parsed, input);
        // Gemini preserves as object
        let gemini = lower_to_gemini(&conv, &[]);
        assert_eq!(
            gemini["contents"][0]["parts"][0]["functionCall"]["args"],
            input
        );
    }

    #[test]
    fn lowered_output_valid_json_all_dialects() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "query"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "checking".into(),
                    },
                    tu("c1", "search", json!({"q": "x"})),
                    IrContentBlock::Thinking { text: "hmm".into() },
                ],
            ))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![tr("c1", "result", false)],
            ))
            .push(IrMessage::new(
                IrRole::User,
                vec![IrContentBlock::Image {
                    media_type: "image/png".into(),
                    data: "base64".into(),
                }],
            ));
        let tools = vec![td("search", "Search")];
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &tools);
            let serialized = serde_json::to_string(&lowered).unwrap();
            let _: serde_json::Value = serde_json::from_str(&serialized)
                .unwrap_or_else(|e| panic!("{dialect}: invalid JSON: {e}"));
        }
    }

    #[test]
    fn newlines_and_tabs_preserved() {
        let text = "line1\nline2\tindented\r\nwindows";
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, text));
        let oai = lower_to_openai(&conv, &[]);
        assert_eq!(oai["messages"][0]["content"], text);
    }

    #[test]
    fn all_role_strings_non_empty() {
        let roles = [
            IrRole::System,
            IrRole::User,
            IrRole::Assistant,
            IrRole::Tool,
        ];
        for dialect in Dialect::all() {
            for role in &roles {
                let s = ir_role_to_dialect(*role, *dialect);
                assert!(
                    !s.is_empty(),
                    "{dialect}/{role:?}: role string must be non-empty"
                );
            }
        }
    }

    #[test]
    fn all_user_roles_are_user_string() {
        for dialect in Dialect::all() {
            assert_eq!(
                ir_role_to_dialect(IrRole::User, *dialect),
                "user",
                "{dialect}: user role should always be 'user'"
            );
        }
    }

    #[test]
    fn normalize_then_lower_deterministic() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "  sys1  "))
            .push(IrMessage::text(IrRole::User, "  query  "))
            .push(IrMessage::text(IrRole::System, "  sys2  "));
        let norm1 = normalize::normalize(&conv);
        let norm2 = normalize::normalize(&conv);
        for dialect in Dialect::all() {
            assert_eq!(
                lower_for_dialect(*dialect, &norm1, &[]),
                lower_for_dialect(*dialect, &norm2, &[]),
                "{dialect}: normalized lowering must be deterministic"
            );
        }
    }
}
