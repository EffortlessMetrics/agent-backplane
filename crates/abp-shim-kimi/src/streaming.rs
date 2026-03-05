// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! SSE-compatible streaming adapter for Kimi chat completions.
//!
//! Provides utilities for parsing Server-Sent Events (SSE) streams that
//! conform to the Moonshot streaming format, formatting stream chunks as
//! SSE text, and accumulating chunks into a complete response.

use std::collections::VecDeque;

use abp_core::{AgentEvent, AgentEventKind};
use abp_kimi_sdk::dialect::{
    KimiChunk, KimiChunkChoice, KimiChunkDelta, KimiChunkFunctionCall, KimiChunkToolCall,
    KimiFunctionCall, KimiToolCall,
};
use chrono::Utc;

use crate::types::{
    KimiChatChoice, KimiChatChoiceMessage, KimiChatResponse, KimiStreamChoice, KimiStreamDelta,
    KimiStreamEvent, Usage,
};

// ── SSE formatting ──────────────────────────────────────────────────────

/// Format a single [`KimiStreamEvent`] as an SSE `data:` line.
///
/// Returns a string like `"data: {json}\n\n"` suitable for writing to
/// an SSE response stream.
pub fn format_sse_chunk(chunk: &KimiStreamEvent) -> Result<String, serde_json::Error> {
    let json = serde_json::to_string(chunk)?;
    Ok(format!("data: {json}\n\n"))
}

/// Format the SSE `[DONE]` sentinel that terminates a stream.
#[must_use]
pub fn format_sse_done() -> String {
    "data: [DONE]\n\n".to_string()
}

/// Format an entire sequence of stream events as a complete SSE text block.
///
/// Includes the `[DONE]` sentinel at the end.
pub fn format_sse_stream(events: &[KimiStreamEvent]) -> Result<String, serde_json::Error> {
    let mut out = String::new();
    for event in events {
        out.push_str(&format_sse_chunk(event)?);
    }
    out.push_str(&format_sse_done());
    Ok(out)
}

// ── SSE parsing ─────────────────────────────────────────────────────────

/// Parse a single SSE `data:` line into a [`KimiStreamEvent`].
///
/// Returns `None` for the `[DONE]` sentinel, comments, and non-data lines.
pub fn parse_sse_line(line: &str) -> Option<Result<KimiStreamEvent, String>> {
    let data = line.strip_prefix("data: ")?.trim();
    if data == "[DONE]" {
        return None;
    }
    Some(serde_json::from_str(data).map_err(|e| format!("SSE parse error: {e}")))
}

/// Parse a complete SSE text block into a list of [`KimiStreamEvent`]s.
pub fn parse_sse_text(text: &str) -> Vec<Result<KimiStreamEvent, String>> {
    let mut results = Vec::new();
    for line in text.lines() {
        if let Some(r) = parse_sse_line(line) {
            results.push(r);
        }
    }
    results
}

// ── AgentEvent ↔ KimiChunk conversion ───────────────────────────────────

