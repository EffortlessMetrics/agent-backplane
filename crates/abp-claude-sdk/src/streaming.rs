// SPDX-License-Identifier: MIT OR Apache-2.0
//! Streaming SSE types and helpers for the Anthropic Messages API.
//!
//! This module re-exports the core streaming types from [`crate::dialect`]
//! and adds builder helpers and ABP event conversion utilities for working
//! with Anthropic's server-sent event stream.
//!
//! Reference: <https://docs.anthropic.com/en/api/messages-streaming>

use abp_core::{AgentEvent, AgentEventKind};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// Re-export core streaming types under clean names.
pub use crate::dialect::{
    ClaudeApiError as ApiError, ClaudeMessageDelta as MessageDelta,
    ClaudeResponse as MessageSnapshot, ClaudeStreamDelta as StreamDelta,
    ClaudeStreamEvent as StreamEvent, ClaudeUsage as Usage,
};

pub use crate::dialect::{
    from_passthrough_event, to_passthrough_event, verify_passthrough_fidelity,
};

// ---------------------------------------------------------------------------
// SSE event wrapper
// ---------------------------------------------------------------------------

/// A parsed server-sent event from the Anthropic streaming API.
///
/// Wraps a [`StreamEvent`] with the original SSE `event:` type name
/// for faithful reconstruction of the wire format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SseEvent {
    /// The SSE `event:` field name (e.g. `"message_start"`, `"content_block_delta"`).
    pub event: String,
    /// The parsed event payload.
    pub data: StreamEvent,
}

impl SseEvent {
    /// Create a new SSE event.
    #[must_use]
    pub fn new(event: impl Into<String>, data: StreamEvent) -> Self {
        Self {
            event: event.into(),
            data,
        }
    }

    /// Render this event as SSE wire format (`event: ...\ndata: ...\n\n`).
    #[must_use]
    pub fn to_sse_string(&self) -> String {
        let data_json = serde_json::to_string(&self.data).unwrap_or_default();
        format!("event: {}\ndata: {}\n\n", self.event, data_json)
    }
}

// ---------------------------------------------------------------------------
// Stream accumulator
// ---------------------------------------------------------------------------

/// Accumulates streaming events into a final [`MessageSnapshot`].
///
/// Tracks text content, tool inputs, thinking blocks, and usage as
/// stream events arrive, then produces a complete response snapshot.
#[derive(Debug, Clone, Default)]
pub struct StreamAccumulator {
    id: String,
    model: String,
    role: String,
    content_blocks: Vec<AccumulatedBlock>,
    stop_reason: Option<String>,
    usage: Option<Usage>,
}

#[derive(Debug, Clone)]
enum AccumulatedBlock {
    Text(String),
    ToolUse {
        id: String,
        name: String,
        input_json: String,
    },
    Thinking {
        text: String,
        signature: Option<String>,
    },
}

