// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for the IR transformation pipeline.
//!
//! Covers IR construction from all SDK dialects, normalization passes,
//! SDK → IR lifting, IR → SDK lowering (with lossy marking), round-trip
//! semantics, cross-dialect translation, content blocks, role mapping,
//! tool definitions, parameter preservation, model mapping, and metadata
//! handling.

use abp_ir::lower::*;
use abp_ir::normalize::*;
use abp_ir::*;
use abp_sdk_types::Dialect;
use serde_json::json;
use std::collections::BTreeMap;

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

fn tool(name: &str, desc: &str) -> IrToolDefinition {
    IrToolDefinition {
        name: name.into(),
        description: desc.into(),
        parameters: json!({"type": "object", "properties": {}}),
    }
}

fn tool_with_params(name: &str, desc: &str, params: serde_json::Value) -> IrToolDefinition {
    IrToolDefinition {
        name: name.into(),
        description: desc.into(),
        parameters: params,
    }
}

fn msg_with_meta(role: IrRole, text: &str, meta: BTreeMap<String, serde_json::Value>) -> IrMessage {
    IrMessage {
        role,
        content: vec![IrContentBlock::Text { text: text.into() }],
        metadata: meta,
    }
}

/// Lift an OpenAI-style JSON request into an IR conversation + tools.
fn lift_openai_json(request: &serde_json::Value) -> (IrConversation, Vec<IrToolDefinition>) {
    let mut conv = IrConversation::new();
    if let Some(msgs) = request["messages"].as_array() {
        for m in msgs {
            let role = match m["role"].as_str().unwrap_or("user") {
                "system" => IrRole::System,
                "user" => IrRole::User,
                "assistant" => IrRole::Assistant,
                "tool" => IrRole::Tool,
                _ => IrRole::User,
            };

            let mut blocks = Vec::new();

            // Text content
            if let Some(text) = m["content"].as_str() {
                blocks.push(IrContentBlock::Text {
                    text: text.to_string(),
                });
            }

            // Tool calls
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

            // Tool result
            if role == IrRole::Tool {
                if let Some(tool_call_id) = m["tool_call_id"].as_str() {
                    let text = m["content"].as_str().unwrap_or("");
                    blocks = vec![IrContentBlock::ToolResult {
                        tool_use_id: tool_call_id.to_string(),
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

    let tools = request["tools"]
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

/// Lift a Claude-style JSON request into an IR conversation + tools.
fn lift_claude_json(request: &serde_json::Value) -> (IrConversation, Vec<IrToolDefinition>) {
    let mut conv = IrConversation::new();

    // System is a top-level field in Claude
    if let Some(sys) = request["system"].as_str() {
        conv = conv.push(IrMessage::text(IrRole::System, sys));
    }

    if let Some(msgs) = request["messages"].as_array() {
        for m in msgs {
            let role = match m["role"].as_str().unwrap_or("user") {
                "user" => IrRole::User,
                "assistant" => IrRole::Assistant,
                _ => IrRole::User,
            };

            let mut blocks = Vec::new();
            if let Some(content) = m["content"].as_array() {
                for block in content {
                    match block["type"].as_str().unwrap_or("text") {
                        "text" => {
                            blocks.push(IrContentBlock::Text {
                                text: block["text"].as_str().unwrap_or("").to_string(),
                            });
                        }
                        "tool_use" => {
                            blocks.push(IrContentBlock::ToolUse {
                                id: block["id"].as_str().unwrap_or("").to_string(),
                                name: block["name"].as_str().unwrap_or("").to_string(),
                                input: block["input"].clone(),
                            });
                        }
                        "tool_result" => {
                            let inner: Vec<IrContentBlock> = block["content"]
                                .as_array()
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|c| {
                                            c["text"].as_str().map(|t| IrContentBlock::Text {
                                                text: t.to_string(),
                                            })
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            blocks.push(IrContentBlock::ToolResult {
                                tool_use_id: block["tool_use_id"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string(),
                                content: inner,
                                is_error: block["is_error"].as_bool().unwrap_or(false),
                            });
                        }
                        "thinking" => {
                            blocks.push(IrContentBlock::Thinking {
                                text: block["thinking"].as_str().unwrap_or("").to_string(),
                            });
                        }
                        "image" => {
                            blocks.push(IrContentBlock::Image {
                                media_type: block["source"]["media_type"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string(),
                                data: block["source"]["data"].as_str().unwrap_or("").to_string(),
                            });
                        }
                        _ => {}
                    }
                }
            } else if let Some(text) = m["content"].as_str() {
                blocks.push(IrContentBlock::Text {
                    text: text.to_string(),
                });
            }

            conv = conv.push(IrMessage::new(role, blocks));
        }
    }

    let tools = request["tools"]
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

/// Lift a Gemini-style JSON request into an IR conversation + tools.
fn lift_gemini_json(request: &serde_json::Value) -> (IrConversation, Vec<IrToolDefinition>) {
    let mut conv = IrConversation::new();

    if let Some(sys) = request.get("system_instruction") {
        let text: String = sys["parts"]
            .as_array()
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        if !text.is_empty() {
            conv = conv.push(IrMessage::text(IrRole::System, &text));
        }
    }

    if let Some(contents) = request["contents"].as_array() {
        for c in contents {
            let role = match c["role"].as_str().unwrap_or("user") {
                "model" => IrRole::Assistant,
                _ => IrRole::User,
            };
            let mut blocks = Vec::new();
            if let Some(parts) = c["parts"].as_array() {
                for p in parts {
                    if let Some(text) = p["text"].as_str() {
                        blocks.push(IrContentBlock::Text {
                            text: text.to_string(),
                        });
                    } else if p.get("functionCall").is_some() {
                        blocks.push(IrContentBlock::ToolUse {
                            id: String::new(),
                            name: p["functionCall"]["name"].as_str().unwrap_or("").to_string(),
                            input: p["functionCall"]["args"].clone(),
                        });
                    } else if p.get("functionResponse").is_some() {
                        let result_text = p["functionResponse"]["response"]["result"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();
                        blocks.push(IrContentBlock::ToolResult {
                            tool_use_id: p["functionResponse"]["name"]
                                .as_str()
                                .unwrap_or("")
                                .to_string(),
                            content: vec![IrContentBlock::Text { text: result_text }],
                            is_error: false,
                        });
                    }
                }
            }
            conv = conv.push(IrMessage::new(role, blocks));
        }
    }

    let tools = request["tools"]
        .as_array()
        .and_then(|arr| arr.first())
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
// 1. IR Construction — build IR nodes from all SDK dialects
// ═══════════════════════════════════════════════════════════════════════

mod ir_construction {
    use super::*;

    #[test]
    fn construct_from_openai_text_messages() {
        let req = json!({
            "messages": [
                {"role": "system", "content": "Be helpful"},
                {"role": "user", "content": "Hi"},
                {"role": "assistant", "content": "Hello!"}
            ]
        });
        let (conv, _) = lift_openai_json(&req);
        assert_eq!(conv.len(), 3);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[1].role, IrRole::User);
        assert_eq!(conv.messages[2].role, IrRole::Assistant);
    }

    #[test]
    fn construct_from_openai_with_tool_calls() {
        let req = json!({
            "messages": [
                {"role": "user", "content": "What is 2+2?"},
                {
                    "role": "assistant",
                    "content": "Let me calculate.",
                    "tool_calls": [{"id": "tc_1", "function": {"name": "calc", "arguments": "{\"expr\":\"2+2\"}"}}]
                },
                {"role": "tool", "tool_call_id": "tc_1", "content": "4"}
            ]
        });
        let (conv, _) = lift_openai_json(&req);
        assert_eq!(conv.len(), 3);

        let assistant = &conv.messages[1];
        assert_eq!(assistant.role, IrRole::Assistant);
        assert!(
            assistant
                .content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolUse { name, .. } if name == "calc"))
        );

        let tool_msg = &conv.messages[2];
        assert_eq!(tool_msg.role, IrRole::Tool);
        assert!(tool_msg.content.iter().any(
            |b| matches!(b, IrContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "tc_1")
        ));
    }

    #[test]
    fn construct_from_openai_with_tools() {
        let req = json!({
            "messages": [{"role": "user", "content": "Hi"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "search",
                    "description": "Search the web",
                    "parameters": {"type": "object", "properties": {"q": {"type": "string"}}}
                }
            }]
        });
        let (_, tools) = lift_openai_json(&req);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "search");
        assert_eq!(tools[0].description, "Search the web");
    }

    #[test]
    fn construct_from_claude_format() {
        let req = json!({
            "system": "Be concise.",
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "Hello"}]},
                {"role": "assistant", "content": [{"type": "text", "text": "Hi!"}]}
            ],
            "tools": [{
                "name": "read_file",
                "description": "Read a file",
                "input_schema": {"type": "object", "properties": {"path": {"type": "string"}}}
            }]
        });
        let (conv, tools) = lift_claude_json(&req);
        assert_eq!(conv.len(), 3); // system + user + assistant
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "Be concise.");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "read_file");
    }

    #[test]
    fn construct_from_claude_with_tool_use() {
        let req = json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "text", "text": "Checking..."},
                    {"type": "tool_use", "id": "tu_1", "name": "search", "input": {"q": "rust"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "tu_1", "content": [{"type": "text", "text": "found it"}], "is_error": false}
                ]}
            ]
        });
        let (conv, _) = lift_claude_json(&req);
        assert_eq!(conv.len(), 2);
        assert!(
            conv.messages[0]
                .content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolUse { .. }))
        );
        assert!(
            conv.messages[1]
                .content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolResult { .. }))
        );
    }

    #[test]
    fn construct_from_gemini_format() {
        let req = json!({
            "system_instruction": {"parts": [{"text": "You are helpful."}]},
            "contents": [
                {"role": "user", "parts": [{"text": "Hello!"}]},
                {"role": "model", "parts": [{"text": "Hi there!"}]}
            ]
        });
        let (conv, _) = lift_gemini_json(&req);
        assert_eq!(conv.len(), 3);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[2].role, IrRole::Assistant);
    }

    #[test]
    fn construct_from_gemini_with_function_call() {
        let req = json!({
            "contents": [
                {"role": "model", "parts": [{"functionCall": {"name": "calc", "args": {"x": 1}}}]},
                {"role": "user", "parts": [{"functionResponse": {"name": "calc", "response": {"result": "42"}}}]}
            ],
            "tools": [{"function_declarations": [{"name": "calc", "description": "Calculator", "parameters": {"type": "object"}}]}]
        });
        let (conv, tools) = lift_gemini_json(&req);
        assert_eq!(conv.len(), 2);
        assert!(
            conv.messages[0]
                .content
                .iter()
                .any(|b| matches!(b, IrContentBlock::ToolUse { name, .. } if name == "calc"))
        );
        assert_eq!(tools.len(), 1);
    }

    #[test]
    fn construct_empty_conversation() {
        let conv = IrConversation::new();
        assert!(conv.is_empty());
        assert_eq!(conv.len(), 0);
        assert!(conv.system_message().is_none());
        assert!(conv.last_assistant().is_none());
    }

    #[test]
    fn construct_conversation_with_all_roles() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "usr"))
            .push(IrMessage::text(IrRole::Assistant, "asst"))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "result".into(),
                    }],
                    is_error: false,
                }],
            ));
        assert_eq!(conv.len(), 4);
        assert_eq!(conv.messages_by_role(IrRole::System).len(), 1);
        assert_eq!(conv.messages_by_role(IrRole::User).len(), 1);
        assert_eq!(conv.messages_by_role(IrRole::Assistant).len(), 1);
        assert_eq!(conv.messages_by_role(IrRole::Tool).len(), 1);
    }

    #[test]
    fn construct_conversation_from_messages_vec() {
        let msgs = vec![
            IrMessage::text(IrRole::User, "a"),
            IrMessage::text(IrRole::Assistant, "b"),
        ];
        let conv = IrConversation::from_messages(msgs);
        assert_eq!(conv.len(), 2);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 2. Normalization passes
// ═══════════════════════════════════════════════════════════════════════

mod normalization {
    use super::*;

    #[test]
    fn dedup_system_merges_multiple_system_messages() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Rule 1"))
            .push(IrMessage::text(IrRole::User, "Hello"))
            .push(IrMessage::text(IrRole::System, "Rule 2"));
        let result = dedup_system(&conv);
        let sys = result.messages_by_role(IrRole::System);
        assert_eq!(sys.len(), 1);
        assert_eq!(sys[0].text_content(), "Rule 1\nRule 2");
    }

    #[test]
    fn dedup_system_preserves_single_system() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Only one"))
            .push(IrMessage::text(IrRole::User, "hi"));
        let result = dedup_system(&conv);
        assert_eq!(result.messages_by_role(IrRole::System).len(), 1);
        assert_eq!(result.messages[0].text_content(), "Only one");
    }

    #[test]
    fn dedup_system_no_system_leaves_unchanged() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::Assistant, "hello"));
        let result = dedup_system(&conv);
        assert_eq!(result.len(), 2);
        assert!(result.messages_by_role(IrRole::System).is_empty());
    }

    #[test]
    fn trim_text_strips_whitespace() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "  hello  "))
            .push(IrMessage::text(IrRole::Assistant, "\t world \n"));
        let result = trim_text(&conv);
        assert_eq!(result.messages[0].text_content(), "hello");
        assert_eq!(result.messages[1].text_content(), "world");
    }

    #[test]
    fn trim_text_ignores_non_text_blocks() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "  hi  ".into(),
                },
                IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "search".into(),
                    input: json!({}),
                },
            ],
        ));
        let result = trim_text(&conv);
        assert_eq!(result.messages[0].content.len(), 2);
        match &result.messages[0].content[0] {
            IrContentBlock::Text { text } => assert_eq!(text, "hi"),
            _ => panic!("expected text"),
        }
        assert!(matches!(
            &result.messages[0].content[1],
            IrContentBlock::ToolUse { .. }
        ));
    }

    #[test]
    fn strip_empty_removes_no_content_messages() {
        let conv = IrConversation::new()
            .push(IrMessage::new(IrRole::User, vec![]))
            .push(IrMessage::text(IrRole::Assistant, "hello"))
            .push(IrMessage::new(IrRole::User, vec![]));
        let result = strip_empty(&conv);
        assert_eq!(result.len(), 1);
        assert_eq!(result.messages[0].text_content(), "hello");
    }

    #[test]
    fn merge_adjacent_text_coalesces_consecutive_texts() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text {
                    text: "Hello ".into(),
                },
                IrContentBlock::Text {
                    text: "world!".into(),
                },
            ],
        ));
        let result = merge_adjacent_text(&conv);
        assert_eq!(result.messages[0].content.len(), 1);
        assert_eq!(result.messages[0].text_content(), "Hello world!");
    }

    #[test]
    fn merge_adjacent_text_preserves_non_text_boundaries() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text { text: "A".into() },
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "f".into(),
                    input: json!({}),
                },
                IrContentBlock::Text { text: "B".into() },
            ],
        ));
        let result = merge_adjacent_text(&conv);
        assert_eq!(result.messages[0].content.len(), 3);
    }

    #[test]
    fn full_normalize_pipeline_applies_all_passes() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "  A  "))
            .push(IrMessage::new(IrRole::User, vec![])) // empty, stripped
            .push(IrMessage::new(
                IrRole::User,
                vec![
                    IrContentBlock::Text {
                        text: " hi ".into(),
                    },
                    IrContentBlock::Text {
                        text: " there ".into(),
                    },
                ],
            ))
            .push(IrMessage::text(IrRole::System, "  B  "));

        let result = normalize(&conv);
        // System messages merged (dedup joins raw, then trim only outer ws)
        assert_eq!(result.messages_by_role(IrRole::System).len(), 1);
        // dedup_system joins "  A  " + "  B  " → "  A  \n  B  ", then trim → "A  \n  B"
        assert!(
            result.messages_by_role(IrRole::System)[0]
                .text_content()
                .starts_with("A")
        );
        // Empty user message stripped, remaining user has merged trimmed text
        let user_msgs = result.messages_by_role(IrRole::User);
        assert_eq!(user_msgs.len(), 1);
        assert_eq!(user_msgs[0].text_content(), "hithere");
    }

    #[test]
    fn normalize_is_idempotent() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "  sys  "))
            .push(IrMessage::text(IrRole::User, "  hi  "))
            .push(IrMessage::text(IrRole::System, "  extra  "));
        let once = normalize(&conv);
        let twice = normalize(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn normalize_empty_conversation() {
        let conv = IrConversation::new();
        let result = normalize(&conv);
        assert!(result.is_empty());
    }

    #[test]
    fn extract_system_separates_system_from_rest() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Be nice."))
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::System, "Be brief."));
        let (sys, rest) = extract_system(&conv);
        assert_eq!(sys.unwrap(), "Be nice.\nBe brief.");
        assert!(rest.messages.iter().all(|m| m.role != IrRole::System));
        assert_eq!(rest.len(), 1);
    }

    #[test]
    fn extract_system_returns_none_when_absent() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let (sys, _) = extract_system(&conv);
        assert!(sys.is_none());
    }

    #[test]
    fn strip_metadata_removes_all_keys_when_empty_keep() {
        let mut meta = BTreeMap::new();
        meta.insert("vendor_id".into(), json!("abc"));
        meta.insert("source".into(), json!("test"));
        let msg = msg_with_meta(IrRole::User, "hi", meta);
        let conv = IrConversation::from_messages(vec![msg]);
        let stripped = strip_metadata(&conv, &[]);
        assert!(stripped.messages[0].metadata.is_empty());
    }

    #[test]
    fn strip_metadata_keeps_specified_keys() {
        let mut meta = BTreeMap::new();
        meta.insert("source".into(), json!("test"));
        meta.insert("vendor_id".into(), json!("x"));
        let msg = msg_with_meta(IrRole::User, "hi", meta);
        let conv = IrConversation::from_messages(vec![msg]);
        let stripped = strip_metadata(&conv, &["source"]);
        assert_eq!(stripped.messages[0].metadata.len(), 1);
        assert!(stripped.messages[0].metadata.contains_key("source"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. SDK → IR (lifting)
// ═══════════════════════════════════════════════════════════════════════

mod sdk_to_ir {
    use super::*;

    #[test]
    fn openai_simple_conversation_to_ir() {
        let req = json!({
            "messages": [
                {"role": "system", "content": "You help with code."},
                {"role": "user", "content": "Write hello world in Rust"},
                {"role": "assistant", "content": "fn main() { println!(\"Hello!\"); }"}
            ]
        });
        let (conv, _) = lift_openai_json(&req);
        assert_eq!(conv.len(), 3);
        assert_eq!(
            conv.system_message().unwrap().text_content(),
            "You help with code."
        );
    }

    #[test]
    fn claude_conversation_to_ir() {
        let req = json!({
            "system": "Code helper",
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "Hello"}]},
                {"role": "assistant", "content": [{"type": "text", "text": "Hi"}]}
            ]
        });
        let (conv, _) = lift_claude_json(&req);
        assert_eq!(conv.len(), 3);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "Code helper");
    }

    #[test]
    fn gemini_conversation_to_ir() {
        let req = json!({
            "system_instruction": {"parts": [{"text": "Helper"}]},
            "contents": [
                {"role": "user", "parts": [{"text": "Hi"}]},
                {"role": "model", "parts": [{"text": "Hello!"}]}
            ]
        });
        let (conv, _) = lift_gemini_json(&req);
        assert_eq!(conv.len(), 3);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[2].role, IrRole::Assistant);
    }

    #[test]
    fn openai_tool_call_round_trip_to_ir() {
        let req = json!({
            "messages": [
                {"role": "user", "content": "Search for rust"},
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "function": {"name": "search", "arguments": "{\"q\":\"rust\"}"}
                    }]
                },
                {"role": "tool", "tool_call_id": "call_1", "content": "Rust is a systems language."}
            ]
        });
        let (conv, _) = lift_openai_json(&req);
        assert_eq!(conv.len(), 3);
        let tool_blocks = conv.tool_calls();
        assert_eq!(tool_blocks.len(), 1);
    }

    #[test]
    fn claude_with_thinking_block_to_ir() {
        let req = json!({
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "Let me think..."},
                    {"type": "text", "text": "The answer is 42."}
                ]
            }]
        });
        let (conv, _) = lift_claude_json(&req);
        assert!(
            conv.messages[0]
                .content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
        );
    }

    #[test]
    fn claude_with_image_to_ir() {
        let req = json!({
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBOR..."}}
                ]
            }]
        });
        let (conv, _) = lift_claude_json(&req);
        assert!(conv.messages[0].content.iter().any(
            |b| matches!(b, IrContentBlock::Image { media_type, .. } if media_type == "image/png")
        ));
    }

    #[test]
    fn gemini_function_response_to_ir() {
        let req = json!({
            "contents": [{
                "role": "user",
                "parts": [{"functionResponse": {"name": "calc", "response": {"result": "42"}}}]
            }]
        });
        let (conv, _) = lift_gemini_json(&req);
        assert!(conv.messages[0].content.iter().any(
            |b| matches!(b, IrContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "calc")
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. IR → SDK (lowering)
// ═══════════════════════════════════════════════════════════════════════

mod ir_to_sdk {
    use super::*;

    fn basic_conv() -> IrConversation {
        IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Be helpful"))
            .push(IrMessage::text(IrRole::User, "Hi"))
            .push(IrMessage::text(IrRole::Assistant, "Hello!"))
    }

    #[test]
    fn lower_to_openai_produces_messages_array() {
        let lowered = lower_to_openai(&basic_conv(), &[]);
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[2]["role"], "assistant");
    }

    #[test]
    fn lower_to_claude_extracts_system_field() {
        let lowered = lower_to_claude(&basic_conv(), &[]);
        assert_eq!(lowered["system"], "Be helpful");
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2); // system extracted
        assert!(msgs.iter().all(|m| m["role"] != "system"));
    }

    #[test]
    fn lower_to_gemini_uses_system_instruction() {
        let lowered = lower_to_gemini(&basic_conv(), &[]);
        assert_eq!(
            lowered["system_instruction"]["parts"][0]["text"],
            "Be helpful"
        );
        let contents = lowered["contents"].as_array().unwrap();
        assert_eq!(contents[1]["role"], "model");
    }

    #[test]
    fn lower_to_openai_with_tools() {
        let tools = vec![tool("calc", "Calculator")];
        let lowered = lower_to_openai(&basic_conv(), &tools);
        let t = lowered["tools"].as_array().unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0]["type"], "function");
        assert_eq!(t[0]["function"]["name"], "calc");
    }

    #[test]
    fn lower_to_claude_with_tools_uses_input_schema() {
        let tools = vec![tool_with_params(
            "search",
            "Search",
            json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        )];
        let lowered = lower_to_claude(&basic_conv(), &tools);
        assert!(lowered["tools"][0].get("input_schema").is_some());
        assert!(lowered["tools"][0].get("parameters").is_none());
    }

    #[test]
    fn lower_to_gemini_with_tools_uses_function_declarations() {
        let tools = vec![tool("read", "Read file")];
        let lowered = lower_to_gemini(&basic_conv(), &tools);
        let decls = lowered["tools"][0]["function_declarations"]
            .as_array()
            .unwrap();
        assert_eq!(decls[0]["name"], "read");
    }

    #[test]
    fn lower_thinking_block_to_claude_preserves() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking { text: "hmm".into() },
                IrContentBlock::Text {
                    text: "answer".into(),
                },
            ],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        let content = lowered["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "thinking");
        assert_eq!(content[1]["type"], "text");
    }

    #[test]
    fn lower_thinking_block_to_gemini_dropped() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking { text: "hmm".into() },
                IrContentBlock::Text {
                    text: "answer".into(),
                },
            ],
        ));
        let lowered = lower_to_gemini(&conv, &[]);
        let parts = lowered["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], "answer");
    }

    #[test]
    fn lower_image_block_to_claude() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "abc123".into(),
            }],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        let block = &lowered["messages"][0]["content"][0];
        assert_eq!(block["type"], "image");
        assert_eq!(block["source"]["media_type"], "image/png");
    }

    #[test]
    fn lower_image_block_to_gemini() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/jpeg".into(),
                data: "xyz789".into(),
            }],
        ));
        let lowered = lower_to_gemini(&conv, &[]);
        let part = &lowered["contents"][0]["parts"][0];
        assert_eq!(part["inline_data"]["mime_type"], "image/jpeg");
        assert_eq!(part["inline_data"]["data"], "xyz789");
    }

    #[test]
    fn lower_no_tools_omits_tools_field() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &[]);
            assert!(
                lowered.get("tools").is_none(),
                "{dialect}: should omit tools when empty"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. Round-trip: SDK → IR → same SDK preserves semantics
// ═══════════════════════════════════════════════════════════════════════

mod round_trip {
    use super::*;

    #[test]
    fn openai_text_round_trip() {
        let original = json!({
            "messages": [
                {"role": "system", "content": "Be helpful."},
                {"role": "user", "content": "Hi"},
                {"role": "assistant", "content": "Hello!"}
            ]
        });
        let (conv, tools) = lift_openai_json(&original);
        let lowered = lower_to_openai(&conv, &tools);

        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "Be helpful.");
        assert_eq!(msgs[1]["content"], "Hi");
        assert_eq!(msgs[2]["content"], "Hello!");
    }

    #[test]
    fn claude_text_round_trip() {
        let original = json!({
            "system": "Be concise.",
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "Hello"}]},
                {"role": "assistant", "content": [{"type": "text", "text": "Hi!"}]}
            ]
        });
        let (conv, tools) = lift_claude_json(&original);
        let lowered = lower_to_claude(&conv, &tools);

        assert_eq!(lowered["system"], "Be concise.");
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["content"][0]["text"], "Hello");
        assert_eq!(msgs[1]["content"][0]["text"], "Hi!");
    }

    #[test]
    fn gemini_text_round_trip() {
        let original = json!({
            "system_instruction": {"parts": [{"text": "Helper"}]},
            "contents": [
                {"role": "user", "parts": [{"text": "Hi"}]},
                {"role": "model", "parts": [{"text": "Hello"}]}
            ]
        });
        let (conv, tools) = lift_gemini_json(&original);
        let lowered = lower_to_gemini(&conv, &tools);

        assert_eq!(lowered["system_instruction"]["parts"][0]["text"], "Helper");
        let contents = lowered["contents"].as_array().unwrap();
        assert_eq!(contents[0]["parts"][0]["text"], "Hi");
        assert_eq!(contents[1]["parts"][0]["text"], "Hello");
        assert_eq!(contents[1]["role"], "model");
    }

    #[test]
    fn openai_tool_call_round_trip() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "calc 2+2"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "calc".into(),
                    input: json!({"expr": "2+2"}),
                }],
            ))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "call_1".into(),
                    content: vec![IrContentBlock::Text { text: "4".into() }],
                    is_error: false,
                }],
            ));

        let lowered = lower_to_openai(&conv, &[]);
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs[1]["tool_calls"][0]["function"]["name"], "calc");
        assert_eq!(msgs[2]["role"], "tool");
        assert_eq!(msgs[2]["tool_call_id"], "call_1");
        assert_eq!(msgs[2]["content"], "4");
    }

    #[test]
    fn claude_tool_use_round_trip() {
        let conv = IrConversation::new()
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "Searching...".into(),
                    },
                    IrContentBlock::ToolUse {
                        id: "tu_1".into(),
                        name: "search".into(),
                        input: json!({"q": "rust"}),
                    },
                ],
            ))
            .push(IrMessage::new(
                IrRole::User,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "tu_1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "found it".into(),
                    }],
                    is_error: false,
                }],
            ));

        let lowered = lower_to_claude(&conv, &[]);
        let asst_content = lowered["messages"][0]["content"].as_array().unwrap();
        assert_eq!(asst_content[0]["type"], "text");
        assert_eq!(asst_content[1]["type"], "tool_use");
        assert_eq!(asst_content[1]["name"], "search");

        let user_content = lowered["messages"][1]["content"].as_array().unwrap();
        assert_eq!(user_content[0]["type"], "tool_result");
    }

    #[test]
    fn normalize_then_lower_is_consistent() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "  A  "))
            .push(IrMessage::text(IrRole::User, "  hi  "))
            .push(IrMessage::text(IrRole::System, "  B  "));

        let normed = normalize(&conv);
        let lowered = lower_to_openai(&normed, &[]);
        let msgs = lowered["messages"].as_array().unwrap();
        // dedup joins raw text then trims outer ws only
        assert!(msgs[0]["content"].as_str().unwrap().starts_with("A"));
        assert!(msgs[0]["content"].as_str().unwrap().contains("B"));
        assert_eq!(msgs[1]["content"], "hi");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. Cross-dialect: SDK A → IR → SDK B
