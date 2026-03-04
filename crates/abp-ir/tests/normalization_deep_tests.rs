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
//! Deep tests for the ABP IR normalization layer.
//!
//! Covers construction, role mapping, content types, normalization,
//! lowering to 6 dialects, roundtrip integrity, tool definition handling,
//! system message handling, content merging, serde roundtrips, and edge cases.

use abp_ir::*;
use abp_sdk_types::Dialect;
use serde_json::json;
use std::collections::BTreeMap;

// ── Helpers: normalization pipeline ─────────────────────────────────────

/// Merge duplicate system messages into a single leading system message.
fn dedup_system(conv: &IrConversation) -> IrConversation {
    let sys_texts: Vec<String> = conv
        .messages
        .iter()
        .filter(|m| m.role == IrRole::System)
        .map(|m| m.text_content())
        .collect();
    let mut out: Vec<IrMessage> = Vec::new();
    if !sys_texts.is_empty() {
        out.push(IrMessage::text(IrRole::System, sys_texts.join("\n")));
    }
    out.extend(
        conv.messages
            .iter()
            .filter(|m| m.role != IrRole::System)
            .cloned(),
    );
    IrConversation::from_messages(out)
}

/// Trim whitespace in text blocks.
fn trim_text(conv: &IrConversation) -> IrConversation {
    let messages = conv
        .messages
        .iter()
        .map(|m| IrMessage {
            role: m.role,
            content: m
                .content
                .iter()
                .map(|b| match b {
                    IrContentBlock::Text { text } => IrContentBlock::Text {
                        text: text.trim().to_string(),
                    },
                    other => other.clone(),
                })
                .collect(),
            metadata: m.metadata.clone(),
        })
        .collect();
    IrConversation::from_messages(messages)
}

/// Remove messages with no content blocks.
fn strip_empty(conv: &IrConversation) -> IrConversation {
    IrConversation::from_messages(
        conv.messages
            .iter()
            .filter(|m| !m.content.is_empty())
            .cloned()
            .collect(),
    )
}

/// Merge adjacent text blocks within each message.
fn merge_adjacent_text(conv: &IrConversation) -> IrConversation {
    let messages = conv
        .messages
        .iter()
        .map(|m| {
            let mut merged: Vec<IrContentBlock> = Vec::new();
            for block in &m.content {
                if let IrContentBlock::Text { text } = block {
                    if let Some(IrContentBlock::Text { text: prev }) = merged.last_mut() {
                        prev.push_str(text);
                        continue;
                    }
                }
                merged.push(block.clone());
            }
            IrMessage {
                role: m.role,
                content: merged,
                metadata: m.metadata.clone(),
            }
        })
        .collect();
    IrConversation::from_messages(messages)
}

/// Full normalization pipeline.
fn normalize(conv: &IrConversation) -> IrConversation {
    strip_empty(&merge_adjacent_text(&trim_text(&dedup_system(conv))))
}

/// Sort tool definitions by name for deterministic output.
fn sort_tools(tools: &mut [IrToolDefinition]) {
    tools.sort_by(|a, b| a.name.cmp(&b.name));
}

// ── Helpers: dialect role names ─────────────────────────────────────────

fn ir_role_to_dialect_role(role: IrRole, dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::OpenAi | Dialect::Codex | Dialect::Copilot | Dialect::Kimi => match role {
            IrRole::System => "system",
            IrRole::User => "user",
            IrRole::Assistant => "assistant",
            IrRole::Tool => "tool",
        },
        Dialect::Claude => match role {
            IrRole::System => "system",
            IrRole::User => "user",
            IrRole::Assistant => "assistant",
            IrRole::Tool => "user",
        },
        Dialect::Gemini => match role {
            IrRole::System => "system",
            IrRole::User => "user",
            IrRole::Assistant => "model",
            IrRole::Tool => "user",
        },
    }
}

// ── Helpers: lowering ───────────────────────────────────────────────────

fn lower_to_openai_like(conv: &IrConversation, tools: &[IrToolDefinition]) -> serde_json::Value {
    let messages: Vec<serde_json::Value> = conv
        .messages
        .iter()
        .map(|m| {
            let role = ir_role_to_dialect_role(m.role, Dialect::OpenAi);
            json!({"role": role, "content": m.text_content()})
        })
        .collect();
    let mut req = json!({"messages": messages});
    if !tools.is_empty() {
        let t: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.parameters}})
            })
            .collect();
        req["tools"] = json!(t);
    }
    req
}

