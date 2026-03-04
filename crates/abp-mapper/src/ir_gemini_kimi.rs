// SPDX-License-Identifier: MIT OR Apache-2.0

//! IR-level mapper between Gemini and Kimi dialects.
//!
//! Gemini models tool results as user-role `functionResponse` turns, while
//! Kimi follows the OpenAI convention with a dedicated `Tool` role for tool
//! results. This mapper bridges those differences. Thinking blocks are
//! dropped in both directions (neither dialect supports them).

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole};
use abp_dialect::Dialect;

use crate::MapError;
use crate::ir_mapper::IrMapper;

/// Bidirectional IR mapper between Gemini and Kimi dialects.
///
/// Covers both `(Gemini, Kimi)` and `(Kimi, Gemini)` pairs.
///
/// ## Lossy conversions
///
/// - **Thinking blocks**: dropped in both directions (neither Gemini nor
///   Kimi supports them).
/// - **Tool-result role**: Gemini uses `User` role for tool results; Kimi
///   uses `Tool` role. The mapper converts between these conventions.
pub struct GeminiKimiIrMapper;

impl IrMapper for GeminiKimiIrMapper {
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
            (Dialect::Gemini, Dialect::Kimi),
            (Dialect::Kimi, Dialect::Gemini),
        ]
    }
}

impl GeminiKimiIrMapper {
    fn map_conversation(
        &self,
        from: Dialect,
        to: Dialect,
        ir: &IrConversation,
    ) -> Result<IrConversation, MapError> {
        match (from, to) {
            (Dialect::Gemini, Dialect::Kimi) => self.gemini_to_kimi(ir),
            (Dialect::Kimi, Dialect::Gemini) => self.kimi_to_gemini(ir),
            _ => Err(MapError::UnsupportedPair { from, to }),
        }
    }

    /// Gemini → Kimi:
    /// - Thinking blocks are dropped.
    /// - System messages are preserved.
    /// - User messages containing only ToolResult blocks are converted to
    ///   Tool-role messages (Kimi's OpenAI-compatible convention).
    /// - ToolUse content blocks in assistant messages are preserved.
    /// - **Fails early** if the conversation contains image blocks
    ///   (Kimi does not support images).
    fn gemini_to_kimi(&self, ir: &IrConversation) -> Result<IrConversation, MapError> {
        // Kimi does not support image content blocks — reject early.
        for msg in &ir.messages {
            if msg
                .content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Image { .. }))
            {
                return Err(MapError::UnmappableContent {
                    field: "content".into(),
                    reason: "Kimi does not support image content blocks".into(),
                });
            }
        }

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
                        // All blocks are tool results → emit as Tool-role messages
                        for block in tool_results {
                            messages.push(IrMessage::new(IrRole::Tool, vec![block]));
                        }
                    } else if !tool_results.is_empty() {
                        // Mixed content: text as User, tool results as Tool
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

    /// Kimi → Gemini:
    /// - Thinking blocks are dropped.
    /// - System messages are preserved.
    /// - Tool-role messages become User-role messages (Gemini models
    ///   `functionResponse` in user turns).
    fn kimi_to_gemini(&self, ir: &IrConversation) -> Result<IrConversation, MapError> {
        let mut messages = Vec::with_capacity(ir.messages.len());

        for msg in &ir.messages {
            match msg.role {
                IrRole::System | IrRole::User | IrRole::Assistant => {
                    messages.push(filter_thinking(msg));
                }
                IrRole::Tool => {
                    // Kimi tool-result messages → User role for Gemini
                    let mut mapped = filter_thinking(msg);
                    mapped.role = IrRole::User;
                    messages.push(mapped);
                }
            }
        }

        Ok(IrConversation::from_messages(messages))
    }
}

/// Remove thinking blocks from a message (neither dialect supports them).
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