// ═══════════════════════════════════════════════════════════════════════

mod cross_dialect {
    use super::*;

    #[test]
    fn openai_to_claude() {
        let req = json!({
            "messages": [
                {"role": "system", "content": "Helper"},
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi!"}
            ]
        });
        let (conv, tools) = lift_openai_json(&req);
        let lowered = lower_to_claude(&conv, &tools);

        assert_eq!(lowered["system"], "Helper");
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2); // system extracted
        assert_eq!(msgs[0]["content"][0]["text"], "Hello");
    }

    #[test]
    fn openai_to_gemini() {
        let req = json!({
            "messages": [
                {"role": "system", "content": "Helper"},
                {"role": "user", "content": "Hi"},
                {"role": "assistant", "content": "Hello!"}
            ]
        });
        let (conv, tools) = lift_openai_json(&req);
        let lowered = lower_to_gemini(&conv, &tools);

        assert_eq!(lowered["system_instruction"]["parts"][0]["text"], "Helper");
        let contents = lowered["contents"].as_array().unwrap();
        assert_eq!(contents[1]["role"], "model");
    }

    #[test]
    fn claude_to_openai() {
        let req = json!({
            "system": "Concise helper",
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "Hello"}]},
                {"role": "assistant", "content": [{"type": "text", "text": "Hi!"}]}
            ]
        });
        let (conv, tools) = lift_claude_json(&req);
        let lowered = lower_to_openai(&conv, &tools);

        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "Concise helper");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[2]["role"], "assistant");
    }

    #[test]
    fn claude_to_gemini() {
        let req = json!({
            "system": "Helper",
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "Hi"}]},
                {"role": "assistant", "content": [{"type": "text", "text": "Hello"}]}
            ]
        });
        let (conv, tools) = lift_claude_json(&req);
        let lowered = lower_to_gemini(&conv, &tools);

        assert_eq!(lowered["system_instruction"]["parts"][0]["text"], "Helper");
        let contents = lowered["contents"].as_array().unwrap();
        assert_eq!(contents[1]["role"], "model");
    }

    #[test]
    fn gemini_to_openai() {
        let req = json!({
            "system_instruction": {"parts": [{"text": "Helper"}]},
            "contents": [
                {"role": "user", "parts": [{"text": "Hi"}]},
                {"role": "model", "parts": [{"text": "Hello"}]}
            ]
        });
        let (conv, tools) = lift_gemini_json(&req);
        let lowered = lower_to_openai(&conv, &tools);

        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[2]["role"], "assistant");
    }

    #[test]
    fn gemini_to_claude() {
        let req = json!({
            "system_instruction": {"parts": [{"text": "Helper"}]},
            "contents": [
                {"role": "user", "parts": [{"text": "Hi"}]},
                {"role": "model", "parts": [{"text": "Hello"}]}
            ]
        });
        let (conv, tools) = lift_gemini_json(&req);
        let lowered = lower_to_claude(&conv, &tools);

        assert_eq!(lowered["system"], "Helper");
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
    }

    #[test]
    fn openai_tool_call_to_claude() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "calc 2+2"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "calc".into(),
                    input: json!({"expr": "2+2"}),
                }],
            ));

        let lowered = lower_to_claude(&conv, &[]);
        let content = lowered["messages"][1]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "tool_use");
        assert_eq!(content[0]["id"], "c1");
    }

    #[test]
    fn openai_tool_call_to_gemini() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "calc 2+2"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "calc".into(),
                    input: json!({"expr": "2+2"}),
                }],
            ));

        let lowered = lower_to_gemini(&conv, &[]);
        let part = &lowered["contents"][1]["parts"][0];
        assert_eq!(part["functionCall"]["name"], "calc");
    }

    #[test]
    fn thinking_block_lossy_openai_to_gemini() {
        // Thinking blocks in Claude are lossy when going to Gemini (dropped)
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "deep thought".into(),
                },
                IrContentBlock::Text {
                    text: "answer".into(),
                },
            ],
        ));

        let gemini = lower_to_gemini(&conv, &[]);
        let parts = gemini["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(
            parts.len(),
            1,
            "thinking block should be dropped for Gemini"
        );

        // But Claude preserves it
        let claude = lower_to_claude(&conv, &[]);
        let blocks = claude["messages"][0]["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 2, "thinking block preserved in Claude");
    }

    #[test]
    fn all_dialects_produce_valid_json_objects() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::Assistant, "hello"));
        let tools = vec![tool("search", "Search")];

        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &tools);
            assert!(
                lowered.is_object(),
                "{dialect} should produce a JSON object"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. Content blocks: text, image, tool_use, tool_result normalization
// ═══════════════════════════════════════════════════════════════════════

mod content_blocks {
    use super::*;

    #[test]
    fn text_block_serde_roundtrip() {
        let block = IrContentBlock::Text {
            text: "Hello world".into(),
        };
        let json = serde_json::to_value(&block).unwrap();
        let back: IrContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn image_block_serde_roundtrip() {
        let block = IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "iVBORw0KGgo=".into(),
        };
        let json = serde_json::to_value(&block).unwrap();
        let back: IrContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn tool_use_block_serde_roundtrip() {
        let block = IrContentBlock::ToolUse {
            id: "call_42".into(),
            name: "read_file".into(),
            input: json!({"path": "/tmp/test.rs"}),
        };
        let json = serde_json::to_value(&block).unwrap();
        let back: IrContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn tool_result_block_serde_roundtrip() {
        let block = IrContentBlock::ToolResult {
            tool_use_id: "call_42".into(),
            content: vec![IrContentBlock::Text {
                text: "file contents".into(),
            }],
            is_error: false,
        };
        let json = serde_json::to_value(&block).unwrap();
        let back: IrContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn tool_result_error_block_roundtrip() {
        let block = IrContentBlock::ToolResult {
            tool_use_id: "call_99".into(),
            content: vec![IrContentBlock::Text {
                text: "permission denied".into(),
            }],
            is_error: true,
        };
        let json = serde_json::to_value(&block).unwrap();
        let back: IrContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn thinking_block_serde_roundtrip() {
        let block = IrContentBlock::Thinking {
            text: "Let me consider...".into(),
        };
        let json = serde_json::to_value(&block).unwrap();
        let back: IrContentBlock = serde_json::from_value(json).unwrap();
        assert_eq!(block, back);
    }

    #[test]
    fn mixed_content_message() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "thinking".into(),
                },
                IrContentBlock::Text {
                    text: "Here is the answer".into(),
                },
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "calc".into(),
                    input: json!({"x": 1}),
                },
            ],
        );
        assert!(!msg.is_text_only());
        assert_eq!(msg.text_content(), "Here is the answer");
        assert_eq!(msg.tool_use_blocks().len(), 1);
    }

    #[test]
    fn text_only_message_check() {
        let msg = IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text { text: "A".into() },
                IrContentBlock::Text { text: "B".into() },
            ],
        );
        assert!(msg.is_text_only());
        assert_eq!(msg.text_content(), "AB");
    }

    #[test]
    fn empty_content_message() {
        let msg = IrMessage::new(IrRole::User, vec![]);
        assert!(msg.is_text_only()); // vacuously true
        assert_eq!(msg.text_content(), "");
        assert!(msg.tool_use_blocks().is_empty());
    }

    #[test]
    fn tool_result_with_multiple_content_blocks() {
        let block = IrContentBlock::ToolResult {
            tool_use_id: "t1".into(),
            content: vec![
                IrContentBlock::Text {
                    text: "line 1\n".into(),
                },
                IrContentBlock::Text {
                    text: "line 2".into(),
                },
            ],
            is_error: false,
        };
        // Lowered to OpenAI should concatenate text
        let conv = IrConversation::new().push(IrMessage::new(IrRole::Tool, vec![block]));
        let lowered = lower_to_openai(&conv, &[]);
        assert_eq!(lowered["messages"][0]["content"], "line 1\nline 2");
    }

    #[test]
    fn image_block_lowered_to_claude_format() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/webp".into(),
                data: "RIFF".into(),
            }],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        let block = &lowered["messages"][0]["content"][0];
        assert_eq!(block["type"], "image");
        assert_eq!(block["source"]["type"], "base64");
        assert_eq!(block["source"]["media_type"], "image/webp");
    }

    #[test]
    fn image_block_lowered_to_gemini_inline_data() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/gif".into(),
                data: "R0lGODlh".into(),
            }],
        ));
        let lowered = lower_to_gemini(&conv, &[]);
        let part = &lowered["contents"][0]["parts"][0];
        assert_eq!(part["inline_data"]["mime_type"], "image/gif");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. Message roles: mapping across dialects
