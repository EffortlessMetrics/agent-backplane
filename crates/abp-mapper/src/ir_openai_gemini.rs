// SPDX-License-Identifier: MIT OR Apache-2.0

//! IR-level mapper between OpenAI and Gemini dialects.
//!
//! Handles bidirectional mapping of:
//! - Role names (`system` → `model`-level instruction in Gemini)
//! - Function calling formats (OpenAI `tool_calls` vs Gemini `functionCall`)
//! - Content block structures
//! - Thinking blocks (dropped for both since Gemini has limited support)

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;

use crate::MapError;
use crate::ir_mapper::IrMapper;

/// Bidirectional IR mapper between OpenAI and Gemini dialects.
///
/// Covers both `(OpenAi, Gemini)` and `(Gemini, OpenAi)` pairs.
pub struct OpenAiGeminiIrMapper;

impl IrMapper for OpenAiGeminiIrMapper {
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
            (Dialect::OpenAi, Dialect::Gemini),
            (Dialect::Gemini, Dialect::OpenAi),
        ]
    }
}

impl OpenAiGeminiIrMapper {
    fn map_conversation(
        &self,
        from: Dialect,
        to: Dialect,
        ir: &IrConversation,
    ) -> Result<IrConversation, MapError> {
        match (from, to) {
            (Dialect::OpenAi, Dialect::Gemini) => self.openai_to_gemini(ir),
            (Dialect::Gemini, Dialect::OpenAi) => self.gemini_to_openai(ir),
            _ => Err(MapError::UnsupportedPair { from, to }),
        }
    }

    /// OpenAI → Gemini:
    /// - System messages are kept (Gemini supports system instructions).
    /// - Tool-role messages become user messages with ToolResult blocks
    ///   (Gemini models tool results as `functionResponse` in user turn).
    /// - Thinking blocks are dropped (no Gemini equivalent).
    fn openai_to_gemini(&self, ir: &IrConversation) -> Result<IrConversation, MapError> {
        let mut messages = Vec::with_capacity(ir.messages.len());

        for msg in &ir.messages {
            match msg.role {
                IrRole::System | IrRole::User | IrRole::Assistant => {
                    messages.push(filter_thinking(msg));
                }
                IrRole::Tool => {
                    // Gemini treats function responses as user-role turns
                    let mut mapped = filter_thinking(msg);
                    mapped.role = IrRole::User;
                    messages.push(mapped);
                }
            }
        }

        Ok(IrConversation::from_messages(messages))
    }

    /// Gemini → OpenAI:
    /// - System messages are preserved.
    /// - User messages containing only ToolResult blocks become Tool-role messages.
    /// - Thinking blocks are dropped.
    fn gemini_to_openai(&self, ir: &IrConversation) -> Result<IrConversation, MapError> {
        let mut messages = Vec::with_capacity(ir.messages.len());

        for msg in &ir.messages {
            match msg.role {
                IrRole::System | IrRole::Assistant => {
                    messages.push(filter_thinking(msg));
                }
                IrRole::User => {
                    let filtered = filter_thinking(msg);
                    let (tool_results, other): (Vec<_>, Vec<_>) = filtered
                        .content
                        .iter()
                        .cloned()
                        .partition(|b| matches!(b, IrContentBlock::ToolResult { .. }));

                    if !tool_results.is_empty() && other.is_empty() {
                        for block in tool_results {
                            messages.push(IrMessage::new(IrRole::Tool, vec![block]));
                        }
                    } else if !tool_results.is_empty() {
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
                        messages.push(filtered);
                    }
                }
                IrRole::Tool => {
                    messages.push(filter_thinking(msg));
                }
            }
        }

        Ok(IrConversation::from_messages(messages))
    }
}

/// Remove thinking blocks from a message (neither OpenAI nor Gemini support them).
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