fn lower_to_claude(conv: &IrConversation, tools: &[IrToolDefinition]) -> serde_json::Value {
    let system = conv.system_message().map(|m| m.text_content());
    let messages: Vec<serde_json::Value> = conv
        .messages
        .iter()
        .filter(|m| m.role != IrRole::System)
        .map(|m| {
            let role = ir_role_to_dialect_role(m.role, Dialect::Claude);
            let content: Vec<serde_json::Value> = m
                .content
                .iter()
                .map(|b| match b {
                    IrContentBlock::Text { text } => json!({"type": "text", "text": text}),
                    IrContentBlock::ToolUse { id, name, input } => {
                        json!({"type": "tool_use", "id": id, "name": name, "input": input})
                    }
                    IrContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => {
                        let inner: Vec<serde_json::Value> = content
                            .iter()
                            .map(|c| match c {
                                IrContentBlock::Text { text } => {
                                    json!({"type": "text", "text": text})
                                }
                                _ => json!({"type": "unknown"}),
                            })
                            .collect();
                        json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": inner, "is_error": is_error})
                    }
                    IrContentBlock::Image { media_type, data } => {
                        json!({"type": "image", "source": {"type": "base64", "media_type": media_type, "data": data}})
                    }
                    IrContentBlock::Thinking { text } => {
                        json!({"type": "thinking", "thinking": text})
                    }
                })
                .collect();
            json!({"role": role, "content": content})
        })
        .collect();
    let mut req = json!({"messages": messages});
    if let Some(sys) = system {
        req["system"] = json!(sys);
    }
    if !tools.is_empty() {
        let t: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                json!({"name": t.name, "description": t.description, "input_schema": t.parameters})
            })
            .collect();
        req["tools"] = json!(t);
    }
    req
}

fn lower_to_gemini(conv: &IrConversation, tools: &[IrToolDefinition]) -> serde_json::Value {
    let system = conv.system_message().map(|m| m.text_content());
    let contents: Vec<serde_json::Value> = conv
        .messages
        .iter()
        .filter(|m| m.role != IrRole::System)
        .map(|m| {
            let role = ir_role_to_dialect_role(m.role, Dialect::Gemini);
            let parts: Vec<serde_json::Value> = m
                .content
                .iter()
                .filter_map(|b| match b {
                    IrContentBlock::Text { text } => Some(json!({"text": text})),
                    IrContentBlock::Image { media_type, data } => {
                        Some(json!({"inline_data": {"mime_type": media_type, "data": data}}))
                    }
                    _ => None,
                })
                .collect();
            json!({"role": role, "parts": parts})
        })
        .collect();
    let mut req = json!({"contents": contents});
    if let Some(sys) = system {
        req["system_instruction"] = json!({"parts": [{"text": sys}]});
    }
    if !tools.is_empty() {
        let decls: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                json!({"name": t.name, "description": t.description, "parameters": t.parameters})
            })
            .collect();
        req["tools"] = json!([{"function_declarations": decls}]);
    }
    req
}

fn lower_to_kimi(conv: &IrConversation, tools: &[IrToolDefinition]) -> serde_json::Value {
    // Kimi uses OpenAI-compatible format
    lower_to_openai_like(conv, tools)
}

fn lower_to_codex(conv: &IrConversation, tools: &[IrToolDefinition]) -> serde_json::Value {
    // Codex uses OpenAI-compatible format
    lower_to_openai_like(conv, tools)
}

fn lower_to_copilot(conv: &IrConversation, tools: &[IrToolDefinition]) -> serde_json::Value {
    // Copilot uses OpenAI-compatible format
    lower_to_openai_like(conv, tools)
}

fn lower_for_dialect(
    dialect: Dialect,
    conv: &IrConversation,
    tools: &[IrToolDefinition],
) -> serde_json::Value {
    match dialect {
        Dialect::OpenAi => lower_to_openai_like(conv, tools),
        Dialect::Claude => lower_to_claude(conv, tools),
        Dialect::Gemini => lower_to_gemini(conv, tools),
        Dialect::Kimi => lower_to_kimi(conv, tools),
        Dialect::Codex => lower_to_codex(conv, tools),
        Dialect::Copilot => lower_to_copilot(conv, tools),
    }
}