// ═══════════════════════════════════════════════════════════════════════

mod message_roles {
    use super::*;

    #[test]
    fn normalize_role_standard_roles() {
        assert_eq!(normalize_role("system"), Some(IrRole::System));
        assert_eq!(normalize_role("user"), Some(IrRole::User));
        assert_eq!(normalize_role("assistant"), Some(IrRole::Assistant));
        assert_eq!(normalize_role("tool"), Some(IrRole::Tool));
    }

    #[test]
    fn normalize_role_gemini_model_alias() {
        assert_eq!(normalize_role("model"), Some(IrRole::Assistant));
    }

    #[test]
    fn normalize_role_openai_function_alias() {
        assert_eq!(normalize_role("function"), Some(IrRole::Tool));
    }

    #[test]
    fn normalize_role_openai_developer_alias() {
        assert_eq!(normalize_role("developer"), Some(IrRole::System));
    }

    #[test]
    fn normalize_role_anthropic_human_alias() {
        assert_eq!(normalize_role("human"), Some(IrRole::User));
    }

    #[test]
    fn normalize_role_bot_alias() {
        assert_eq!(normalize_role("bot"), Some(IrRole::Assistant));
    }

    #[test]
    fn normalize_role_unknown_returns_none() {
        assert_eq!(normalize_role("narrator"), None);
        assert_eq!(normalize_role(""), None);
        assert_eq!(normalize_role("admin"), None);
        assert_eq!(normalize_role("SYSTEM"), None); // case sensitive
    }

