// SPDX-License-Identifier: MIT OR Apache-2.0

//! Emulation strategies for partially-supported features.
//!
//! When a source conversation uses a feature the target dialect does not
//! natively support, an emulation strategy provides a best-effort
//! approximation rather than a hard failure.
//!
//! Each strategy is a pure function that transforms the IR in-place.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};

/// Describes what emulation was applied during mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmulationNote {
    /// Which feature was emulated.
    pub feature: String,
    /// Human-readable description of the emulation.
    pub description: String,
}

/// Result of applying emulation strategies to a conversation.
#[derive(Debug, Clone)]
pub struct EmulationResult {
    /// The transformed conversation.
    pub conversation: IrConversation,
    /// Notes about which emulations were applied.
    pub notes: Vec<EmulationNote>,
}

/// Convert thinking blocks into text blocks prefixed with `[Thinking]`.
///
/// Use when the target dialect does not support extended-thinking but the
/// caller wants to preserve the reasoning as visible text rather than
/// silently dropping it.
#[must_use]
pub fn emulate_thinking_as_text(ir: &IrConversation) -> EmulationResult {
    let mut applied = false;
    let messages = ir
        .messages
        .iter()
        .map(|msg| {
            let content: Vec<IrContentBlock> = msg
                .content
                .iter()
                .map(|block| match block {
                    IrContentBlock::Thinking { text } => {
                        applied = true;
                        IrContentBlock::Text {
                            text: format!("[Thinking] {text}"),
                        }
                    }
                    other => other.clone(),
                })
                .collect();
            IrMessage {
                role: msg.role,
                content,
                metadata: msg.metadata.clone(),
            }
        })
        .collect();

    let mut notes = Vec::new();
    if applied {
        notes.push(EmulationNote {
            feature: "thinking".into(),
            description: "Thinking blocks converted to [Thinking]-prefixed text".into(),
        });
    }

    EmulationResult {
        conversation: IrConversation::from_messages(messages),
        notes,
    }
}

/// Convert system messages into user messages prefixed with `[System]`.
///
/// Use when the target dialect has no system prompt support (e.g., Codex).
#[must_use]
pub fn emulate_system_as_user(ir: &IrConversation) -> EmulationResult {
    let mut applied = false;
    let messages = ir
        .messages
        .iter()
        .map(|msg| {
            if msg.role == IrRole::System {
                applied = true;
                let text = msg.text_content();
                IrMessage {
                    role: IrRole::User,
                    content: vec![IrContentBlock::Text {
                        text: format!("[System] {text}"),
                    }],
                    metadata: msg.metadata.clone(),
                }
            } else {
                msg.clone()
            }
        })
        .collect();

    let mut notes = Vec::new();
    if applied {
        notes.push(EmulationNote {
            feature: "system_prompt".into(),
            description: "System messages converted to [System]-prefixed user messages".into(),
        });
    }

    EmulationResult {
        conversation: IrConversation::from_messages(messages),
        notes,
    }
}

/// Replace image blocks with a text placeholder.
///
/// Use when the target dialect does not support images (e.g., Codex, Kimi).
#[must_use]
pub fn emulate_images_as_placeholder(ir: &IrConversation) -> EmulationResult {
    let mut applied = false;
    let messages = ir
        .messages
        .iter()
        .map(|msg| {
            let content: Vec<IrContentBlock> = msg
                .content
                .iter()
                .map(|block| match block {
                    IrContentBlock::Image { media_type, .. } => {
                        applied = true;
                        IrContentBlock::Text {
                            text: format!("[Image: {media_type}]"),
                        }
                    }
                    other => other.clone(),
                })
                .collect();
            IrMessage {
                role: msg.role,
                content,
                metadata: msg.metadata.clone(),
            }
        })
        .collect();

    let mut notes = Vec::new();
    if applied {
        notes.push(EmulationNote {
            feature: "images".into(),
            description: "Image blocks replaced with [Image: <type>] text placeholders".into(),
        });
    }

    EmulationResult {
        conversation: IrConversation::from_messages(messages),
        notes,
    }
}

/// Strip thinking blocks entirely (the default lossy strategy).
#[must_use]
pub fn strip_thinking(ir: &IrConversation) -> EmulationResult {
    let mut applied = false;
    let messages = ir
        .messages
        .iter()
        .map(|msg| {
            let had_thinking = msg
                .content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }));
            if had_thinking {
                applied = true;
            }
            let content: Vec<IrContentBlock> = msg
                .content
                .iter()
                .filter(|b| !matches!(b, IrContentBlock::Thinking { .. }))
                .cloned()
                .collect();
            IrMessage {
                role: msg.role,
                content,
                metadata: msg.metadata.clone(),
            }
        })
        .collect();

    let mut notes = Vec::new();
    if applied {
        notes.push(EmulationNote {
            feature: "thinking".into(),
            description: "Thinking blocks silently dropped".into(),
        });
    }

    EmulationResult {
        conversation: IrConversation::from_messages(messages),
        notes,
    }
}