/// Lift OpenAI-format JSON back to IR.
fn lift_from_openai(req: &serde_json::Value) -> (IrConversation, Vec<IrToolDefinition>) {
    let mut conv = IrConversation::new();
    if let Some(msgs) = req["messages"].as_array() {
        for m in msgs {
            let role = match m["role"].as_str().unwrap() {
                "system" => IrRole::System,
                "user" => IrRole::User,
                "assistant" => IrRole::Assistant,
                "tool" => IrRole::Tool,
                _ => panic!("unknown role"),
            };
            let text = m["content"].as_str().unwrap_or("");
            conv = conv.push(IrMessage::text(role, text));
        }
    }
    let tools = req["tools"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|t| {
                    let f = &t["function"];
                    IrToolDefinition {
                        name: f["name"].as_str().unwrap().to_string(),
                        description: f["description"].as_str().unwrap().to_string(),
                        parameters: f["parameters"].clone(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    (conv, tools)
}

// ═══════════════════════════════════════════════════════════════════════
// 1. IrMessage construction
// ═══════════════════════════════════════════════════════════════════════

mod construction {
    use super::*;

    #[test]
    fn from_text() {
        let msg = IrMessage::text(IrRole::User, "hello world");
        assert_eq!(msg.role, IrRole::User);
        assert_eq!(msg.text_content(), "hello world");
        assert!(msg.is_text_only());
        assert_eq!(msg.content.len(), 1);
    }

    #[test]
    fn from_tool_calls() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "read_file".into(),
                    input: json!({"path": "/tmp/a.rs"}),
                },
                IrContentBlock::ToolUse {
                    id: "call_2".into(),
                    name: "write_file".into(),
                    input: json!({"path": "/tmp/b.rs", "content": "fn main() {}"}),
                },
            ],
        );
        assert_eq!(msg.role, IrRole::Assistant);
        assert!(!msg.is_text_only());
        assert_eq!(msg.tool_use_blocks().len(), 2);
        assert!(msg.text_content().is_empty());
    }

    #[test]
    fn from_tool_result() {
        let msg = IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "file contents here".into(),
                }],
                is_error: false,
            }],
        );
        assert_eq!(msg.role, IrRole::Tool);
        assert!(!msg.is_text_only());
        assert!(msg.tool_use_blocks().is_empty());
    }

    #[test]
    fn from_tool_result_with_error() {
        let msg = IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_x".into(),
                content: vec![IrContentBlock::Text {
                    text: "Permission denied".into(),
                }],
                is_error: true,
            }],
        );
        if let IrContentBlock::ToolResult { is_error, .. } = &msg.content[0] {
            assert!(is_error);
        } else {
            panic!("expected ToolResult");
        }
    }

    #[test]
    fn mixed_text_and_tool_use() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me check.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "search".into(),
                    input: json!({"q": "rust"}),
                },
            ],
        );
        assert!(!msg.is_text_only());
        assert_eq!(msg.tool_use_blocks().len(), 1);
        assert_eq!(msg.text_content(), "Let me check.");
    }

    #[test]
    fn metadata_preserved_on_construction() {
        let mut meta = BTreeMap::new();
        meta.insert("source".to_string(), json!("test"));
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text { text: "hi".into() }],
            metadata: meta.clone(),
        };
        assert_eq!(msg.metadata, meta);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. IrRole mapping across dialects
// ═══════════════════════════════════════════════════════════════════════

mod role_mapping {
    use super::*;

    #[test]
    fn openai_roles_map_directly() {
        assert_eq!(
            ir_role_to_dialect_role(IrRole::System, Dialect::OpenAi),
            "system"
        );
        assert_eq!(
            ir_role_to_dialect_role(IrRole::User, Dialect::OpenAi),
            "user"
        );
        assert_eq!(
            ir_role_to_dialect_role(IrRole::Assistant, Dialect::OpenAi),
            "assistant"
        );
        assert_eq!(
            ir_role_to_dialect_role(IrRole::Tool, Dialect::OpenAi),
            "tool"
        );
    }

    #[test]
    fn claude_tool_role_maps_to_user() {
        assert_eq!(
            ir_role_to_dialect_role(IrRole::Tool, Dialect::Claude),
            "user"
        );
    }

    #[test]
    fn gemini_assistant_maps_to_model() {
        assert_eq!(
            ir_role_to_dialect_role(IrRole::Assistant, Dialect::Gemini),
            "model"
        );
    }

    #[test]
    fn gemini_tool_maps_to_user() {
        assert_eq!(
            ir_role_to_dialect_role(IrRole::Tool, Dialect::Gemini),
            "user"
        );
    }

    #[test]
    fn all_dialects_have_user_role() {
        for dialect in Dialect::all() {
            assert_eq!(ir_role_to_dialect_role(IrRole::User, *dialect), "user");
        }
    }

