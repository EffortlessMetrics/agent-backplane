// SPDX-License-Identifier: MIT OR Apache-2.0

//! IR-level mapper between OpenAI and Copilot dialects.
//!
//! GitHub Copilot follows an OpenAI-compatible API surface, so the mapping
//! is nearly identity. The only lossy transform is dropping thinking blocks
//! (Copilot has no equivalent).

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage};
use abp_dialect::Dialect;

use crate::ir_mapper::IrMapper;
use crate::MapError;

/// Bidirectional IR mapper between OpenAI and Copilot dialects.
///
/// Covers both `(OpenAi, Copilot)` and `(Copilot, OpenAi)` pairs.
///
/// ## Near-identity mapping
///
/// Copilot follows the OpenAI chat-completions convention (system/user/assistant
/// roles, function-calling tool surface). The only transforms are:
///
/// - **Thinking blocks**: dropped in both directions (neither dialect
///   natively supports them).
pub struct OpenAiCopilotIrMapper;

impl IrMapper for OpenAiCopilotIrMapper {
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
            (Dialect::OpenAi, Dialect::Copilot),
            (Dialect::Copilot, Dialect::OpenAi),
        ]
    }
}

impl OpenAiCopilotIrMapper {
    fn map_conversation(
        &self,
        from: Dialect,
        to: Dialect,
        ir: &IrConversation,
    ) -> Result<IrConversation, MapError> {
        match (from, to) {
            (Dialect::OpenAi, Dialect::Copilot) => {
                // Copilot does not support image content blocks — reject early.
                for msg in &ir.messages {
                    if msg
                        .content
                        .iter()
                        .any(|b| matches!(b, IrContentBlock::Image { .. }))
                    {
                        return Err(MapError::UnmappableContent {
                            field: "content".into(),
                            reason: "Copilot does not support image content blocks".into(),
                        });
                    }
                }
                Ok(self.filter_all_thinking(ir))
            }
            (Dialect::Copilot, Dialect::OpenAi) => Ok(self.filter_all_thinking(ir)),
            _ => Err(MapError::UnsupportedPair { from, to }),
        }
    }

    /// Strips thinking blocks from all messages (neither OpenAI nor Copilot
    /// supports them natively).
    fn filter_all_thinking(&self, ir: &IrConversation) -> IrConversation {
        let messages = ir
            .messages
            .iter()
            .map(|msg| {
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
        IrConversation::from_messages(messages)
    }
}
