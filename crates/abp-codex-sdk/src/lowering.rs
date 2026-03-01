// SPDX-License-Identifier: MIT OR Apache-2.0
//! Lowering between ABP IR and the OpenAI Codex Responses API format.
//!
//! [`to_ir`] converts a slice of [`CodexResponseItem`]s into an
//! [`IrConversation`], and [`from_ir`] converts an [`IrConversation`] back
//! into Codex response items.  [`input_to_ir`] converts [`CodexInputItem`]s
//! into an [`IrConversation`] for the request path.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrUsage};

use crate::dialect::{
    CodexContentPart, CodexInputItem, CodexResponseItem, CodexUsage, ReasoningSummary,
};

/// Convert a slice of [`CodexInputItem`]s into an [`IrConversation`].
///
/// Each input item is mapped to an [`IrMessage`] with the appropriate role.
#[must_use]
pub fn input_to_ir(items: &[CodexInputItem]) -> IrConversation {
    let messages: Vec<IrMessage> = items.iter().map(input_item_to_ir).collect();
    IrConversation::from_messages(messages)
}

/// Convert a slice of [`CodexResponseItem`]s into an [`IrConversation`].
///
/// Maps response items (message, function_call, function_call_output,
/// reasoning) into the corresponding IR messages and content blocks.
#[must_use]
pub fn to_ir(items: &[CodexResponseItem]) -> IrConversation {
    let messages: Vec<IrMessage> = items.iter().map(response_item_to_ir).collect();
    IrConversation::from_messages(messages)
}

/// Convert an [`IrConversation`] back into a `Vec<CodexResponseItem>`.
///
/// System and user messages are skipped since Codex response items only
/// represent model output.  Assistant messages produce `Message` items,
/// tool-use blocks produce `FunctionCall` items, tool-result messages
/// produce `FunctionCallOutput` items, and thinking blocks produce
/// `Reasoning` items.
#[must_use]
pub fn from_ir(conv: &IrConversation) -> Vec<CodexResponseItem> {
    let mut items = Vec::new();
    for msg in &conv.messages {
        match msg.role {
            IrRole::Assistant => {
                items.extend(assistant_msg_to_items(msg));
            }
            IrRole::Tool => {
                items.extend(tool_msg_to_items(msg));
            }
            // System / user messages have no Codex output representation
            _ => {}
        }
    }
    items
}

/// Convert a [`CodexUsage`] into an [`IrUsage`].
#[must_use]
pub fn usage_to_ir(usage: &CodexUsage) -> IrUsage {
    IrUsage::from_io(usage.input_tokens, usage.output_tokens)
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn map_role_to_ir(role: &str) -> IrRole {
    match role {
        "system" => IrRole::System,
        "assistant" => IrRole::Assistant,
        _ => IrRole::User,
    }
}

fn input_item_to_ir(item: &CodexInputItem) -> IrMessage {
    match item {
        CodexInputItem::Message { role, content } => {
            let ir_role = map_role_to_ir(role);
            if content.is_empty() {
                IrMessage::new(ir_role, Vec::new())
            } else {
                IrMessage::text(ir_role, content.clone())
            }
        }
    }
}

fn response_item_to_ir(item: &CodexResponseItem) -> IrMessage {
    match item {
        CodexResponseItem::Message { role, content } => {
            let ir_role = map_role_to_ir(role);
            let blocks: Vec<IrContentBlock> = content.iter().map(content_part_to_ir).collect();
            IrMessage::new(ir_role, blocks)
        }
        CodexResponseItem::FunctionCall {
            id,
            name,
            arguments,
            ..
        } => {
            let input: serde_json::Value = serde_json::from_str(arguments)
                .unwrap_or(serde_json::Value::String(arguments.clone()));
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input,
                }],
            )
        }
        CodexResponseItem::FunctionCallOutput { call_id, output } => IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: call_id.clone(),
                content: vec![IrContentBlock::Text {
                    text: output.clone(),
                }],
                is_error: false,
            }],
        ),
        CodexResponseItem::Reasoning { summary } => {
            let text = summary
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            IrMessage::new(IrRole::Assistant, vec![IrContentBlock::Thinking { text }])
        }
    }
}

fn content_part_to_ir(part: &CodexContentPart) -> IrContentBlock {
    match part {
        CodexContentPart::OutputText { text } => IrContentBlock::Text { text: text.clone() },
    }
}

