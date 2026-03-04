// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep integration tests for the ABP Intermediate Representation layer.
//!
//! Covers construction from vendor-like formats, normalization patterns,
//! lowering to vendor-like formats, and serde invariants.

use abp_ir::*;
use serde_json::json;
use std::collections::BTreeMap;

// ═══════════════════════════════════════════════════════════════════════
// Module: ir_construction
// ═══════════════════════════════════════════════════════════════════════

mod ir_construction {
    use super::*;

    #[test]
    fn ir_from_openai_format_request() {
        // OpenAI-style: messages array with role/content string pairs,
        // system as first message, tools as top-level array.
        let openai_request = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "Hello!"},
                {"role": "assistant", "content": "Hi there!"},
                {"role": "user", "content": "What is 2+2?"}
            ],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "calculator",
                        "description": "Evaluate math expressions",
                        "parameters": {"type": "object", "properties": {"expr": {"type": "string"}}}
                    }
                }
            ]
        });

        // Lift into IR
        let msgs = openai_request["messages"].as_array().unwrap();
        let mut conv = IrConversation::new();
        for m in msgs {
            let role = match m["role"].as_str().unwrap() {
                "system" => IrRole::System,
                "user" => IrRole::User,
                "assistant" => IrRole::Assistant,
                _ => panic!("unknown role"),
            };
            let text = m["content"].as_str().unwrap();
            conv = conv.push(IrMessage::text(role, text));
        }

        let tools: Vec<IrToolDefinition> = openai_request["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                let f = &t["function"];
                IrToolDefinition {
                    name: f["name"].as_str().unwrap().to_string(),
                    description: f["description"].as_str().unwrap().to_string(),
                    parameters: f["parameters"].clone(),
                }
            })
            .collect();

        assert_eq!(conv.len(), 4, "should have 4 messages");
        assert_eq!(
            conv.system_message().unwrap().text_content(),
            "You are a helpful assistant.",
            "system prompt should be preserved"
        );
        assert_eq!(tools.len(), 1, "should have 1 tool");
        assert_eq!(tools[0].name, "calculator", "tool name should match");
    }

    #[test]
    fn ir_from_claude_format_request() {
        // Claude-style: system is a top-level field, messages use content blocks.
        let claude_request = json!({
            "model": "claude-sonnet-4-20250514",
            "system": "You are a coding assistant.",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "Explain Rust ownership."}
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "Rust ownership is a memory management system..."}
                    ]
                }
            ],
            "tools": [
                {
                    "name": "read_file",
                    "description": "Read a file from disk",
                    "input_schema": {"type": "object", "properties": {"path": {"type": "string"}}}
                }
            ]
        });

        // Lift into IR
        let mut conv = IrConversation::new();
        if let Some(sys) = claude_request["system"].as_str() {
            conv = conv.push(IrMessage::text(IrRole::System, sys));
        }
        for m in claude_request["messages"].as_array().unwrap() {
            let role = match m["role"].as_str().unwrap() {
                "user" => IrRole::User,
                "assistant" => IrRole::Assistant,
                _ => panic!("unknown role"),
            };
            let blocks: Vec<IrContentBlock> = m["content"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| IrContentBlock::Text {
                    text: b["text"].as_str().unwrap().to_string(),
                })
                .collect();
            conv = conv.push(IrMessage::new(role, blocks));
        }

        let tools: Vec<IrToolDefinition> = claude_request["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| IrToolDefinition {
                name: t["name"].as_str().unwrap().to_string(),
                description: t["description"].as_str().unwrap().to_string(),
                parameters: t["input_schema"].clone(),
            })
            .collect();

        assert_eq!(conv.len(), 3, "system + 2 messages");
        assert_eq!(
            conv.system_message().unwrap().text_content(),
            "You are a coding assistant.",
            "system prompt from top-level field should be preserved"
        );
        assert_eq!(tools[0].name, "read_file");
    }

    #[test]
    fn ir_from_gemini_format_request() {
        // Gemini-style: system_instruction separate, parts-based content.
        let gemini_request = json!({
            "model": "gemini-2.0-flash",
            "system_instruction": {"parts": [{"text": "Be concise."}]},
            "contents": [
                {"role": "user", "parts": [{"text": "Summarize quantum computing."}]},
                {"role": "model", "parts": [{"text": "Quantum computing uses qubits..."}]}
            ],
            "tools": [{"function_declarations": [
                {
                    "name": "search",
                    "description": "Web search",
                    "parameters": {"type": "object", "properties": {"query": {"type": "string"}}}
                }
            ]}]
        });

        let mut conv = IrConversation::new();
        if let Some(sys) = gemini_request.get("system_instruction") {
            let text = sys["parts"][0]["text"].as_str().unwrap();
            conv = conv.push(IrMessage::text(IrRole::System, text));
        }
        for c in gemini_request["contents"].as_array().unwrap() {
            let role = match c["role"].as_str().unwrap() {
                "user" => IrRole::User,
                "model" => IrRole::Assistant,
                _ => panic!("unknown role"),
            };
            let blocks: Vec<IrContentBlock> = c["parts"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|p| {
                    p["text"].as_str().map(|t| IrContentBlock::Text {
                        text: t.to_string(),
                    })
                })
                .collect();
            conv = conv.push(IrMessage::new(role, blocks));
        }

        let tools: Vec<IrToolDefinition> = gemini_request["tools"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|t| t["function_declarations"].as_array().unwrap())
            .map(|f| IrToolDefinition {
                name: f["name"].as_str().unwrap().to_string(),
                description: f["description"].as_str().unwrap().to_string(),
                parameters: f["parameters"].clone(),
            })
            .collect();

        assert_eq!(conv.len(), 3, "system + 2 content turns");
        assert_eq!(conv.system_message().unwrap().text_content(), "Be concise.");
        assert_eq!(tools[0].name, "search");
    }

    #[test]
    fn ir_preserves_message_ordering() {
        let roles = [
            IrRole::System,
            IrRole::User,
            IrRole::Assistant,
            IrRole::User,
            IrRole::Assistant,
            IrRole::Tool,
            IrRole::Assistant,
        ];
        let mut conv = IrConversation::new();
        for (i, role) in roles.iter().enumerate() {
            conv = conv.push(IrMessage::text(*role, format!("msg-{i}")));
        }

        assert_eq!(conv.len(), roles.len(), "all messages should be present");
        for (i, msg) in conv.messages.iter().enumerate() {
            assert_eq!(
                msg.role, roles[i],
                "message {i} should have role {:?}",
                roles[i]
            );
            assert_eq!(
                msg.text_content(),
                format!("msg-{i}"),
                "message {i} text should match"
            );
        }
    }

    #[test]
    fn ir_preserves_tool_definitions() {
        let tools = vec![
            IrToolDefinition {
                name: "read_file".into(),
                description: "Read contents of a file".into(),
                parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            },
            IrToolDefinition {
                name: "write_file".into(),
                description: "Write contents to a file".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                        "content": {"type": "string"}
                    },
                    "required": ["path", "content"]
                }),
            },
            IrToolDefinition {
                name: "run_command".into(),
                description: "Execute a shell command".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {"cmd": {"type": "string"}, "args": {"type": "array"}}
                }),
            },
        ];

        let json = serde_json::to_string(&tools).unwrap();
        let restored: Vec<IrToolDefinition> = serde_json::from_str(&json).unwrap();

        assert_eq!(
            restored.len(),
            3,
            "all tool definitions should survive serialization"
        );
        for (orig, back) in tools.iter().zip(restored.iter()) {
            assert_eq!(orig.name, back.name, "tool name must match");
            assert_eq!(
                orig.description, back.description,
                "tool description must match"
            );
            assert_eq!(
                orig.parameters, back.parameters,
                "tool parameter schema must match"
            );
        }
    }

    #[test]
    fn ir_preserves_system_prompts() {
        let system_text = "You are a helpful assistant specialized in Rust programming.\n\
                           Always provide examples with your explanations.";
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, system_text))
            .push(IrMessage::text(IrRole::User, "Help me with lifetimes."));

        let sys = conv.system_message().expect("system message should exist");
        assert_eq!(
            sys.text_content(),
            system_text,
            "system prompt including newlines should be preserved exactly"
        );
        assert_eq!(sys.role, IrRole::System);
        assert!(sys.is_text_only(), "system message should be text-only");
    }

    #[test]
    fn ir_from_empty_request() {
        let conv = IrConversation::new();
        assert!(conv.is_empty(), "empty conversation should be empty");
        assert_eq!(conv.len(), 0);
        assert!(
            conv.system_message().is_none(),
            "no system message in empty conversation"
        );
        assert!(
            conv.last_assistant().is_none(),
            "no assistant in empty conversation"
        );
        assert!(
            conv.last_message().is_none(),
            "no last message in empty conversation"
        );
        assert!(
            conv.tool_calls().is_empty(),
            "no tool calls in empty conversation"
        );
    }

    #[test]
    fn ir_from_messages_constructor() {
        let messages = vec![
            IrMessage::text(IrRole::User, "hello"),
            IrMessage::text(IrRole::Assistant, "hi"),
        ];
        let conv = IrConversation::from_messages(messages.clone());
        assert_eq!(conv.messages, messages, "from_messages should wrap exactly");
    }

    #[test]
    fn ir_message_with_tool_use_blocks() {
        let msg = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me read that file.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "call_001".into(),
                    name: "read_file".into(),
                    input: json!({"path": "/tmp/test.rs"}),
                },
                IrContentBlock::ToolUse {
                    id: "call_002".into(),
                    name: "list_dir".into(),
                    input: json!({"path": "/tmp"}),
                },
            ],
        );

        let tool_uses = msg.tool_use_blocks();
        assert_eq!(
            tool_uses.len(),
            2,
            "should extract exactly the tool_use blocks"
        );
        assert!(
            !msg.is_text_only(),
            "message with tool_use is not text-only"
        );
    }

    #[test]
    fn ir_conversation_accessors() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "u1"))
            .push(IrMessage::text(IrRole::Assistant, "a1"))
            .push(IrMessage::text(IrRole::User, "u2"))
            .push(IrMessage::text(IrRole::Assistant, "a2"));

        assert_eq!(
            conv.messages_by_role(IrRole::User).len(),
            2,
            "should find 2 user messages"
        );
        assert_eq!(
            conv.messages_by_role(IrRole::Assistant).len(),
            2,
            "should find 2 assistant messages"
        );
        assert_eq!(
            conv.last_assistant().unwrap().text_content(),
            "a2",
            "last_assistant should return most recent"
        );
        assert_eq!(
            conv.last_message().unwrap().text_content(),
            "a2",
            "last_message should return final message"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: ir_normalization
// ═══════════════════════════════════════════════════════════════════════

mod ir_normalization {
    use super::*;

    /// Normalize: merge duplicate system messages into one.
    fn dedup_system_messages(conv: &IrConversation) -> IrConversation {
        let system_texts: Vec<String> = conv
            .messages
            .iter()
            .filter(|m| m.role == IrRole::System)
            .map(|m| m.text_content())
            .collect();
        let mut messages: Vec<IrMessage> = Vec::new();
        if !system_texts.is_empty() {
            messages.push(IrMessage::text(IrRole::System, system_texts.join("\n")));
        }
        messages.extend(
            conv.messages
                .iter()
                .filter(|m| m.role != IrRole::System)
                .cloned(),
        );
        IrConversation::from_messages(messages)
    }

    /// Normalize: trim whitespace from text blocks.
    fn trim_whitespace(conv: &IrConversation) -> IrConversation {
        let messages = conv
            .messages
            .iter()
            .map(|m| {
                let content = m
                    .content
                    .iter()
                    .map(|b| match b {
                        IrContentBlock::Text { text } => IrContentBlock::Text {
                            text: text.trim().to_string(),
                        },
                        other => other.clone(),
                    })
                    .collect();
                IrMessage {
                    role: m.role,
                    content,
                    metadata: m.metadata.clone(),
                }
            })
            .collect();
        IrConversation::from_messages(messages)
    }

    /// Normalize: sort tool definitions by name for determinism.
    fn sort_tools(tools: &[IrToolDefinition]) -> Vec<IrToolDefinition> {
        let mut sorted = tools.to_vec();
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        sorted
    }

    /// Normalize: remove messages with no content blocks.
    fn remove_empty_messages(conv: &IrConversation) -> IrConversation {
        let messages = conv
            .messages
            .iter()
            .filter(|m| !m.content.is_empty())
            .cloned()
            .collect();
        IrConversation::from_messages(messages)
    }

    /// Full normalization pipeline.
    fn normalize(conv: &IrConversation) -> IrConversation {
        let step1 = dedup_system_messages(conv);
        let step2 = trim_whitespace(&step1);
        remove_empty_messages(&step2)
    }

    #[test]
    fn normalize_deduplicates_system_messages() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Be helpful."))
            .push(IrMessage::text(IrRole::User, "Hi"))
            .push(IrMessage::text(IrRole::System, "Be concise."));

        let normalized = dedup_system_messages(&conv);
        let sys_msgs = normalized.messages_by_role(IrRole::System);
        assert_eq!(
            sys_msgs.len(),
            1,
            "duplicate system messages should be merged into one"
        );
        assert_eq!(
            sys_msgs[0].text_content(),
            "Be helpful.\nBe concise.",
            "system texts should be joined with newline"
        );
    }

    #[test]
    fn normalize_trims_whitespace() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "  hello  "))
            .push(IrMessage::text(IrRole::Assistant, "\n  world\t "));

        let normalized = trim_whitespace(&conv);
        assert_eq!(
            normalized.messages[0].text_content(),
            "hello",
            "leading/trailing whitespace should be trimmed"
        );
        assert_eq!(normalized.messages[1].text_content(), "world");
    }

    #[test]
    fn normalize_sorts_tool_definitions_for_determinism() {
        let tools = vec![
            IrToolDefinition {
                name: "zebra".into(),
                description: "z tool".into(),
                parameters: json!({}),
            },
            IrToolDefinition {
                name: "alpha".into(),
                description: "a tool".into(),
                parameters: json!({}),
            },
            IrToolDefinition {
                name: "middle".into(),
                description: "m tool".into(),
                parameters: json!({}),
            },
        ];

        let sorted = sort_tools(&tools);
        let names: Vec<&str> = sorted.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["alpha", "middle", "zebra"],
            "tools should be sorted alphabetically by name"
        );
    }

    #[test]
    fn normalize_removes_empty_messages() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "keep"))
            .push(IrMessage::new(IrRole::Assistant, vec![]))
            .push(IrMessage::text(IrRole::Assistant, "also keep"));

        let normalized = remove_empty_messages(&conv);
        assert_eq!(
            normalized.len(),
            2,
            "message with empty content should be removed"
        );
        assert_eq!(normalized.messages[0].text_content(), "keep");
        assert_eq!(normalized.messages[1].text_content(), "also keep");
    }

    #[test]
    fn normalized_ir_is_idempotent() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "  Be helpful. "))
            .push(IrMessage::text(IrRole::User, " Hi there "))
            .push(IrMessage::new(IrRole::User, vec![]))
            .push(IrMessage::text(IrRole::System, " Be concise. "));

        let once = normalize(&conv);
        let twice = normalize(&once);
        assert_eq!(
            once, twice,
            "normalizing an already-normalized conversation should be a no-op"
        );
    }

    #[test]
    fn normalization_preserves_content_integrity() {
        let tool_block = IrContentBlock::ToolUse {
            id: "call_1".into(),
            name: "search".into(),
            input: json!({"query": "rust ownership"}),
        };
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::text(IrRole::User, "  question  "))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: " I will search ".into(),
                    },
                    tool_block.clone(),
                ],
            ));

        let normalized = normalize(&conv);
        let assistant = normalized
            .last_assistant()
            .expect("assistant message should survive normalization");

        // Tool use block should be untouched
        assert_eq!(
            assistant.content[1], tool_block,
            "tool_use block should not be modified by normalization"
        );
        // Text should be trimmed
        assert_eq!(
            assistant.content[0],
            IrContentBlock::Text {
                text: "I will search".into()
            },
            "text block should be trimmed"
        );
    }

    #[test]
    fn normalize_preserves_metadata() {
        let mut meta = BTreeMap::new();
        meta.insert("temperature".to_string(), json!(0.7));
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text {
                text: "  padded  ".into(),
            }],
            metadata: meta.clone(),
        };
        let conv = IrConversation::from_messages(vec![msg]);
        let normalized = trim_whitespace(&conv);

        assert_eq!(
            normalized.messages[0].metadata, meta,
            "metadata should be preserved through normalization"
        );
    }

    #[test]
    fn normalize_single_system_message_unchanged() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Only one system message."))
            .push(IrMessage::text(IrRole::User, "Hello"));

        let normalized = dedup_system_messages(&conv);
        assert_eq!(
            normalized.messages_by_role(IrRole::System).len(),
            1,
            "single system message should remain"
        );
        assert_eq!(
            normalized.system_message().unwrap().text_content(),
            "Only one system message."
        );
    }

    #[test]
    fn normalize_empty_conversation_stays_empty() {
        let conv = IrConversation::new();
        let normalized = normalize(&conv);
        assert!(
            normalized.is_empty(),
            "normalizing empty conversation should stay empty"
        );
    }

    #[test]
    fn normalize_all_empty_messages_yields_empty() {
        let conv = IrConversation::new()
            .push(IrMessage::new(IrRole::User, vec![]))
            .push(IrMessage::new(IrRole::Assistant, vec![]));

        let normalized = remove_empty_messages(&conv);
        assert!(
            normalized.is_empty(),
            "conversation with only empty messages should normalize to empty"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: ir_lowering
// ═══════════════════════════════════════════════════════════════════════

mod ir_lowering {
    use super::*;

    /// Lower IR conversation to an OpenAI-format JSON request.
    fn lower_to_openai(conv: &IrConversation, tools: &[IrToolDefinition]) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = conv
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    IrRole::System => "system",
                    IrRole::User => "user",
                    IrRole::Assistant => "assistant",
                    IrRole::Tool => "tool",
                };
                json!({"role": role, "content": m.text_content()})
            })
            .collect();
        let tools_json: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                })
            })
            .collect();
        let mut req = json!({"messages": messages});
        if !tools_json.is_empty() {
            req["tools"] = json!(tools_json);
        }
        req
    }

    /// Lower IR conversation to a Claude-format JSON request.
    fn lower_to_claude(conv: &IrConversation, tools: &[IrToolDefinition]) -> serde_json::Value {
        let system = conv.system_message().map(|m| m.text_content());
        let messages: Vec<serde_json::Value> = conv
            .messages
            .iter()
            .filter(|m| m.role != IrRole::System)
            .map(|m| {
                let role = match m.role {
                    IrRole::User => "user",
                    IrRole::Assistant => "assistant",
                    IrRole::Tool => "user", // Claude sends tool results as user role
                    IrRole::System => unreachable!(),
                };
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
                            json!({
                                "type": "tool_result",
                                "tool_use_id": tool_use_id,
                                "content": inner,
                                "is_error": is_error
                            })
                        }
                        _ => json!({"type": "text", "text": ""}),
                    })
                    .collect();
                json!({"role": role, "content": content})
            })
            .collect();
        let tools_json: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters
                })
            })
            .collect();
        let mut req = json!({"messages": messages});
        if let Some(sys) = system {
            req["system"] = json!(sys);
        }
        if !tools_json.is_empty() {
            req["tools"] = json!(tools_json);
        }
        req
    }

    /// Lower IR conversation to a Gemini-format JSON request.
    fn lower_to_gemini(conv: &IrConversation, tools: &[IrToolDefinition]) -> serde_json::Value {
        let system = conv.system_message().map(|m| m.text_content());
        let contents: Vec<serde_json::Value> = conv
            .messages
            .iter()
            .filter(|m| m.role != IrRole::System)
            .map(|m| {
                let role = match m.role {
                    IrRole::User | IrRole::Tool => "user",
                    IrRole::Assistant => "model",
                    IrRole::System => unreachable!(),
                };
                let parts: Vec<serde_json::Value> = m
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        IrContentBlock::Text { text } => Some(json!({"text": text})),
                        _ => None,
                    })
                    .collect();
                json!({"role": role, "parts": parts})
            })
            .collect();
        let tools_json: Vec<serde_json::Value> = if !tools.is_empty() {
            let decls: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    })
                })
                .collect();
            vec![json!({"function_declarations": decls})]
        } else {
            vec![]
        };
        let mut req = json!({"contents": contents});
        if let Some(sys) = system {
            req["system_instruction"] = json!({"parts": [{"text": sys}]});
        }
        if !tools_json.is_empty() {
            req["tools"] = json!(tools_json);
        }
        req
    }

    /// Lift an OpenAI-format JSON back to IR.
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

    #[test]
    fn lower_ir_to_openai_format() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "You are helpful."))
            .push(IrMessage::text(IrRole::User, "Hello"));
        let tools = vec![IrToolDefinition {
            name: "calc".into(),
            description: "Calculator".into(),
            parameters: json!({"type": "object"}),
        }];

        let lowered = lower_to_openai(&conv, &tools);
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2, "should have 2 messages");
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "You are helpful.");
        assert_eq!(msgs[1]["role"], "user");

        let tools_out = lowered["tools"].as_array().unwrap();
        assert_eq!(tools_out.len(), 1);
        assert_eq!(tools_out[0]["function"]["name"], "calc");
    }

    #[test]
    fn lower_ir_to_claude_format() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Be brief."))
            .push(IrMessage::text(IrRole::User, "Hi"));
        let tools = vec![IrToolDefinition {
            name: "search".into(),
            description: "Search things".into(),
            parameters: json!({"type": "object"}),
        }];

        let lowered = lower_to_claude(&conv, &tools);
        assert_eq!(
            lowered["system"], "Be brief.",
            "system should be top-level in Claude format"
        );
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(
            msgs.len(),
            1,
            "system message should not appear in messages array"
        );
        assert_eq!(msgs[0]["role"], "user");

        let tools_out = lowered["tools"].as_array().unwrap();
        assert_eq!(tools_out[0]["name"], "search");
        assert!(
            tools_out[0].get("input_schema").is_some(),
            "Claude uses input_schema not parameters"
        );
    }

    #[test]
    fn lower_ir_to_gemini_format() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "Be concise."))
            .push(IrMessage::text(IrRole::User, "Explain Rust."))
            .push(IrMessage::text(
                IrRole::Assistant,
                "Rust is a systems language.",
            ));

        let lowered = lower_to_gemini(&conv, &[]);
        assert_eq!(
            lowered["system_instruction"]["parts"][0]["text"], "Be concise.",
            "system should be in system_instruction"
        );
        let contents = lowered["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 2, "system excluded from contents");
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[1]["role"], "model", "assistant maps to 'model'");
    }

    #[test]
    fn lowering_roundtrip_openai() {
        let original_conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "You are helpful."))
            .push(IrMessage::text(IrRole::User, "What is 1+1?"))
            .push(IrMessage::text(IrRole::Assistant, "2"));
        let original_tools = vec![IrToolDefinition {
            name: "calc".into(),
            description: "Math evaluator".into(),
            parameters: json!({"type": "object", "properties": {"expr": {"type": "string"}}}),
        }];

        // IR → OpenAI → IR
        let openai_json = lower_to_openai(&original_conv, &original_tools);
        let (restored_conv, restored_tools) = lift_from_openai(&openai_json);

        assert_eq!(
            original_conv.len(),
            restored_conv.len(),
            "roundtrip should preserve message count"
        );
        for (orig, back) in original_conv
            .messages
            .iter()
            .zip(restored_conv.messages.iter())
        {
            assert_eq!(orig.role, back.role, "roles should match after roundtrip");
            assert_eq!(
                orig.text_content(),
                back.text_content(),
                "text content should match after roundtrip"
            );
        }
        assert_eq!(
            original_tools.len(),
            restored_tools.len(),
            "tool count should match"
        );
        assert_eq!(original_tools[0].name, restored_tools[0].name);
    }

    #[test]
    fn lowering_preserves_tool_call_structure() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "Search for cats"))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "I'll search for that.".into(),
                    },
                    IrContentBlock::ToolUse {
                        id: "tu_001".into(),
                        name: "search".into(),
                        input: json!({"query": "cats"}),
                    },
                ],
            ))
            .push(IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "tu_001".into(),
                    content: vec![IrContentBlock::Text {
                        text: "Found 42 results about cats.".into(),
                    }],
                    is_error: false,
                }],
            ));

        // Lower to Claude format (which preserves content blocks)
        let lowered = lower_to_claude(&conv, &[]);
        let msgs = lowered["messages"].as_array().unwrap();

        // Assistant message should have tool_use block
        let assistant_content = msgs[1]["content"].as_array().unwrap();
        assert_eq!(
            assistant_content.len(),
            2,
            "assistant should have text + tool_use"
        );
        assert_eq!(assistant_content[1]["type"], "tool_use");
        assert_eq!(assistant_content[1]["id"], "tu_001");
        assert_eq!(assistant_content[1]["name"], "search");

        // Tool result message
        let tool_content = msgs[2]["content"].as_array().unwrap();
        assert_eq!(tool_content[0]["type"], "tool_result");
        assert_eq!(tool_content[0]["tool_use_id"], "tu_001");
    }

    #[test]
    fn lowering_handles_missing_optional_fields() {
        // No system message, no tools
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "Hi"));

        let openai = lower_to_openai(&conv, &[]);
        assert!(
            openai.get("tools").is_none(),
            "tools field should be absent when empty"
        );

        let claude = lower_to_claude(&conv, &[]);
        assert!(
            claude.get("system").is_none(),
            "system field should be absent when no system message"
        );
        assert!(claude.get("tools").is_none());

        let gemini = lower_to_gemini(&conv, &[]);
        assert!(gemini.get("system_instruction").is_none());
        assert!(gemini.get("tools").is_none());
    }

    #[test]
    fn lowering_openai_no_tools_omits_field() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "just chatting"));

        let lowered = lower_to_openai(&conv, &[]);
        assert!(
            lowered.get("tools").is_none(),
            "tools key should not be present when tool list is empty"
        );
    }

    #[test]
    fn lowering_multiple_tools_preserved() {
        let tools = vec![
            IrToolDefinition {
                name: "tool_a".into(),
                description: "A".into(),
                parameters: json!({}),
            },
            IrToolDefinition {
                name: "tool_b".into(),
                description: "B".into(),
                parameters: json!({}),
            },
            IrToolDefinition {
                name: "tool_c".into(),
                description: "C".into(),
                parameters: json!({}),
            },
        ];
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));

        let openai = lower_to_openai(&conv, &tools);
        assert_eq!(
            openai["tools"].as_array().unwrap().len(),
            3,
            "all tools should be present in lowered output"
        );

        let claude = lower_to_claude(&conv, &tools);
        assert_eq!(claude["tools"].as_array().unwrap().len(), 3);

        let gemini = lower_to_gemini(&conv, &tools);
        let decls = gemini["tools"][0]["function_declarations"]
            .as_array()
            .unwrap();
        assert_eq!(decls.len(), 3, "Gemini nests declarations in a wrapper");
    }

    #[test]
    fn lowering_gemini_assistant_role_maps_to_model() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::User, "question"))
            .push(IrMessage::text(IrRole::Assistant, "answer"));

        let gemini = lower_to_gemini(&conv, &[]);
        let contents = gemini["contents"].as_array().unwrap();
        assert_eq!(
            contents[1]["role"], "model",
            "IrRole::Assistant should lower to 'model' in Gemini format"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Module: ir_serde
// ═══════════════════════════════════════════════════════════════════════

mod ir_serde {
    use super::*;

    #[test]
    fn ir_serialization_roundtrip_conversation() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "sys"))
            .push(IrMessage::new(
                IrRole::User,
                vec![
                    IrContentBlock::Text {
                        text: "Look at this image".into(),
                    },
                    IrContentBlock::Image {
                        media_type: "image/png".into(),
                        data: "iVBOR...".into(),
                    },
                ],
            ))
            .push(IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Text {
                        text: "I see an image.".into(),
                    },
                    IrContentBlock::ToolUse {
                        id: "tu_1".into(),
                        name: "analyze_image".into(),
                        input: json!({"detail": "high"}),
                    },
                ],
            ));

        let json = serde_json::to_string(&conv).unwrap();
        let back: IrConversation = serde_json::from_str(&json).unwrap();
        assert_eq!(conv, back, "full conversation should survive roundtrip");
    }

    #[test]
    fn ir_debug_display_output() {
        let msg = IrMessage::text(IrRole::User, "Hello");
        let debug = format!("{msg:?}");
        assert!(
            debug.contains("User"),
            "Debug output should contain role name"
        );
        assert!(
            debug.contains("Hello"),
            "Debug output should contain message text"
        );

        let role = IrRole::Assistant;
        let debug_role = format!("{role:?}");
        assert_eq!(debug_role, "Assistant");
    }

    #[test]
    fn ir_clone_eq_behavior() {
        let original = IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "thinking...".into(),
                },
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "read".into(),
                    input: json!({"path": "test.rs"}),
                },
            ],
        );
        let cloned = original.clone();
        assert_eq!(original, cloned, "cloned message should equal original");

        // Modify clone, original should be unaffected
        let mut modified = cloned;
        modified.content.push(IrContentBlock::Text {
            text: "extra".into(),
        });
        assert_ne!(
            original, modified,
            "modified clone should differ from original"
        );
    }

    #[test]
    fn ir_deterministic_serialization_btreemap() {
        let mut meta = BTreeMap::new();
        meta.insert("zebra".to_string(), json!("last"));
        meta.insert("alpha".to_string(), json!("first"));
        meta.insert("middle".to_string(), json!("between"));

        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text {
                text: "test".into(),
            }],
            metadata: meta,
        };

        let json1 = serde_json::to_string(&msg).unwrap();
        let json2 = serde_json::to_string(&msg).unwrap();
        assert_eq!(
            json1, json2,
            "BTreeMap should produce deterministic key ordering"
        );

        // Verify key order in serialized output
        let alpha_pos = json1.find("alpha").expect("alpha key should exist");
        let middle_pos = json1.find("middle").expect("middle key should exist");
        let zebra_pos = json1.find("zebra").expect("zebra key should exist");
        assert!(
            alpha_pos < middle_pos && middle_pos < zebra_pos,
            "BTreeMap keys should appear in alphabetical order: alpha < middle < zebra"
        );
    }

    #[test]
    fn ir_empty_metadata_omitted() {
        let msg = IrMessage::text(IrRole::User, "no meta");
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            !json.contains("metadata"),
            "empty metadata should be skipped via skip_serializing_if"
        );
    }

    #[test]
    fn ir_content_block_type_tag_serialization() {
        let text = IrContentBlock::Text {
            text: "hello".into(),
        };
        let val: serde_json::Value = serde_json::to_value(&text).unwrap();
        assert_eq!(
            val["type"], "text",
            "Text variant should serialize with type=text"
        );

        let tool_use = IrContentBlock::ToolUse {
            id: "t1".into(),
            name: "test".into(),
            input: json!({}),
        };
        let val: serde_json::Value = serde_json::to_value(&tool_use).unwrap();
        assert_eq!(val["type"], "tool_use", "should use snake_case tag");

        let tool_result = IrContentBlock::ToolResult {
            tool_use_id: "t1".into(),
            content: vec![],
            is_error: false,
        };
        let val: serde_json::Value = serde_json::to_value(&tool_result).unwrap();
        assert_eq!(val["type"], "tool_result");

        let thinking = IrContentBlock::Thinking { text: "hmm".into() };
        let val: serde_json::Value = serde_json::to_value(&thinking).unwrap();
        assert_eq!(val["type"], "thinking");

        let image = IrContentBlock::Image {
            media_type: "image/png".into(),
            data: "abc".into(),
        };
        let val: serde_json::Value = serde_json::to_value(&image).unwrap();
        assert_eq!(val["type"], "image");
    }

    #[test]
    fn ir_usage_serde_roundtrip() {
        let usage = IrUsage::with_cache(1000, 500, 200, 100);
        let json = serde_json::to_string(&usage).unwrap();
        let back: IrUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(usage, back, "IrUsage should roundtrip through JSON");
        assert_eq!(
            back.total_tokens,
            back.input_tokens + back.output_tokens,
            "total_tokens invariant should hold after roundtrip"
        );
    }

    #[test]
    fn ir_role_serde_snake_case() {
        assert_eq!(
            serde_json::to_string(&IrRole::System).unwrap(),
            r#""system""#
        );
        assert_eq!(serde_json::to_string(&IrRole::User).unwrap(), r#""user""#);
        assert_eq!(
            serde_json::to_string(&IrRole::Assistant).unwrap(),
            r#""assistant""#
        );
        assert_eq!(serde_json::to_string(&IrRole::Tool).unwrap(), r#""tool""#);
    }
}
