// SPDX-License-Identifier: MIT OR Apache-2.0
//! Streaming types and conversions for the Gemini `streamGenerateContent` endpoint.
//!
//! Provides [`StreamGenerateContentResponse`] (the SSE chunk type) and
//! mapping functions to convert streaming chunks into ABP [`AgentEvent`]s.
//! Also includes a [`FunctionCallAccumulator`] for reassembling streamed
//! function-call fragments.

use abp_core::{AgentEvent, AgentEventKind};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::types::{Candidate, Content, Part, UsageMetadata};

// ---------------------------------------------------------------------------
// Stream chunk type
// ---------------------------------------------------------------------------

/// A single chunk in a `streamGenerateContent` SSE stream.
///
/// Mirrors the Gemini API wire format — each line of the SSE stream
/// deserialises into one of these. The final chunk typically carries
/// `usage_metadata`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StreamGenerateContentResponse {
    /// Candidate completions in this chunk.
    #[serde(default)]
    pub candidates: Vec<Candidate>,

    /// Token usage metadata (usually only in the final chunk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_metadata: Option<UsageMetadata>,
}

// ---------------------------------------------------------------------------
// Mapping to ABP events
// ---------------------------------------------------------------------------

/// Map a single streaming chunk into zero or more ABP [`AgentEvent`]s.
///
/// Text parts are emitted as [`AgentEventKind::AssistantDelta`] (incremental),
/// function calls as [`AgentEventKind::ToolCall`], and function responses as
/// [`AgentEventKind::ToolResult`].
pub fn map_stream_chunk(chunk: &StreamGenerateContentResponse) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    let now = Utc::now();

    for candidate in &chunk.candidates {
        for part in &candidate.content.parts {
            match part {
                Part::Text(text) => {
                    events.push(AgentEvent {
                        ts: now,
                        kind: AgentEventKind::AssistantDelta { text: text.clone() },
                        ext: None,
                    });
                }
                Part::FunctionCall { name, args } => {
                    events.push(AgentEvent {
                        ts: now,
                        kind: AgentEventKind::ToolCall {
                            tool_name: name.clone(),
                            tool_use_id: None,
                            parent_tool_use_id: None,
                            input: args.clone(),
                        },
                        ext: None,
                    });
                }
                Part::FunctionResponse { name, response } => {
                    events.push(AgentEvent {
                        ts: now,
                        kind: AgentEventKind::ToolResult {
                            tool_name: name.clone(),
                            tool_use_id: None,
                            output: response.clone(),
                            is_error: false,
                        },
                        ext: None,
                    });
                }
                Part::InlineData { .. } => {
                    // Inline data has no streaming event representation.
                }
            }
        }
    }

    events
}

/// Check whether a streaming chunk is the final chunk.
///
/// The final chunk typically carries `usage_metadata` and/or a
/// `finish_reason` on the last candidate.
#[must_use]
pub fn is_final_chunk(chunk: &StreamGenerateContentResponse) -> bool {
    // usage_metadata presence signals the final chunk
    if chunk.usage_metadata.is_some() {
        return true;
    }
    // A finish_reason on any candidate also signals completion
    chunk.candidates.iter().any(|c| c.finish_reason.is_some())
}

/// Construct a final streaming chunk with just a finish reason and optional usage.
#[must_use]
pub fn final_chunk(
    finish_reason: &str,
    usage: Option<UsageMetadata>,
) -> StreamGenerateContentResponse {
    StreamGenerateContentResponse {
        candidates: vec![Candidate {
            content: Content {
                role: Some("model".into()),
                parts: vec![],
            },
            finish_reason: Some(finish_reason.into()),
            safety_ratings: None,
        }],
        usage_metadata: usage,
    }
}

// ---------------------------------------------------------------------------
// Function call accumulator
// ---------------------------------------------------------------------------

/// Accumulates streamed function-call fragments into complete calls.
///
/// Gemini may stream function calls across multiple chunks. This
/// accumulator collects fragments by name and emits complete
/// [`AgentEvent`]s when [`finish`](FunctionCallAccumulator::finish) is called.
#[derive(Debug, Default)]
pub struct FunctionCallAccumulator {
    entries: Vec<AccEntry>,
}

#[derive(Debug, Clone)]
struct AccEntry {
    name: String,
    args_json: String,
}