    #[test]
    fn ir_role_to_openai_dialect() {
        assert_eq!(
            ir_role_to_dialect(IrRole::System, Dialect::OpenAi),
            "system"
        );
        assert_eq!(ir_role_to_dialect(IrRole::User, Dialect::OpenAi), "user");
        assert_eq!(
            ir_role_to_dialect(IrRole::Assistant, Dialect::OpenAi),
            "assistant"
        );
        assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::OpenAi), "tool");
    }

    #[test]
    fn ir_role_to_claude_dialect() {
        assert_eq!(ir_role_to_dialect(IrRole::User, Dialect::Claude), "user");
        assert_eq!(
            ir_role_to_dialect(IrRole::Assistant, Dialect::Claude),
            "assistant"
        );
        // Claude maps Tool to "user"
        assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::Claude), "user");
    }

    #[test]
    fn ir_role_to_gemini_dialect() {
        assert_eq!(ir_role_to_dialect(IrRole::User, Dialect::Gemini), "user");
        assert_eq!(
            ir_role_to_dialect(IrRole::Assistant, Dialect::Gemini),
            "model"
        );
        // Gemini maps Tool to "user"
        assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::Gemini), "user");
    }

    #[test]
    fn user_role_consistent_across_all_dialects() {
        for dialect in Dialect::all() {
            assert_eq!(
                ir_role_to_dialect(IrRole::User, *dialect),
                "user",
                "{dialect}: user should always be 'user'"
            );
        }
    }

    #[test]
    fn openai_compatible_dialects_match() {
        let oai_compatible = [
            Dialect::OpenAi,
            Dialect::Codex,
            Dialect::Copilot,
            Dialect::Kimi,
        ];
        for role in [
            IrRole::System,
            IrRole::User,
            IrRole::Assistant,
            IrRole::Tool,
        ] {
            let expected = ir_role_to_dialect(role, Dialect::OpenAi);
            for dialect in &oai_compatible {
                assert_eq!(
                    ir_role_to_dialect(role, *dialect),
                    expected,
                    "{dialect} should match OpenAI for {role:?}"
                );
            }
        }
    }

    #[test]
    fn ir_role_serde_roundtrip() {
        for role in [
            IrRole::System,
            IrRole::User,
            IrRole::Assistant,
            IrRole::Tool,
        ] {
            let json = serde_json::to_value(role).unwrap();
            let back: IrRole = serde_json::from_value(json).unwrap();
            assert_eq!(role, back);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 9. Tool definitions: function schema normalization
// ═══════════════════════════════════════════════════════════════════════

mod tool_definitions {
    use super::*;

    #[test]
    fn normalize_tool_schemas_adds_missing_type() {
        let tools = vec![tool_with_params(
            "search",
            "Search",
            json!({"properties": {"q": {"type": "string"}}}),
        )];
        let normalized = normalize_tool_schemas(&tools);
        assert_eq!(normalized[0].parameters["type"], "object");
    }

    #[test]
    fn normalize_tool_schemas_preserves_existing_type() {
        let tools = vec![tool_with_params(
            "search",
            "Search",
            json!({"type": "object", "properties": {}}),
        )];
        let normalized = normalize_tool_schemas(&tools);
        assert_eq!(normalized[0].parameters["type"], "object");
    }

    #[test]
    fn normalize_tool_schemas_preserves_properties() {
        let params = json!({
            "properties": {
                "q": {"type": "string", "description": "Query"},
                "limit": {"type": "integer"}
            },
            "required": ["q"]
        });
        let tools = vec![tool_with_params("search", "Search", params.clone())];
        let normalized = normalize_tool_schemas(&tools);
        assert_eq!(normalized[0].parameters["properties"], params["properties"]);
        assert_eq!(normalized[0].parameters["required"], params["required"]);
    }

    #[test]
    fn sort_tools_alphabetical() {
        let mut tools = vec![
            tool("zebra", "z tool"),
            tool("alpha", "a tool"),
            tool("middle", "m tool"),
        ];
        sort_tools(&mut tools);
        assert_eq!(tools[0].name, "alpha");
        assert_eq!(tools[1].name, "middle");
        assert_eq!(tools[2].name, "zebra");
    }

    #[test]
    fn sort_tools_empty() {
        let mut tools: Vec<IrToolDefinition> = vec![];
        sort_tools(&mut tools);
        assert!(tools.is_empty());
    }

    #[test]
    fn sort_tools_single() {
        let mut tools = vec![tool("only", "the only tool")];
        sort_tools(&mut tools);
        assert_eq!(tools[0].name, "only");
    }

    #[test]
    fn tool_definition_serde_roundtrip() {
        let t = IrToolDefinition {
            name: "read_file".into(),
            description: "Read a file from disk".into(),
            parameters: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }),
        };
        let json = serde_json::to_value(&t).unwrap();
        let back: IrToolDefinition = serde_json::from_value(json).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn tool_lowered_differently_per_dialect() {
        let tools = vec![tool_with_params(
            "calc",
            "Calculator",
            json!({"type": "object", "properties": {"expr": {"type": "string"}}}),
        )];
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));

        // OpenAI wraps in {type: "function", function: {...}}
        let openai = lower_to_openai(&conv, &tools);
        assert_eq!(openai["tools"][0]["type"], "function");
        assert!(openai["tools"][0]["function"]["parameters"].is_object());

        // Claude uses {name, description, input_schema}
        let claude = lower_to_claude(&conv, &tools);
        assert!(claude["tools"][0].get("input_schema").is_some());

        // Gemini uses {function_declarations: [{name, description, parameters}]}
        let gemini = lower_to_gemini(&conv, &tools);
        let decl = &gemini["tools"][0]["function_declarations"][0];
        assert_eq!(decl["name"], "calc");
    }

    #[test]
    fn multiple_tools_preserved_across_lowering() {
        let tools = vec![
            tool("tool_a", "First"),
            tool("tool_b", "Second"),
            tool("tool_c", "Third"),
        ];
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));

        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &tools);
            let tool_array = if *dialect == Dialect::Gemini {
                lowered["tools"][0]["function_declarations"]
                    .as_array()
                    .unwrap()
            } else {
                lowered["tools"].as_array().unwrap()
            };
            assert_eq!(tool_array.len(), 3, "{dialect} should preserve all 3 tools");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 10. Parameters preservation