    #[test]
    fn kimi_codex_copilot_match_openai_roles() {
        let openai_compatible = [Dialect::Kimi, Dialect::Codex, Dialect::Copilot];
        let roles = [
            IrRole::System,
            IrRole::User,
            IrRole::Assistant,
            IrRole::Tool,
        ];
        for dialect in &openai_compatible {
            for role in &roles {
                assert_eq!(
                    ir_role_to_dialect_role(*role, *dialect),
                    ir_role_to_dialect_role(*role, Dialect::OpenAi),
                    "{role:?} should map identically for {dialect:?} and OpenAI"
                );
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. IrContent types — all variants
// ═══════════════════════════════════════════════════════════════════════

mod content_types {
    use super::*;

    #[test]
    fn text_block() {
        let block = IrContentBlock::Text {
            text: "Hello".into(),
        };
        assert!(matches!(block, IrContentBlock::Text { .. }));
    }

    #[test]
    fn tool_use_block() {
        let block = IrContentBlock::ToolUse {
            id: "id_1".into(),
            name: "grep".into(),
            input: json!({"pattern": "fn main"}),
        };
        if let IrContentBlock::ToolUse { id, name, input } = &block {
            assert_eq!(id, "id_1");
            assert_eq!(name, "grep");
            assert_eq!(input["pattern"], "fn main");
        } else {
            panic!("expected ToolUse");
        }
    }

    #[test]
    fn tool_result_block() {
        let block = IrContentBlock::ToolResult {
            tool_use_id: "id_1".into(),
            content: vec![IrContentBlock::Text {
                text: "result".into(),
            }],
            is_error: false,
        };
        if let IrContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } = &block
        {
            assert_eq!(tool_use_id, "id_1");
            assert_eq!(content.len(), 1);
            assert!(!is_error);
        } else {
            panic!("expected ToolResult");
        }
    }

    #[test]
    fn image_block() {
        let block = IrContentBlock::Image {
            media_type: "image/jpeg".into(),
            data: "base64data==".into(),
        };
        if let IrContentBlock::Image { media_type, data } = &block {
            assert_eq!(media_type, "image/jpeg");
            assert_eq!(data, "base64data==");
        } else {
            panic!("expected Image");
        }
    }

    #[test]
    fn thinking_block() {
        let block = IrContentBlock::Thinking {
            text: "Let me reason about this...".into(),
        };
        if let IrContentBlock::Thinking { text } = &block {
            assert!(text.contains("reason"));
        } else {
            panic!("expected Thinking");
        }
    }

    #[test]
    fn all_content_types_in_single_message() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "thinking".into(),
                },
                IrContentBlock::Text {
                    text: "response".into(),
                },
                IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "tool".into(),
                    input: json!({}),
                },
                IrContentBlock::Image {
                    media_type: "image/png".into(),
                    data: "abc".into(),
                },
            ],
        );
        assert!(!msg.is_text_only());
        assert_eq!(msg.tool_use_blocks().len(), 1);
        assert_eq!(msg.text_content(), "response");
        assert_eq!(msg.content.len(), 4);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. Normalization
// ═══════════════════════════════════════════════════════════════════════

mod normalization {
    use super::*;

