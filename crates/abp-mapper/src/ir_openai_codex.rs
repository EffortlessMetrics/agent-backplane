// SPDX-License-Identifier: MIT OR Apache-2.0

//! IR-level mapper between OpenAI and Codex dialects.
//!
//! **Lossy**: Codex (Responses API) is output-only — it has no tool-calling
//! surface, no system instruction field, and no thinking blocks. Mapping
//! *to* Codex silently drops those constructs; mapping *from* Codex is
//! lossless since Codex output is simple text.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;

use crate::MapError;
use crate::ir_mapper::IrMapper;

/// Bidirectional (but lossy) IR mapper between OpenAI and Codex dialects.
///
/// Covers both `(OpenAi, Codex)` and `(Codex, OpenAi)` pairs.
///
/// ## Lossy conversions (OpenAI → Codex)
///
/// - **System messages**: dropped (Codex has no system instruction).
/// - **Tool calls**: `ToolUse` and `ToolResult` blocks are dropped.
/// - **Thinking blocks**: dropped.
/// - **Tool-role messages**: dropped entirely.
/// - **Image blocks**: dropped (Codex is text-only output).
pub struct OpenAiCodexIrMapper;

impl IrMapper for OpenAiCodexIrMapper {
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
            (Dialect::OpenAi, Dialect::Codex),
            (Dialect::Codex, Dialect::OpenAi),
        ]
    }
}

impl OpenAiCodexIrMapper {
    fn map_conversation(
        &self,
        from: Dialect,
        to: Dialect,
        ir: &IrConversation,
    ) -> Result<IrConversation, MapError> {
        match (from, to) {
            (Dialect::OpenAi, Dialect::Codex) => self.openai_to_codex(ir),
            (Dialect::Codex, Dialect::OpenAi) => self.codex_to_openai(ir),
            _ => Err(MapError::UnsupportedPair { from, to }),
        }
    }

    /// OpenAI → Codex (lossy):
    /// - System messages are dropped.
    /// - Tool-role messages are dropped.
    /// - ToolUse, ToolResult, Thinking, and Image blocks are stripped.
    /// - Only Text content blocks in User/Assistant messages survive.
    fn openai_to_codex(&self, ir: &IrConversation) -> Result<IrConversation, MapError> {
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

    /// Codex → OpenAI (lossless):
    /// Codex output is simple text that maps cleanly to OpenAI format.
    fn codex_to_openai(&self, ir: &IrConversation) -> Result<IrConversation, MapError> {
        Ok(ir.clone())
    }
}
