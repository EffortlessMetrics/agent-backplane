// SPDX-License-Identifier: MIT OR Apache-2.0
//! Streaming SSE types for the OpenAI Chat Completions API.
//!
//! These types model the `chat.completion.chunk` objects emitted during
//! server-sent event (SSE) streaming, and map to ABP's `AgentEvent` stream.

use abp_core::{AgentEvent, AgentEventKind};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::dialect::OpenAIFunctionCall;

// ---------------------------------------------------------------------------
// Streaming chunk types
// ---------------------------------------------------------------------------

/// A single streaming chunk from the Chat Completions API.
///
/// Corresponds to the `chat.completion.chunk` SSE event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatCompletionChunk {
    /// Unique chunk identifier (e.g. `chatcmpl-...`).
    pub id: String,
    /// Object type — always `"chat.completion.chunk"`.
    pub object: String,
    /// Unix timestamp when the chunk was created.
    pub created: u64,
    /// Model that generated the chunk.
    pub model: String,
    /// Streaming choices (typically one element).
    pub choices: Vec<ChunkChoice>,
    /// Token usage (only present on the final chunk when requested).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ChunkUsage>,
}

/// A single choice inside a streaming chunk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChunkChoice {
    /// Zero-based index of this choice.
    pub index: u32,
    /// The incremental delta for this choice.
    pub delta: ChunkDelta,
    /// Finish reason (`null` while streaming, then `"stop"` or `"tool_calls"`).
    pub finish_reason: Option<String>,
}

/// The delta payload inside a streaming choice.
///
/// Each field is `Option` — only the fields that changed are present.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ChunkDelta {
    /// Role of the message (only in the first chunk).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Incremental text content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Incremental tool call fragments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ChunkToolCall>>,
}

/// A tool call fragment inside a streaming delta.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChunkToolCall {
    /// Index of the tool call in the overall tool_calls array.
    pub index: u32,
    /// Tool call ID (only present in the first fragment for this index).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Call type (only present in the first fragment).
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,
    /// Incremental function call data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<ChunkFunctionCall>,
}

/// Incremental function call data inside a streaming tool call fragment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChunkFunctionCall {
    /// Function name (only present in the first fragment).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Incremental arguments string fragment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

// ---------------------------------------------------------------------------
// Usage in final chunk
// ---------------------------------------------------------------------------

/// Token usage statistics attached to the final streaming chunk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChunkUsage {
    /// Tokens consumed by the prompt.
    pub prompt_tokens: u64,
    /// Tokens generated in the completion.
    pub completion_tokens: u64,
    /// Total tokens (prompt + completion).
    pub total_tokens: u64,
}

// ---------------------------------------------------------------------------
// Mapping to ABP events
// ---------------------------------------------------------------------------

/// Map a single [`ChatCompletionChunk`] to zero or more ABP [`AgentEvent`]s.
///
/// - `delta.content` → `AgentEventKind::AssistantDelta`
/// - `delta.tool_calls` → `AgentEventKind::ToolCall` (only when complete enough)
///
/// Incomplete tool call fragments (arguments still accumulating) are not emitted;
/// callers should use [`ToolCallAccumulator`] to reassemble them.
pub fn map_chunk(chunk: &ChatCompletionChunk) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    let now = Utc::now();

    for choice in &chunk.choices {
        // Emit text deltas.
        if let Some(text) = &choice.delta.content
            && !text.is_empty()
        {
            events.push(AgentEvent {
                ts: now,
                kind: AgentEventKind::AssistantDelta { text: text.clone() },
                ext: None,
            });
        }
    }

    events
}

// ---------------------------------------------------------------------------
// Tool-call accumulator for streaming
// ---------------------------------------------------------------------------

/// Accumulates streamed tool call fragments into complete [`OpenAIFunctionCall`]s.
///
/// OpenAI streams tool calls in pieces: the first fragment carries `id`, `type`,
/// and the function `name`; subsequent fragments append to `arguments`.
/// This accumulator reassembles them.
#[derive(Debug, Default)]
pub struct ToolCallAccumulator {
    entries: Vec<AccEntry>,
}

#[derive(Debug, Clone)]
struct AccEntry {
    id: String,
    name: String,
    arguments: String,
}

impl ToolCallAccumulator {
    /// Create a new empty accumulator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a slice of [`ChunkToolCall`] fragments into the accumulator.
    pub fn feed(&mut self, fragments: &[ChunkToolCall]) {
        for frag in fragments {
            let idx = frag.index as usize;

            // Grow the entries vector if needed.
            while self.entries.len() <= idx {
                self.entries.push(AccEntry {
                    id: String::new(),
                    name: String::new(),
                    arguments: String::new(),
                });
            }

            let entry = &mut self.entries[idx];

            if let Some(id) = &frag.id {
                entry.id.clone_from(id);
            }
            if let Some(func) = &frag.function {
                if let Some(name) = &func.name {
                    entry.name.clone_from(name);
                }
                if let Some(args) = &func.arguments {
                    entry.arguments.push_str(args);
                }
            }
        }
    }

    /// Consume the accumulator and return completed tool calls as ABP events.
    pub fn finish(self) -> Vec<AgentEvent> {
        let now = Utc::now();
        self.entries
            .into_iter()
            .filter(|e| !e.name.is_empty())
            .map(|e| {
                let input = serde_json::from_str(&e.arguments)
                    .unwrap_or(serde_json::Value::String(e.arguments));
                AgentEvent {
                    ts: now,
                    kind: AgentEventKind::ToolCall {
                        tool_name: e.name,
                        tool_use_id: if e.id.is_empty() { None } else { Some(e.id) },
                        parent_tool_use_id: None,
                        input,
                    },
                    ext: None,
                }
            })
            .collect()
    }

    /// Return completed tool calls as [`OpenAIFunctionCall`] pairs (id, call).
    pub fn finish_as_openai(&self) -> Vec<(String, OpenAIFunctionCall)> {
        self.entries
            .iter()
            .filter(|e| !e.name.is_empty())
            .map(|e| {
                (
                    e.id.clone(),
                    OpenAIFunctionCall {
                        name: e.name.clone(),
                        arguments: e.arguments.clone(),
                    },
                )
            })
            .collect()
    }
}
