// SPDX-License-Identifier: MIT OR Apache-2.0
//! Conversation normalization passes for the IR layer.
//!
//! Each function is a pure pass that takes an [`IrConversation`] (by reference)
//! and returns a new, normalized copy.  Passes can be composed into a pipeline
//! via [`normalize`], which applies the full default chain.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use std::collections::BTreeMap;

// ── Individual passes ──────────────────────────────────────────────────

/// Merge all system messages into a single leading system message.
///
/// When multiple system messages appear (e.g. interleaved with user turns),
/// their text is concatenated with newlines and placed at position 0.
/// Non-system messages preserve their relative order.
#[must_use]
pub fn dedup_system(conv: &IrConversation) -> IrConversation {
    let sys_texts: Vec<String> = conv
        .messages
        .iter()
        .filter(|m| m.role == IrRole::System)
        .map(|m| m.text_content())
        .collect();
    let mut out: Vec<IrMessage> = Vec::new();
    if !sys_texts.is_empty() {
        out.push(IrMessage::text(IrRole::System, sys_texts.join("\n")));
    }
    out.extend(
        conv.messages
            .iter()
            .filter(|m| m.role != IrRole::System)
            .cloned(),
    );
    IrConversation::from_messages(out)
}

/// Trim leading/trailing whitespace from every [`IrContentBlock::Text`] block.
///
/// Non-text blocks (tool calls, images, thinking) are left untouched.
#[must_use]
pub fn trim_text(conv: &IrConversation) -> IrConversation {
    let messages = conv
        .messages
        .iter()
        .map(|m| IrMessage {
            role: m.role,
            content: m
                .content
                .iter()
                .map(|b| match b {
                    IrContentBlock::Text { text } => IrContentBlock::Text {
                        text: text.trim().to_string(),
                    },
                    other => other.clone(),
                })
                .collect(),
            metadata: m.metadata.clone(),
        })
        .collect();
    IrConversation::from_messages(messages)
}

/// Remove messages that contain no content blocks.
#[must_use]
pub fn strip_empty(conv: &IrConversation) -> IrConversation {
    IrConversation::from_messages(
        conv.messages
            .iter()
            .filter(|m| !m.content.is_empty())
            .cloned()
            .collect(),
    )
}

/// Merge adjacent [`IrContentBlock::Text`] blocks within each message.
///
/// Text blocks separated by a non-text block are **not** merged.
#[must_use]
pub fn merge_adjacent_text(conv: &IrConversation) -> IrConversation {
    let messages = conv
        .messages
        .iter()
        .map(|m| {
            let mut merged: Vec<IrContentBlock> = Vec::new();
            for block in &m.content {
                if let IrContentBlock::Text { text } = block {
                    if let Some(IrContentBlock::Text { text: prev }) = merged.last_mut() {
                        prev.push_str(text);
                        continue;
                    }
                }
                merged.push(block.clone());
            }
            IrMessage {
                role: m.role,
                content: merged,
                metadata: m.metadata.clone(),
            }
        })
        .collect();
    IrConversation::from_messages(messages)
}

