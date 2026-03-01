// SPDX-License-Identifier: MIT OR Apache-2.0
//! Lowering between ABP IR and the GitHub Copilot message format.
//!
//! [`to_ir`] converts a slice of [`CopilotMessage`]s into an
//! [`IrConversation`], and [`from_ir`] converts an [`IrConversation`] back
//! into Copilot messages.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use std::collections::BTreeMap;

use crate::dialect::{CopilotMessage, CopilotReference};

/// Convert a slice of [`CopilotMessage`]s into an [`IrConversation`].
///
/// Maps Copilot roles (`system`, `user`, `assistant`) to IR roles.
/// Copilot references attached to messages are preserved as metadata
/// in the IR message's `metadata` map under the key `"copilot_references"`.
#[must_use]
pub fn to_ir(messages: &[CopilotMessage]) -> IrConversation {
    let ir_messages: Vec<IrMessage> = messages.iter().map(message_to_ir).collect();
    IrConversation::from_messages(ir_messages)
}

/// Convert an [`IrConversation`] back into a `Vec<CopilotMessage>`.
///
/// IR metadata under the key `"copilot_references"` is deserialized back
/// into [`CopilotReference`] values on the resulting messages.
#[must_use]
pub fn from_ir(conv: &IrConversation) -> Vec<CopilotMessage> {
    conv.messages.iter().map(message_from_ir).collect()
}

/// Extract references from all messages in an [`IrConversation`].
///
/// Collects every `"copilot_references"` metadata entry across messages,
/// which is useful for populating the top-level `references` field on a
/// [`CopilotRequest`](crate::dialect::CopilotRequest).
#[must_use]
pub fn extract_references(conv: &IrConversation) -> Vec<CopilotReference> {
    let mut refs = Vec::new();
    for msg in &conv.messages {
        if let Some(val) = msg.metadata.get("copilot_references")
            && let Ok(r) = serde_json::from_value::<Vec<CopilotReference>>(val.clone())
        {
            refs.extend(r);
        }
    }
    refs
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn map_role_to_ir(role: &str) -> IrRole {
    match role {
        "system" => IrRole::System,
        "assistant" => IrRole::Assistant,
        _ => IrRole::User,
    }
}

fn map_role_from_ir(role: IrRole) -> &'static str {
    match role {
        IrRole::System => "system",
        IrRole::User => "user",
        IrRole::Assistant => "assistant",
        // Copilot doesn't have a dedicated tool role; tool results go as user
        IrRole::Tool => "user",
    }
}

fn message_to_ir(msg: &CopilotMessage) -> IrMessage {
    let role = map_role_to_ir(&msg.role);
    let blocks = if msg.content.is_empty() {
        Vec::new()
    } else {
        vec![IrContentBlock::Text {
            text: msg.content.clone(),
        }]
    };

    let mut metadata = BTreeMap::new();

    // Preserve references as metadata
    if !msg.copilot_references.is_empty()
        && let Ok(val) = serde_json::to_value(&msg.copilot_references)
    {
        metadata.insert("copilot_references".to_string(), val);
    }

    // Preserve display name as metadata
    if let Some(name) = &msg.name {
        metadata.insert(
            "copilot_name".to_string(),
            serde_json::Value::String(name.clone()),
        );
    }

    IrMessage {
        role,
        content: blocks,
        metadata,
    }
}