// ═══════════════════════════════════════════════════════════════════════

mod parameters_preservation {
    use super::*;
    use abp_sdk_types::ModelConfig;

    #[test]
    fn model_config_roundtrip() {
        let cfg = ModelConfig {
            model: "gpt-4o".into(),
            max_tokens: Some(4096),
            temperature: Some(0.7),
            top_p: Some(0.9),
            stop_sequences: Some(vec!["STOP".into(), "END".into()]),
            ..Default::default()
        };
        let json = serde_json::to_value(&cfg).unwrap();
        let back: ModelConfig = serde_json::from_value(json).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn model_config_optional_fields_default_to_none() {
        let cfg = ModelConfig {
            model: "claude-sonnet-4-20250514".into(),
            ..Default::default()
        };
        assert!(cfg.max_tokens.is_none());
        assert!(cfg.temperature.is_none());
        assert!(cfg.top_p.is_none());
        assert!(cfg.stop_sequences.is_none());
    }

    #[test]
    fn model_config_with_extra_params() {
        let mut extra = BTreeMap::new();
        extra.insert("frequency_penalty".into(), json!(0.5));
        extra.insert("presence_penalty".into(), json!(0.3));
        let cfg = ModelConfig {
            model: "gpt-4o".into(),
            extra,
            ..Default::default()
        };
        let json = serde_json::to_value(&cfg).unwrap();
        let back: ModelConfig = serde_json::from_value(json).unwrap();
        assert_eq!(cfg, back);
        assert_eq!(back.extra["frequency_penalty"], 0.5);
    }

    #[test]
    fn model_config_stop_sequences_preserved() {
        let cfg = ModelConfig {
            model: "test".into(),
            stop_sequences: Some(vec!["\n\nHuman:".into(), "\n\nAssistant:".into()]),
            ..Default::default()
        };
        let json = serde_json::to_value(&cfg).unwrap();
        let back: ModelConfig = serde_json::from_value(json).unwrap();
        assert_eq!(back.stop_sequences.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn model_config_skip_serializing_none_fields() {
        let cfg = ModelConfig {
            model: "test".into(),
            ..Default::default()
        };
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(!json.contains("max_tokens"));
        assert!(!json.contains("temperature"));
        assert!(!json.contains("top_p"));
        assert!(!json.contains("stop_sequences"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 11. Model mapping: canonical model naming
// ═══════════════════════════════════════════════════════════════════════

mod model_mapping {
    use super::*;
    use abp_sdk_types::ModelConfig;

    /// Simple model name canonicalization for testing.
    fn canonical_model(model: &str) -> &str {
        match model {
            "gpt-4o" | "gpt-4o-2024-08-06" => "gpt-4o",
            "gpt-4" | "gpt-4-0613" => "gpt-4",
            "gpt-3.5-turbo" | "gpt-3.5-turbo-0125" => "gpt-3.5-turbo",
            "claude-sonnet-4-20250514" | "claude-3-5-sonnet-20241022" => "claude-sonnet",
            "claude-3-opus-20240229" => "claude-opus",
            "gemini-1.5-pro" | "gemini-1.5-pro-latest" => "gemini-pro",
            "gemini-1.5-flash" => "gemini-flash",
            other => other,
        }
    }

    #[test]
    fn openai_model_variants_canonicalize() {
        assert_eq!(canonical_model("gpt-4o"), "gpt-4o");
        assert_eq!(canonical_model("gpt-4o-2024-08-06"), "gpt-4o");
    }

    #[test]
    fn claude_model_variants_canonicalize() {
        assert_eq!(canonical_model("claude-sonnet-4-20250514"), "claude-sonnet");
        assert_eq!(
            canonical_model("claude-3-5-sonnet-20241022"),
            "claude-sonnet"
        );
    }

    #[test]
    fn gemini_model_variants_canonicalize() {
        assert_eq!(canonical_model("gemini-1.5-pro"), "gemini-pro");
        assert_eq!(canonical_model("gemini-1.5-pro-latest"), "gemini-pro");
        assert_eq!(canonical_model("gemini-1.5-flash"), "gemini-flash");
    }

    #[test]
    fn unknown_model_passes_through() {
        assert_eq!(canonical_model("custom-model-v2"), "custom-model-v2");
    }

    #[test]
    fn model_config_preserves_model_name() {
        let cfg = ModelConfig {
            model: "gpt-4o-2024-08-06".into(),
            ..Default::default()
        };
        assert_eq!(cfg.model, "gpt-4o-2024-08-06");
    }

    #[test]
    fn model_in_dialect_request_extracted() {
        use abp_sdk_types::{DialectRequest, openai::OpenAiRequest};
        let req = DialectRequest::OpenAi(OpenAiRequest {
            model: "gpt-4o".into(),
            messages: vec![],
            tools: None,
            tool_choice: None,
            temperature: None,
            max_tokens: None,
            response_format: None,
            stream: None,
        });
        assert_eq!(req.model(), "gpt-4o");
        assert_eq!(req.dialect(), Dialect::OpenAi);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 12. Metadata handling: preserved through transformations
// ═══════════════════════════════════════════════════════════════════════

mod metadata_handling {
    use super::*;

    #[test]
    fn metadata_survives_dedup_system() {
        let mut meta = BTreeMap::new();
        meta.insert("trace_id".into(), json!("abc-123"));
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text { text: "hi".into() }],
            metadata: meta,
        };
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(msg);
        let result = dedup_system(&conv);
        assert_eq!(result.messages[1].metadata["trace_id"], "abc-123");
    }

    #[test]
    fn metadata_survives_trim_text() {
        let mut meta = BTreeMap::new();
        meta.insert("source".into(), json!("api"));
        let msg = msg_with_meta(IrRole::User, "  hello  ", meta);
        let conv = IrConversation::from_messages(vec![msg]);
        let result = trim_text(&conv);
        assert_eq!(result.messages[0].text_content(), "hello");
        assert_eq!(result.messages[0].metadata["source"], "api");
    }

    #[test]
    fn metadata_survives_merge_adjacent_text() {
        let mut meta = BTreeMap::new();
        meta.insert("tag".into(), json!("test"));
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![
                IrContentBlock::Text { text: "A".into() },
                IrContentBlock::Text { text: "B".into() },
            ],
            metadata: meta,
        };
        let conv = IrConversation::from_messages(vec![msg]);
        let result = merge_adjacent_text(&conv);
        assert_eq!(result.messages[0].metadata["tag"], "test");
    }

    #[test]
    fn metadata_survives_full_normalize() {
        let mut meta = BTreeMap::new();
        meta.insert("request_id".into(), json!("req-42"));
        let msg = msg_with_meta(IrRole::User, "  hi  ", meta);
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(msg);
        let result = normalize(&conv);
        let user_msgs = result.messages_by_role(IrRole::User);
        assert_eq!(user_msgs[0].metadata["request_id"], "req-42");
    }

    #[test]
    fn metadata_not_serialized_when_empty() {
        let msg = IrMessage::text(IrRole::User, "hi");
        let json = serde_json::to_string(&msg).unwrap();
        assert!(!json.contains("metadata"));
    }

    #[test]
    fn metadata_serialized_when_present() {
        let mut meta = BTreeMap::new();
        meta.insert("key".into(), json!("value"));
        let msg = msg_with_meta(IrRole::User, "hi", meta);
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("metadata"));
        assert!(json.contains("key"));
    }

    #[test]
    fn strip_metadata_selective_filtering() {
        let mut meta = BTreeMap::new();
        meta.insert("keep_me".into(), json!(1));
        meta.insert("drop_me".into(), json!(2));
        meta.insert("also_keep".into(), json!(3));
        let msg = msg_with_meta(IrRole::User, "hi", meta);
        let conv = IrConversation::from_messages(vec![msg]);
        let stripped = strip_metadata(&conv, &["keep_me", "also_keep"]);
        assert_eq!(stripped.messages[0].metadata.len(), 2);
        assert!(stripped.messages[0].metadata.contains_key("keep_me"));
        assert!(stripped.messages[0].metadata.contains_key("also_keep"));
        assert!(!stripped.messages[0].metadata.contains_key("drop_me"));
    }

    #[test]
    fn metadata_complex_json_values() {
        let mut meta = BTreeMap::new();
        meta.insert("nested".into(), json!({"a": [1, 2, 3], "b": {"c": true}}));
        meta.insert("array".into(), json!([1, "two", null]));
        let msg = msg_with_meta(IrRole::User, "hi", meta);
        let json = serde_json::to_value(&msg).unwrap();
        let back: IrMessage = serde_json::from_value(json).unwrap();
        assert_eq!(back.metadata["nested"]["a"][1], 2);
        assert_eq!(back.metadata["array"][1], "two");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Additional: IrUsage, IrConversation accessors, serde invariants
// ═══════════════════════════════════════════════════════════════════════

mod ir_usage {
    use super::*;

    #[test]
    fn usage_from_io_computes_total() {
        let u = IrUsage::from_io(100, 50);
        assert_eq!(u.total_tokens, 150);
        assert_eq!(u.cache_read_tokens, 0);
        assert_eq!(u.cache_write_tokens, 0);
    }

    #[test]
    fn usage_with_cache() {
        let u = IrUsage::with_cache(100, 50, 20, 10);
        assert_eq!(u.total_tokens, 150);
        assert_eq!(u.cache_read_tokens, 20);
        assert_eq!(u.cache_write_tokens, 10);
    }

    #[test]
    fn usage_merge() {
        let a = IrUsage::from_io(100, 50);
        let b = IrUsage::with_cache(200, 75, 30, 15);
        let merged = a.merge(b);
        assert_eq!(merged.input_tokens, 300);
        assert_eq!(merged.output_tokens, 125);
        assert_eq!(merged.total_tokens, 425);
        assert_eq!(merged.cache_read_tokens, 30);
        assert_eq!(merged.cache_write_tokens, 15);
    }

    #[test]
    fn usage_default_is_zero() {
        let u = IrUsage::default();
        assert_eq!(u.input_tokens, 0);
        assert_eq!(u.output_tokens, 0);
        assert_eq!(u.total_tokens, 0);
    }

    #[test]
    fn usage_serde_roundtrip() {
        let u = IrUsage::with_cache(100, 50, 20, 10);
        let json = serde_json::to_value(u).unwrap();
        let back: IrUsage = serde_json::from_value(json).unwrap();
        assert_eq!(u, back);
    }
}

mod conversation_accessors {
    use super::*;

    #[test]
    fn system_message_returns_first() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "first"))
            .push(IrMessage::text(IrRole::System, "second"));
        assert_eq!(conv.system_message().unwrap().text_content(), "first");
    }

    #[test]
    fn last_assistant_returns_last() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::Assistant, "first"))
            .push(IrMessage::text(IrRole::User, "middle"))
            .push(IrMessage::text(IrRole::Assistant, "last"));
        assert_eq!(conv.last_assistant().unwrap().text_content(), "last");
    }

    #[test]
    fn last_message_returns_last() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "a"))
            .push(IrMessage::text(IrRole::Assistant, "b"));
        assert_eq!(conv.last_message().unwrap().text_content(), "b");
    }

    #[test]
    fn tool_calls_collects_across_messages() {
        let conv = IrConversation::new()
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "a".into(),
                    input: json!({}),
                }],
            ))
            .push(IrMessage::text(IrRole::User, "ok"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "t2".into(),
                    name: "b".into(),
                    input: json!({}),
                }],
            ));
        assert_eq!(conv.tool_calls().len(), 2);
    }

    #[test]
    fn messages_by_role_filters_correctly() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "a"))
            .push(IrMessage::text(IrRole::Assistant, "b"))
            .push(IrMessage::text(IrRole::User, "c"));
        assert_eq!(conv.messages_by_role(IrRole::User).len(), 2);
        assert_eq!(conv.messages_by_role(IrRole::Assistant).len(), 1);
        assert_eq!(conv.messages_by_role(IrRole::System).len(), 0);
    }

    #[test]
    fn conversation_serde_roundtrip() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::Assistant, "hello"));
        let json = serde_json::to_value(&conv).unwrap();
        let back: IrConversation = serde_json::from_value(json).unwrap();
        assert_eq!(conv, back);
    }

    #[test]
    fn conversation_default_eq_new() {
        assert_eq!(IrConversation::default(), IrConversation::new());
    }
}

