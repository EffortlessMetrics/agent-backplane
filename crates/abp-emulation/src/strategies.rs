// SPDX-License-Identifier: MIT OR Apache-2.0
//! Concrete emulation strategy implementations.
//!
//! These operate on [`IrConversation`] and response text to emulate
//! capabilities that a backend does not natively support.

use abp_core::ir::{IrContentBlock, IrConversation, IrMessage, IrRole, IrToolDefinition};
use serde::{Deserialize, Serialize};

// ── Thinking Emulation ──────────────────────────────────────────────────

/// Detail level for chain-of-thought prompting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingDetail {
    /// Brief: single sentence instruction.
    Brief,
    /// Standard: structured reasoning with explicit markers.
    Standard,
    /// Detailed: deep analysis with verification steps.
    Detailed,
}

/// Emulates extended thinking / chain-of-thought for backends that lack
/// native reasoning support.
#[derive(Debug, Clone)]
pub struct ThinkingEmulation {
    detail: ThinkingDetail,
}

const THINKING_START: &str = "<thinking>";
const THINKING_END: &str = "</thinking>";

impl ThinkingEmulation {
    /// Create with the given detail level.
    #[must_use]
    pub fn new(detail: ThinkingDetail) -> Self {
        Self { detail }
    }

    /// Brief chain-of-thought.
    #[must_use]
    pub fn brief() -> Self {
        Self::new(ThinkingDetail::Brief)
    }

    /// Standard chain-of-thought with XML markers.
    #[must_use]
    pub fn standard() -> Self {
        Self::new(ThinkingDetail::Standard)
    }

    /// Detailed chain-of-thought with verification.
    #[must_use]
    pub fn detailed() -> Self {
        Self::new(ThinkingDetail::Detailed)
    }

    /// The prompt text for the configured detail level.
    #[must_use]
    pub fn prompt_text(&self) -> &'static str {
        match self.detail {
            ThinkingDetail::Brief => "Think step by step before answering.",
            ThinkingDetail::Standard => concat!(
                "Before answering, think through the problem step by step.\n",
                "Wrap your reasoning in <thinking></thinking> tags.\n",
                "Then provide your final answer outside the tags."
            ),
            ThinkingDetail::Detailed => concat!(
                "Before answering, perform a thorough analysis:\n",
                "1. Break the problem into sub-problems.\n",
                "2. Analyze each sub-problem step by step.\n",
                "3. Verify your reasoning for correctness.\n",
                "4. Wrap all reasoning in <thinking></thinking> tags.\n",
                "5. Provide your final answer outside the tags."
            ),
        }
    }

    /// Inject chain-of-thought prompting into a conversation.
    pub fn inject(&self, conv: &mut IrConversation) {
        let prompt = self.prompt_text();
        if let Some(sys) = conv.messages.iter_mut().find(|m| m.role == IrRole::System) {
            sys.content.push(IrContentBlock::Text {
                text: format!("\n{prompt}"),
            });
        } else {
            conv.messages
                .insert(0, IrMessage::text(IrRole::System, prompt));
        }
    }

    /// Extract thinking content from a response that uses `<thinking>` tags.
    ///
    /// Returns `(thinking, answer)` where thinking is empty if no tags found.
    #[must_use]
    pub fn extract_thinking(text: &str) -> (String, String) {
        if let Some(start) = text.find(THINKING_START) {
            if let Some(end) = text.find(THINKING_END) {
                let thinking_start = start + THINKING_START.len();
                if thinking_start <= end {
                    let thinking = text[thinking_start..end].trim().to_string();
                    let before = text[..start].trim();
                    let after = text[end + THINKING_END.len()..].trim();
                    let answer = if before.is_empty() {
                        after.to_string()
                    } else if after.is_empty() {
                        before.to_string()
                    } else {
                        format!("{before} {after}")
                    };
                    return (thinking, answer);
                }
            }
        }
        (String::new(), text.to_string())
    }

    /// Convert extracted thinking text into an [`IrContentBlock::Thinking`] block.
    #[must_use]
    pub fn to_thinking_block(text: &str) -> Option<IrContentBlock> {
        let (thinking, _) = Self::extract_thinking(text);
        if thinking.is_empty() {
            None
        } else {
            Some(IrContentBlock::Thinking { text: thinking })
        }
    }
}

// ── Tool Use Emulation ──────────────────────────────────────────────────

/// A parsed tool call extracted from text output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedToolCall {
    /// Tool name.
    pub name: String,
    /// Tool arguments as JSON.
    pub arguments: serde_json::Value,
}

const TOOL_CALL_START: &str = "<tool_call>";
const TOOL_CALL_END: &str = "</tool_call>";

/// Emulates tool use / function calling for backends without native support.
///
/// Converts tool definitions into structured text prompts and parses
/// tool call responses from text output.
#[derive(Debug, Clone)]
pub struct ToolUseEmulation;

