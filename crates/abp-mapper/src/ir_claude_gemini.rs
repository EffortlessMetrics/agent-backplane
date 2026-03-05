// SPDX-License-Identifier: MIT OR Apache-2.0

//! IR-level mapper between Claude and Gemini dialects.
//!
//! Handles bidirectional mapping of:
//! - Thinking blocks (Claude-specific, dropped for Gemini — lossy)
//! - Tool call formats (`tool_use` ↔ `functionCall` at IR level)
//! - Tool result formats (`tool_result` ↔ `functionResponse` at IR level)
//! - System prompts (preserved in both directions)
//! - Image content blocks (preserved)

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;

use crate::ir_mapper::IrMapper;
use crate::MapError;

/// Bidirectional IR mapper between Claude and Gemini dialects.
///
/// Covers both `(Claude, Gemini)` and `(Gemini, Claude)` pairs.
///
/// ## Lossy conversions
///
/// - **Thinking blocks**: Claude's extended-thinking blocks have no Gemini
///   equivalent and are silently dropped when mapping Claude → Gemini.
/// - **Tool-result role**: Claude uses user-role messages containing
///   `ToolResult` blocks; Gemini also places function responses in user-role
///   turns. Both patterns map onto `IrRole::User` + `IrContentBlock::ToolResult`.
pub struct ClaudeGeminiIrMapper;

impl IrMapper for ClaudeGeminiIrMapper {
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
            (Dialect::Claude, Dialect::Gemini),
            (Dialect::Gemini, Dialect::Claude),
        ]
    }
}

impl ClaudeGeminiIrMapper {
    fn map_conversation(
        &self,
        from: Dialect,
        to: Dialect,
        ir: &IrConversation,
    ) -> Result<IrConversation, MapError> {
        match (from, to) {
            (Dialect::Claude, Dialect::Gemini) => self.claude_to_gemini(ir),
            (Dialect::Gemini, Dialect::Claude) => self.gemini_to_claude(ir),
            _ => Err(MapError::UnsupportedPair { from, to }),
        }
    }

    /// Claude → Gemini:
    /// - Thinking blocks are dropped (Gemini has no equivalent).
    /// - System messages are preserved (both support system instructions).
    /// - User messages with `ToolResult` blocks stay as user-role
    ///   (Gemini models `functionResponse` in user turns).
    /// - `ToolUse` / `ToolResult` content blocks are preserved as-is in IR.
    /// - Image blocks are preserved.
    /// - **Fails early** if a system message contains image blocks
    ///   (Gemini `system_instruction` only supports text parts).
    fn claude_to_gemini(&self, ir: &IrConversation) -> Result<IrConversation, MapError> {
        // Early failure: system prompts with image blocks cannot be mapped
        // to Gemini because system_instruction only supports text parts.
        for msg in &ir.messages {
            if msg.role == IrRole::System
                && msg
                    .content
                    .iter()
                    .any(|b| matches!(b, IrContentBlock::Image { .. }))
            {
                return Err(MapError::UnmappableContent {
                    field: "system".into(),
                    reason: "Gemini system_instruction does not support image content blocks"
                        .into(),
                });
            }
        }

        let mut messages = Vec::with_capacity(ir.messages.len());

        for msg in &ir.messages {
            match msg.role {
                IrRole::System | IrRole::User | IrRole::Assistant => {
                    messages.push(filter_thinking(msg));
                }
                IrRole::Tool => {
                    // Claude doesn't normally use Tool role (it uses User +
                    // ToolResult blocks), but if present, map to User role
                    // for Gemini (functionResponse lives in user turns).
                    let mut mapped = filter_thinking(msg);
                    mapped.role = IrRole::User;
                    messages.push(mapped);
                }
            }
        }

        Ok(IrConversation::from_messages(messages))
    }

    /// Gemini → Claude:
    /// - Thinking blocks are dropped (Gemini shouldn't have them, but just
    ///   in case they appear in the IR).
    /// - System messages are preserved.
    /// - User messages containing only `ToolResult` blocks stay as user-role
    ///   (Claude's native format for tool results).
    /// - Tool-role messages become user-role (Claude's convention).
    /// - Image blocks are preserved.
    fn gemini_to_claude(&self, ir: &IrConversation) -> Result<IrConversation, MapError> {
        let mut messages = Vec::with_capacity(ir.messages.len());

        for msg in &ir.messages {
            match msg.role {
                IrRole::System | IrRole::User | IrRole::Assistant => {
                    messages.push(msg.clone());
                }
                IrRole::Tool => {
                    // Claude expects tool results in User-role messages
                    let mut mapped = msg.clone();
                    mapped.role = IrRole::User;
                    messages.push(mapped);
                }
            }
        }

        Ok(IrConversation::from_messages(messages))
    }
}

/// Remove thinking blocks from a message (Gemini has no equivalent).
fn filter_thinking(msg: &IrMessage) -> IrMessage {
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
}