    #[test]
    fn whitespace_trimmed() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "  hello  "))
            .push(IrMessage::text(IrRole::Assistant, "\n answer \t"));
        let n = normalize(&conv);
        assert_eq!(n.messages[0].text_content(), "hello");
        assert_eq!(n.messages[1].text_content(), "answer");
    }

    #[test]
    fn duplicate_system_merged() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Be nice."))
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::System, "Be brief."));
        let n = normalize(&conv);
        assert_eq!(n.messages_by_role(IrRole::System).len(), 1);
        assert_eq!(
            n.system_message().unwrap().text_content(),
            "Be nice.\nBe brief."
        );
    }

    #[test]
    fn empty_messages_removed() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "keep"))
            .push(IrMessage::new(IrRole::Assistant, vec![]))
            .push(IrMessage::text(IrRole::Assistant, "also keep"));
        let n = normalize(&conv);
        assert_eq!(n.len(), 2);
    }

    #[test]
    fn idempotent() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "  sys  "))
            .push(IrMessage::text(IrRole::User, " hi "))
            .push(IrMessage::text(IrRole::System, " extra "));
        let once = normalize(&conv);
        let twice = normalize(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn tool_use_blocks_untouched_by_normalization() {
        let tool_block = IrContentBlock::ToolUse {
            id: "c1".into(),
            name: "search".into(),
            input: json!({"q": "  spaces  "}),
        };
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "  I will search  ".into(),
                },
                tool_block.clone(),
            ],
        ));
        let n = normalize(&conv);
        assert_eq!(
            n.messages[0].content[1], tool_block,
            "tool use inputs should not be trimmed"
        );
    }

    #[test]
    fn adjacent_text_blocks_merged() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text {
                    text: "Hello ".into(),
                },
                IrContentBlock::Text {
                    text: "world".into(),
                },
            ],
        ));
        let n = merge_adjacent_text(&conv);
        assert_eq!(n.messages[0].content.len(), 1);
        assert_eq!(n.messages[0].text_content(), "Hello world");
    }

    #[test]
    fn text_blocks_not_merged_across_tool_use() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "before".into(),
                },
                IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "t".into(),
                    input: json!({}),
                },
                IrContentBlock::Text {
                    text: "after".into(),
                },
            ],
        ));
        let n = merge_adjacent_text(&conv);
        assert_eq!(
            n.messages[0].content.len(),
            3,
            "non-adjacent text blocks should not merge"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Lowering to 6 dialects
// ═══════════════════════════════════════════════════════════════════════

mod lowering {
    use super::*;

    fn sample_conv() -> IrConversation {
        IrConversation::new()
            .push(IrMessage::text(IrRole::System, "You are helpful."))
            .push(IrMessage::text(IrRole::User, "Hi"))
            .push(IrMessage::text(IrRole::Assistant, "Hello!"))
    }

    fn sample_tools() -> Vec<IrToolDefinition> {
        vec![IrToolDefinition {
            name: "calc".into(),
            description: "Math evaluator".into(),
            parameters: json!({"type": "object", "properties": {"expr": {"type": "string"}}}),
        }]
    }

    #[test]
    fn lower_to_all_six_dialects() {
        let conv = sample_conv();
        let tools = sample_tools();
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &tools);
            // All dialects should produce valid JSON
            assert!(
                lowered.is_object(),
                "{dialect}: lowered output should be a JSON object"
            );
        }
    }

    #[test]
    fn openai_format_structure() {
        let lowered = lower_to_openai_like(&sample_conv(), &sample_tools());
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["role"], "system");
        assert!(!lowered["tools"].as_array().unwrap().is_empty());
    }

    #[test]
    fn claude_system_extraction() {
        let lowered = lower_to_claude(&sample_conv(), &sample_tools());
        assert_eq!(lowered["system"], "You are helpful.");
        let msgs = lowered["messages"].as_array().unwrap();
        // System message should not appear in messages
        assert!(msgs.iter().all(|m| m["role"] != "system"));
    }

    #[test]
    fn gemini_system_instruction() {
        let lowered = lower_to_gemini(&sample_conv(), &[]);
        assert_eq!(
            lowered["system_instruction"]["parts"][0]["text"],
            "You are helpful."
        );
        let contents = lowered["contents"].as_array().unwrap();
        assert_eq!(contents[1]["role"], "model");
    }

    #[test]
    fn claude_tool_schema_uses_input_schema() {
        let lowered = lower_to_claude(&sample_conv(), &sample_tools());
        let tool = &lowered["tools"][0];
        assert!(tool.get("input_schema").is_some());
        assert!(tool.get("parameters").is_none());
    }

    #[test]
    fn gemini_tool_uses_function_declarations() {
        let lowered = lower_to_gemini(&sample_conv(), &sample_tools());
        let decls = lowered["tools"][0]["function_declarations"]
            .as_array()
            .unwrap();
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0]["name"], "calc");
    }

    #[test]
    fn no_system_no_tools_omits_optional_fields() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &[]);
            assert!(
                lowered.get("tools").is_none(),
                "{dialect}: tools should be omitted when empty"
            );
        }
    }

    #[test]
    fn kimi_format_matches_openai() {
        let conv = sample_conv();
        let tools = sample_tools();
        let openai = lower_to_openai_like(&conv, &tools);
        let kimi = lower_to_kimi(&conv, &tools);
        assert_eq!(openai, kimi, "Kimi should use OpenAI-compatible format");
    }

    #[test]
    fn codex_format_matches_openai() {
        let conv = sample_conv();
        let tools = sample_tools();
        let openai = lower_to_openai_like(&conv, &tools);
        let codex = lower_to_codex(&conv, &tools);
        assert_eq!(openai, codex, "Codex should use OpenAI-compatible format");
    }

    #[test]
    fn copilot_format_matches_openai() {
        let conv = sample_conv();
        let tools = sample_tools();
        let openai = lower_to_openai_like(&conv, &tools);
        let copilot = lower_to_copilot(&conv, &tools);
        assert_eq!(
            openai, copilot,
            "Copilot should use OpenAI-compatible format"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Roundtrip integrity
// ═══════════════════════════════════════════════════════════════════════

mod roundtrip {
    use super::*;

    #[test]
    fn openai_roundtrip_preserves_semantics() {
        let orig = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "You are helpful."))
            .push(IrMessage::text(IrRole::User, "2+2?"))
            .push(IrMessage::text(IrRole::Assistant, "4"));
        let tools = vec![IrToolDefinition {
            name: "calc".into(),
            description: "Math".into(),
            parameters: json!({"type": "object"}),
        }];

        let lowered = lower_to_openai_like(&orig, &tools);
        let (restored, restored_tools) = lift_from_openai(&lowered);

        assert_eq!(orig.len(), restored.len());
        for (o, r) in orig.messages.iter().zip(restored.messages.iter()) {
            assert_eq!(o.role, r.role);
            assert_eq!(o.text_content(), r.text_content());
        }
        assert_eq!(tools.len(), restored_tools.len());
        assert_eq!(tools[0].name, restored_tools[0].name);
    }

    #[test]
    fn normalization_then_lowering_preserves_content() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "  sys  "))
            .push(IrMessage::text(IrRole::User, " hello "))
            .push(IrMessage::new(IrRole::Assistant, vec![]));

        let normalized = normalize(&conv);
        let lowered = lower_to_openai_like(&normalized, &[]);
        let (restored, _) = lift_from_openai(&lowered);

        assert_eq!(restored.len(), 2, "empty assistant should be removed");
        assert_eq!(restored.messages[0].text_content(), "sys");
        assert_eq!(restored.messages[1].text_content(), "hello");
    }

    #[test]
    fn tool_call_roundtrip_through_claude() {
        let orig = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "search for cats"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "Searching...".into(),
                    },
                    IrContentBlock::ToolUse {
                        id: "tu_1".into(),
                        name: "search".into(),
                        input: json!({"q": "cats"}),
                    },
                ],
            ));

        let lowered = lower_to_claude(&orig, &[]);
        let assistant_content = lowered["messages"][1]["content"].as_array().unwrap();
        assert_eq!(assistant_content[0]["type"], "text");
        assert_eq!(assistant_content[1]["type"], "tool_use");
        assert_eq!(assistant_content[1]["name"], "search");
        assert_eq!(assistant_content[1]["input"]["q"], "cats");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Tool definition normalization