mod kimi_codex_copilot_compat {
    use super::*;

    fn conv_with_tools() -> (IrConversation, Vec<IrToolDefinition>) {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::text(IrRole::Assistant, "hello"));
        let tools = vec![tool("calc", "Calculator")];
        (conv, tools)
    }

    #[test]
    fn kimi_matches_openai() {
        let (conv, tools) = conv_with_tools();
        assert_eq!(lower_to_openai(&conv, &tools), lower_to_kimi(&conv, &tools));
    }

    #[test]
    fn codex_matches_openai() {
        let (conv, tools) = conv_with_tools();
        assert_eq!(
            lower_to_openai(&conv, &tools),
            lower_to_codex(&conv, &tools)
        );
    }

    #[test]
    fn copilot_matches_openai() {
        let (conv, tools) = conv_with_tools();
        assert_eq!(
            lower_to_openai(&conv, &tools),
            lower_to_copilot(&conv, &tools)
        );
    }

    #[test]
    fn all_openai_compat_dialects_identical_with_tool_calls() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "hi"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "search".into(),
                    input: json!({"q": "rust"}),
                }],
            ));
        let tools = vec![tool("search", "Search")];
        let openai = lower_to_openai(&conv, &tools);
        assert_eq!(lower_to_kimi(&conv, &tools), openai);
        assert_eq!(lower_to_codex(&conv, &tools), openai);
        assert_eq!(lower_to_copilot(&conv, &tools), openai);
    }
}