impl StreamAccumulator {
    /// Create a new empty accumulator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a stream event into the accumulator.
    pub fn process(&mut self, event: &StreamEvent) {
        match event {
            StreamEvent::MessageStart { message } => {
                self.id.clone_from(&message.id);
                self.model.clone_from(&message.model);
                self.role.clone_from(&message.role);
                if let Some(u) = &message.usage {
                    self.usage = Some(u.clone());
                }
            }
            StreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                let idx = *index as usize;
                // Ensure enough capacity
                while self.content_blocks.len() <= idx {
                    self.content_blocks
                        .push(AccumulatedBlock::Text(String::new()));
                }
                match content_block {
                    crate::dialect::ClaudeContentBlock::Text { text } => {
                        self.content_blocks[idx] = AccumulatedBlock::Text(text.clone());
                    }
                    crate::dialect::ClaudeContentBlock::ToolUse { id, name, .. } => {
                        self.content_blocks[idx] = AccumulatedBlock::ToolUse {
                            id: id.clone(),
                            name: name.clone(),
                            input_json: String::new(),
                        };
                    }
                    crate::dialect::ClaudeContentBlock::Thinking {
                        thinking,
                        signature,
                    } => {
                        self.content_blocks[idx] = AccumulatedBlock::Thinking {
                            text: thinking.clone(),
                            signature: signature.clone(),
                        };
                    }
                    _ => {}
                }
            }
            StreamEvent::ContentBlockDelta { index, delta } => {
                let idx = *index as usize;
                if idx < self.content_blocks.len() {
                    match delta {
                        StreamDelta::TextDelta { text } => {
                            if let AccumulatedBlock::Text(ref mut buf) = self.content_blocks[idx] {
                                buf.push_str(text);
                            }
                        }
                        StreamDelta::InputJsonDelta { partial_json } => {
                            if let AccumulatedBlock::ToolUse {
                                ref mut input_json, ..
                            } = self.content_blocks[idx]
                            {
                                input_json.push_str(partial_json);
                            }
                        }
                        StreamDelta::ThinkingDelta { thinking } => {
                            if let AccumulatedBlock::Thinking { ref mut text, .. } =
                                self.content_blocks[idx]
                            {
                                text.push_str(thinking);
                            }
                        }
                        StreamDelta::SignatureDelta { signature } => {
                            if let AccumulatedBlock::Thinking {
                                signature: ref mut sig,
                                ..
                            } = self.content_blocks[idx]
                            {
                                let s = sig.get_or_insert_with(String::new);
                                s.push_str(signature);
                            }
                        }
                    }
                }
            }
            StreamEvent::MessageDelta { delta, usage } => {
                if let Some(sr) = &delta.stop_reason {
                    self.stop_reason = Some(sr.clone());
                }
                if let Some(u) = usage {
                    self.usage = Some(u.clone());
                }
            }
            StreamEvent::ContentBlockStop { .. }
            | StreamEvent::MessageStop {}
            | StreamEvent::Ping {}
            | StreamEvent::Error { .. } => {}
        }
    }

    /// Produce the final accumulated [`MessageSnapshot`].
    #[must_use]
    pub fn finish(&self) -> MessageSnapshot {
        use crate::dialect::{ClaudeContentBlock, ClaudeResponse};

        let content: Vec<ClaudeContentBlock> = self
            .content_blocks
            .iter()
            .map(|b| match b {
                AccumulatedBlock::Text(text) => ClaudeContentBlock::Text { text: text.clone() },
                AccumulatedBlock::ToolUse {
                    id,
                    name,
                    input_json,
                } => {
                    let input = serde_json::from_str(input_json)
                        .unwrap_or(serde_json::Value::Object(Default::default()));
                    ClaudeContentBlock::ToolUse {
                        id: id.clone(),
                        name: name.clone(),
                        input,
                    }
                }
                AccumulatedBlock::Thinking { text, signature } => ClaudeContentBlock::Thinking {
                    thinking: text.clone(),
                    signature: signature.clone(),
                },
            })
            .collect();

        ClaudeResponse {
            id: self.id.clone(),
            model: self.model.clone(),
            role: self.role.clone(),
            content,
            stop_reason: self.stop_reason.clone(),
            usage: self.usage.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// ABP event mapping for individual stream events
// ---------------------------------------------------------------------------

/// Map a single [`StreamEvent`] to zero or more ABP [`AgentEvent`]s.
///
/// This is the public entry point for stream-to-ABP conversion.
/// Delegates to [`crate::dialect::map_stream_event`] but is placed
/// here for discoverability alongside the streaming types.
pub fn stream_event_to_agent_events(event: &StreamEvent) -> Vec<AgentEvent> {
    crate::dialect::map_stream_event(event)
}

/// Convert a complete sequence of stream events into ABP [`AgentEvent`]s.
///
/// Processes each event in order and flattens the results.
pub fn stream_to_agent_events(events: &[StreamEvent]) -> Vec<AgentEvent> {
    events
        .iter()
        .flat_map(stream_event_to_agent_events)
        .collect()
}

/// Map an ABP [`AgentEvent`] back to a [`StreamEvent`], if possible.
///
/// Not all ABP events have a streaming equivalent; returns `None` for
/// unmappable events.
#[must_use]
pub fn agent_event_to_stream_event(event: &AgentEvent) -> Option<StreamEvent> {
    let now_usage = || Usage {
        input_tokens: 0,
        output_tokens: 0,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    };

    match &event.kind {
        AgentEventKind::AssistantDelta { text } => {
            let is_thinking = event
                .ext
                .as_ref()
                .and_then(|e| e.get("thinking"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if is_thinking {
                Some(StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: StreamDelta::ThinkingDelta {
                        thinking: text.clone(),
                    },
                })
            } else {
                Some(StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: StreamDelta::TextDelta { text: text.clone() },
                })
            }
        }
        AgentEventKind::RunStarted { .. } => Some(StreamEvent::MessageStart {
            message: MessageSnapshot {
                id: String::new(),
                model: String::new(),
                role: "assistant".into(),
                content: vec![],
                stop_reason: None,
                usage: Some(now_usage()),
            },
        }),
        AgentEventKind::RunCompleted { .. } => Some(StreamEvent::MessageStop {}),
        AgentEventKind::Error { message, .. } => Some(StreamEvent::Error {
            error: ApiError {
                error_type: "api_error".into(),
                message: message.clone(),
            },
        }),
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            input,
            ..
        } => Some(StreamEvent::ContentBlockStart {
            index: 0,
            content_block: crate::dialect::ClaudeContentBlock::ToolUse {
                id: tool_use_id.clone().unwrap_or_default(),
                name: tool_name.clone(),
                input: input.clone(),
            },
        }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialect::{ClaudeContentBlock, ClaudeResponse};
    use chrono::Utc;
    use std::collections::BTreeMap;

    // -- SseEvent --

    #[test]
    fn sse_event_to_string() {
        let event = SseEvent::new(
            "message_start",
            StreamEvent::MessageStart {
                message: ClaudeResponse {
                    id: "msg_1".into(),
                    model: "claude-sonnet-4-20250514".into(),
                    role: "assistant".into(),
                    content: vec![],
                    stop_reason: None,
                    usage: None,
                },
            },
        );
        let sse = event.to_sse_string();
        assert!(sse.starts_with("event: message_start\n"));
        assert!(sse.contains("data: "));
        assert!(sse.ends_with("\n\n"));
    }

    #[test]
    fn sse_event_serde_roundtrip() {
        let event = SseEvent::new("ping", StreamEvent::Ping {});
        let json = serde_json::to_string(&event).unwrap();
        let parsed: SseEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    // -- StreamAccumulator --

    #[test]
    fn accumulator_text_stream() {
        let mut acc = StreamAccumulator::new();
        acc.process(&StreamEvent::MessageStart {
            message: ClaudeResponse {
                id: "msg_1".into(),
                model: "claude-sonnet-4-20250514".into(),
                role: "assistant".into(),
                content: vec![],
                stop_reason: None,
                usage: Some(Usage {
                    input_tokens: 10,
                    output_tokens: 0,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                }),
            },
        });
        acc.process(&StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ClaudeContentBlock::Text {
                text: String::new(),
            },
        });
        acc.process(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::TextDelta {
                text: "Hello".into(),
            },
        });
        acc.process(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::TextDelta {
                text: " world!".into(),
            },
        });
        acc.process(&StreamEvent::ContentBlockStop { index: 0 });
        acc.process(&StreamEvent::MessageDelta {
            delta: MessageDelta {
                stop_reason: Some("end_turn".into()),
                stop_sequence: None,
            },
            usage: Some(Usage {
                input_tokens: 10,
                output_tokens: 5,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }),
        });
        acc.process(&StreamEvent::MessageStop {});

        let snapshot = acc.finish();
        assert_eq!(snapshot.id, "msg_1");
        assert_eq!(snapshot.model, "claude-sonnet-4-20250514");
        assert_eq!(snapshot.content.len(), 1);
        assert!(matches!(
            &snapshot.content[0],
            ClaudeContentBlock::Text { text } if text == "Hello world!"
        ));
        assert_eq!(snapshot.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn accumulator_tool_use_stream() {
        let mut acc = StreamAccumulator::new();
        acc.process(&StreamEvent::MessageStart {
            message: ClaudeResponse {
                id: "msg_2".into(),
                model: "claude-sonnet-4-20250514".into(),
                role: "assistant".into(),
                content: vec![],
                stop_reason: None,
                usage: None,
            },
        });
        acc.process(&StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ClaudeContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "read_file".into(),
                input: serde_json::json!({}),
            },
        });
        acc.process(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::InputJsonDelta {
                partial_json: r#"{"path":"#.into(),
            },
        });
        acc.process(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::InputJsonDelta {
                partial_json: r#""src/lib.rs"}"#.into(),
            },
        });
        acc.process(&StreamEvent::ContentBlockStop { index: 0 });

        let snapshot = acc.finish();
        assert_eq!(snapshot.content.len(), 1);
        match &snapshot.content[0] {
            ClaudeContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tu_1");
                assert_eq!(name, "read_file");
                assert_eq!(input["path"], "src/lib.rs");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn accumulator_thinking_stream() {
        let mut acc = StreamAccumulator::new();
        acc.process(&StreamEvent::MessageStart {
            message: ClaudeResponse {
                id: "msg_3".into(),
                model: "claude-sonnet-4-20250514".into(),
                role: "assistant".into(),
                content: vec![],
                stop_reason: None,
                usage: None,
            },
        });
        acc.process(&StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ClaudeContentBlock::Thinking {
                thinking: String::new(),
                signature: None,
            },
        });
        acc.process(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::ThinkingDelta {
                thinking: "Let me think".into(),
            },
        });
        acc.process(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::SignatureDelta {
                signature: "sig_abc".into(),
            },
        });
        acc.process(&StreamEvent::ContentBlockStop { index: 0 });

        let snapshot = acc.finish();
        match &snapshot.content[0] {
            ClaudeContentBlock::Thinking {
                thinking,
                signature,
            } => {
                assert_eq!(thinking, "Let me think");
                assert_eq!(signature.as_deref(), Some("sig_abc"));
            }
            other => panic!("expected Thinking, got {other:?}"),
        }
    }

    // -- stream_event_to_agent_events --

    #[test]
    fn text_delta_maps_to_assistant_delta() {
        let event = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::TextDelta { text: "hi".into() },
        };
        let events = stream_event_to_agent_events(&event);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::AssistantDelta { text } if text == "hi"
        ));
    }

    #[test]
    fn message_stop_maps_to_run_completed() {
        let events = stream_event_to_agent_events(&StreamEvent::MessageStop {});
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].kind,
            AgentEventKind::RunCompleted { .. }
        ));
    }

    #[test]
    fn ping_maps_to_nothing() {
        let events = stream_event_to_agent_events(&StreamEvent::Ping {});
        assert!(events.is_empty());
    }

    #[test]
    fn error_maps_to_error_event() {
        let event = StreamEvent::Error {
            error: ApiError {
                error_type: "overloaded_error".into(),
                message: "API overloaded".into(),
            },
        };
        let events = stream_event_to_agent_events(&event);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::Error { message, .. } => {
                assert!(message.contains("overloaded_error"));
                assert!(message.contains("API overloaded"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    // -- stream_to_agent_events --

    #[test]
    fn full_stream_to_agent_events() {
        let events = vec![
            StreamEvent::MessageStart {
                message: ClaudeResponse {
                    id: "msg_1".into(),
                    model: "claude-sonnet-4-20250514".into(),
                    role: "assistant".into(),
                    content: vec![],
                    stop_reason: None,
                    usage: None,
                },
            },
            StreamEvent::Ping {},
            StreamEvent::ContentBlockStart {
                index: 0,
                content_block: ClaudeContentBlock::Text {
                    text: String::new(),
                },
            },
            StreamEvent::ContentBlockDelta {
                index: 0,
                delta: StreamDelta::TextDelta {
                    text: "Hello".into(),
                },
            },
            StreamEvent::ContentBlockStop { index: 0 },
            StreamEvent::MessageDelta {
                delta: MessageDelta {
                    stop_reason: Some("end_turn".into()),
                    stop_sequence: None,
                },
                usage: None,
            },
            StreamEvent::MessageStop {},
        ];
        let abp_events = stream_to_agent_events(&events);
        // message_start -> RunStarted, text_delta -> AssistantDelta, message_stop -> RunCompleted
        assert!(abp_events.len() >= 3);
    }

    // -- agent_event_to_stream_event --

    #[test]
    fn assistant_delta_maps_to_text_delta() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta {
                text: "hello".into(),
            },
            ext: None,
        };
        let stream = agent_event_to_stream_event(&event).unwrap();
        assert!(matches!(
            stream,
            StreamEvent::ContentBlockDelta {
                delta: StreamDelta::TextDelta { .. },
                ..
            }
        ));
    }

    #[test]
    fn thinking_delta_maps_to_thinking_delta() {
        let mut ext = BTreeMap::new();
        ext.insert("thinking".into(), serde_json::Value::Bool(true));
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::AssistantDelta { text: "hmm".into() },
            ext: Some(ext),
        };
        let stream = agent_event_to_stream_event(&event).unwrap();
        assert!(matches!(
            stream,
            StreamEvent::ContentBlockDelta {
                delta: StreamDelta::ThinkingDelta { .. },
                ..
            }
        ));
    }

    #[test]
    fn run_started_maps_to_message_start() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunStarted {
                message: "go".into(),
            },
            ext: None,
        };
        let stream = agent_event_to_stream_event(&event).unwrap();
        assert!(matches!(stream, StreamEvent::MessageStart { .. }));
    }

    #[test]
    fn run_completed_maps_to_message_stop() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::RunCompleted {
                message: "done".into(),
            },
            ext: None,
        };
        let stream = agent_event_to_stream_event(&event).unwrap();
        assert!(matches!(stream, StreamEvent::MessageStop {}));
    }

    #[test]
    fn tool_call_maps_to_content_block_start() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::ToolCall {
                tool_name: "bash".into(),
                tool_use_id: Some("tu_1".into()),
                parent_tool_use_id: None,
                input: serde_json::json!({"command": "ls"}),
            },
            ext: None,
        };
        let stream = agent_event_to_stream_event(&event).unwrap();
        match stream {
            StreamEvent::ContentBlockStart {
                content_block: ClaudeContentBlock::ToolUse { id, name, .. },
                ..
            } => {
                assert_eq!(id, "tu_1");
                assert_eq!(name, "bash");
            }
            other => panic!("expected ContentBlockStart/ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn file_changed_unmappable() {
        let event = AgentEvent {
            ts: Utc::now(),
            kind: AgentEventKind::FileChanged {
                path: "foo.rs".into(),
                summary: "changed".into(),
            },
            ext: None,
        };
        assert!(agent_event_to_stream_event(&event).is_none());
    }
}
