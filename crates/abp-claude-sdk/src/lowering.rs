// SPDX-License-Identifier: MIT OR Apache-2.0
//! Lowering between ABP IR and the Anthropic Claude message format.
//!
//! [`to_ir`] converts a slice of [`ClaudeMessage`]s (plus optional system
//! prompt) into an [`IrConversation`], and [`from_ir`] converts an
//! [`IrConversation`] back into Claude messages.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};

use crate::dialect::{ClaudeContentBlock, ClaudeImageSource, ClaudeMessage};

/// Convert a slice of [`ClaudeMessage`]s into an [`IrConversation`].
///
/// An optional `system_prompt` is prepended as a [`IrRole::System`] message.
/// Claude messages have a flat `content` string; if the content parses as a
/// JSON array of [`ClaudeContentBlock`]s it is expanded, otherwise it is
/// treated as plain text.
#[must_use]
pub fn to_ir(messages: &[ClaudeMessage], system_prompt: Option<&str>) -> IrConversation {
    let mut ir_messages = Vec::new();

    if let Some(sys) = system_prompt
        && !sys.is_empty()
    {
        ir_messages.push(IrMessage::text(IrRole::System, sys));
    }

    for msg in messages {
        ir_messages.push(message_to_ir(msg));
    }

    IrConversation::from_messages(ir_messages)
}

/// Convert an [`IrConversation`] back into a `Vec<ClaudeMessage>`.
///
/// System messages are **skipped** — callers should extract the system
/// prompt from the conversation separately via
/// [`IrConversation::system_message`] and pass it as the request-level
/// `system` field.
///
/// Tool-result IR messages are serialised as a JSON array of
/// [`ClaudeContentBlock`]s in the `content` field (role `"user"`),
/// matching the Anthropic Messages API convention.
#[must_use]
pub fn from_ir(conv: &IrConversation) -> Vec<ClaudeMessage> {
    conv.messages
        .iter()
        .filter(|m| m.role != IrRole::System)
        .map(message_from_ir)
        .collect()
}

/// Extract the system prompt text from an [`IrConversation`], if present.
#[must_use]
pub fn extract_system_prompt(conv: &IrConversation) -> Option<String> {
    conv.system_message().map(|m| m.text_content())
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn map_role_to_ir(role: &str) -> IrRole {
    match role {
        "assistant" => IrRole::Assistant,
        _ => IrRole::User,
    }
}

fn map_role_from_ir(role: IrRole) -> &'static str {
    match role {
        IrRole::Assistant => "assistant",
        // Claude uses "user" for both user and tool-result messages
        _ => "user",
    }
}

fn message_to_ir(msg: &ClaudeMessage) -> IrMessage {
    let role = map_role_to_ir(&msg.role);

    // Try parsing content as a JSON array of ClaudeContentBlock
    if let Ok(blocks) = serde_json::from_str::<Vec<ClaudeContentBlock>>(&msg.content) {
        let ir_blocks: Vec<IrContentBlock> = blocks.iter().map(block_to_ir).collect();
        return IrMessage::new(role, ir_blocks);
    }

    // Plain text content
    IrMessage::text(role, &msg.content)
}

fn block_to_ir(block: &ClaudeContentBlock) -> IrContentBlock {
    match block {
        ClaudeContentBlock::Text { text } => IrContentBlock::Text { text: text.clone() },
        ClaudeContentBlock::ToolUse { id, name, input } => IrContentBlock::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        },
        ClaudeContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            let inner = content
                .as_ref()
                .map(|c| vec![IrContentBlock::Text { text: c.clone() }])
                .unwrap_or_default();
            IrContentBlock::ToolResult {
                tool_use_id: tool_use_id.clone(),
                content: inner,
                is_error: is_error.unwrap_or(false),
            }
        }
        ClaudeContentBlock::Thinking { thinking, .. } => IrContentBlock::Thinking {
            text: thinking.clone(),
        },
        ClaudeContentBlock::Image { source } => match source {
            ClaudeImageSource::Base64 { media_type, data } => IrContentBlock::Image {
                media_type: media_type.clone(),
                data: data.clone(),
            },
            ClaudeImageSource::Url { url } => IrContentBlock::Text {
                text: format!("[image: {url}]"),
            },
        },
    }
}

fn message_from_ir(msg: &IrMessage) -> ClaudeMessage {
    let role = map_role_from_ir(msg.role);

    // Check if message contains structured blocks (tool_use, tool_result, images, thinking)
    let has_structured = msg.content.iter().any(|b| {
        matches!(
            b,
            IrContentBlock::ToolUse { .. }
                | IrContentBlock::ToolResult { .. }
                | IrContentBlock::Image { .. }
                | IrContentBlock::Thinking { .. }
        )
    });

    if has_structured {
        let blocks: Vec<ClaudeContentBlock> = msg.content.iter().map(block_from_ir).collect();
        let content = serde_json::to_string(&blocks).unwrap_or_default();
        ClaudeMessage {
            role: role.to_string(),
            content,
        }
    } else {
        // Simple text
        ClaudeMessage {
            role: role.to_string(),
            content: msg.text_content(),
        }
    }
}

