// SPDX-License-Identifier: MIT OR Apache-2.0
//! Lowering functions that transform normalized IR into vendor-specific formats.
//!
//! Each `lower_to_*` function is a **pure** transformation: it takes an
//! [`IrConversation`] and optional tool definitions, then returns the
//! vendor's request structure as a [`serde_json::Value`].  No I/O, no
//! network calls — just data reshaping.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use abp_sdk_types::Dialect;

// ── Role mapping ───────────────────────────────────────────────────────

/// Map an [`IrRole`] to the dialect-specific role string.
///
/// | IrRole      | OpenAI / Codex / Copilot / Kimi | Claude      | Gemini  |
/// |-------------|----------------------------------|-------------|---------|
/// | System      | `"system"`                       | `"system"`  | `"system"` |
/// | User        | `"user"`                         | `"user"`    | `"user"` |
/// | Assistant   | `"assistant"`                    | `"assistant"`| `"model"` |
/// | Tool        | `"tool"`                         | `"user"`    | `"user"` |
#[must_use]
pub fn ir_role_to_dialect(role: IrRole, dialect: Dialect) -> &'static str {
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

// ── OpenAI-compatible lowering ─────────────────────────────────────────

/// Lower an IR conversation to the **OpenAI Chat Completions** format.
///
/// Produces a JSON object with `"messages"` (and optionally `"tools"`).
/// System messages are kept inline as `role: "system"` messages.
///
/// Tool use blocks in assistant messages are lowered to the `tool_calls`
/// array; tool result messages become `role: "tool"` messages with a
/// `tool_call_id`.
#[must_use]
pub fn lower_to_openai(conv: &IrConversation, tools: &[IrToolDefinition]) -> serde_json::Value {
    lower_to_openai_like(conv, tools, Dialect::OpenAi)
}

/// Lower to an OpenAI-compatible format with a specific dialect for role mapping.
///
/// Used internally by [`lower_to_openai`], [`lower_to_kimi`], [`lower_to_codex`],
/// and [`lower_to_copilot`].
#[must_use]
fn lower_to_openai_like(
    conv: &IrConversation,
    tools: &[IrToolDefinition],
    dialect: Dialect,
) -> serde_json::Value {
    let messages: Vec<serde_json::Value> = conv
        .messages
        .iter()
        .flat_map(|m| lower_openai_message(m, dialect))
        .collect();
    let mut req = serde_json::json!({"messages": messages});
    if !tools.is_empty() {
        let t: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect();
        req["tools"] = serde_json::json!(t);
    }
    req
}

/// Lower a single IR message to one or more OpenAI message JSON values.
///
/// An assistant message with both text and tool_use blocks produces a single
/// message with both `content` and `tool_calls` fields.  A tool-role message
/// with `ToolResult` blocks may expand to multiple messages (one per result).
fn lower_openai_message(msg: &IrMessage, dialect: Dialect) -> Vec<serde_json::Value> {
    let role = ir_role_to_dialect(msg.role, dialect);

    // Collect tool calls from assistant messages
    let tool_calls: Vec<serde_json::Value> = msg
        .content
        .iter()
        .filter_map(|b| match b {
            IrContentBlock::ToolUse { id, name, input } => Some(serde_json::json!({
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": input.to_string(),
                }
            })),
            _ => None,
        })
        .collect();

    // Collect tool results (each becomes its own message)
    let tool_results: Vec<serde_json::Value> = msg
        .content
        .iter()
        .filter_map(|b| match b {
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                ..
            } => {
                let text: String = content
                    .iter()
                    .filter_map(|c| match c {
                        IrContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                Some(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": tool_use_id,
                    "content": text,
                }))
            }
            _ => None,
        })
        .collect();

    if !tool_results.is_empty() {
        return tool_results;
    }

    let text = msg.text_content();

    let mut message = serde_json::json!({"role": role});
    if !text.is_empty() || tool_calls.is_empty() {
        message["content"] = serde_json::json!(text);
    }
    if !tool_calls.is_empty() {
        message["tool_calls"] = serde_json::json!(tool_calls);
    }

    vec![message]
}