// ═══════════════════════════════════════════════════════════════════════

mod tool_definitions {
    use super::*;

    #[test]
    fn sort_tools_deterministic() {
        let mut tools = vec![
            IrToolDefinition {
                name: "zebra".into(),
                description: "z".into(),
                parameters: json!({}),
            },
            IrToolDefinition {
                name: "alpha".into(),
                description: "a".into(),
                parameters: json!({}),
            },
            IrToolDefinition {
                name: "beta".into(),
                description: "b".into(),
                parameters: json!({}),
            },
        ];
        sort_tools(&mut tools);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta", "zebra"]);
    }

    #[test]
    fn tool_schema_preserved_across_dialects() {
        let tools = vec![IrToolDefinition {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }),
        }];

        let openai = lower_to_openai_like(&IrConversation::new(), &tools);
        let claude = lower_to_claude(&IrConversation::new(), &tools);
        let gemini = lower_to_gemini(&IrConversation::new(), &tools);

        // OpenAI: tools[0].function.parameters
        let openai_params = &openai["tools"][0]["function"]["parameters"];
        // Claude: tools[0].input_schema
        let claude_params = &claude["tools"][0]["input_schema"];
        // Gemini: tools[0].function_declarations[0].parameters
        let gemini_params = &gemini["tools"][0]["function_declarations"][0]["parameters"];

        assert_eq!(openai_params, claude_params);
        assert_eq!(claude_params, gemini_params);
    }

    #[test]
    fn tool_name_and_description_lowered_consistently() {
        let tools = vec![IrToolDefinition {
            name: "run_cmd".into(),
            description: "Execute a command".into(),
            parameters: json!({}),
        }];
        let conv = IrConversation::new();

        let openai = lower_to_openai_like(&conv, &tools);
        assert_eq!(openai["tools"][0]["function"]["name"], "run_cmd");
        assert_eq!(
            openai["tools"][0]["function"]["description"],
            "Execute a command"
        );

        let claude = lower_to_claude(&conv, &tools);
        assert_eq!(claude["tools"][0]["name"], "run_cmd");
        assert_eq!(claude["tools"][0]["description"], "Execute a command");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. System message handling
// ═══════════════════════════════════════════════════════════════════════

mod system_messages {
    use super::*;

    #[test]
    fn claude_extracts_system_to_top_level() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Be helpful."))
            .push(IrMessage::text(IrRole::User, "Hi"));
        let lowered = lower_to_claude(&conv, &[]);
        assert_eq!(lowered["system"], "Be helpful.");
        // Messages should not contain system
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn openai_keeps_system_in_messages() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Be helpful."))
            .push(IrMessage::text(IrRole::User, "Hi"));
        let lowered = lower_to_openai_like(&conv, &[]);
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "Be helpful.");
    }

    #[test]
    fn gemini_extracts_system_to_system_instruction() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Be concise."))
            .push(IrMessage::text(IrRole::User, "Tell me about Rust."));
        let lowered = lower_to_gemini(&conv, &[]);
        assert_eq!(
            lowered["system_instruction"]["parts"][0]["text"],
            "Be concise."
        );
        let contents = lowered["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
    }

    #[test]
    fn no_system_message_no_field() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));
        let claude = lower_to_claude(&conv, &[]);
        assert!(claude.get("system").is_none());
        let gemini = lower_to_gemini(&conv, &[]);
        assert!(gemini.get("system_instruction").is_none());
    }

    #[test]
    fn multiple_system_messages_merged_before_lowering() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Rule 1."))
            .push(IrMessage::text(IrRole::User, "Hi"))
            .push(IrMessage::text(IrRole::System, "Rule 2."));
        let normalized = dedup_system(&conv);
        let lowered = lower_to_claude(&normalized, &[]);
        assert_eq!(lowered["system"], "Rule 1.\nRule 2.");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Content merging
