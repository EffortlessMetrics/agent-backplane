// SPDX-License-Identifier: MIT OR Apache-2.0
//! Lowering between ABP IR and the Moonshot Kimi message format.
//!
//! [`to_ir`] converts a slice of [`KimiMessage`]s into an [`IrConversation`],
//! and [`from_ir`] converts an [`IrConversation`] back into Kimi messages.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};

use crate::dialect::{KimiFunctionCall, KimiMessage, KimiToolCall, KimiUsage};

/// Convert a slice of [`KimiMessage`]s into an [`IrConversation`].
///
/// Maps Kimi roles (`system`, `user`, `assistant`, `tool`) to IR roles and
/// translates tool calls and tool-result messages into the corresponding
/// IR content blocks.
#[must_use]
pub fn to_ir(messages: &[KimiMessage]) -> IrConversation {
    let ir_messages: Vec<IrMessage> = messages.iter().map(message_to_ir).collect();
    IrConversation::from_messages(ir_messages)
}

/// Convert an [`IrConversation`] back into a `Vec<KimiMessage>`.
///
/// System, user, and simple assistant messages become text messages.
/// Assistant messages containing [`IrContentBlock::ToolUse`] blocks produce
/// `tool_calls`.  [`IrRole::Tool`] messages produce tool-result messages
/// with a `tool_call_id`.
#[must_use]
pub fn from_ir(conv: &IrConversation) -> Vec<KimiMessage> {
    conv.messages.iter().map(message_from_ir).collect()
}

/// Convert a [`KimiUsage`] into an [`IrUsage`].
#[must_use]
pub fn usage_to_ir(usage: &KimiUsage) -> IrUsage {
    IrUsage::from_io(usage.prompt_tokens, usage.completion_tokens)
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn map_role_to_ir(role: &str) -> IrRole {
    match role {
        "system" => IrRole::System,
        "assistant" => IrRole::Assistant,
        "tool" => IrRole::Tool,
        _ => IrRole::User,
    }
}

fn map_role_from_ir(role: IrRole) -> &'static str {
    match role {
        IrRole::System => "system",
        IrRole::User => "user",
        IrRole::Assistant => "assistant",
        IrRole::Tool => "tool",
    }
}

fn message_to_ir(msg: &KimiMessage) -> IrMessage {
    let role = map_role_to_ir(&msg.role);
    let mut blocks = Vec::new();

    // Text content
    if let Some(text) = &msg.content
        && !text.is_empty()
    {
        blocks.push(IrContentBlock::Text { text: text.clone() });
    }

    // Tool calls (assistant requesting tool invocations)
    if let Some(tool_calls) = &msg.tool_calls {
        for tc in tool_calls {
            let input: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                .unwrap_or(serde_json::Value::String(tc.function.arguments.clone()));
            blocks.push(IrContentBlock::ToolUse {
                id: tc.id.clone(),
                name: tc.function.name.clone(),
                input,
            });
        }
    }

    // Tool result message (role == "tool" with tool_call_id)
    if role == IrRole::Tool
        && let Some(tcid) = &msg.tool_call_id
    {
        let content_blocks = if let Some(text) = &msg.content {
            vec![IrContentBlock::Text { text: text.clone() }]
        } else {
            Vec::new()
        };
        let tool_result = IrContentBlock::ToolResult {
            tool_use_id: tcid.clone(),
            content: content_blocks,
            is_error: false,
        };
        return IrMessage::new(role, vec![tool_result]);
    }

    IrMessage::new(role, blocks)
}