/// Convert an [`AgentEvent`] into a [`KimiChunk`] for streaming.
///
/// Returns `None` for event types with no streaming representation
/// (e.g. `ToolResult`, `FileChanged`, `CommandExecuted`).
pub fn agent_event_to_chunk(event: &AgentEvent, model: &str, run_id: &str) -> Option<KimiChunk> {
    let created = event.ts.timestamp() as u64;
    match &event.kind {
        AgentEventKind::AssistantDelta { text } => Some(KimiChunk {
            id: run_id.to_string(),
            object: "chat.completion.chunk".into(),
            created,
            model: model.to_string(),
            choices: vec![KimiChunkChoice {
                index: 0,
                delta: KimiChunkDelta {
                    role: None,
                    content: Some(text.clone()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
            refs: None,
        }),
        AgentEventKind::AssistantMessage { text } => Some(KimiChunk {
            id: run_id.to_string(),
            object: "chat.completion.chunk".into(),
            created,
            model: model.to_string(),
            choices: vec![KimiChunkChoice {
                index: 0,
                delta: KimiChunkDelta {
                    role: Some("assistant".into()),
                    content: Some(text.clone()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
            refs: None,
        }),
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => Some(KimiChunk {
            id: run_id.to_string(),
            object: "chat.completion.chunk".into(),
            created,
            model: model.to_string(),
            choices: vec![KimiChunkChoice {
                index: 0,
                delta: KimiChunkDelta {
                    role: None,
                    content: None,
                    tool_calls: Some(vec![KimiChunkToolCall {
                        index: 0,
                        id: Some(
                            tool_use_id
                                .clone()
                                .unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4())),
                        ),
                        call_type: Some("function".into()),
                        function: Some(KimiChunkFunctionCall {
                            name: Some(tool_name.clone()),
                            arguments: Some(serde_json::to_string(input).unwrap_or_default()),
                        }),
                    }]),
                },
                finish_reason: None,
            }],
            usage: None,
            refs: None,
        }),
        AgentEventKind::Error { message, .. } => Some(KimiChunk {
            id: run_id.to_string(),
            object: "chat.completion.chunk".into(),
            created,
            model: model.to_string(),
            choices: vec![KimiChunkChoice {
                index: 0,
                delta: KimiChunkDelta {
                    role: None,
                    content: Some(format!("Error: {message}")),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
            refs: None,
        }),
        _ => None,
    }
}

/// Create a final stop [`KimiChunk`] to signal end of stream.
#[must_use]
pub fn stop_chunk(model: &str, run_id: &str) -> KimiChunk {
    let created = Utc::now().timestamp() as u64;
    KimiChunk {
        id: run_id.to_string(),
        object: "chat.completion.chunk".into(),
        created,
        model: model.to_string(),
        choices: vec![KimiChunkChoice {
            index: 0,
            delta: KimiChunkDelta::default(),
            finish_reason: Some("stop".into()),
        }],
        usage: None,
        refs: None,
    }
}

// ── Stream accumulator ──────────────────────────────────────────────────

/// Accumulates streaming chunks into a complete response.
///
/// Feed chunks incrementally and call [`StreamAccumulator::finish`] to
/// build the final [`KimiChatResponse`].
#[derive(Debug)]
pub struct StreamAccumulator {
    id: String,
    model: String,
    created: u64,
    content: String,
    tool_calls: Vec<KimiToolCall>,
    finish_reason: Option<String>,
    usage: Option<Usage>,
    chunk_count: usize,
}

impl StreamAccumulator {
    /// Create a new accumulator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            id: String::new(),
            model: String::new(),
            created: 0,
            content: String::new(),
            tool_calls: Vec::new(),
            finish_reason: None,
            usage: None,
            chunk_count: 0,
        }
    }

    /// Feed a [`KimiStreamEvent`] into the accumulator.
    pub fn feed(&mut self, event: &KimiStreamEvent) {
        if self.id.is_empty() {
            self.id.clone_from(&event.id);
            self.model.clone_from(&event.model);
            self.created = event.created;
        }
        self.chunk_count += 1;

        for choice in &event.choices {
            if let Some(text) = &choice.delta.content {
                self.content.push_str(text);
            }
            if let Some(tcs) = &choice.delta.tool_calls {
                for tc in tcs {
                    self.merge_tool_call(tc);
                }
            }
            if choice.finish_reason.is_some() {
                self.finish_reason.clone_from(&choice.finish_reason);
            }
        }
        if event.usage.is_some() {
            self.usage = event.usage.clone();
        }
    }

    /// Feed a [`KimiChunk`] (SDK type) into the accumulator.
    pub fn feed_chunk(&mut self, chunk: &KimiChunk) {
        if self.id.is_empty() {
            self.id.clone_from(&chunk.id);
            self.model.clone_from(&chunk.model);
            self.created = chunk.created;
        }
        self.chunk_count += 1;

        for choice in &chunk.choices {
            if let Some(text) = &choice.delta.content {
                self.content.push_str(text);
            }
            if let Some(tcs) = &choice.delta.tool_calls {
                for tc in tcs {
                    self.merge_chunk_tool_call(tc);
                }
            }
            if choice.finish_reason.is_some() {
                self.finish_reason.clone_from(&choice.finish_reason);
            }
        }
        if let Some(u) = &chunk.usage {
            self.usage = Some(Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            });
        }
    }

    /// Merge an incoming tool call (from shim stream events).
    fn merge_tool_call(&mut self, tc: &KimiToolCall) {
        if let Some(existing) = self.tool_calls.iter_mut().find(|t| t.id == tc.id) {
            existing.function.arguments.push_str(&tc.function.arguments);
        } else {
            self.tool_calls.push(tc.clone());
        }
    }

    /// Merge an incoming chunk tool call fragment (from SDK chunks).
    fn merge_chunk_tool_call(&mut self, tc: &KimiChunkToolCall) {
        let id = tc.id.clone().unwrap_or_default();
        let name = tc
            .function
            .as_ref()
            .and_then(|f| f.name.clone())
            .unwrap_or_default();
        let args = tc
            .function
            .as_ref()
            .and_then(|f| f.arguments.clone())
            .unwrap_or_default();

        if let Some(existing) = self.tool_calls.iter_mut().find(|t| t.id == id) {
            existing.function.arguments.push_str(&args);
        } else if !id.is_empty() {
            self.tool_calls.push(KimiToolCall {
                id,
                call_type: tc.call_type.clone().unwrap_or_else(|| "function".into()),
                function: KimiFunctionCall {
                    name,
                    arguments: args,
                },
            });
        }
    }

    /// The number of chunks processed so far.
    #[must_use]
    pub fn chunk_count(&self) -> usize {
        self.chunk_count
    }

    /// Collect all accumulated text content.
    #[must_use]
    pub fn collected_text(&self) -> &str {
        &self.content
    }

    /// Build the final [`KimiChatResponse`] from accumulated chunks.
    #[must_use]
    pub fn finish(self) -> KimiChatResponse {
        let content = if self.content.is_empty() {
            None
        } else {
            Some(self.content)
        };
        let tool_calls = if self.tool_calls.is_empty() {
            None
        } else {
            Some(self.tool_calls)
        };
        let finish_reason = self.finish_reason.or_else(|| Some("stop".into()));

        KimiChatResponse {
            id: self.id,
            object: "chat.completion".into(),
            created: self.created,
            model: self.model,
            choices: vec![KimiChatChoice {
                index: 0,
                message: KimiChatChoiceMessage {
                    role: "assistant".into(),
                    content,
                    tool_calls,
                },
                finish_reason,
            }],
            usage: self.usage,
            search_results: None,
        }
    }
}

impl Default for StreamAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

// ── Convenience helpers ─────────────────────────────────────────────────

/// Collect streaming chunks into a single assembled response text.
///
/// Concatenates all `delta.content` fields across all chunks.
#[must_use]
pub fn collect_text(chunks: &[KimiStreamEvent]) -> String {
    let mut text = String::new();
    for chunk in chunks {
        for choice in &chunk.choices {
            if let Some(content) = &choice.delta.content {
                text.push_str(content);
            }
        }
    }
    text
}

/// Extract the finish reason from the last chunk.
#[must_use]
pub fn finish_reason(chunks: &[KimiStreamEvent]) -> Option<String> {
    chunks
        .last()
        .and_then(|c| c.choices.first())
        .and_then(|ch| ch.finish_reason.clone())
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::KimiStreamDelta;
    use serde_json::json;

    fn make_stream_event(id: &str, content: Option<&str>, finish: Option<&str>) -> KimiStreamEvent {
        KimiStreamEvent {
            id: id.into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "moonshot-v1-8k".into(),
            choices: vec![KimiStreamChoice {
                index: 0,
                delta: KimiStreamDelta {
                    role: None,
                    content: content.map(|s| s.to_string()),
                    tool_calls: None,
                },
                finish_reason: finish.map(|s| s.to_string()),
            }],
            usage: None,
            search_results: None,
        }
    }

    // ── SSE formatting tests ────────────────────────────────────────────

    #[test]
    fn format_single_sse_chunk() {
        let event = make_stream_event("c1", Some("Hi"), None);
        let sse = format_sse_chunk(&event).unwrap();
        assert!(sse.starts_with("data: "));
        assert!(sse.ends_with("\n\n"));
        assert!(sse.contains("\"content\":\"Hi\""));
    }

    #[test]
    fn format_sse_done_sentinel() {
        assert_eq!(format_sse_done(), "data: [DONE]\n\n");
    }

    #[test]
    fn format_complete_sse_stream() {
        let events = vec![
            make_stream_event("c1", Some("Hel"), None),
            make_stream_event("c1", Some("lo"), None),
            make_stream_event("c1", None, Some("stop")),
        ];
        let sse = format_sse_stream(&events).unwrap();
        assert!(sse.contains("data: [DONE]"));
        let data_count = sse.matches("data: ").count();
        assert_eq!(data_count, 4); // 3 events + [DONE]
    }

    // ── SSE parsing tests ───────────────────────────────────────────────

    #[test]
    fn parse_sse_text_roundtrip() {
        let event = make_stream_event("c1", Some("test"), None);
        let sse = format_sse_chunk(&event).unwrap();
        let full = format!("{sse}data: [DONE]\n\n");
        let results = parse_sse_text(&full);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
        assert_eq!(
            results[0].as_ref().unwrap().choices[0]
                .delta
                .content
                .as_deref(),
            Some("test")
        );
    }

    #[test]
    fn parse_sse_line_data() {
        let event = make_stream_event("c1", Some("data"), None);
        let json_str = serde_json::to_string(&event).unwrap();
        let line = format!("data: {json_str}");
        let result = parse_sse_line(&line).unwrap().unwrap();
        assert_eq!(result.id, "c1");
    }

    #[test]
    fn parse_sse_line_done_returns_none() {
        assert!(parse_sse_line("data: [DONE]").is_none());
    }

    #[test]
    fn parse_sse_line_comment_returns_none() {
        assert!(parse_sse_line(": keepalive").is_none());
    }

    #[test]
    fn parse_sse_line_non_data_returns_none() {
        assert!(parse_sse_line("event: message").is_none());
    }

    // ── AgentEvent → KimiChunk conversion tests ─────────────────────────

    #[test]
    fn agent_event_delta_to_chunk() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "Hello".into(),
            },
            ext: None,
        };
        let chunk = agent_event_to_chunk(&event, "moonshot-v1-8k", "run-1").unwrap();
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hello"));
        assert!(chunk.choices[0].delta.role.is_none());
        assert!(chunk.choices[0].finish_reason.is_none());
    }

    #[test]
    fn agent_event_message_to_chunk() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantMessage {
                text: "Full message".into(),
            },
            ext: None,
        };
        let chunk = agent_event_to_chunk(&event, "moonshot-v1-8k", "run-1").unwrap();
        assert_eq!(
            chunk.choices[0].delta.content.as_deref(),
            Some("Full message")
        );
        assert_eq!(chunk.choices[0].delta.role.as_deref(), Some("assistant"));
    }

    #[test]
    fn agent_event_tool_call_to_chunk() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "web_search".into(),
                tool_use_id: Some("call_abc".into()),
                parent_tool_use_id: None,
                input: json!({"query": "rust"}),
            },
            ext: None,
        };
        let chunk = agent_event_to_chunk(&event, "moonshot-v1-8k", "run-1").unwrap();
        let tcs = chunk.choices[0].delta.tool_calls.as_ref().unwrap();
        assert_eq!(tcs[0].id.as_deref(), Some("call_abc"));
        assert_eq!(
            tcs[0].function.as_ref().unwrap().name.as_deref(),
            Some("web_search")
        );
    }

    #[test]
    fn agent_event_error_to_chunk() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::Error {
                message: "something broke".into(),
                error_code: None,
            },
            ext: None,
        };
        let chunk = agent_event_to_chunk(&event, "moonshot-v1-8k", "run-1").unwrap();
        assert!(
            chunk.choices[0]
                .delta
                .content
                .as_deref()
                .unwrap()
                .contains("something broke")
        );
        assert_eq!(chunk.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn agent_event_unknown_returns_none() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolResult {
                tool_name: "test".into(),
                tool_use_id: Some("t1".into()),
                output: json!("ok"),
                is_error: false,
            },
            ext: None,
        };
        assert!(agent_event_to_chunk(&event, "moonshot-v1-8k", "run-1").is_none());
    }

    #[test]
    fn stop_chunk_has_finish_reason() {
        let chunk = stop_chunk("moonshot-v1-8k", "run-1");
        assert_eq!(chunk.choices[0].finish_reason.as_deref(), Some("stop"));
        assert!(chunk.choices[0].delta.content.is_none());
    }

    // ── Stream accumulator tests ────────────────────────────────────────

    #[test]
    fn accumulator_collects_text() {
        let mut acc = StreamAccumulator::new();
        acc.feed(&make_stream_event("c1", Some("Hel"), None));
        acc.feed(&make_stream_event("c1", Some("lo!"), None));
        acc.feed(&make_stream_event("c1", None, Some("stop")));

        assert_eq!(acc.collected_text(), "Hello!");
        assert_eq!(acc.chunk_count(), 3);

        let resp = acc.finish();
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hello!"));
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
        assert_eq!(resp.object, "chat.completion");
    }

    #[test]
    fn accumulator_empty_produces_none_content() {
        let acc = StreamAccumulator::new();
        let resp = acc.finish();
        assert!(resp.choices[0].message.content.is_none());
    }

    #[test]
    fn accumulator_merges_tool_calls() {
        let mut acc = StreamAccumulator::new();

        // First chunk: tool call with initial arguments
        let mut event1 = make_stream_event("c1", None, None);
        event1.choices[0].delta.tool_calls = Some(vec![KimiToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: KimiFunctionCall {
                name: "search".into(),
                arguments: r#"{"q"#.into(),
            },
        }]);
        acc.feed(&event1);

        // Second chunk: continuation of arguments
        let mut event2 = make_stream_event("c1", None, None);
        event2.choices[0].delta.tool_calls = Some(vec![KimiToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: KimiFunctionCall {
                name: "search".into(),
                arguments: r#"uery":"rust"}"#.into(),
            },
        }]);
        acc.feed(&event2);

        acc.feed(&make_stream_event("c1", None, Some("tool_calls")));

        let resp = acc.finish();
        let tcs = resp.choices[0].message.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].function.arguments, r#"{"query":"rust"}"#);
    }

    // ── Convenience helper tests ────────────────────────────────────────

    #[test]
    fn collect_text_from_events() {
        let events = vec![
            make_stream_event("c1", Some("Hello"), None),
            make_stream_event("c1", Some(" "), None),
            make_stream_event("c1", Some("world"), None),
        ];
        assert_eq!(collect_text(&events), "Hello world");
    }

    #[test]
    fn collect_text_skips_none_content() {
        let events = vec![
            make_stream_event("c1", Some("Hi"), None),
            make_stream_event("c1", None, Some("stop")),
        ];
        assert_eq!(collect_text(&events), "Hi");
    }

    #[test]
    fn finish_reason_from_last_event() {
        let events = vec![
            make_stream_event("c1", Some("Hi"), None),
            make_stream_event("c1", None, Some("stop")),
        ];
        assert_eq!(finish_reason(&events), Some("stop".into()));
    }

    #[test]
    fn finish_reason_none_when_empty() {
        let events: Vec<KimiStreamEvent> = vec![];
        assert_eq!(finish_reason(&events), None);
    }
}