// ═══════════════════════════════════════════════════════════════════════

mod content_merging {
    use super::*;

    #[test]
    fn three_adjacent_text_blocks_merge_to_one() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text { text: "a".into() },
                IrContentBlock::Text { text: "b".into() },
                IrContentBlock::Text { text: "c".into() },
            ],
        ));
        let merged = merge_adjacent_text(&conv);
        assert_eq!(merged.messages[0].content.len(), 1);
        assert_eq!(merged.messages[0].text_content(), "abc");
    }

    #[test]
    fn non_text_blocks_prevent_merge() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text { text: "a".into() },
                IrContentBlock::Image {
                    media_type: "image/png".into(),
                    data: "x".into(),
                },
                IrContentBlock::Text { text: "b".into() },
            ],
        ));
        let merged = merge_adjacent_text(&conv);
        assert_eq!(merged.messages[0].content.len(), 3);
    }

    #[test]
    fn single_text_block_unchanged() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hello"));
        let merged = merge_adjacent_text(&conv);
        assert_eq!(merged.messages[0].content.len(), 1);
        assert_eq!(merged.messages[0].text_content(), "hello");
    }

    #[test]
    fn merge_preserves_tool_use_blocks() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "a".into(),
                    input: json!({}),
                },
                IrContentBlock::ToolUse {
                    id: "c2".into(),
                    name: "b".into(),
                    input: json!({}),
                },
            ],
        ));
        let merged = merge_adjacent_text(&conv);
        assert_eq!(merged.messages[0].content.len(), 2);
        assert_eq!(merged.messages[0].tool_use_blocks().len(), 2);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Serde roundtrip
// ═══════════════════════════════════════════════════════════════════════

mod serde_roundtrip {
    use super::*;

