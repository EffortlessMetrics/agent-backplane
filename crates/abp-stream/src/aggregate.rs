// SPDX-License-Identifier: MIT OR Apache-2.0
//! Stream aggregation utilities for assembling final responses from event streams.

use std::collections::BTreeMap;
use std::time::Instant;

use abp_core::{AgentEvent, AgentEventKind};
use serde::{Deserialize, Serialize};

/// Aggregated view of a single tool call assembled from [`AgentEventKind::ToolCall`]
/// and [`AgentEventKind::ToolResult`] events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallAggregate {
    /// Tool use identifier (from the originating `ToolCall` event).
    pub id: String,
    /// Name of the tool.
    pub name: String,
    /// Assembled input arguments serialized as a JSON string.
    pub arguments: String,
    /// Result text from the corresponding `ToolResult`, if received.
    pub result: Option<String>,
}

/// High-level summary of a completed (or in-progress) event stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSummary {
    /// Total number of events observed.
    pub total_events: usize,
    /// Total length (in bytes) of the assembled assistant text.
    pub text_length: usize,
    /// Number of distinct tool calls observed.
    pub tool_call_count: usize,
    /// Whether any thinking text was captured.
    pub has_thinking: bool,
    /// Whether any error events were observed.
    pub has_errors: bool,
    /// Wall-clock duration in milliseconds from first to last event, if available.
    pub duration_ms: Option<u64>,
}

/// Collects [`AgentEvent`]s and assembles final text output, tool calls,
/// thinking, and error information from an event stream.
#[derive(Debug, Clone)]
pub struct StreamAggregator {
    /// Assembled assistant text from `AssistantDelta` events.
    text: String,
    /// Aggregated tool calls keyed by `tool_use_id` (or tool name as fallback).
    tool_calls: Vec<ToolCallAggregate>,
    /// Index from tool_use_id → position in `tool_calls`.
    tool_call_index: BTreeMap<String, usize>,
    /// Assembled thinking text (reserved for future `ThinkingDelta` events).
    thinking_text: String,
    /// Error events collected during the stream.
    errors: Vec<AgentEvent>,
    /// Whether a `RunCompleted` event has been observed.
    complete: bool,
    /// Total number of events pushed.
    event_count: usize,
    /// Timestamp of the first event pushed.
    first_event: Option<Instant>,
    /// Timestamp of the most recent event pushed.
    last_event: Option<Instant>,
}

impl StreamAggregator {
    /// Create a new empty aggregator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            text: String::new(),
            tool_calls: Vec::new(),
            tool_call_index: BTreeMap::new(),
            thinking_text: String::new(),
            errors: Vec::new(),
            complete: false,
            event_count: 0,
            first_event: None,
            last_event: None,
        }
    }

    /// Push an event into the aggregator, updating all internal state.
    pub fn push(&mut self, event: &AgentEvent) {
        let now = Instant::now();
        if self.first_event.is_none() {
            self.first_event = Some(now);
        }
        self.last_event = Some(now);
        self.event_count += 1;

        match &event.kind {
            AgentEventKind::AssistantDelta { text } => {
                self.text.push_str(text);
            }
            AgentEventKind::AssistantMessage { text } => {
                self.text.push_str(text);
            }
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                input,
                ..
            } => {
                let key = tool_use_id
                    .clone()
                    .unwrap_or_else(|| format!("{}_{}", tool_name, self.tool_calls.len()));
                let agg = ToolCallAggregate {
                    id: key.clone(),
                    name: tool_name.clone(),
                    arguments: input.to_string(),
                    result: None,
                };
                let idx = self.tool_calls.len();
                self.tool_calls.push(agg);
                self.tool_call_index.insert(key, idx);
            }
            AgentEventKind::ToolResult {
                tool_use_id: Some(id),
                output,
                ..
            } => {
                if let Some(&idx) = self.tool_call_index.get(id) {
                    self.tool_calls[idx].result = Some(output.to_string());
                }
            }
            AgentEventKind::RunCompleted { .. } => {
                self.complete = true;
            }
            AgentEventKind::Error { .. } => {
                self.errors.push(event.clone());
            }
            // RunStarted, FileChanged, CommandExecuted, Warning — tracked only by count
            _ => {}
        }
    }

    /// Return the assembled assistant text from `AssistantDelta` and
    /// `AssistantMessage` events.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Return the aggregated tool calls.
    #[must_use]
    pub fn tool_calls(&self) -> &[ToolCallAggregate] {
        &self.tool_calls
    }

    /// Return assembled thinking text, or `None` if no thinking events were
    /// received.
    #[must_use]
    pub fn thinking(&self) -> Option<&str> {
        if self.thinking_text.is_empty() {
            None
        } else {
            Some(&self.thinking_text)
        }
    }

    /// Return all error events collected during the stream.
    #[must_use]
    pub fn errors(&self) -> &[AgentEvent] {
        &self.errors
    }

    /// Whether a `RunCompleted` event has been observed.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// Total number of events pushed into the aggregator.
    #[must_use]
    pub fn event_count(&self) -> usize {
        self.event_count
    }

    /// Produce a [`StreamSummary`] snapshot of the current aggregation state.
    #[must_use]
    pub fn to_summary(&self) -> StreamSummary {
        let duration_ms = match (self.first_event, self.last_event) {
            (Some(first), Some(last)) => {
                let dur = last.duration_since(first);
                Some(dur.as_millis() as u64)
            }
            _ => None,
        };

        StreamSummary {
            total_events: self.event_count,
            text_length: self.text.len(),
            tool_call_count: self.tool_calls.len(),
            has_thinking: !self.thinking_text.is_empty(),
            has_errors: !self.errors.is_empty(),
            duration_ms,
        }
    }
}

impl Default for StreamAggregator {
    fn default() -> Self {
        Self::new()
    }
}