fn assistant_msg_to_items(msg: &IrMessage) -> Vec<CodexResponseItem> {
    let mut items = Vec::new();
    let mut text_parts: Vec<CodexContentPart> = Vec::new();

    for block in &msg.content {
        match block {
            IrContentBlock::Text { text } => {
                text_parts.push(CodexContentPart::OutputText { text: text.clone() });
            }
            IrContentBlock::ToolUse { id, name, input } => {
                // Flush accumulated text parts as a message first
                if !text_parts.is_empty() {
                    items.push(CodexResponseItem::Message {
                        role: "assistant".to_string(),
                        content: std::mem::take(&mut text_parts),
                    });
                }
                items.push(CodexResponseItem::FunctionCall {
                    id: id.clone(),
                    call_id: None,
                    name: name.clone(),
                    arguments: serde_json::to_string(input).unwrap_or_default(),
                });
            }
            IrContentBlock::Thinking { text } => {
                // Flush accumulated text parts first
                if !text_parts.is_empty() {
                    items.push(CodexResponseItem::Message {
                        role: "assistant".to_string(),
                        content: std::mem::take(&mut text_parts),
                    });
                }
                items.push(CodexResponseItem::Reasoning {
                    summary: vec![ReasoningSummary { text: text.clone() }],
                });
            }
            // Image and ToolResult blocks are not representable in assistant output
            _ => {}
        }
    }

    // Flush remaining text parts
    if !text_parts.is_empty() {
        items.push(CodexResponseItem::Message {
            role: "assistant".to_string(),
            content: text_parts,
        });
    }

    items
}

