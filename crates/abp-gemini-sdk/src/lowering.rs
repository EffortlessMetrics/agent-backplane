// SPDX-License-Identifier: MIT OR Apache-2.0
//! Lowering between ABP IR and the Google Gemini message format.
//!
//! [`to_ir`] converts a slice of [`GeminiContent`]s into an [`IrConversation`],
//! and [`from_ir`] converts an [`IrConversation`] back into Gemini contents.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};

use crate::dialect::{GeminiContent, GeminiInlineData, GeminiPart};

/// Convert a slice of [`GeminiContent`]s into an [`IrConversation`].
///
/// Maps Gemini roles (`user`, `model`) to IR roles and translates
/// function calls / function responses into the corresponding IR content
/// blocks.  An optional `system_instruction` is prepended as a
/// [`IrRole::System`] message.
#[must_use]
pub fn to_ir(
    contents: &[GeminiContent],
    system_instruction: Option<&GeminiContent>,
) -> IrConversation {
    let mut ir_messages = Vec::new();

    if let Some(sys) = system_instruction {
        let text = sys
            .parts
            .iter()
            .filter_map(|p| match p {
                GeminiPart::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        if !text.is_empty() {
            ir_messages.push(IrMessage::text(IrRole::System, text));
        }
    }

    for content in contents {
        ir_messages.push(content_to_ir(content));
    }

    IrConversation::from_messages(ir_messages)
}

/// Convert an [`IrConversation`] back into a `Vec<GeminiContent>`.
///
/// System messages are **skipped** — callers should extract the system
/// prompt and pass it as the request-level `system_instruction` field.
/// Tool-result IR messages are emitted as `user` role with
/// [`GeminiPart::FunctionResponse`] parts.
#[must_use]
pub fn from_ir(conv: &IrConversation) -> Vec<GeminiContent> {
    conv.messages
        .iter()
        .filter(|m| m.role != IrRole::System)
        .map(content_from_ir)
        .collect()
}

/// Extract the system instruction from an [`IrConversation`] as a [`GeminiContent`].
#[must_use]
pub fn extract_system_instruction(conv: &IrConversation) -> Option<GeminiContent> {
    conv.system_message().map(|m| GeminiContent {
        role: "user".to_string(),
        parts: vec![GeminiPart::Text(m.text_content())],
    })
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn map_role_to_ir(role: &str) -> IrRole {
    match role {
        "model" => IrRole::Assistant,
        _ => IrRole::User,
    }
}

fn map_role_from_ir(role: IrRole) -> &'static str {
    match role {
        IrRole::Assistant => "model",
        // Gemini uses "user" for user, tool-result, and system messages
        _ => "user",
    }
}

fn content_to_ir(content: &GeminiContent) -> IrMessage {
    let role = map_role_to_ir(&content.role);
    let blocks: Vec<IrContentBlock> = content.parts.iter().map(part_to_ir).collect();
    IrMessage::new(role, blocks)
}

fn part_to_ir(part: &GeminiPart) -> IrContentBlock {
    match part {
        GeminiPart::Text(text) => IrContentBlock::Text { text: text.clone() },
        GeminiPart::InlineData(data) => IrContentBlock::Image {
            media_type: data.mime_type.clone(),
            data: data.data.clone(),
        },
        GeminiPart::FunctionCall { name, args } => {
            // Gemini doesn't have per-call IDs; synthesize one from the name
            IrContentBlock::ToolUse {
                id: format!("gemini_{name}"),
                name: name.clone(),
                input: args.clone(),
            }
        }
        GeminiPart::FunctionResponse { name, response } => {
            let content_blocks = match response {
                serde_json::Value::String(s) => vec![IrContentBlock::Text { text: s.clone() }],
                other => vec![IrContentBlock::Text {
                    text: serde_json::to_string(other).unwrap_or_default(),
                }],
            };
            IrContentBlock::ToolResult {
                tool_use_id: format!("gemini_{name}"),
                content: content_blocks,
                is_error: false,
            }
        }
    }
}

fn content_from_ir(msg: &IrMessage) -> GeminiContent {
    let role = map_role_from_ir(msg.role);
    let parts: Vec<GeminiPart> = msg.content.iter().map(part_from_ir).collect();
    GeminiContent {
        role: role.to_string(),
        parts,
    }
}

fn part_from_ir(block: &IrContentBlock) -> GeminiPart {
    match block {
        IrContentBlock::Text { text } => GeminiPart::Text(text.clone()),
        IrContentBlock::Image { media_type, data } => GeminiPart::InlineData(GeminiInlineData {
            mime_type: media_type.clone(),
            data: data.clone(),
        }),
        IrContentBlock::ToolUse { name, input, .. } => GeminiPart::FunctionCall {
            name: name.clone(),
            args: input.clone(),
        },
        IrContentBlock::ToolResult {
            content,
            tool_use_id,
            ..
        } => {
            // Extract the function name from the synthesized id or fall back
            let name = tool_use_id
                .strip_prefix("gemini_")
                .unwrap_or(tool_use_id)
                .to_string();
            let text = content
                .iter()
                .filter_map(|b| match b {
                    IrContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");
            GeminiPart::FunctionResponse {
                name,
                response: serde_json::Value::String(text),
            }
        }
        IrContentBlock::Thinking { text } => GeminiPart::Text(text.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Basic text messages ─────────────────────────────────────────────

    #[test]
    fn user_text_roundtrip() {
        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Hello".into())],
        }];
        let conv = to_ir(&contents, None);
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello");

        let back = from_ir(&conv);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, "user");
        match &back[0].parts[0] {
            GeminiPart::Text(t) => assert_eq!(t, "Hello"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn model_text_roundtrip() {
        let contents = vec![GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::Text("Sure!".into())],
        }];
        let conv = to_ir(&contents, None);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);

        let back = from_ir(&conv);
        assert_eq!(back[0].role, "model");
    }

    // ── System instruction ──────────────────────────────────────────────

    #[test]
    fn system_instruction_to_ir() {
        let sys = GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Be helpful".into())],
        };
        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("Hi".into())],
        }];
        let conv = to_ir(&contents, Some(&sys));
        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "Be helpful");
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

    #[test]
    fn extract_system_instruction_works() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be concise"),
            IrMessage::text(IrRole::User, "hi"),
        ]);
        let sys = extract_system_instruction(&conv).unwrap();
        match &sys.parts[0] {
            GeminiPart::Text(t) => assert_eq!(t, "Be concise"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    // ── Function call ───────────────────────────────────────────────────

    #[test]
    fn function_call_to_ir() {
        let contents = vec![GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "search".into(),
                args: json!({"query": "rust"}),
            }],
        }];
        let conv = to_ir(&contents, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolUse { name, input, .. } => {
                assert_eq!(name, "search");
                assert_eq!(input, &json!({"query": "rust"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn function_call_roundtrip() {
        let contents = vec![GeminiContent {
            role: "model".into(),
            parts: vec![GeminiPart::FunctionCall {
                name: "read".into(),
                args: json!({"file": "a.rs"}),
            }],
        }];
        let conv = to_ir(&contents, None);
        let back = from_ir(&conv);
        match &back[0].parts[0] {
            GeminiPart::FunctionCall { name, args } => {
                assert_eq!(name, "read");
                assert_eq!(args, &json!({"file": "a.rs"}));
            }
            other => panic!("expected FunctionCall, got {other:?}"),
        }
    }

    // ── Function response ───────────────────────────────────────────────

    #[test]
    fn function_response_to_ir() {
        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::FunctionResponse {
                name: "search".into(),
                response: json!("results here"),
            }],
        }];
        let conv = to_ir(&contents, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "gemini_search");
                assert!(!is_error);
                assert_eq!(content.len(), 1);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn function_response_roundtrip() {
        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::FunctionResponse {
                name: "read".into(),
                response: json!("file data"),
            }],
        }];
        let conv = to_ir(&contents, None);
        let back = from_ir(&conv);
        match &back[0].parts[0] {
            GeminiPart::FunctionResponse { name, response } => {
                assert_eq!(name, "read");
                assert_eq!(response, &json!("file data"));
            }
            other => panic!("expected FunctionResponse, got {other:?}"),
        }
    }

    // ── Multi-turn conversations ────────────────────────────────────────

    #[test]
    fn multi_turn_conversation() {
        let contents = vec![
            GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Hi".into())],
            },
            GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("Hello!".into())],
            },
            GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Bye".into())],
            },
        ];
        let conv = to_ir(&contents, None);
        assert_eq!(conv.len(), 3);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[1].role, IrRole::Assistant);
        assert_eq!(conv.messages[2].role, IrRole::User);

        let back = from_ir(&conv);
        assert_eq!(back.len(), 3);
        assert_eq!(back[1].role, "model");
    }

    #[test]
    fn function_call_then_response_multi_turn() {
        let contents = vec![
            GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::Text("Search for rust".into())],
            },
            GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::FunctionCall {
                    name: "search".into(),
                    args: json!({"q": "rust"}),
                }],
            },
            GeminiContent {
                role: "user".into(),
                parts: vec![GeminiPart::FunctionResponse {
                    name: "search".into(),
                    response: json!("results"),
                }],
            },
            GeminiContent {
                role: "model".into(),
                parts: vec![GeminiPart::Text("Here are the results.".into())],
            },
        ];
        let conv = to_ir(&contents, None);
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
    fn empty_contents() {
        let conv = to_ir(&[], None);
        assert!(conv.is_empty());
        let back = from_ir(&conv);
        assert!(back.is_empty());
    }

    #[test]
    fn inline_data_to_ir() {
        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::InlineData(GeminiInlineData {
                mime_type: "image/jpeg".into(),
                data: "base64data".into(),
            })],
        }];
        let conv = to_ir(&contents, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::Image { media_type, data } => {
                assert_eq!(media_type, "image/jpeg");
                assert_eq!(data, "base64data");
            }
            other => panic!("expected Image, got {other:?}"),
        }
    }

    #[test]
    fn inline_data_roundtrip() {
        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::InlineData(GeminiInlineData {
                mime_type: "image/png".into(),
                data: "xyz".into(),
            })],
        }];
        let conv = to_ir(&contents, None);
        let back = from_ir(&conv);
        match &back[0].parts[0] {
            GeminiPart::InlineData(d) => {
                assert_eq!(d.mime_type, "image/png");
                assert_eq!(d.data, "xyz");
            }
            other => panic!("expected InlineData, got {other:?}"),
        }
    }

    #[test]
    fn multiple_parts_in_one_content() {
        let contents = vec![GeminiContent {
            role: "model".into(),
            parts: vec![
                GeminiPart::Text("Let me search.".into()),
                GeminiPart::FunctionCall {
                    name: "search".into(),
                    args: json!({}),
                },
            ],
        }];
        let conv = to_ir(&contents, None);
        assert_eq!(conv.messages[0].content.len(), 2);
    }

    #[test]
    fn empty_system_instruction_not_added() {
        let sys = GeminiContent {
            role: "user".into(),
            parts: vec![],
        };
        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::Text("hi".into())],
        }];
        let conv = to_ir(&contents, Some(&sys));
        // No system message because text is empty
        assert_eq!(conv.len(), 1);
    }

    #[test]
    fn function_response_with_object_payload() {
        let contents = vec![GeminiContent {
            role: "user".into(),
            parts: vec![GeminiPart::FunctionResponse {
                name: "api".into(),
                response: json!({"status": 200, "body": "ok"}),
            }],
        }];
        let conv = to_ir(&contents, None);
        match &conv.messages[0].content[0] {
            IrContentBlock::ToolResult { content, .. } => {
                // Object payloads are serialized as JSON text
                assert_eq!(content.len(), 1);
                let text = match &content[0] {
                    IrContentBlock::Text { text } => text.as_str(),
                    _ => panic!("expected text block"),
                };
                assert!(text.contains("200"));
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }
}