impl FunctionCallAccumulator {
    /// Create a new empty accumulator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a function-call part into the accumulator.
    ///
    /// If a call with the same name already exists, the arguments JSON
    /// fragment is appended. Otherwise a new entry is created.
    pub fn feed(&mut self, name: &str, args: &serde_json::Value) {
        let args_str = serde_json::to_string(args).unwrap_or_default();

        if let Some(entry) = self.entries.iter_mut().find(|e| e.name == name) {
            entry.args_json.push_str(&args_str);
        } else {
            self.entries.push(AccEntry {
                name: name.to_string(),
                args_json: args_str,
            });
        }
    }

    /// Returns the number of accumulated function calls.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if no function calls have been accumulated.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Consume the accumulator and return completed tool-call events.
    pub fn finish(self) -> Vec<AgentEvent> {
        let now = Utc::now();
        self.entries
            .into_iter()
            .filter(|e| !e.name.is_empty())
            .map(|e| {
                let input = serde_json::from_str(&e.args_json)
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                AgentEvent {
                    ts: now,
                    kind: AgentEventKind::ToolCall {
                        tool_name: e.name,
                        tool_use_id: None,
                        parent_tool_use_id: None,
                        input,
                    },
                    ext: None,
                }
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text_chunk(text: &str) -> StreamGenerateContentResponse {
        StreamGenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts: vec![Part::Text(text.into())],
                },
                finish_reason: None,
                safety_ratings: None,
            }],
            usage_metadata: None,
        }
    }

    fn fc_chunk(name: &str, args: serde_json::Value) -> StreamGenerateContentResponse {
        StreamGenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts: vec![Part::FunctionCall {
                        name: name.into(),
                        args,
                    }],
                },
                finish_reason: None,
                safety_ratings: None,
            }],
            usage_metadata: None,
        }
    }

    // ── map_stream_chunk tests ──────────────────────────────────────────

    #[test]
    fn text_chunk_maps_to_assistant_delta() {
        let events = map_stream_chunk(&text_chunk("Hello"));
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "Hello"),
            other => panic!("expected AssistantDelta, got {other:?}"),
        }
    }

    #[test]
    fn function_call_chunk_maps_to_tool_call() {
        let events = map_stream_chunk(&fc_chunk("search", json!({"q": "rust"})));
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name, input, ..
            } => {
                assert_eq!(tool_name, "search");
                assert_eq!(input["q"], "rust");
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn function_response_chunk_maps_to_tool_result() {
        let chunk = StreamGenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts: vec![Part::FunctionResponse {
                        name: "search".into(),
                        response: json!({"results": []}),
                    }],
                },
                finish_reason: None,
                safety_ratings: None,
            }],
            usage_metadata: None,
        };
        let events = map_stream_chunk(&chunk);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolResult {
                tool_name, output, ..
            } => {
                assert_eq!(tool_name, "search");
                assert_eq!(output, &json!({"results": []}));
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn inline_data_chunk_produces_no_events() {
        let chunk = StreamGenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts: vec![Part::InlineData {
                        mime_type: "image/png".into(),
                        data: "abc".into(),
                    }],
                },
                finish_reason: None,
                safety_ratings: None,
            }],
            usage_metadata: None,
        };
        let events = map_stream_chunk(&chunk);
        assert!(events.is_empty());
    }

    #[test]
    fn empty_candidates_produce_no_events() {
        let chunk = StreamGenerateContentResponse {
            candidates: vec![],
            usage_metadata: None,
        };
        let events = map_stream_chunk(&chunk);
        assert!(events.is_empty());
    }

    #[test]
    fn multi_part_chunk_produces_multiple_events() {
        let chunk = StreamGenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts: vec![
                        Part::Text("Let me search.".into()),
                        Part::FunctionCall {
                            name: "search".into(),
                            args: json!({}),
                        },
                    ],
                },
                finish_reason: None,
                safety_ratings: None,
            }],
            usage_metadata: None,
        };
        let events = map_stream_chunk(&chunk);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::AssistantDelta { .. }
        ));
        assert!(matches!(&events[1].kind, AgentEventKind::ToolCall { .. }));
    }

    #[test]
    fn multi_candidate_chunk_maps_all() {
        let chunk = StreamGenerateContentResponse {
            candidates: vec![
                Candidate {
                    content: Content {
                        role: Some("model".into()),
                        parts: vec![Part::Text("A".into())],
                    },
                    finish_reason: None,
                    safety_ratings: None,
                },
                Candidate {
                    content: Content {
                        role: Some("model".into()),
                        parts: vec![Part::Text("B".into())],
                    },
                    finish_reason: None,
                    safety_ratings: None,
                },
            ],
            usage_metadata: None,
        };
        let events = map_stream_chunk(&chunk);
        assert_eq!(events.len(), 2);
    }

    // ── StreamGenerateContentResponse serde tests ───────────────────────

    #[test]
    fn stream_chunk_serde_roundtrip() {
        let chunk = text_chunk("hello");
        let json = serde_json::to_string(&chunk).unwrap();
        let parsed: StreamGenerateContentResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(chunk, parsed);
    }

    #[test]
    fn stream_chunk_with_usage_serde_roundtrip() {
        let chunk = StreamGenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts: vec![],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
            }],
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: 10,
                candidates_token_count: 20,
                total_token_count: 30,
            }),
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let parsed: StreamGenerateContentResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(chunk, parsed);
    }

    #[test]
    fn stream_chunk_camel_case_keys() {
        let chunk = StreamGenerateContentResponse {
            candidates: vec![],
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: 1,
                candidates_token_count: 2,
                total_token_count: 3,
            }),
        };
        let json = serde_json::to_string(&chunk).unwrap();
        assert!(json.contains("usageMetadata"));
        assert!(json.contains("promptTokenCount"));
        assert!(json.contains("candidatesTokenCount"));
        assert!(json.contains("totalTokenCount"));
    }

    // ── is_final_chunk tests ────────────────────────────────────────────

    #[test]
    fn is_final_with_usage_metadata() {
        let chunk = StreamGenerateContentResponse {
            candidates: vec![],
            usage_metadata: Some(UsageMetadata {
                prompt_token_count: 10,
                candidates_token_count: 20,
                total_token_count: 30,
            }),
        };
        assert!(is_final_chunk(&chunk));
    }

    #[test]
    fn is_final_with_finish_reason() {
        let chunk = StreamGenerateContentResponse {
            candidates: vec![Candidate {
                content: Content {
                    role: Some("model".into()),
                    parts: vec![],
                },
                finish_reason: Some("STOP".into()),
                safety_ratings: None,
            }],
            usage_metadata: None,
        };
        assert!(is_final_chunk(&chunk));
    }

    #[test]
    fn is_not_final_without_markers() {
        assert!(!is_final_chunk(&text_chunk("hello")));
    }

    // ── final_chunk helper ──────────────────────────────────────────────

    #[test]
    fn final_chunk_has_finish_reason() {
        let chunk = final_chunk("STOP", None);
        assert_eq!(chunk.candidates[0].finish_reason.as_deref(), Some("STOP"));
        assert!(chunk.usage_metadata.is_none());
    }

    #[test]
    fn final_chunk_with_usage() {
        let usage = UsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 5,
            total_token_count: 15,
        };
        let chunk = final_chunk("STOP", Some(usage.clone()));
        assert_eq!(chunk.usage_metadata, Some(usage));
    }

    // ── FunctionCallAccumulator tests ───────────────────────────────────

    #[test]
    fn accumulator_empty_finish_returns_empty() {
        let acc = FunctionCallAccumulator::new();
        assert!(acc.is_empty());
        assert_eq!(acc.len(), 0);
        let events = acc.finish();
        assert!(events.is_empty());
    }

    #[test]
    fn accumulator_single_call() {
        let mut acc = FunctionCallAccumulator::new();
        acc.feed("search", &json!({"q": "rust"}));
        assert_eq!(acc.len(), 1);
        assert!(!acc.is_empty());
        let events = acc.finish();
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name, input, ..
            } => {
                assert_eq!(tool_name, "search");
                assert_eq!(input["q"], "rust");
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn accumulator_multiple_calls() {
        let mut acc = FunctionCallAccumulator::new();
        acc.feed("search", &json!({"q": "rust"}));
        acc.feed("read", &json!({"file": "a.rs"}));
        assert_eq!(acc.len(), 2);
        let events = acc.finish();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn accumulator_skips_empty_name() {
        let mut acc = FunctionCallAccumulator::new();
        acc.feed("", &json!({}));
        let events = acc.finish();
        assert!(events.is_empty());
    }
}
