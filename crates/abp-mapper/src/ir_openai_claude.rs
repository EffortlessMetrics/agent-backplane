// SPDX-License-Identifier: MIT OR Apache-2.0

//! IR-level mapper between OpenAI and Claude dialects.
//!
//! Handles bidirectional mapping of:
//! - Role names (system prompt extraction/injection)
//! - Tool call formats (function vs tool_use content blocks)
//! - Content block structures (string vs array-of-blocks)
//! - Thinking blocks (Claude-specific, dropped for OpenAI)

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;

use crate::ir_mapper::IrMapper;
use crate::MapError;

/// Bidirectional IR mapper between OpenAI and Claude dialects.
///
/// Covers both `(OpenAi, Claude)` and `(Claude, OpenAi)` pairs.
pub struct OpenAiClaudeIrMapper;

impl IrMapper for OpenAiClaudeIrMapper {
    fn map_request(
        &self,
        from: Dialect,
        to: Dialect,
        ir: &IrConversation,
    ) -> Result<IrConversation, MapError> {
        self.map_conversation(from, to, ir)
    }

    fn map_response(
        &self,
        from: Dialect,
        to: Dialect,
        ir: &IrConversation,
    ) -> Result<IrConversation, MapError> {
        self.map_conversation(from, to, ir)
    }

    fn supported_pairs(&self) -> Vec<(Dialect, Dialect)> {
        vec![
            (Dialect::OpenAi, Dialect::Claude),
            (Dialect::Claude, Dialect::OpenAi),
        ]
    }
}

impl OpenAiClaudeIrMapper {
    fn map_conversation(
        &self,
        from: Dialect,
        to: Dialect,
        ir: &IrConversation,
    ) -> Result<IrConversation, MapError> {
        match (from, to) {
            (Dialect::OpenAi, Dialect::Claude) => self.openai_to_claude(ir),
            (Dialect::Claude, Dialect::OpenAi) => self.claude_to_openai(ir),
            _ => Err(MapError::UnsupportedPair { from, to }),
        }
    }

    /// OpenAI → Claude: tool_calls become ToolUse content blocks;
    /// system messages stay as-is in IR (both dialects use IrRole::System);
    /// thinking blocks are preserved (Claude supports them natively).
    ///
    /// **Fails early** if a system message contains image blocks
    /// (Claude does not support images in system prompts).
    fn openai_to_claude(&self, ir: &IrConversation) -> Result<IrConversation, MapError> {
        // Reject system messages with image blocks — Claude's system
        // prompt does not support image content.
        for msg in &ir.messages {
            if msg.role == IrRole::System
                && msg
                    .content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::Image { .. }))
            {
                return Err(MapError::UnmappableContent {
                    field: "system".into(),
                    reason: "Claude system prompt does not support image content blocks".into(),
                });
            }
        }

        let mut messages = Vec::with_capacity(ir.messages.len());

        for msg in &ir.messages {
            match msg.role {
                IrRole::System | IrRole::User | IrRole::Assistant => {
                    // Content blocks map directly; ToolUse/ToolResult blocks
                    // are already in the canonical IR form.
                    messages.push(map_message_content(msg, Dialect::OpenAi, Dialect::Claude)?);
                }
                IrRole::Tool => {
                    // OpenAI tool-result messages become user messages with
                    // ToolResult content blocks in Claude's model.
                    let mut mapped = msg.clone();
                    mapped.role = IrRole::User;
                    messages.push(mapped);
                }
            }
        }

        Ok(IrConversation::from_messages(messages))
    }

    /// Claude → OpenAI: ToolUse content blocks stay as-is in IR;
    /// user messages carrying ToolResult blocks become Tool-role messages;
    /// thinking blocks are dropped (OpenAI has no equivalent).
    fn claude_to_openai(&self, ir: &IrConversation) -> Result<IrConversation, MapError> {
        let mut messages = Vec::with_capacity(ir.messages.len());

        for msg in &ir.messages {
            match msg.role {
                IrRole::System | IrRole::Assistant => {
                    messages.push(map_message_content(msg, Dialect::Claude, Dialect::OpenAi)?);
                }
                IrRole::User => {
                    // Check if this user message contains only ToolResult blocks
                    // (Claude pattern) → split into Tool-role messages.
                    let (tool_results, other): (Vec<_>, Vec<_>) = msg
                        .content
                        .iter()
                        .cloned()
                        .partition(|b| matches!(b, IrContentBlock::ToolResult { .. }));

                    if !tool_results.is_empty() && other.is_empty() {
                        // All blocks are tool results → emit as Tool-role messages
                        for block in tool_results {
                            messages.push(IrMessage::new(IrRole::Tool, vec![block]));
                        }
                    } else if !tool_results.is_empty() {
                        // Mixed content: emit text blocks as User, tool results as Tool
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
                        messages.push(map_message_content(msg, Dialect::Claude, Dialect::OpenAi)?);
                    }
                }
                IrRole::Tool => {
                    messages.push(msg.clone());
                }
            }
        }

        Ok(IrConversation::from_messages(messages))
    }
}

/// Maps content blocks within a single message, handling dialect-specific
/// differences like thinking blocks.
fn map_message_content(
    msg: &IrMessage,
    _from: Dialect,
    to: Dialect,
) -> Result<IrMessage, MapError> {
    let content: Vec<IrContentBlock> = msg
        .content
        .iter()
        .filter_map(|block| match block {
            IrContentBlock::Thinking { .. } if to == Dialect::OpenAi => {
                // OpenAI has no thinking block — drop it
                None
            }
            other => Some(other.clone()),
        })
        .collect();

    Ok(IrMessage {
        role: msg.role,
        content,
        metadata: msg.metadata.clone(),
    })
}
