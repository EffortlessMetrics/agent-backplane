// SPDX-License-Identifier: MIT OR Apache-2.0

//! IR-level mapper between Codex and Claude dialects.
//!
//! **Lossy**: Codex (Responses API) is output-only — it has no tool-calling
//! surface, no system instruction field, and no thinking blocks. Mapping
//! *from* Claude *to* Codex silently drops those constructs; mapping *from*
//! Codex *to* Claude is lossless since Codex output is simple text.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;

use crate::MapError;
use crate::ir_mapper::IrMapper;

/// Bidirectional (but lossy) IR mapper between Codex and Claude dialects.
///
/// Covers both `(Codex, Claude)` and `(Claude, Codex)` pairs.
///
/// ## Lossy conversions (Claude → Codex)
///
/// - **System messages**: dropped (Codex has no system instruction).
/// - **Tool calls**: `ToolUse` and `ToolResult` blocks are dropped.
/// - **Thinking blocks**: dropped.
/// - **Tool-role messages**: dropped entirely.
/// - **Image blocks**: dropped (Codex is text-only output).
///
/// ## Lossless conversions (Codex → Claude)
///
/// Codex output is simple text that maps cleanly to Claude format.
pub struct CodexClaudeIrMapper;

impl IrMapper for CodexClaudeIrMapper {
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
            (Dialect::Codex, Dialect::Claude),
            (Dialect::Claude, Dialect::Codex),
        ]
    }
}

impl CodexClaudeIrMapper {
    fn map_conversation(
        &self,
        from: Dialect,
        to: Dialect,
        ir: &IrConversation,
    ) -> Result<IrConversation, MapError> {
        match (from, to) {
            (Dialect::Claude, Dialect::Codex) => self.claude_to_codex(ir),
            (Dialect::Codex, Dialect::Claude) => self.codex_to_claude(ir),
            _ => Err(MapError::UnsupportedPair { from, to }),
        }
    }

    /// Claude → Codex (lossy):
    /// - System messages are dropped.
    /// - Tool-role messages are dropped.
    /// - ToolUse, ToolResult, Thinking, and Image blocks are stripped.
    /// - Only Text content blocks in User/Assistant messages survive.
    fn claude_to_codex(&self, ir: &IrConversation) -> Result<IrConversation, MapError> {
        let mut messages = Vec::new();

        for msg in &ir.messages {
            match msg.role {
                IrRole::System | IrRole::Tool => {
                    // Dropped — Codex has no system or tool role
                    continue;
                }
                IrRole::User | IrRole::Assistant => {
                    let text_blocks: Vec<IrContentBlock> = msg
                        .content
                        .iter()
                        .filter(|b| matches!(b, IrContentBlock::Text { .. }))
                        .cloned()
                        .collect();
                    if !text_blocks.is_empty() {
                        messages.push(IrMessage {
                            role: msg.role,
                            content: text_blocks,
                            metadata: msg.metadata.clone(),
                        });
                    }
                }
            }
        }

        Ok(IrConversation::from_messages(messages))
    }

    /// Codex → Claude (lossless):
    /// Codex output is simple text that maps cleanly to Claude format.
    ///
    /// **Fails early** if the conversation contains Codex-specific file
    /// operation tools (`apply_patch`, `apply_diff`) that have no Claude
    /// equivalent.
    fn codex_to_claude(&self, ir: &IrConversation) -> Result<IrConversation, MapError> {
        const UNMAPPABLE_TOOLS: &[&str] = &["apply_patch", "apply_diff"];
        for msg in &ir.messages {
            for block in &msg.content {
                if let IrContentBlock::ToolUse { name, .. } = block {
                    if UNMAPPABLE_TOOLS.contains(&name.as_str()) {
                        return Err(MapError::UnmappableTool {
                            name: name.clone(),
                            reason: "Codex file operation has no Claude equivalent".into(),
                        });
                    }
                }
            }
        }
        Ok(ir.clone())
    }
}
