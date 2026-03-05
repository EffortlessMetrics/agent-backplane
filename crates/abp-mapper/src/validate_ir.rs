// SPDX-License-Identifier: MIT OR Apache-2.0

//! Structural validation of IR conversations before and after mapping.
//!
//! These checks catch common problems that individual mappers might miss:
//! orphaned tool results, empty messages, and conversations that violate
//! dialect-specific structural constraints.

use abp_core::ir::{IrContentBlock, IrConversation, IrRole};

use crate::capabilities::{dialect_capabilities, DialectCapabilities};

/// A single issue found during IR validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrValidationIssue {
    /// Zero-based message index where the issue was found.
    pub message_index: usize,
    /// Machine-readable issue code.
    pub code: &'static str,
    /// Human-readable description.
    pub description: String,
}

/// Result of validating an IR conversation.
#[derive(Debug, Clone)]
pub struct IrValidationResult {
    /// All issues found (empty = valid).
    pub issues: Vec<IrValidationIssue>,
}

impl IrValidationResult {
    /// Returns `true` when no issues were found.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Validate structural correctness of an IR conversation.
///
/// Checks:
/// - No empty messages (messages with zero content blocks)
/// - No orphaned tool results (ToolResult without a prior ToolUse with matching ID)
/// - No ToolUse blocks without matching ToolResult (warning-level, not blocking)
#[must_use]
pub fn validate_ir_structure(ir: &IrConversation) -> IrValidationResult {
    let mut issues = Vec::new();

    // Collect all tool-use IDs
    let mut tool_use_ids: Vec<String> = Vec::new();
    for msg in &ir.messages {
        for block in &msg.content {
            if let IrContentBlock::ToolUse { id, .. } = block {
                tool_use_ids.push(id.clone());
            }
        }
    }

    for (idx, msg) in ir.messages.iter().enumerate() {
        // Empty content check
        if msg.content.is_empty() {
            issues.push(IrValidationIssue {
                message_index: idx,
                code: "empty_message",
                description: format!("message at index {idx} has no content blocks"),
            });
        }

        // Orphaned tool result check
        for block in &msg.content {
            if let IrContentBlock::ToolResult { tool_use_id, .. } = block {
                if !tool_use_ids.contains(tool_use_id) {
                    issues.push(IrValidationIssue {
                        message_index: idx,
                        code: "orphaned_tool_result",
                        description: format!(
                            "ToolResult references tool_use_id '{tool_use_id}' which has no matching ToolUse"
                        ),
                    });
                }
            }
        }
    }

    IrValidationResult { issues }
}

/// Validate that an IR conversation is compatible with a target dialect's
/// capabilities.
///
/// Returns issues for features present in the conversation that the target
/// dialect does not support (e.g., images in a Codex-bound conversation).
#[must_use]
pub fn validate_for_target(
    ir: &IrConversation,
    target: &DialectCapabilities,
) -> IrValidationResult {
    let mut issues = Vec::new();

    for (idx, msg) in ir.messages.iter().enumerate() {
        for block in &msg.content {
            match block {
                IrContentBlock::Thinking { .. } if !target.thinking.is_native() => {
                    issues.push(IrValidationIssue {
                        message_index: idx,
                        code: "unsupported_thinking",
                        description: format!(
                            "thinking block at index {idx} not supported by {}",
                            target.dialect.label()
                        ),
                    });
                }
                IrContentBlock::Image { .. } if !target.images.is_native() => {
                    issues.push(IrValidationIssue {
                        message_index: idx,
                        code: "unsupported_image",
                        description: format!(
                            "image block at index {idx} not supported by {}",
                            target.dialect.label()
                        ),
                    });
                }
                IrContentBlock::Image { .. }
                    if msg.role == IrRole::System && !target.system_images.is_native() =>
                {
                    issues.push(IrValidationIssue {
                        message_index: idx,
                        code: "unsupported_system_image",
                        description: format!(
                            "image in system message at index {idx} not supported by {}",
                            target.dialect.label()
                        ),
                    });
                }
                IrContentBlock::ToolUse { .. } if !target.tool_use.is_native() => {
                    issues.push(IrValidationIssue {
                        message_index: idx,
                        code: "unsupported_tool_use",
                        description: format!(
                            "tool use at index {idx} not supported by {}",
                            target.dialect.label()
                        ),
                    });
                }
                _ => {}
            }
        }

        // System message without system support
        if msg.role == IrRole::System && !target.system_prompt.is_native() {
            issues.push(IrValidationIssue {
                message_index: idx,
                code: "unsupported_system_prompt",
                description: format!(
                    "system message at index {idx} not supported by {}",
                    target.dialect.label()
                ),
            });
        }
    }

    IrValidationResult { issues }
}

/// Convenience: validate structure + target compatibility in one call.
#[must_use]
pub fn validate_ir_for_mapping(
    ir: &IrConversation,
    target: abp_dialect::Dialect,
) -> IrValidationResult {
    let caps = dialect_capabilities(target);
    let mut structural = validate_ir_structure(ir);
    let target_issues = validate_for_target(ir, &caps);
    structural.issues.extend(target_issues.issues);
    structural
}

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
    use abp_dialect::Dialect;
    use serde_json::json;

