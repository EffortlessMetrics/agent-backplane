// SPDX-License-Identifier: MIT OR Apache-2.0
//! Lowering between ABP IR and the OpenAI Chat Completions message format.
//!
//! [`to_ir`] converts a slice of [`OpenAIMessage`]s into an [`IrConversation`],
//! and [`from_ir`] converts an [`IrConversation`] back into OpenAI messages.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};

use crate::dialect::{OpenAIFunctionCall, OpenAIMessage, OpenAIToolCall};

/// Convert a slice of [`OpenAIMessage`]s into an [`IrConversation`].
///
/// Maps OpenAI roles (`system`, `user`, `assistant`, `tool`) to IR roles
/// and translates tool calls and tool-result messages into the corresponding
/// IR content blocks.
#[must_use]
pub fn to_ir(messages: &[OpenAIMessage]) -> IrConversation {
    let ir_messages: Vec<IrMessage> = messages.iter().map(message_to_ir).collect();
    IrConversation::from_messages(ir_messages)
}

/// Convert an [`IrConversation`] back into a `Vec<OpenAIMessage>`.
///
/// System, user, and simple assistant messages become text messages.
/// Assistant messages containing [`IrContentBlock::ToolUse`] blocks produce
/// `tool_calls`.  [`IrRole::Tool`] messages produce tool-result messages
/// with a `tool_call_id`.
#[must_use]
pub fn from_ir(conv: &IrConversation) -> Vec<OpenAIMessage> {
    conv.messages.iter().map(message_from_ir).collect()
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

fn message_to_ir(msg: &OpenAIMessage) -> IrMessage {
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
        // Replace the plain text block with a proper ToolResult block
        let tool_result = IrContentBlock::ToolResult {
            tool_use_id: tcid.clone(),
            content: content_blocks,
            is_error: false,
        };
        // For tool role, the only block should be the ToolResult
        return IrMessage::new(role, vec![tool_result]);
    }

    IrMessage::new(role, blocks)
}