fn message_from_ir(msg: &IrMessage) -> CopilotMessage {
    let role = map_role_from_ir(msg.role);

    // Collect text content
    let text = msg
        .content
        .iter()
        .filter_map(|b| match b {
            IrContentBlock::Text { text } => Some(text.as_str()),
            IrContentBlock::Thinking { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");

    // Restore references from metadata
    let copilot_references = msg
        .metadata
        .get("copilot_references")
        .and_then(|v| serde_json::from_value::<Vec<CopilotReference>>(v.clone()).ok())
        .unwrap_or_default();

    // Restore display name from metadata
    let name = msg
        .metadata
        .get("copilot_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    CopilotMessage {
        role: role.to_string(),
        content: text,
        name,
        copilot_references,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialect::CopilotReferenceType;
    use serde_json::json;

    // ── Basic text messages ─────────────────────────────────────────────

    #[test]
    fn user_text_roundtrip() {
        let msgs = vec![CopilotMessage {
            role: "user".into(),
            content: "Hello".into(),
            name: None,
            copilot_references: vec![],
        }];
        let conv = to_ir(&msgs);
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, IrRole::User);
        assert_eq!(conv.messages[0].text_content(), "Hello");

        let back = from_ir(&conv);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].role, "user");
        assert_eq!(back[0].content, "Hello");
    }

    #[test]
    fn system_text_roundtrip() {
        let msgs = vec![CopilotMessage {
            role: "system".into(),
            content: "You are helpful.".into(),
            name: None,
            copilot_references: vec![],
        }];
        let conv = to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::System);
        assert_eq!(conv.messages[0].text_content(), "You are helpful.");

        let back = from_ir(&conv);
        assert_eq!(back[0].role, "system");
        assert_eq!(back[0].content, "You are helpful.");
    }

    #[test]
    fn assistant_text_roundtrip() {
        let msgs = vec![CopilotMessage {
            role: "assistant".into(),
            content: "Sure!".into(),
            name: None,
            copilot_references: vec![],
        }];
        let conv = to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::Assistant);
        let back = from_ir(&conv);
        assert_eq!(back[0].role, "assistant");
        assert_eq!(back[0].content, "Sure!");
    }

    // ── References ──────────────────────────────────────────────────────

    #[test]
    fn references_preserved_through_roundtrip() {
        let refs = vec![CopilotReference {
            ref_type: CopilotReferenceType::File,
            id: "file-0".into(),
            data: json!({"path": "src/main.rs"}),
            metadata: None,
        }];
        let msgs = vec![CopilotMessage {
            role: "user".into(),
            content: "Read this file".into(),
            name: None,
            copilot_references: refs.clone(),
        }];
        let conv = to_ir(&msgs);
        assert!(conv.messages[0].metadata.contains_key("copilot_references"));

        let back = from_ir(&conv);
        assert_eq!(back[0].copilot_references.len(), 1);
        assert_eq!(back[0].copilot_references[0].id, "file-0");
        assert_eq!(
            back[0].copilot_references[0].ref_type,
            CopilotReferenceType::File
        );
    }

    #[test]
    fn multiple_references() {
        let refs = vec![
            CopilotReference {
                ref_type: CopilotReferenceType::File,
                id: "file-0".into(),
                data: json!({"path": "a.rs"}),
                metadata: None,
            },
            CopilotReference {
                ref_type: CopilotReferenceType::Repository,
                id: "repo-0".into(),
                data: json!({"owner": "octocat", "name": "hello"}),
                metadata: None,
            },
        ];
        let msgs = vec![CopilotMessage {
            role: "user".into(),
            content: "Check these".into(),
            name: None,
            copilot_references: refs,
        }];
        let conv = to_ir(&msgs);
        let back = from_ir(&conv);
        assert_eq!(back[0].copilot_references.len(), 2);
    }

    #[test]
    fn extract_references_across_messages() {
        let msgs = vec![
            CopilotMessage {
                role: "user".into(),
                content: "msg1".into(),
                name: None,
                copilot_references: vec![CopilotReference {
                    ref_type: CopilotReferenceType::File,
                    id: "f1".into(),
                    data: json!({}),
                    metadata: None,
                }],
            },
            CopilotMessage {
                role: "user".into(),
                content: "msg2".into(),
                name: None,
                copilot_references: vec![CopilotReference {
                    ref_type: CopilotReferenceType::Snippet,
                    id: "s1".into(),
                    data: json!({}),
                    metadata: None,
                }],
            },
        ];
        let conv = to_ir(&msgs);
        let all_refs = extract_references(&conv);
        assert_eq!(all_refs.len(), 2);
    }

    // ── Display name ────────────────────────────────────────────────────

    #[test]
    fn name_preserved_through_roundtrip() {
        let msgs = vec![CopilotMessage {
            role: "user".into(),
            content: "Hi".into(),
            name: Some("alice".into()),
            copilot_references: vec![],
        }];
        let conv = to_ir(&msgs);
        assert_eq!(
            conv.messages[0]
                .metadata
                .get("copilot_name")
                .and_then(|v| v.as_str()),
            Some("alice")
        );

        let back = from_ir(&conv);
        assert_eq!(back[0].name.as_deref(), Some("alice"));
    }

    // ── Multi-turn conversations ────────────────────────────────────────

    #[test]
    fn multi_turn_conversation() {
        let msgs = vec![
            CopilotMessage {
                role: "system".into(),
                content: "Be concise.".into(),
                name: None,
                copilot_references: vec![],
            },
            CopilotMessage {
                role: "user".into(),
                content: "Hi".into(),
                name: None,
                copilot_references: vec![],
            },
            CopilotMessage {
                role: "assistant".into(),
                content: "Hello!".into(),
                name: None,
                copilot_references: vec![],
            },
            CopilotMessage {
                role: "user".into(),
                content: "Bye".into(),
                name: None,
                copilot_references: vec![],
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
        assert_eq!(back[3].content, "Bye");
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
        let msgs = vec![CopilotMessage {
            role: "user".into(),
            content: String::new(),
            name: None,
            copilot_references: vec![],
        }];
        let conv = to_ir(&msgs);
        assert!(conv.messages[0].content.is_empty());
        let back = from_ir(&conv);
        assert!(back[0].content.is_empty());
    }

    #[test]
    fn unknown_role_defaults_to_user() {
        let msgs = vec![CopilotMessage {
            role: "developer".into(),
            content: "hi".into(),
            name: None,
            copilot_references: vec![],
        }];
        let conv = to_ir(&msgs);
        assert_eq!(conv.messages[0].role, IrRole::User);
    }

    #[test]
    fn tool_role_mapped_to_user_on_output() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![IrContentBlock::Text {
                    text: "result".into(),
                }],
                is_error: false,
            }],
        )]);
        let back = from_ir(&conv);
        // Copilot has no tool role; mapped to user
        assert_eq!(back[0].role, "user");
    }

    #[test]
    fn no_references_means_empty_vec() {
        let msgs = vec![CopilotMessage {
            role: "user".into(),
            content: "no refs".into(),
            name: None,
            copilot_references: vec![],
        }];
        let conv = to_ir(&msgs);
        assert!(!conv.messages[0].metadata.contains_key("copilot_references"));
        let back = from_ir(&conv);
        assert!(back[0].copilot_references.is_empty());
    }

    #[test]
    fn snippet_reference_roundtrip() {
        let refs = vec![CopilotReference {
            ref_type: CopilotReferenceType::Snippet,
            id: "snippet-0".into(),
            data: json!({"name": "helper.rs", "content": "fn foo() {}"}),
            metadata: Some({
                let mut m = BTreeMap::new();
                m.insert("label".into(), json!("helper"));
                m
            }),
        }];
        let msgs = vec![CopilotMessage {
            role: "user".into(),
            content: "check snippet".into(),
            name: None,
            copilot_references: refs,
        }];
        let conv = to_ir(&msgs);
        let back = from_ir(&conv);
        assert_eq!(back[0].copilot_references[0].id, "snippet-0");
        assert_eq!(
            back[0].copilot_references[0].ref_type,
            CopilotReferenceType::Snippet
        );
        assert!(back[0].copilot_references[0].metadata.is_some());
    }

    #[test]
    fn thinking_block_becomes_text_in_copilot() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking {
                text: "reasoning...".into(),
            }],
        )]);
        let back = from_ir(&conv);
        assert_eq!(back[0].role, "assistant");
        assert_eq!(back[0].content, "reasoning...");
    }
}