/// Strip vendor-specific metadata keys from every message.
///
/// Only keys in the `keep` set are retained.  If `keep` is empty **all**
/// metadata is removed.
#[must_use]
pub fn strip_metadata(conv: &IrConversation, keep: &[&str]) -> IrConversation {
    let messages = conv
        .messages
        .iter()
        .map(|m| {
            let metadata = if keep.is_empty() {
                BTreeMap::new()
            } else {
                m.metadata
                    .iter()
                    .filter(|(k, _)| keep.contains(&k.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            };
            IrMessage {
                role: m.role,
                content: m.content.clone(),
                metadata,
            }
        })
        .collect();
    IrConversation::from_messages(messages)
}

/// Extract the system message from any position and return it separately.
///
/// Returns `(Option<String>, IrConversation)` where the first element is
/// the merged system text (if any) and the second is the conversation with
/// all system messages removed.  This matches Claude's API shape where
/// system is a top-level field rather than an inline message.
#[must_use]
pub fn extract_system(conv: &IrConversation) -> (Option<String>, IrConversation) {
    let sys_texts: Vec<String> = conv
        .messages
        .iter()
        .filter(|m| m.role == IrRole::System)
        .map(|m| m.text_content())
        .filter(|t| !t.is_empty())
        .collect();

    let system = if sys_texts.is_empty() {
        None
    } else {
        Some(sys_texts.join("\n"))
    };

    let remaining = IrConversation::from_messages(
        conv.messages
            .iter()
            .filter(|m| m.role != IrRole::System)
            .cloned()
            .collect(),
    );

    (system, remaining)
}

/// Map a vendor-specific role string to the canonical [`IrRole`].
///
/// Handles the common aliases used by different vendors:
/// - `"model"` (Gemini) → [`IrRole::Assistant`]
/// - `"function"` (legacy OpenAI) → [`IrRole::Tool`]
/// - `"developer"` (OpenAI o-series) → [`IrRole::System`]
///
/// Returns `None` if the role string is unrecognised.
#[must_use]
pub fn normalize_role(role: &str) -> Option<IrRole> {
    match role {
        "system" | "developer" => Some(IrRole::System),
        "user" | "human" => Some(IrRole::User),
        "assistant" | "model" | "bot" => Some(IrRole::Assistant),
        "tool" | "function" => Some(IrRole::Tool),
        _ => None,
    }
}

/// Sort tool definitions by name for deterministic output.
pub fn sort_tools(tools: &mut [IrToolDefinition]) {
    tools.sort_by(|a, b| a.name.cmp(&b.name));
}

/// Ensure tool definition parameter schemas have `"type": "object"` at root.
///
/// Some vendors omit the top-level `type` field; this pass injects it when
/// missing so downstream consumers can rely on a consistent schema shape.
#[must_use]
pub fn normalize_tool_schemas(tools: &[IrToolDefinition]) -> Vec<IrToolDefinition> {
    tools
        .iter()
        .map(|t| {
            let mut params = t.parameters.clone();
            if let Some(obj) = params.as_object_mut() {
                obj.entry("type").or_insert_with(|| serde_json::json!("object"));
            }
            IrToolDefinition {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: params,
            }
        })
        .collect()
}

// ── Composite pipeline ─────────────────────────────────────────────────

/// Apply the full default normalization pipeline.
///
/// The pipeline order is:
/// 1. [`dedup_system`] — merge scattered system messages
/// 2. [`trim_text`] — strip whitespace from text blocks
/// 3. [`merge_adjacent_text`] — coalesce adjacent text blocks
/// 4. [`strip_empty`] — remove empty messages
#[must_use]
pub fn normalize(conv: &IrConversation) -> IrConversation {
    strip_empty(&merge_adjacent_text(&trim_text(&dedup_system(conv))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_role_known_roles() {
        assert_eq!(normalize_role("system"), Some(IrRole::System));
        assert_eq!(normalize_role("user"), Some(IrRole::User));
        assert_eq!(normalize_role("assistant"), Some(IrRole::Assistant));
        assert_eq!(normalize_role("tool"), Some(IrRole::Tool));
    }

    #[test]
    fn normalize_role_vendor_aliases() {
        assert_eq!(normalize_role("model"), Some(IrRole::Assistant));
        assert_eq!(normalize_role("function"), Some(IrRole::Tool));
        assert_eq!(normalize_role("developer"), Some(IrRole::System));
        assert_eq!(normalize_role("human"), Some(IrRole::User));
        assert_eq!(normalize_role("bot"), Some(IrRole::Assistant));
    }

    #[test]
    fn normalize_role_unknown() {
        assert_eq!(normalize_role("narrator"), None);
        assert_eq!(normalize_role(""), None);
    }

    #[test]
    fn extract_system_returns_merged_text() {
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
    fn extract_system_none_when_absent() {
        let conv = IrConversation::new().push(IrMessage::text(IrRole::User, "hi"));
        let (sys, rest) = extract_system(&conv);
        assert!(sys.is_none());
        assert_eq!(rest.len(), 1);
    }

    #[test]
    fn strip_metadata_keeps_specified_keys() {
        let mut meta = BTreeMap::new();
        meta.insert("source".to_string(), serde_json::json!("test"));
        meta.insert("vendor_id".to_string(), serde_json::json!("abc"));
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text {
                text: "hi".into(),
            }],
            metadata: meta,
        };
        let conv = IrConversation::from_messages(vec![msg]);
        let stripped = strip_metadata(&conv, &["source"]);
        assert_eq!(stripped.messages[0].metadata.len(), 1);
        assert!(stripped.messages[0].metadata.contains_key("source"));
    }

    #[test]
    fn strip_metadata_removes_all_when_empty_keep() {
        let mut meta = BTreeMap::new();
        meta.insert("x".to_string(), serde_json::json!(1));
        let msg = IrMessage {
            role: IrRole::User,
            content: vec![IrContentBlock::Text {
                text: "hi".into(),
            }],
            metadata: meta,
        };
        let conv = IrConversation::from_messages(vec![msg]);
        let stripped = strip_metadata(&conv, &[]);
        assert!(stripped.messages[0].metadata.is_empty());
    }

    #[test]
    fn normalize_tool_schemas_adds_type_object() {
        let tools = vec![IrToolDefinition {
            name: "search".into(),
            description: "Search".into(),
            parameters: serde_json::json!({"properties": {"q": {"type": "string"}}}),
        }];
        let normalized = normalize_tool_schemas(&tools);
        assert_eq!(normalized[0].parameters["type"], "object");
    }

    #[test]
    fn normalize_tool_schemas_preserves_existing_type() {
        let tools = vec![IrToolDefinition {
            name: "search".into(),
            description: "Search".into(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }];
        let normalized = normalize_tool_schemas(&tools);
        assert_eq!(normalized[0].parameters["type"], "object");
    }

    #[test]
    fn full_pipeline_is_idempotent() {
        let conv = IrConversation::new()
            .push(IrMessage::text(IrRole::System, "  sys  "))
            .push(IrMessage::text(IrRole::User, " hi "))
            .push(IrMessage::text(IrRole::System, " extra "));
        let once = normalize(&conv);
        let twice = normalize(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn sort_tools_by_name() {
        let mut tools = vec![
            IrToolDefinition {
                name: "zebra".into(),
                description: "z".into(),
                parameters: serde_json::json!({}),
            },
            IrToolDefinition {
                name: "apple".into(),
                description: "a".into(),
                parameters: serde_json::json!({}),
            },
        ];
        sort_tools(&mut tools);
        assert_eq!(tools[0].name, "apple");
        assert_eq!(tools[1].name, "zebra");
    }
}