fn message_from_ir(msg: &IrMessage) -> OpenAIMessage {
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
        return OpenAIMessage {
            role: role.to_string(),
            content: Some(text),
            tool_calls: None,
            tool_call_id: Some(tool_use_id.clone()),
        };
    }

    // Separate text blocks from tool-use blocks
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for block in &msg.content {
        match block {
            IrContentBlock::Text { text } => text_parts.push(text.as_str()),
            IrContentBlock::ToolUse { id, name, input } => {
                tool_calls.push(OpenAIToolCall {
                    id: id.clone(),
                    call_type: "function".to_string(),
                    function: OpenAIFunctionCall {
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

    OpenAIMessage {
        role: role.to_string(),
        content,
        tool_calls: tool_calls_opt,
        tool_call_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Basic text messages ─────────────────────────────────────────────

    #[test]
    fn user_text_roundtrip() {
        let msgs = vec![OpenAIMessage {
            role: "user".into(),
            content: Some("Hello".into()),
            tool_calls: None,
            tool_call_id: None,
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
        let msgs = vec![OpenAIMessage {
            role: "system".into(),
            content: Some("You are helpful.".into()),
            tool_calls: None,
            tool_call_id: None,
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
        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: Some("Sure!".into()),
            tool_calls: None,
            tool_call_id: None,
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
        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"main.rs"}"#.into(),
                },
            }]),
            tool_call_id: None,
        }];
        let conv = to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "read_file");
                assert_eq!(input, &json!({"path": "main.rs"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn assistant_tool_call_roundtrip() {
        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_42".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "search".into(),
                    arguments: r#"{"q":"rust"}"#.into(),
                },
            }]),
            tool_call_id: None,
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
        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: Some("Let me check.".into()),
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_7".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "ls".into(),
                    arguments: "{}".into(),
                },
            }]),
            tool_call_id: None,
        }];
        let conv = to_ir(&msgs);
        assert_eq!(conv.messages[0].content.len(), 2);
        let back = from_ir(&conv);
        assert_eq!(back[0].content.as_deref(), Some("Let me check."));
        assert!(back[0].tool_calls.is_some());
    }

    // ── Tool results ────────────────────────────────────────────────────

    #[test]
    fn tool_result_to_ir() {
        let msgs = vec![OpenAIMessage {
            role: "tool".into(),
            content: Some("file contents here".into()),
            tool_calls: None,
            tool_call_id: Some("call_1".into()),
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
        let msgs = vec![OpenAIMessage {
            role: "tool".into(),
            content: Some("ok".into()),
            tool_calls: None,
            tool_call_id: Some("call_99".into()),
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
            OpenAIMessage {
                role: "system".into(),
                content: Some("Be concise.".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some("Hi".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "assistant".into(),
                content: Some("Hello!".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some("Bye".into()),
                tool_calls: None,
                tool_call_id: None,
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
            OpenAIMessage {
                role: "user".into(),
                content: Some("Read main.rs".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![OpenAIToolCall {
                    id: "c1".into(),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: "read_file".into(),
                        arguments: r#"{"path":"main.rs"}"#.into(),
                    },
                }]),
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "tool".into(),
                content: Some("fn main() {}".into()),
                tool_calls: None,
                tool_call_id: Some("c1".into()),
            },
            OpenAIMessage {
                role: "assistant".into(),
                content: Some("Done.".into()),
                tool_calls: None,
                tool_call_id: None,
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
    fn empty_content_string() {
        let msgs = vec![OpenAIMessage {
            role: "user".into(),
            content: Some(String::new()),
            tool_calls: None,
            tool_call_id: None,
        }];
        let conv = to_ir(&msgs);
        assert!(conv.messages[0].content.is_empty());
    }

    #[test]
    fn none_content() {
        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: None,
            tool_call_id: None,
        }];
        let conv = to_ir(&msgs);
        assert!(conv.messages[0].content.is_empty());
    }

    #[test]
    fn malformed_tool_arguments_kept_as_string() {
        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAIToolCall {
                id: "call_bad".into(),
                call_type: "function".into(),
                function: OpenAIFunctionCall {
                    name: "foo".into(),
                    arguments: "not-json".into(),
                },
            }]),
            tool_call_id: None,
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
        let msgs = vec![OpenAIMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![
                OpenAIToolCall {
                    id: "c1".into(),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: "a".into(),
                        arguments: "{}".into(),
                    },
                },
                OpenAIToolCall {
                    id: "c2".into(),
                    call_type: "function".into(),
                    function: OpenAIFunctionCall {
                        name: "b".into(),
                        arguments: "{}".into(),
                    },
                },
            ]),
            tool_call_id: None,
        }];
        let conv = to_ir(&msgs);
        assert_eq!(conv.messages[0].content.len(), 2);
        let back = from_ir(&conv);
        assert_eq!(back[0].tool_calls.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn unknown_role_defaults_to_user() {
        let msgs = vec![OpenAIMessage {
            role: "developer".into(),
            content: Some("hi".into()),
            tool_calls: None,
            tool_call_id: None,
        }];
        let conv = to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    #[test]
    fn conversation_system_message_accessor() {
        let msgs = vec![
            OpenAIMessage {
                role: "system".into(),
                content: Some("instructions".into()),
                tool_calls: None,
                tool_call_id: None,
            },
            OpenAIMessage {
                role: "user".into(),
                content: Some("hi".into()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];
        let conv = to_ir(&msgs);
        let sys = conv.system_message().unwrap();
        assert_eq!(sys.text_content(), "instructions");
    }

    #[test]
    fn tool_result_without_content() {
        let msgs = vec![OpenAIMessage {
            role: "tool".into(),
            content: None,
            tool_calls: None,
            tool_call_id: Some("c1".into()),
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