// ── Claude lowering ────────────────────────────────────────────────────

/// Lower an IR conversation to the **Anthropic Claude Messages API** format.
///
/// System messages are extracted to a top-level `"system"` string field.
/// Content blocks are lowered to typed objects (`text`, `tool_use`,
/// `tool_result`, `image`, `thinking`).
#[must_use]
pub fn lower_to_claude(conv: &IrConversation, tools: &[IrToolDefinition]) -> serde_json::Value {
    let system = conv.system_message().map(|m| m.text_content());
    let messages: Vec<serde_json::Value> = conv
        .messages
        .iter()
        .filter(|m| m.role != IrRole::System)
        .map(|m| {
            let role = ir_role_to_dialect(m.role, Dialect::Claude);
            let content: Vec<serde_json::Value> = m
                .content
                .iter()
                .map(lower_claude_content_block)
                .collect();
            serde_json::json!({"role": role, "content": content})
        })
        .collect();

    let mut req = serde_json::json!({"messages": messages});
    if let Some(sys) = system {
        req["system"] = serde_json::json!(sys);
    }
    if !tools.is_empty() {
        let t: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                })
            })
            .collect();
        req["tools"] = serde_json::json!(t);
    }
    req
}

/// Lower a single IR content block to Claude's JSON representation.
fn lower_claude_content_block(block: &IrContentBlock) -> serde_json::Value {
    match block {
        IrContentBlock::Text { text } => {
            serde_json::json!({"type": "text", "text": text})
        }
        IrContentBlock::ToolUse { id, name, input } => {
            serde_json::json!({"type": "tool_use", "id": id, "name": name, "input": input})
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
                        serde_json::json!({"type": "text", "text": text})
                    }
                    _ => serde_json::json!({"type": "unknown"}),
                })
                .collect();
            serde_json::json!({
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": inner,
                "is_error": is_error,
            })
        }
        IrContentBlock::Image { media_type, data } => {
            serde_json::json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": media_type,
                    "data": data,
                }
            })
        }
        IrContentBlock::Thinking { text } => {
            serde_json::json!({"type": "thinking", "thinking": text})
        }
    }
}

// ── Gemini lowering ────────────────────────────────────────────────────

/// Lower an IR conversation to the **Google Gemini** `generateContent` format.
///
/// System messages are extracted to `"system_instruction"` with a `parts`
/// array.  Content blocks are lowered to Gemini's `parts` format.
/// Tool calls become `functionCall` parts; tool results become
/// `functionResponse` parts.
#[must_use]
pub fn lower_to_gemini(conv: &IrConversation, tools: &[IrToolDefinition]) -> serde_json::Value {
    let system = conv.system_message().map(|m| m.text_content());
    let contents: Vec<serde_json::Value> = conv
        .messages
        .iter()
        .filter(|m| m.role != IrRole::System)
        .map(|m| {
            let role = ir_role_to_dialect(m.role, Dialect::Gemini);
            let parts: Vec<serde_json::Value> = m
                .content
                .iter()
                .filter_map(lower_gemini_part)
                .collect();
            serde_json::json!({"role": role, "parts": parts})
        })
        .collect();

    let mut req = serde_json::json!({"contents": contents});
    if let Some(sys) = system {
        req["system_instruction"] = serde_json::json!({"parts": [{"text": sys}]});
    }
    if !tools.is_empty() {
        let decls: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                })
            })
            .collect();
        req["tools"] = serde_json::json!([{"function_declarations": decls}]);
    }
    req
}