impl ToolUseEmulation {
    /// Convert tool definitions into a text prompt describing available tools.
    #[must_use]
    pub fn tools_to_prompt(tools: &[IrToolDefinition]) -> String {
        if tools.is_empty() {
            return String::new();
        }

        let mut prompt = String::from("You have access to the following tools:\n\n");

        for tool in tools {
            prompt.push_str(&format!("## {}\n", tool.name));
            prompt.push_str(&format!("Description: {}\n", tool.description));
            if !tool.parameters.is_null() {
                if let Ok(pretty) = serde_json::to_string_pretty(&tool.parameters) {
                    prompt.push_str(&format!("Parameters: {pretty}\n"));
                }
            }
            prompt.push('\n');
        }

        prompt.push_str(concat!(
            "To call a tool, respond with a <tool_call> block:\n",
            "<tool_call>\n",
            "{\"name\": \"tool_name\", \"arguments\": {\"arg1\": \"value1\"}}\n",
            "</tool_call>\n\n",
            "You may call multiple tools by including multiple <tool_call> blocks.",
        ));

        prompt
    }

    /// Inject tool definitions into a conversation's system prompt.
    pub fn inject_tools(conv: &mut IrConversation, tools: &[IrToolDefinition]) {
        let prompt = Self::tools_to_prompt(tools);
        if prompt.is_empty() {
            return;
        }
        if let Some(sys) = conv.messages.iter_mut().find(|m| m.role == IrRole::System) {
            sys.content.push(IrContentBlock::Text {
                text: format!("\n{prompt}"),
            });
        } else {
            conv.messages
                .insert(0, IrMessage::text(IrRole::System, &prompt));
        }
    }

    /// Parse tool calls from a text response containing `<tool_call>` blocks.
    #[must_use]
    pub fn parse_tool_calls(text: &str) -> Vec<Result<ParsedToolCall, String>> {
        let mut results = Vec::new();
        let mut search_from = 0;

        while let Some(start) = text[search_from..].find(TOOL_CALL_START) {
            let abs_start = search_from + start + TOOL_CALL_START.len();
            if let Some(end) = text[abs_start..].find(TOOL_CALL_END) {
                let abs_end = abs_start + end;
                let json_str = text[abs_start..abs_end].trim();

                match serde_json::from_str::<serde_json::Value>(json_str) {
                    Ok(val) => {
                        let name = val
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let arguments = val
                            .get("arguments")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);
                        if name.is_empty() {
                            results.push(Err("tool_call missing 'name' field".to_string()));
                        } else {
                            results.push(Ok(ParsedToolCall { name, arguments }));
                        }
                    }
                    Err(e) => {
                        results.push(Err(format!("invalid JSON in tool_call: {e}")));
                    }
                }
                search_from = abs_end + TOOL_CALL_END.len();
            } else {
                results.push(Err("unclosed <tool_call> tag".to_string()));
                break;
            }
        }
        results
    }

    /// Convert a [`ParsedToolCall`] into an [`IrContentBlock::ToolUse`].
    #[must_use]
    pub fn to_tool_use_block(call: &ParsedToolCall, id: &str) -> IrContentBlock {
        IrContentBlock::ToolUse {
            id: id.to_string(),
            name: call.name.clone(),
            input: call.arguments.clone(),
        }
    }

    /// Format a tool result for injection back into the conversation as text.
    #[must_use]
    pub fn format_tool_result(name: &str, result: &str, is_error: bool) -> String {
        if is_error {
            format!("Tool '{name}' returned an error:\n{result}")
        } else {
            format!("Tool '{name}' returned:\n{result}")
        }
    }

    /// Extract the text outside any `<tool_call>` blocks.
    #[must_use]
    pub fn extract_text_outside_tool_calls(text: &str) -> String {
        let mut result = String::new();
        let mut search_from = 0;

        while let Some(start) = text[search_from..].find(TOOL_CALL_START) {
            let abs_start = search_from + start;
            result.push_str(&text[search_from..abs_start]);
            if let Some(end) = text[abs_start..].find(TOOL_CALL_END) {
                search_from = abs_start + end + TOOL_CALL_END.len();
            } else {
                break;
            }
        }
        result.push_str(&text[search_from..]);
        result.trim().to_string()
    }
}

// ── Vision Emulation ────────────────────────────────────────────────────

/// Emulates image/vision inputs for text-only backends.
///
/// Replaces image content blocks with text placeholders and optionally
/// injects a system prompt explaining the substitution.
#[derive(Debug, Clone)]
pub struct VisionEmulation;