    #[test]
    fn valid_simple_conversation() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "Be helpful"),
            IrMessage::text(IrRole::User, "Hi"),
            IrMessage::text(IrRole::Assistant, "Hello!"),
        ]);
        let result = validate_ir_structure(&conv);
        assert!(result.is_valid());
    }

    #[test]
    fn empty_message_detected() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::User, "Hi"),
            IrMessage::new(IrRole::Assistant, vec![]),
        ]);
        let result = validate_ir_structure(&conv);
        assert!(!result.is_valid());
        assert_eq!(result.issues[0].code, "empty_message");
        assert_eq!(result.issues[0].message_index, 1);
    }

    #[test]
    fn orphaned_tool_result_detected() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Tool,
            vec![IrContentBlock::ToolResult {
                tool_use_id: "nonexistent".into(),
                content: vec![IrContentBlock::Text {
                    text: "result".into(),
                }],
                is_error: false,
            }],
        )]);
        let result = validate_ir_structure(&conv);
        assert!(!result.is_valid());
        assert_eq!(result.issues[0].code, "orphaned_tool_result");
    }

    #[test]
    fn matched_tool_use_result_is_valid() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::new(
                IrRole::Assistant,
                vec![IrContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "test".into(),
                    input: json!({}),
                }],
            ),
            IrMessage::new(
                IrRole::Tool,
                vec![IrContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: vec![IrContentBlock::Text {
                        text: "done".into(),
                    }],
                    is_error: false,
                }],
            ),
        ]);
        let result = validate_ir_structure(&conv);
        assert!(result.is_valid());
    }

    #[test]
    fn thinking_detected_for_openai() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking { text: "hmm".into() }],
        )]);
        let caps = dialect_capabilities(Dialect::OpenAi);
        let result = validate_for_target(&conv, &caps);
        assert!(!result.is_valid());
        assert_eq!(result.issues[0].code, "unsupported_thinking");
    }

    #[test]
    fn thinking_ok_for_claude() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::Thinking { text: "hmm".into() }],
        )]);
        let caps = dialect_capabilities(Dialect::Claude);
        let result = validate_for_target(&conv, &caps);
        assert!(result.is_valid());
    }

    #[test]
    fn images_detected_for_codex() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::User,
            vec![IrContentBlock::Image {
                media_type: "image/png".into(),
                data: "data".into(),
            }],
        )]);
        let caps = dialect_capabilities(Dialect::Codex);
        let result = validate_for_target(&conv, &caps);
        assert!(!result.is_valid());
        assert_eq!(result.issues[0].code, "unsupported_image");
    }

    #[test]
    fn system_prompt_detected_for_codex() {
        let conv =
            IrConversation::from_messages(vec![IrMessage::text(IrRole::System, "Be helpful")]);
        let caps = dialect_capabilities(Dialect::Codex);
        let result = validate_for_target(&conv, &caps);
        assert!(!result.is_valid());
        assert!(result
            .issues
            .iter()
            .any(|i| i.code == "unsupported_system_prompt"));
    }

    #[test]
    fn tool_use_detected_for_codex() {
        let conv = IrConversation::from_messages(vec![IrMessage::new(
            IrRole::Assistant,
            vec![IrContentBlock::ToolUse {
                id: "t1".into(),
                name: "test".into(),
                input: json!({}),
            }],
        )]);
        let caps = dialect_capabilities(Dialect::Codex);
        let result = validate_for_target(&conv, &caps);
        assert!(!result.is_valid());
        assert_eq!(result.issues[0].code, "unsupported_tool_use");
    }

    #[test]
    fn combined_validation_catches_both() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::text(IrRole::System, "sys"),
            IrMessage::new(IrRole::Assistant, vec![]),
        ]);
        let result = validate_ir_for_mapping(&conv, Dialect::Codex);
        assert!(!result.is_valid());
        let codes: Vec<_> = result.issues.iter().map(|i| i.code).collect();
        assert!(codes.contains(&"empty_message"));
        assert!(codes.contains(&"unsupported_system_prompt"));
    }

    #[test]
    fn empty_conversation_is_valid() {
        let conv = IrConversation::new();
        let result = validate_ir_structure(&conv);
        assert!(result.is_valid());
    }

    #[test]
    fn validation_result_issue_count() {
        let conv = IrConversation::from_messages(vec![
            IrMessage::new(IrRole::User, vec![]),
            IrMessage::new(IrRole::Assistant, vec![]),
        ]);
        let result = validate_ir_structure(&conv);
        assert_eq!(result.issues.len(), 2);
    }
}