fn block_from_ir(block: &IrContentBlock) -> ClaudeContentBlock {
    match block {
        IrContentBlock::Text { text } => ClaudeContentBlock::Text { text: text.clone() },
        IrContentBlock::ToolUse { id, name, input } => ClaudeContentBlock::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        },
        IrContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            let text = content
                .iter()
                .filter_map(|b| match b {
                    IrContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");
            ClaudeContentBlock::ToolResult {
                tool_use_id: tool_use_id.clone(),
                content: if text.is_empty() { None } else { Some(text) },
                is_error: if *is_error { Some(true) } else { None },
            }
        }
        IrContentBlock::Thinking { text } => ClaudeContentBlock::Thinking {
            thinking: text.clone(),
            signature: None,
        },
        IrContentBlock::Image { media_type, data } => ClaudeContentBlock::Image {
            source: ClaudeImageSource::Base64 {
                media_type: media_type.clone(),
                data: data.clone(),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Basic text messages ─────────────────────────────────────────────

    #[test]
    fn user_text_roundtrip() {
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: "Hello".into(),
        }];
        let conv = to_ir(&msgs, None);
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello");

        let back = from_ir(&conv);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, "user");
        assert_eq!(back[0].content, "Hello");
    }

    #[test]
    fn assistant_text_roundtrip() {
        let msgs = vec![ClaudeMessage {
            role: "assistant".into(),
            content: "Sure thing!".into(),
        }];
        let conv = to_ir(&msgs, None);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);

        let back = from_ir(&conv);
        assert_eq!(back[0].role, "assistant");
        assert_eq!(back[0].content, "Sure thing!");
    }

    // ── System prompt ───────────────────────────────────────────────────

    #[test]
    fn system_prompt_to_ir() {
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: "Hi".into(),
        }];
        let conv = to_ir(&msgs, Some("Be helpful"));
        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "Be helpful");
    }

    #[test]
    fn system_prompt_extracted() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "instructions"),
            IrMessage::text(IrRole::User, "hi"),
        ]);
        let sys = extract_system_prompt(&conv);
        assert_eq!(sys.as_deref(), Some("instructions"));
    }

    #[test]
    fn system_messages_skipped_in_from_ir() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "instructions"),
            IrMessage::text(IrRole::User, "hello"),
        ]);
        let back = from_ir(&conv);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, "user");
    }

    // ── Tool use ────────────────────────────────────────────────────────

    #[test]
    fn tool_use_to_ir() {
        let blocks = vec![ClaudeContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "read_file".into(),
            input: json!({"path": "lib.rs"}),
        }];
        let msgs = vec![ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let conv = to_ir(&msgs, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tu_1");
                assert_eq!(name, "read_file");
                assert_eq!(input, &json!({"path": "lib.rs"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn tool_use_roundtrip() {
        let blocks = vec![ClaudeContentBlock::ToolUse {
            id: "tu_42".into(),
            name: "grep".into(),
            input: json!({"pattern": "fn main"}),
        }];
        let msgs = vec![ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let conv = to_ir(&msgs, None);
        let back = from_ir(&conv);
        let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
        match &parsed[0] {
            ClaudeContentBlock::ToolUse { id, name, .. } => {
                assert_eq!(id, "tu_42");
                assert_eq!(name, "grep");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    // ── Tool results ────────────────────────────────────────────────────

    #[test]
    fn tool_result_to_ir() {
        let blocks = vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: Some("file contents".into()),
            is_error: None,
        }];
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let conv = to_ir(&msgs, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "tu_1");
                assert!(!is_error);
                assert_eq!(content.len(), 1);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn tool_result_error_roundtrip() {
        let blocks = vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "tu_err".into(),
            content: Some("not found".into()),
            is_error: Some(true),
        }];
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let conv = to_ir(&msgs, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult { is_error, .. } => assert!(is_error),
            other => panic!("expected ToolResult, got {other:?}"),
        }

        let back = from_ir(&conv);
        let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
        match &parsed[0] {
            ClaudeContentBlock::ToolResult { is_error, .. } => {
                assert_eq!(*is_error, Some(true));
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    // ── Thinking blocks ─────────────────────────────────────────────────

    #[test]
    fn thinking_block_to_ir() {
        let blocks = vec![
            ClaudeContentBlock::Thinking {
                thinking: "Let me reason...".into(),
                signature: Some("sig123".into()),
            },
            ClaudeContentBlock::Text {
                text: "Answer".into(),
            },
        ];
        let msgs = vec![ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let conv = to_ir(&msgs, None);
        assert_eq!(conv.messages[0].content.len(), 2);
        match &conv.messages[0].content[0] {
            IrContentBlock::Thinking { text } => assert_eq!(text, "Let me reason..."),
            other => panic!("expected Thinking, got {other:?}"),
        }
    }

    #[test]
    fn thinking_roundtrip() {
        let blocks = vec![ClaudeContentBlock::Thinking {
            thinking: "hmm".into(),
            signature: None,
        }];
        let msgs = vec![ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let conv = to_ir(&msgs, None);
        let back = from_ir(&conv);
        let parsed: Vec<ClaudeContentBlock> = serde_json::from_str(&back[0].content).unwrap();
        match &parsed[0] {
            ClaudeContentBlock::Thinking { thinking, .. } => assert_eq!(thinking, "hmm"),
            other => panic!("expected Thinking, got {other:?}"),
        }
    }

    // ── Multi-turn conversations ────────────────────────────────────────

    #[test]
    fn multi_turn_conversation() {
        let msgs = vec![
            ClaudeMessage {
                role: "user".into(),
                content: "Hi".into(),
            },
            ClaudeMessage {
                role: "assistant".into(),
                content: "Hello!".into(),
            },
            ClaudeMessage {
                role: "user".into(),
                content: "Bye".into(),
            },
        ];
        let conv = to_ir(&msgs, Some("Be nice"));
        assert_eq!(conv.len(), 4);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[1].role, IrRole::User);
        assert_eq!(conv.messages[2].role, IrRole::Assistant);
        assert_eq!(conv.messages[3].role, IrRole::User);
    }

    #[test]
    fn tool_call_then_result_multi_turn() {
        let tool_use = vec![ClaudeContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "read".into(),
            input: json!({}),
        }];
        let tool_result = vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: Some("data".into()),
            is_error: None,
        }];
        let msgs = vec![
            ClaudeMessage {
                role: "user".into(),
                content: "Do something".into(),
            },
            ClaudeMessage {
                role: "assistant".into(),
                content: serde_json::to_string(&tool_use).unwrap(),
            },
            ClaudeMessage {
                role: "user".into(),
                content: serde_json::to_string(&tool_result).unwrap(),
            },
            ClaudeMessage {
                role: "assistant".into(),
                content: "Done.".into(),
            },
        ];
        let conv = to_ir(&msgs, None);
        assert_eq!(conv.len(), 4);
        assert!(matches!(
            &conv.messages[1].content[0],
            IrContentBlock::ToolUse { .. }
        ));
        assert!(matches!(
            &conv.messages[2].content[0],
            IrContentBlock::ToolResult { .. }
        ));
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn empty_messages() {
        let conv = to_ir(&[], None);
        assert!(conv.is_empty());
        let back = from_ir(&conv);
        assert!(back.is_empty());
    }

    #[test]
    fn empty_system_prompt_skipped() {
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: "hi".into(),
        }];
        let conv = to_ir(&msgs, Some(""));
        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    #[test]
    fn empty_content_string() {
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: String::new(),
        }];
        let conv = to_ir(&msgs, None);
        assert_eq!(conv.messages[0].text_content(), "");
    }

    #[test]
    fn image_block_base64_roundtrip() {
        let blocks = vec![ClaudeContentBlock::Image {
            source: ClaudeImageSource::Base64 {
                media_type: "image/png".into(),
                data: "abc123".into(),
            },
        }];
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let conv = to_ir(&msgs, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::Image { media_type, data } => {
                assert_eq!(media_type, "image/png");
                assert_eq!(data, "abc123");
            }
            other => panic!("expected Image, got {other:?}"),
        }
    }

    #[test]
    fn mixed_content_blocks() {
        let blocks = vec![
            ClaudeContentBlock::Text {
                text: "Here:".into(),
            },
            ClaudeContentBlock::ToolUse {
                id: "t1".into(),
                name: "search".into(),
                input: json!({"q": "rust"}),
            },
        ];
        let msgs = vec![ClaudeMessage {
            role: "assistant".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let conv = to_ir(&msgs, None);
        assert_eq!(conv.messages[0].content.len(), 2);
    }

    #[test]
    fn tool_result_no_content() {
        let blocks = vec![ClaudeContentBlock::ToolResult {
            tool_use_id: "tu_x".into(),
            content: None,
            is_error: None,
        }];
        let msgs = vec![ClaudeMessage {
            role: "user".into(),
            content: serde_json::to_string(&blocks).unwrap(),
        }];
        let conv = to_ir(&msgs, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult { content, .. } => assert!(content.is_empty()),
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }
}