impl VisionEmulation {
    /// Replace all [`IrContentBlock::Image`] blocks with text placeholders.
    ///
    /// Returns the number of images replaced.
    pub fn replace_images_with_placeholders(conv: &mut IrConversation) -> usize {
        let mut count = 0;
        for msg in &mut conv.messages {
            let mut new_content = Vec::new();
            for block in &msg.content {
                match block {
                    IrContentBlock::Image { media_type, .. } => {
                        count += 1;
                        new_content.push(IrContentBlock::Text {
                            text: format!(
                                "[Image {count}: {media_type} — \
                                 this backend does not support vision]"
                            ),
                        });
                    }
                    other => new_content.push(other.clone()),
                }
            }
            msg.content = new_content;
        }
        count
    }

    /// Inject a system prompt notifying the model that images were replaced.
    pub fn inject_vision_fallback_prompt(conv: &mut IrConversation, image_count: usize) {
        if image_count == 0 {
            return;
        }
        let prompt = format!(
            "{image_count} image(s) were provided but this backend does not support \
             vision input. Image placeholders have been inserted."
        );
        if let Some(sys) = conv.messages.iter_mut().find(|m| m.role == IrRole::System) {
            sys.content.push(IrContentBlock::Text {
                text: format!("\n{prompt}"),
            });
        } else {
            conv.messages
                .insert(0, IrMessage::text(IrRole::System, &prompt));
        }
    }

    /// Full vision emulation: replace images and inject fallback prompt.
    ///
    /// Returns the number of images replaced.
    pub fn apply(conv: &mut IrConversation) -> usize {
        let count = Self::replace_images_with_placeholders(conv);
        Self::inject_vision_fallback_prompt(conv, count);
        count
    }

    /// Check if a conversation contains any image content blocks.
    #[must_use]
    pub fn has_images(conv: &IrConversation) -> bool {
        conv.messages.iter().any(|msg| {
            msg.content
                .iter()
                .any(|b| matches!(b, IrContentBlock::Image { .. }))
        })
    }
}

// ── Streaming Emulation ─────────────────────────────────────────────────

/// A chunk of content from a simulated stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamChunk {
    /// The text content of this chunk.
    pub content: String,
    /// Zero-based index of this chunk.
    pub index: usize,
    /// Whether this is the final chunk.
    pub is_final: bool,
}

/// Emulates streaming output from non-streaming backends by splitting a
/// complete response into chunks that can be delivered incrementally.
#[derive(Debug, Clone)]
pub struct StreamingEmulation {
    chunk_size: usize,
}

impl StreamingEmulation {
    /// Create with the given chunk size (minimum 1).
    #[must_use]
    pub fn new(chunk_size: usize) -> Self {
        Self {
            chunk_size: chunk_size.max(1),
        }
    }

    /// Default chunk size of 20 characters.
    #[must_use]
    pub fn default_chunk_size() -> Self {
        Self::new(20)
    }

    /// The configured chunk size.
    #[must_use]
    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    /// Split text into chunks, preferring word boundaries.
    #[must_use]
    pub fn split_into_chunks(&self, text: &str) -> Vec<StreamChunk> {
        if text.is_empty() {
            return vec![StreamChunk {
                content: String::new(),
                index: 0,
                is_final: true,
            }];
        }

        let mut chunks = Vec::new();
        let mut remaining = text;
        let mut index = 0;

        while !remaining.is_empty() {
            let take = if remaining.len() <= self.chunk_size {
                remaining.len()
            } else {
                let candidate = &remaining[..self.chunk_size];
                match candidate.rfind(|c: char| c.is_whitespace()) {
                    Some(pos) if pos > 0 => pos + 1,
                    _ => self.chunk_size,
                }
            };

            let (chunk, rest) = remaining.split_at(take);
            remaining = rest;

            chunks.push(StreamChunk {
                content: chunk.to_string(),
                index,
                is_final: remaining.is_empty(),
            });
            index += 1;
        }

        chunks
    }

    /// Split by fixed character count (no word-boundary preference).
    #[must_use]
    pub fn split_fixed(&self, text: &str) -> Vec<StreamChunk> {
        if text.is_empty() {
            return vec![StreamChunk {
                content: String::new(),
                index: 0,
                is_final: true,
            }];
        }

        let mut chunks = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let mut index = 0;
        let mut pos = 0;

        while pos < chars.len() {
            let end = (pos + self.chunk_size).min(chars.len());
            let content: String = chars[pos..end].iter().collect();
            let is_final = end == chars.len();

            chunks.push(StreamChunk {
                content,
                index,
                is_final,
            });
            pos = end;
            index += 1;
        }

        chunks
    }

    /// Reassemble chunks back into the original text.
    #[must_use]
    pub fn reassemble(chunks: &[StreamChunk]) -> String {
        chunks.iter().map(|c| c.content.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thinking_detail_variants() {
        let _ = ThinkingEmulation::brief();
        let _ = ThinkingEmulation::standard();
        let _ = ThinkingEmulation::detailed();
    }

    #[test]
    fn streaming_chunk_size_minimum_is_one() {
        let emu = StreamingEmulation::new(0);
        assert_eq!(emu.chunk_size(), 1);
    }
}