/// Convert tool-result messages from `Tool` role to `User` role.
///
/// Use for dialects that model tool results inside user turns (Claude, Gemini).
#[must_use]
pub fn tool_results_to_user_role(ir: &IrConversation) -> EmulationResult {
    let mut applied = false;
    let messages = ir
        .messages
        .iter()
        .map(|msg| {
            if msg.role == IrRole::Tool {
                applied = true;
                IrMessage {
                    role: IrRole::User,
                    content: msg.content.clone(),
                    metadata: msg.metadata.clone(),
                }
            } else {
                msg.clone()
            }
        })
        .collect();

    let mut notes = Vec::new();
    if applied {
        notes.push(EmulationNote {
            feature: "tool_role".into(),
            description: "Tool-role messages converted to User-role".into(),
        });
    }

    EmulationResult {
        conversation: IrConversation::from_messages(messages),
        notes,
    }
}

/// Split user messages containing `ToolResult` blocks into separate `Tool`-role
/// messages (one per result).
///
/// Use for dialects that expect a dedicated tool role (OpenAI, Kimi, Copilot).
#[must_use]
pub fn user_tool_results_to_tool_role(ir: &IrConversation) -> EmulationResult {
    let mut applied = false;
    let mut messages = Vec::with_capacity(ir.messages.len());

    for msg in &ir.messages {
        if msg.role == IrRole::User {
            let (tool_results, other): (Vec<_>, Vec<_>) = msg
                .content
                .iter()
                .cloned()
                .partition(|b| matches!(b, IrContentBlock::ToolResult { .. }));

            if !tool_results.is_empty() {
                applied = true;
                if !other.is_empty() {
                    messages.push(IrMessage {
                        role: IrRole::User,
                        content: other,
                        metadata: msg.metadata.clone(),
                    });
                }
                for block in tool_results {
                    messages.push(IrMessage::new(IrRole::Tool, vec![block]));
                }
            } else {
                messages.push(msg.clone());
            }
        } else {
            messages.push(msg.clone());
        }
    }

    let mut notes = Vec::new();
    if applied {
        notes.push(EmulationNote {
            feature: "tool_role".into(),
            description: "User messages with ToolResult blocks split into Tool-role messages"
                .into(),
        });
    }

    EmulationResult {
        conversation: IrConversation::from_messages(messages),
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn thinking_conv() -> IrConversation {
        IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Solve this"),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Thinking {
                        text: "Let me think...".into(),
                    },
                    IrContentBlock::Text {
                        text: "Answer: 42".into(),
                    },
                ],
            ),
        ])
    }

    #[test]
    fn thinking_as_text_converts() {
        let result = emulate_thinking_as_text(&thinking_conv());
        assert_eq!(result.notes.len(), 1);
        assert_eq!(result.notes[0].feature, "thinking");
        let asst = &result.conversation.messages[1];
        assert_eq!(asst.content.len(), 2);
        assert!(
            matches!(&asst.content[0], IrContentBlock::Text { text } if text.starts_with("[Thinking]"))
        );
    }

    #[test]
    fn thinking_as_text_noop_when_absent() {
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hello")]);
        let result = emulate_thinking_as_text(&conv);
        assert!(result.notes.is_empty());
        assert_eq!(result.conversation, conv);
    }

    #[test]
    fn system_as_user_converts() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be helpful."),
            IrMessage::text(IrRole::User, "Hi"),
        ]);
        let result = emulate_system_as_user(&conv);
        assert_eq!(result.notes.len(), 1);
        assert_eq!(result.conversation.messages[0].role, IrRole::User);
        assert!(
            result.conversation.messages[0]
                .text_content()
                .starts_with("[System]")
        );
    }

    #[test]
    fn system_as_user_noop_when_absent() {
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hello")]);
        let result = emulate_system_as_user(&conv);
        assert!(result.notes.is_empty());
    }

    #[test]
    fn images_as_placeholder_replaces() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text {
                    text: "What is this?".into(),
                },
                IrContentBlock::Image {
                    media_type: "image/png".into(),
                    data: "base64data".into(),
                },
            ],
        )]);
        let result = emulate_images_as_placeholder(&conv);
        assert_eq!(result.notes.len(), 1);
        let user = &result.conversation.messages[0];
        assert_eq!(user.content.len(), 2);
        assert!(
            matches!(&user.content[1], IrContentBlock::Text { text } if text == "[Image: image/png]")
        );
    }

    #[test]
    fn strip_thinking_removes_blocks() {
        let result = strip_thinking(&thinking_conv());
        assert_eq!(result.notes.len(), 1);
        let asst = &result.conversation.messages[1];
        assert_eq!(asst.content.len(), 1);
        assert!(
            !asst
                .content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Thinking { .. }))
        );
    }

    #[test]
    fn tool_results_to_user_role_converts() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: vec![IrContentBlock::Text {
                    text: "result".into(),
                }],
                is_error: false,
            }],
        )]);
        let result = tool_results_to_user_role(&conv);
        assert_eq!(result.notes.len(), 1);
        assert_eq!(result.conversation.messages[0].role, IrRole::User);
    }

    #[test]
    fn user_tool_results_to_tool_role_splits() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: vec![IrContentBlock::Text { text: "r1".into() }],
                    is_error: false,
                },
                IrContentBlock::ToolResult {
                    tool_use_id: "t2".into(),
                    content: vec![IrContentBlock::Text { text: "r2".into() }],
                    is_error: false,
                },
            ],
        )]);
        let result = user_tool_results_to_tool_role(&conv);
        assert_eq!(result.notes.len(), 1);
        assert_eq!(result.conversation.messages.len(), 2);
        assert_eq!(result.conversation.messages[0].role, IrRole::Tool);
        assert_eq!(result.conversation.messages[1].role, IrRole::Tool);
    }

    #[test]
    fn user_tool_results_mixed_content() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::User,
            vec![
                IrContentBlock::Text {
                    text: "Here's the result".into(),
                },
                IrContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: vec![IrContentBlock::Text { text: "r1".into() }],
                    is_error: false,
                },
            ],
        )]);
        let result = user_tool_results_to_tool_role(&conv);
        assert_eq!(result.conversation.messages.len(), 2);
        assert_eq!(result.conversation.messages[0].role, IrRole::User);
        assert_eq!(result.conversation.messages[1].role, IrRole::Tool);
    }

    #[test]
    fn images_as_placeholder_noop_when_no_images() {
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hello")]);
        let result = emulate_images_as_placeholder(&conv);
        assert!(result.notes.is_empty());
    }

    #[test]
    fn strip_thinking_noop_when_none() {
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hello")]);
        let result = strip_thinking(&conv);
        assert!(result.notes.is_empty());
    }

    #[test]
    fn emulation_note_fields() {
        let note = EmulationNote {
            feature: "thinking".into(),
            description: "dropped".into(),
        };
        assert_eq!(note.feature, "thinking");
        assert_eq!(note.description, "dropped");
    }

    #[test]
    fn tool_results_to_user_noop_when_no_tool_msgs() {
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
        let result = tool_results_to_user_role(&conv);
        assert!(result.notes.is_empty());
    }

    #[test]
    fn user_tool_results_noop_when_no_results() {
        let conv = IrConversation::from_messages(vec![IrMessage::text(IrRole::User, "hi")]);
        let result = user_tool_results_to_tool_role(&conv);
        assert!(result.notes.is_empty());
    }

    #[test]
    fn emulate_system_preserves_later_messages() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "sys"),
            IrMessage::text(IrRole::User, "hi"),
            IrMessage::text(IrRole::Assistant, "hey"),
        ]);
        let result = emulate_system_as_user(&conv);
        assert_eq!(result.conversation.messages.len(), 3);
        assert_eq!(result.conversation.messages[1].role, IrRole::User);
        assert_eq!(result.conversation.messages[1].text_content(), "hi");
        assert_eq!(result.conversation.messages[2].role, IrRole::Assistant);
    }

    #[test]
    fn chained_emulations() {
        // System + thinking + image all emulated
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be helpful"),
            IrMessage::new(
                IrRole::User,
                vec![
                    IrContentBlock::Text {
                        text: "Look at this".into(),
                    },
                    IrContentBlock::Image {
                        media_type: "image/jpeg".into(),
                        data: "data".into(),
                    },
                ],
            ),
            IrMessage::new(
                IrRole::Assistant,
                vec![
                    IrContentBlock::Thinking { text: "hmm".into() },
                    IrContentBlock::Text {
                        text: "I see".into(),
                    },
                ],
            ),
        ]);

        let r1 = emulate_system_as_user(&conv);
        let r2 = emulate_images_as_placeholder(&r1.conversation);
        let r3 = emulate_thinking_as_text(&r2.conversation);

        // System became user
        assert_eq!(r3.conversation.messages[0].role, IrRole::User);
        // Image became placeholder
        assert!(
            r3.conversation.messages[1]
                .text_content()
                .contains("[Image:")
        );
        // Thinking became text
        assert!(
            r3.conversation.messages[2]
                .text_content()
                .contains("[Thinking]")
        );

        // Collect all notes
        let all_notes: Vec<_> = r1
            .notes
            .iter()
            .chain(r2.notes.iter())
            .chain(r3.notes.iter())
            .collect();
        assert_eq!(all_notes.len(), 3);
    }
}