/// Lower a single IR content block to a Gemini `part`, if representable.
fn lower_gemini_part(block: &IrContentBlock) -> Option<serde_json::Value> {
    match block {
        IrContentBlock::Text { text } => Some(serde_json::json!({"text": text})),
        IrContentBlock::Image { media_type, data } => {
            Some(serde_json::json!({"inline_data": {"mime_type": media_type, "data": data}}))
        }
        IrContentBlock::ToolUse { name, input, .. } => {
            Some(serde_json::json!({"functionCall": {"name": name, "args": input}}))
        }
        IrContentBlock::ToolResult {
            tool_use_id,
            content,
            ..
        } => {
            let text: String = content
                .iter()
                .filter_map(|c| match c {
                    IrContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");
            Some(serde_json::json!({
                "functionResponse": {
                    "name": tool_use_id,
                    "response": {"result": text},
                }
            }))
        }
        // Gemini does not have a thinking block equivalent
        IrContentBlock::Thinking { .. } => None,
    }
}

// ── Convenience aliases ────────────────────────────────────────────────

/// Lower to the **Moonshot Kimi** format (OpenAI-compatible).
#[must_use]
pub fn lower_to_kimi(conv: &IrConversation, tools: &[IrToolDefinition]) -> serde_json::Value {
    lower_to_openai_like(conv, tools, Dialect::Kimi)
}

/// Lower to the **OpenAI Codex** format (OpenAI-compatible).
#[must_use]
pub fn lower_to_codex(conv: &IrConversation, tools: &[IrToolDefinition]) -> serde_json::Value {
    lower_to_openai_like(conv, tools, Dialect::Codex)
}

/// Lower to the **GitHub Copilot** format (OpenAI-compatible).
#[must_use]
pub fn lower_to_copilot(conv: &IrConversation, tools: &[IrToolDefinition]) -> serde_json::Value {
    lower_to_openai_like(conv, tools, Dialect::Copilot)
}

/// Lower an IR conversation to a specific [`Dialect`].
///
/// Dispatches to the appropriate `lower_to_*` function.
#[must_use]
pub fn lower_for_dialect(
    dialect: Dialect,
    conv: &IrConversation,
    tools: &[IrToolDefinition],
) -> serde_json::Value {
    match dialect {
        Dialect::OpenAi => lower_to_openai(conv, tools),
        Dialect::Claude => lower_to_claude(conv, tools),
        Dialect::Gemini => lower_to_gemini(conv, tools),
        Dialect::Kimi => lower_to_kimi(conv, tools),
        Dialect::Codex => lower_to_codex(conv, tools),
        Dialect::Copilot => lower_to_copilot(conv, tools),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::ir::IrMessage;

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
            parameters: serde_json::json!({
                "type": "object",
                "properties": {"expr": {"type": "string"}}
            }),
        }]
    }

    // ── Role mapping ───────────────────────────────────────────────────

    #[test]
    fn openai_roles() {
        assert_eq!(ir_role_to_dialect(IrRole::System, Dialect::OpenAi), "system");
        assert_eq!(ir_role_to_dialect(IrRole::User, Dialect::OpenAi), "user");
        assert_eq!(
            ir_role_to_dialect(IrRole::Assistant, Dialect::OpenAi),
            "assistant"
        );
        assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::OpenAi), "tool");
    }

    #[test]
    fn claude_tool_becomes_user() {
        assert_eq!(ir_role_to_dialect(IrRole::Tool, Dialect::Claude), "user");
    }

    #[test]
    fn gemini_assistant_becomes_model() {
        assert_eq!(
            ir_role_to_dialect(IrRole::Assistant, Dialect::Gemini),
            "model"
        );
    }

    #[test]
    fn all_dialects_have_user_role() {
        for dialect in Dialect::all() {
            assert_eq!(ir_role_to_dialect(IrRole::User, *dialect), "user");
        }
    }

    // ── OpenAI lowering ────────────────────────────────────────────────

    #[test]
    fn openai_format_structure() {
        let lowered = lower_to_openai(&sample_conv(), &sample_tools());
        let msgs = lowered["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["role"], "system");
        assert!(!lowered["tools"].as_array().unwrap().is_empty());
    }

    #[test]
    fn openai_tool_use_produces_tool_calls() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Let me check.".into(),
                },
                IrContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "read_file".into(),
                    input: serde_json::json!({"path": "/tmp/a.rs"}),
                },
            ],
        ));
        let lowered = lower_to_openai(&conv, &[]);
        let msg = &lowered["messages"][0];
        assert_eq!(msg["content"], "Let me check.");
        assert_eq!(msg["tool_calls"][0]["id"], "call_1");
        assert_eq!(msg["tool_calls"][0]["function"]["name"], "read_file");
    }

    #[test]
    fn openai_tool_result_produces_tool_message() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "call_1".into(),
                content: vec![IrContentBlock::Text {
                    text: "file contents".into(),
                }],
                is_error: false,
            }],
        ));
        let lowered = lower_to_openai(&conv, &[]);
        let msg = &lowered["messages"][0];
        assert_eq!(msg["role"], "tool");
        assert_eq!(msg["tool_call_id"], "call_1");
        assert_eq!(msg["content"], "file contents");
    }

    // ── Claude lowering ────────────────────────────────────────────────

    #[test]
    fn claude_system_extraction() {
        let lowered = lower_to_claude(&sample_conv(), &sample_tools());
        assert_eq!(lowered["system"], "You are helpful.");
        let msgs = lowered["messages"].as_array().unwrap();
        assert!(msgs.iter().all(|m| m["role"] != "system"));
    }

    #[test]
    fn claude_tool_schema_uses_input_schema() {
        let lowered = lower_to_claude(&sample_conv(), &sample_tools());
        let tool = &lowered["tools"][0];
        assert!(tool.get("input_schema").is_some());
        assert!(tool.get("parameters").is_none());
    }

    #[test]
    fn claude_content_blocks_typed() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "hi".into(),
                },
                IrContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "search".into(),
                    input: serde_json::json!({"q": "rust"}),
                },
            ],
        ));
        let lowered = lower_to_claude(&conv, &[]);
        let content = lowered["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "tool_use");
    }

    // ── Gemini lowering ────────────────────────────────────────────────

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
    fn gemini_tool_uses_function_declarations() {
        let lowered = lower_to_gemini(&sample_conv(), &sample_tools());
        let decls = lowered["tools"][0]["function_declarations"]
            .as_array()
            .unwrap();
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0]["name"], "calc");
    }

    #[test]
    fn gemini_thinking_blocks_skipped() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Thinking {
                    text: "hmm".into(),
                },
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
    fn gemini_function_call_parts() {
        let conv = IrConversation::new().push(IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "c1".into(),
                name: "search".into(),
                input: serde_json::json!({"q": "rust"}),
            }],
        ));
        let lowered = lower_to_gemini(&conv, &[]);
        let part = &lowered["contents"][0]["parts"][0];
        assert_eq!(part["functionCall"]["name"], "search");
    }

    // ── Dialect dispatch ───────────────────────────────────────────────

    #[test]
    fn all_dialects_produce_valid_json() {
        let conv = sample_conv();
        let tools = sample_tools();
        for dialect in Dialect::all() {
            let lowered = lower_for_dialect(*dialect, &conv, &tools);
            assert!(
                lowered.is_object(),
                "{dialect}: should produce a JSON object"
            );
        }
    }

    #[test]
    fn no_tools_omits_tools_field() {
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
    fn kimi_codex_copilot_match_openai() {
        let conv = sample_conv();
        let tools = sample_tools();
        let openai = lower_to_openai(&conv, &tools);
        assert_eq!(lower_to_kimi(&conv, &tools), openai);
        assert_eq!(lower_to_codex(&conv, &tools), openai);
        assert_eq!(lower_to_copilot(&conv, &tools), openai);
    }
}