    #[test]
    fn ir_message_roundtrip() {
        let msg = IrMessage::text(IrRole::User, "hello");
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn ir_conversation_roundtrip() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "resp".into(),
                    },
                    IrContentBlock::ToolUse {
                        id: "t1".into(),
                        name: "f".into(),
                        input: json!({"a": 1}),
                    },
                ],
            ));
        let json = serde_json::to_string(&conv).unwrap();
        let back: IrConversation = serde_json::from_str(&json).unwrap();
        assert_eq!(conv, back);
    }

    #[test]
    fn ir_tool_definition_roundtrip() {
        let tool = IrToolDefinition {
            name: "read".into(),
            description: "Read file".into(),
            parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(tool, back);
    }

    #[test]
    fn ir_usage_roundtrip() {
        let usage = IrUsage::with_cache(100, 50, 20, 10);
        let json = serde_json::to_string(&usage).unwrap();
        let back: IrUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(usage, back);
        assert_eq!(back.total_tokens, 150);
    }

    #[test]
    fn ir_content_block_tags() {
        let blocks = [
            IrContentBlock::Text { text: "t".into() },
            IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "d".into(),
            },
            IrContentBlock::ToolUse {
                id: "i".into(),
                name: "n".into(),
                input: json!({}),
            },
            IrContentBlock::ToolResult {
                tool_use_id: "i".into(),
                content: vec![],
                is_error: false,
            },
            IrContentBlock::Thinking { text: "th".into() },
        ];
        let expected_tags = ["text", "image", "tool_use", "tool_result", "thinking"];
        for (block, expected_tag) in blocks.iter().zip(expected_tags.iter()) {
            let val: serde_json::Value = serde_json::to_value(block).unwrap();
            assert_eq!(val["type"], *expected_tag);
            // Roundtrip
            let back: IrContentBlock = serde_json::from_value(val).unwrap();
            assert_eq!(*block, back);
        }
    }

    #[test]
    fn ir_role_serde() {
        let roles = [
            IrRole::System,
            IrRole::User,
            IrRole::Assistant,
            IrRole::Tool,
        ];
        let expected = ["\"system\"", "\"user\"", "\"assistant\"", "\"tool\""];
        for (role, exp) in roles.iter().zip(expected.iter()) {
            let json = serde_json::to_string(role).unwrap();
            assert_eq!(json, *exp);
            let back: IrRole = serde_json::from_str(&json).unwrap();
            assert_eq!(*role, back);
        }
    }

    #[test]
    fn ir_usage_merge() {
        let a = IrUsage::from_io(100, 50);
        let b = IrUsage::with_cache(200, 100, 30, 10);
        let merged = a.merge(b);
        assert_eq!(merged.input_tokens, 300);
        assert_eq!(merged.output_tokens, 150);
        assert_eq!(merged.total_tokens, 450);
        assert_eq!(merged.cache_read_tokens, 30);
        assert_eq!(merged.cache_write_tokens, 10);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Edge cases
// ═══════════════════════════════════════════════════════════════════════

mod edge_cases {
    use super::*;

    #[test]
    fn empty_conversation() {
        let conv = IrConversation::new();
        assert!(conv.is_empty());
        assert_eq!(conv.len(), 0);
        assert!(conv.system_message().is_none());
        assert!(conv.last_assistant().is_none());
        assert!(conv.last_message().is_none());
        assert!(conv.tool_calls().is_empty());
    }

    #[test]
    fn empty_message_content() {
        let msg = IrMessage::new(IrRole::User, vec![]);
        assert!(msg.is_text_only());
        assert!(msg.text_content().is_empty());
        assert!(msg.tool_use_blocks().is_empty());
    }

    #[test]
    fn empty_text_block() {
        let msg = IrMessage::text(IrRole::User, "");
        assert_eq!(msg.text_content(), "");
        assert!(msg.is_text_only());
    }

    #[test]
    fn unicode_content() {
        let msg = IrMessage::text(IrRole::User, "日本語テスト 🦀 émojis café");
        assert_eq!(msg.text_content(), "日本語テスト 🦀 émojis café");
        // Serde roundtrip with unicode
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn unicode_in_tool_input() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "search".into(),
                input: json!({"query": "日本語 🦀"}),
            }],
        );
        let json = serde_json::to_string(&msg).unwrap();
        let back: IrMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, back);
    }

    #[test]
    fn very_long_text() {
        let long_text = "x".repeat(100_000);
        let msg = IrMessage::text(IrRole::User, &long_text);
        assert_eq!(msg.text_content().len(), 100_000);
    }

    #[test]
    fn conversation_with_only_tool_messages() {
        let conv = IrConversation::new()
            .push(IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "c1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "result".into(),
                    }],
                    is_error: false,
                }],
            ))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "c2".into(),
                    content: vec![],
                    is_error: true,
                }],
            ));
        assert!(conv.system_message().is_none());
        assert!(conv.last_assistant().is_none());
        assert_eq!(conv.messages_by_role(IrRole::Tool).len(), 2);
    }

    #[test]
    fn normalize_preserves_non_system_ordering() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "1"))
            .push(IrMessage::text(IrRole::Assistant, "2"))
            .push(IrMessage::text(IrRole::User, "3"))
            .push(IrMessage::text(IrRole::Assistant, "4"));
        let n = normalize(&conv);
        let texts: Vec<String> = n.messages.iter().map(|m| m.text_content()).collect();
        assert_eq!(texts, vec!["1", "2", "3", "4"]);
    }

    #[test]
    fn normalize_empty_stays_empty() {
        let conv = IrConversation::new();
        let n = normalize(&conv);
        assert!(n.is_empty());
    }

    #[test]
    fn default_conversation_is_empty() {
        let conv = IrConversation::default();
        assert!(conv.is_empty());
        assert_eq!(conv.len(), 0);
    }

    #[test]
    fn tool_result_with_nested_empty_content() {
        let msg = IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![],
                is_error: false,
            }],
        );
        assert!(!msg.is_text_only());
        assert!(msg.text_content().is_empty());
    }

    #[test]
    fn special_characters_in_tool_name() {
        let tool = IrToolDefinition {
            name: "my-tool_v2.0".into(),
            description: "A tool with special chars: <>&\"'".into(),
            parameters: json!({}),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let back: IrToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(tool, back);
    }

    #[test]
    fn lowering_empty_conversation_for_all_dialects() {
        let conv = IrConversation::new();
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &[]);
            assert!(
                lowered.is_object(),
                "{dialect}: should produce valid JSON object"
            );
        }
    }
}