fn message_from_ir(msg: &IrMessage) -> KimiMessage {
    let role = map_role_from_ir(msg.role);

    // Tool result messages
    if msg.role == IrRole::Tool
        && let Some(IrContentBlock::ToolResult {
            tool_use_id,
            content,
            ..
        }) = msg.content.first()
    {
        let text = content
            .iter()
            .filter_map(|b| match b {
                IrContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        return KimiMessage {
            role: role.to_string(),
            content: Some(text),
            tool_call_id: Some(tool_use_id.clone()),
            tool_calls: None,
        };
    }

    // Separate text blocks from tool-use blocks
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for block in &msg.content {
        match block {
            IrContentBlock::Text { text } => text_parts.push(text.as_str()),
            IrContentBlock::ToolUse { id, name, input } => {
                tool_calls.push(KimiToolCall {
                    id: id.clone(),
                    call_type: "function".to_string(),
                    function: KimiFunctionCall {
                        name: name.clone(),
                        arguments: serde_json::to_string(input).unwrap_or_default(),
                    },
                });
            }
            IrContentBlock::Thinking { text } => text_parts.push(text.as_str()),
            _ => {}
        }
    }

    let content = if text_parts.is_empty() {
        None
    } else {
        Some(text_parts.join(""))
    };
    let tool_calls_opt = if tool_calls.is_empty() {
        None
    } else {
        Some(tool_calls)
    };

    KimiMessage {
        role: role.to_string(),
        content,
        tool_call_id: None,
        tool_calls: tool_calls_opt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Basic text messages ─────────────────────────────────────────────

    #[test]
    fn user_text_roundtrip() {
        let msgs = vec![KimiMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_call_id: None,
            tool_calls: None,
        }];
        let conv = to_ir(&msgs);
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello");

        let back = from_ir(&conv);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, "user");
        assert_eq!(back[0].content.as_deref(), Some("Hello"));
    }

    #[test]
    fn system_text_roundtrip() {
        let msgs = vec![KimiMessage {
            role: "system".into(),
            content: Some("You are helpful.".into()),
            tool_call_id: None,
            tool_calls: None,
        }];
        let conv = to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "You are helpful.");

        let back = from_ir(&conv);
        assert_eq!(back[0].role, "system");
        assert_eq!(back[0].content.as_deref(), Some("You are helpful."));
    }

    #[test]
    fn assistant_text_roundtrip() {
        let msgs = vec![KimiMessage {
            role: "assistant".into(),
            content: Some("Sure!".into()),
            tool_call_id: None,
            tool_calls: None,
        }];
        let conv = to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        let back = from_ir(&conv);
        assert_eq!(back[0].role, "assistant");
        assert_eq!(back[0].content.as_deref(), Some("Sure!"));
    }

    // ── Tool calls ──────────────────────────────────────────────────────

    #[test]
    fn assistant_tool_call_to_ir() {
        let msgs = vec![KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: Some(vec![KimiToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "web_search".into(),
                    arguments: r#"{"query":"rust async"}"#.into(),
                },
            }]),
        }];
        let conv = to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "web_search");
                assert_eq!(input, &json!({"query": "rust async"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn assistant_tool_call_roundtrip() {
        let msgs = vec![KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: Some(vec![KimiToolCall {
                id: "call_42".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"rust"}"#.into(),
                },
            }]),
        }];
        let conv = to_ir(&msgs);
        let back = from_ir(&conv);
        assert_eq!(back[0].role, "assistant");
        assert!(back[0].content.is_none());
        let tc = &back[0].tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.id, "call_42");
        assert_eq!(tc.function.name, "search");
    }

    #[test]
    fn assistant_text_and_tool_call() {
        let msgs = vec![KimiMessage {
            role: "assistant".into(),
            content: Some("Let me search.".into()),
            tool_call_id: None,
            tool_calls: Some(vec![KimiToolCall {
                id: "call_7".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "search".into(),
                    arguments: "{}".into(),
                },
            }]),
        }];
        let conv = to_ir(&msgs);
        assert_eq!(conv.messages[0].content.len(), 2);
        let back = from_ir(&conv);
        assert_eq!(back[0].content.as_deref(), Some("Let me search."));
        assert!(back[0].tool_calls.is_some());
    }

    // ── Tool results ────────────────────────────────────────────────────

    #[test]
    fn tool_result_to_ir() {
        let msgs = vec![KimiMessage {
            role: "tool".into(),
            content: Some("search results here".into()),
            tool_call_id: Some("call_1".into()),
            tool_calls: None,
        }];
        let conv = to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::Tool);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "call_1");
                assert!(!is_error);
                assert_eq!(content.len(), 1);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_roundtrip() {
        let msgs = vec![KimiMessage {
            role: "tool".into(),
            content: Some("ok".into()),
            tool_call_id: Some("call_99".into()),
            tool_calls: None,
        }];
        let conv = to_ir(&msgs);
        let back = from_ir(&conv);
        assert_eq!(back[0].role, "tool");
        assert_eq!(back[0].content.as_deref(), Some("ok"));
        assert_eq!(back[0].tool_call_id.as_deref(), Some("call_99"));
    }

    // ── Multi-turn conversations ────────────────────────────────────────

    #[test]
    fn multi_turn_conversation() {
        let msgs = vec![
            KimiMessage {
                role: "system".into(),
                content: Some("Be concise.".into()),
                tool_call_id: None,
                tool_calls: None,
            },
            KimiMessage {
                role: "user".into(),
                content: Some("Hi".into()),
                tool_call_id: None,
                tool_calls: None,
            },
            KimiMessage {
                role: "assistant".into(),
                content: Some("Hello!".into()),
                tool_call_id: None,
                tool_calls: None,
            },
            KimiMessage {
                role: "user".into(),
                content: Some("Bye".into()),
                tool_call_id: None,
                tool_calls: None,
            },
        ];
        let conv = to_ir(&msgs);
        assert_eq!(conv.len(), 4);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[1].role, IrRole::User);
        assert_eq!(conv.messages[2].role, IrRole::Assistant);
        assert_eq!(conv.messages[3].role, IrRole::User);

        let back = from_ir(&conv);
        assert_eq!(back.len(), 4);
        assert_eq!(back[0].role, "system");
        assert_eq!(back[3].content.as_deref(), Some("Bye"));
    }

    #[test]
    fn tool_call_then_result_multi_turn() {
        let msgs = vec![
            KimiMessage {
                role: "user".into(),
                content: Some("Search for rust".into()),
                tool_call_id: None,
                tool_calls: None,
            },
            KimiMessage {
                role: "assistant".into(),
                content: None,
                tool_call_id: None,
                tool_calls: Some(vec![KimiToolCall {
                    id: "c1".into(),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: "web_search".into(),
                        arguments: r#"{"q":"rust"}"#.into(),
                    },
                }]),
            },
            KimiMessage {
                role: "tool".into(),
                content: Some("results here".into()),
                tool_call_id: Some("c1".into()),
                tool_calls: None,
            },
            KimiMessage {
                role: "assistant".into(),
                content: Some("Here are the results.".into()),
                tool_call_id: None,
                tool_calls: None,
            },
        ];
        let conv = to_ir(&msgs);
        assert_eq!(conv.len(), 4);
        assert_eq!(conv.messages[1].role, IrRole::Assistant);
        assert_eq!(conv.messages[2].role, IrRole::Tool);

        let back = from_ir(&conv);
        assert_eq!(back.len(), 4);
        assert_eq!(back[2].tool_call_id.as_deref(), Some("c1"));
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn empty_messages() {
        let conv = to_ir(&[]);
        assert!(conv.is_empty());
        let back = from_ir(&conv);
        assert!(back.is_empty());
    }

    #[test]
    fn none_content() {
        let msgs = vec![KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: None,
        }];
        let conv = to_ir(&msgs);
        assert!(conv.messages[0].content.is_empty());
    }

    #[test]
    fn malformed_tool_arguments_kept_as_string() {
        let msgs = vec![KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: Some(vec![KimiToolCall {
                id: "call_bad".into(),
                call_type: "function".into(),
                function: KimiFunctionCall {
                    name: "foo".into(),
                    arguments: "not-json".into(),
                },
            }]),
        }];
        let conv = to_ir(&msgs);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { input, .. } => {
                assert_eq!(input, &serde_json::Value::String("not-json".into()));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn multiple_tool_calls_in_one_message() {
        let msgs = vec![KimiMessage {
            role: "assistant".into(),
            content: None,
            tool_call_id: None,
            tool_calls: Some(vec![
                KimiToolCall {
                    id: "c1".into(),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: "a".into(),
                        arguments: "{}".into(),
                    },
                },
                KimiToolCall {
                    id: "c2".into(),
                    call_type: "function".into(),
                    function: KimiFunctionCall {
                        name: "b".into(),
                        arguments: "{}".into(),
                    },
                },
            ]),
        }];
        let conv = to_ir(&msgs);
        assert_eq!(conv.messages[0].content.len(), 2);
        let back = from_ir(&conv);
        assert_eq!(back[0].tool_calls.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn unknown_role_defaults_to_user() {
        let msgs = vec![KimiMessage {
            role: "developer".into(),
            content: Some("hi".into()),
            tool_call_id: None,
            tool_calls: None,
        }];
        let conv = to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    #[test]
    fn usage_to_ir_computes_total() {
        let usage = KimiUsage {
            prompt_tokens: 200,
            completion_tokens: 80,
            total_tokens: 280,
        };
        let ir = usage_to_ir(&usage);
        assert_eq!(ir.input_tokens, 200);
        assert_eq!(ir.output_tokens, 80);
        assert_eq!(ir.total_tokens, 280);
    }

    #[test]
    fn tool_result_without_content() {
        let msgs = vec![KimiMessage {
            role: "tool".into(),
            content: None,
            tool_call_id: Some("c1".into()),
            tool_calls: None,
        }];
        let conv = to_ir(&msgs);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult { content, .. } => {
                assert!(content.is_empty());
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }
}