fn tool_msg_to_items(msg: &IrMessage) -> Vec<CodexResponseItem> {
    let mut items = Vec::new();
    for block in &msg.content {
        if let IrContentBlock::ToolResult {
            tool_use_id,
            content,
            ..
        } = block
        {
            let text = content
                .iter()
                .filter_map(|b| match b {
                    IrContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");
            items.push(CodexResponseItem::FunctionCallOutput {
                call_id: tool_use_id.clone(),
                output: text,
            });
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Input item conversion ───────────────────────────────────────────

    #[test]
    fn input_user_message_to_ir() {
        let items = vec![CodexInputItem::Message {
            role: "user".into(),
            content: "Hello".into(),
        }];
        let conv = input_to_ir(&items);
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello");
    }

    #[test]
    fn input_system_message_to_ir() {
        let items = vec![CodexInputItem::Message {
            role: "system".into(),
            content: "Be helpful".into(),
        }];
        let conv = input_to_ir(&items);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "Be helpful");
    }

    #[test]
    fn input_empty_content() {
        let items = vec![CodexInputItem::Message {
            role: "user".into(),
            content: String::new(),
        }];
        let conv = input_to_ir(&items);
        assert!(conv.messages[0].content.is_empty());
    }

    // ── Response item: Message ──────────────────────────────────────────

    #[test]
    fn response_message_to_ir() {
        let items = vec![CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "Done!".into(),
            }],
        }];
        let conv = to_ir(&items);
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        assert_eq!(conv.messages[0].text_content(), "Done!");
    }

    #[test]
    fn response_message_roundtrip() {
        let items = vec![CodexResponseItem::Message {
            role: "assistant".into(),
            content: vec![CodexContentPart::OutputText {
                text: "Hello".into(),
            }],
        }];
        let conv = to_ir(&items);
        let back = from_ir(&conv);
        assert_eq!(back.len(), 1);
        match &back[0] {
            CodexResponseItem::Message { role, content } => {
                assert_eq!(role, "assistant");
                assert_eq!(content.len(), 1);
                match &content[0] {
                    CodexContentPart::OutputText { text } => assert_eq!(text, "Hello"),
                }
            }
            other => panic!("expected Message, got {other:?}"),
        }
    }

    // ── Response item: FunctionCall ─────────────────────────────────────

    #[test]
    fn function_call_to_ir() {
        let items = vec![CodexResponseItem::FunctionCall {
            id: "fc_1".into(),
            call_id: None,
            name: "shell".into(),
            arguments: r#"{"command":"ls"}"#.into(),
        }];
        let conv = to_ir(&items);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "fc_1");
                assert_eq!(name, "shell");
                assert_eq!(input, &json!({"command": "ls"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn function_call_roundtrip() {
        let items = vec![CodexResponseItem::FunctionCall {
            id: "fc_42".into(),
            call_id: Some("corr_1".into()),
            name: "read".into(),
            arguments: r#"{"path":"a.rs"}"#.into(),
        }];
        let conv = to_ir(&items);
        let back = from_ir(&conv);
        assert_eq!(back.len(), 1);
        match &back[0] {
            CodexResponseItem::FunctionCall {
                id,
                name,
                arguments,
                ..
            } => {
                assert_eq!(id, "fc_42");
                assert_eq!(name, "read");
                assert!(arguments.contains("a.rs"));
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn malformed_function_arguments_kept_as_string() {
        let items = vec![CodexResponseItem::FunctionCall {
            id: "fc_bad".into(),
            call_id: None,
            name: "foo".into(),
            arguments: "not-json".into(),
        }];
        let conv = to_ir(&items);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { input, .. } => {
                assert_eq!(input, &serde_json::Value::String("not-json".into()));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    // ── Response item: FunctionCallOutput ────────────────────────────────

    #[test]
    fn function_call_output_to_ir() {
        let items = vec![CodexResponseItem::FunctionCallOutput {
            call_id: "fc_1".into(),
            output: "file contents".into(),
        }];
        let conv = to_ir(&items);
        assert_eq!(conv.messages[0].role, IrRole::Tool);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "fc_1");
                assert!(!is_error);
                assert_eq!(content.len(), 1);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn function_call_output_roundtrip() {
        let items = vec![CodexResponseItem::FunctionCallOutput {
            call_id: "fc_99".into(),
            output: "ok".into(),
        }];
        let conv = to_ir(&items);
        let back = from_ir(&conv);
        assert_eq!(back.len(), 1);
        match &back[0] {
            CodexResponseItem::FunctionCallOutput { call_id, output } => {
                assert_eq!(call_id, "fc_99");
                assert_eq!(output, "ok");
            }
            other => panic!("expected FunctionCallOutput, got {other:?}"),
        }
    }

    // ── Response item: Reasoning ────────────────────────────────────────

    #[test]
    fn reasoning_to_ir() {
        let items = vec![CodexResponseItem::Reasoning {
            summary: vec![
                ReasoningSummary {
                    text: "Step 1".into(),
                },
                ReasoningSummary {
                    text: "Step 2".into(),
                },
            ],
        }];
        let conv = to_ir(&items);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        match &conv.messages[0].content[0] {
            IrContentBlock::Thinking { text } => {
                assert!(text.contains("Step 1"));
                assert!(text.contains("Step 2"));
            }
            other => panic!("expected Thinking, got {other:?}"),
        }
    }

    #[test]
    fn reasoning_roundtrip() {
        let items = vec![CodexResponseItem::Reasoning {
            summary: vec![ReasoningSummary {
                text: "thinking...".into(),
            }],
        }];
        let conv = to_ir(&items);
        let back = from_ir(&conv);
        assert_eq!(back.len(), 1);
        match &back[0] {
            CodexResponseItem::Reasoning { summary } => {
                assert_eq!(summary.len(), 1);
                assert_eq!(summary[0].text, "thinking...");
            }
            other => panic!("expected Reasoning, got {other:?}"),
        }
    }

    // ── Multi-item conversations ────────────────────────────────────────

    #[test]
    fn multi_item_response() {
        let items = vec![
            CodexResponseItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "Let me run that.".into(),
                }],
            },
            CodexResponseItem::FunctionCall {
                id: "fc_1".into(),
                call_id: None,
                name: "shell".into(),
                arguments: r#"{"cmd":"ls"}"#.into(),
            },
            CodexResponseItem::FunctionCallOutput {
                call_id: "fc_1".into(),
                output: "file.txt".into(),
            },
            CodexResponseItem::Message {
                role: "assistant".into(),
                content: vec![CodexContentPart::OutputText {
                    text: "Found file.txt".into(),
                }],
            },
        ];
        let conv = to_ir(&items);
        assert_eq!(conv.len(), 4);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        assert_eq!(conv.messages[1].role, IrRole::Assistant);
        assert_eq!(conv.messages[2].role, IrRole::Tool);
        assert_eq!(conv.messages[3].role, IrRole::Assistant);
    }

    #[test]
    fn multi_item_roundtrip() {
        let items = vec![
            CodexResponseItem::FunctionCall {
                id: "fc_a".into(),
                call_id: None,
                name: "read".into(),
                arguments: r#"{"p":"x"}"#.into(),
            },
            CodexResponseItem::FunctionCallOutput {
                call_id: "fc_a".into(),
                output: "data".into(),
            },
        ];
        let conv = to_ir(&items);
        let back = from_ir(&conv);
        assert_eq!(back.len(), 2);
        assert!(matches!(&back[0], CodexResponseItem::FunctionCall { .. }));
        assert!(matches!(
            &back[1],
            CodexResponseItem::FunctionCallOutput { .. }
        ));
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn empty_items() {
        let conv = to_ir(&[]);
        assert!(conv.is_empty());
        let back = from_ir(&conv);
        assert!(back.is_empty());
    }

    #[test]
    fn system_and_user_messages_skipped_in_from_ir() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "instructions"),
            IrMessage::text(IrRole::User, "hello"),
            IrMessage::text(IrRole::Assistant, "hi"),
        ]);
        let back = from_ir(&conv);
        assert_eq!(back.len(), 1);
        match &back[0] {
            CodexResponseItem::Message { role, .. } => assert_eq!(role, "assistant"),
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn usage_to_ir_computes_total() {
        let usage = CodexUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
        };
        let ir = usage_to_ir(&usage);
        assert_eq!(ir.input_tokens, 100);
        assert_eq!(ir.output_tokens, 50);
        assert_eq!(ir.total_tokens, 150);
    }

    #[test]
    fn assistant_with_text_and_tool_use() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![
                IrContentBlock::Text {
                    text: "Checking...".into(),
                },
                IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "search".into(),
                    input: json!({"q": "rust"}),
                },
            ],
        )]);
        let items = from_ir(&conv);
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[0], CodexResponseItem::Message { .. }));
        assert!(matches!(&items[1], CodexResponseItem::FunctionCall { .. }));
    }
}
