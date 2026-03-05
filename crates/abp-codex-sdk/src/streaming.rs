// SPDX-License-Identifier: MIT OR Apache-2.0
//! Streaming helpers for the Codex SDK.
//!
//! Re-exports the streaming types from [`types`](crate::types) and provides
//! helper functions for mapping between Codex streaming chunks and ABP
//! [`AgentEvent`]s.

use abp_core::{AgentEvent, AgentEventKind};
use chrono::Utc;

use crate::types::{CodexStreamChunk, CodexStreamDelta};

/// Map a [`CodexStreamChunk`] to a sequence of ABP [`AgentEvent`]s.
///
/// Each choice delta that carries content or a tool call produces an event.
/// Empty deltas (e.g. role-only first chunk) are skipped.
pub fn stream_chunk_to_events(chunk: &CodexStreamChunk) -> Vec<AgentEvent> {
    let now = Utc::now();
    let mut events = Vec::new();

    for choice in &chunk.choices {
        // Text content delta
        if let Some(text) = &choice.delta.content {
            if !text.is_empty() {
                events.push(AgentEvent {
                    ts: now,
                    kind: AgentEventKind::AssistantDelta { text: text.clone() },
                    ext: None,
                });
            }
        }

        // Tool call deltas
        if let Some(tool_calls) = &choice.delta.tool_calls {
            for tc in tool_calls {
                if let (Some(id), Some(func)) = (&tc.id, &tc.function) {
                    if let Some(name) = &func.name {
                        let args = func.arguments.clone().unwrap_or_default();
                        let input =
                            serde_json::from_str(&args).unwrap_or(serde_json::Value::String(args));
                        events.push(AgentEvent {
                            ts: now,
                            kind: AgentEventKind::ToolCall {
                                tool_name: name.clone(),
                                tool_use_id: Some(id.clone()),
                                parent_tool_use_id: None,
                                input,
                            },
                            ext: None,
                        });
                    }
                }
            }
        }

        // Finish reason → RunCompleted
        if let Some(reason) = &choice.finish_reason {
            events.push(AgentEvent {
                ts: now,
                kind: AgentEventKind::RunCompleted {
                    message: format!("Codex stream finished: {reason}"),
                },
                ext: None,
            });
        }
    }

    events
}

/// Build a [`CodexStreamDelta`] from a text fragment.
///
/// Convenience for constructing deltas programmatically.
#[must_use]
pub fn text_delta(text: impl Into<String>) -> CodexStreamDelta {
    CodexStreamDelta {
        role: None,
        content: Some(text.into()),
        tool_calls: None,
    }
}

/// Build a role-only [`CodexStreamDelta`] (typically the first chunk).
#[must_use]
pub fn role_delta(role: impl Into<String>) -> CodexStreamDelta {
    CodexStreamDelta {
        role: Some(role.into()),
        content: None,
        tool_calls: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        CodexStreamChoice, CodexStreamChunk, CodexStreamFunctionCall, CodexStreamToolCall,
    };

    fn make_chunk(delta: CodexStreamDelta, finish_reason: Option<String>) -> CodexStreamChunk {
        CodexStreamChunk {
            id: "chunk-1".into(),
            object: "chat.completion.chunk".into(),
            created: 1700000000,
            model: "codex-mini-latest".into(),
            choices: vec![CodexStreamChoice {
                index: 0,
                delta,
                finish_reason,
            }],
        }
    }

    #[test]
    fn text_delta_produces_assistant_delta_event() {
        let chunk = make_chunk(text_delta("Hello"), None);
        let events = stream_chunk_to_events(&chunk);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "Hello"),
            other => panic!("expected AssistantDelta, got {other:?}"),
        }
    }

    #[test]
    fn empty_content_delta_produces_no_events() {
        let chunk = make_chunk(text_delta(""), None);
        let events = stream_chunk_to_events(&chunk);
        assert!(events.is_empty());
    }

    #[test]
    fn role_only_delta_produces_no_events() {
        let chunk = make_chunk(role_delta("assistant"), None);
        let events = stream_chunk_to_events(&chunk);
        assert!(events.is_empty());
    }

    #[test]
    fn finish_reason_produces_run_completed() {
        let chunk = make_chunk(CodexStreamDelta::default(), Some("stop".into()));
        let events = stream_chunk_to_events(&chunk);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::RunCompleted { message } => {
                assert!(message.contains("stop"));
            }
            other => panic!("expected RunCompleted, got {other:?}"),
        }
    }

    #[test]
    fn tool_call_delta_produces_tool_call_event() {
        let delta = CodexStreamDelta {
            role: None,
            content: None,
            tool_calls: Some(vec![CodexStreamToolCall {
                index: 0,
                id: Some("call_abc".into()),
                call_type: Some("function".into()),
                function: Some(CodexStreamFunctionCall {
                    name: Some("read_file".into()),
                    arguments: Some(r#"{"path":"main.rs"}"#.into()),
                }),
            }]),
        };
        let chunk = make_chunk(delta, None);
        let events = stream_chunk_to_events(&chunk);
        assert_eq!(events.len(), 1);
        match &events[0].kind {
            AgentEventKind::ToolCall {
                tool_name,
                tool_use_id,
                ..
            } => {
                assert_eq!(tool_name, "read_file");
                assert_eq!(tool_use_id.as_deref(), Some("call_abc"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn text_delta_helper() {
        let d = text_delta("hi");
        assert_eq!(d.content.as_deref(), Some("hi"));
        assert!(d.role.is_none());
        assert!(d.tool_calls.is_none());
    }

    #[test]
    fn role_delta_helper() {
        let d = role_delta("assistant");
        assert_eq!(d.role.as_deref(), Some("assistant"));
        assert!(d.content.is_none());
    }
}
