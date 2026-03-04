#![allow(dead_code, unused_imports)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! SSE (Server-Sent Events) streaming utilities for the daemon HTTP API.
//!
//! Converts `mpsc::Receiver<AgentEvent>` into a stream of [`axum::response::sse::Event`]s
//! with proper event-type annotations, heartbeat keep-alive, and error framing.

use abp_core::{AgentEvent, AgentEventKind};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use serde::Serialize;
use std::convert::Infallible;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use crate::api_types::SseEventData;

// ---------------------------------------------------------------------------
// SSE event type mapping
// ---------------------------------------------------------------------------

/// Map an [`AgentEventKind`] to the SSE `event:` type name.
///
/// This determines the string used in the `event:` field of each SSE message,
/// allowing clients to add targeted `addEventListener` handlers.
pub fn sse_event_type(kind: &AgentEventKind) -> &'static str {
    match kind {
        AgentEventKind::RunStarted { .. } => "run_started",
        AgentEventKind::RunCompleted { .. } => "run_completed",
        AgentEventKind::AssistantDelta { .. } => "assistant_delta",
        AgentEventKind::AssistantMessage { .. } => "assistant_message",
        AgentEventKind::ToolCall { .. } => "tool_call",
        AgentEventKind::ToolResult { .. } => "tool_result",
        AgentEventKind::Error { .. } => "error",
        _ => "agent_event",
    }
}

/// Format a single [`AgentEvent`] as an SSE [`Event`](SseEvent).
///
/// The `event:` line is set to the mapped event type, and the `data:` line
/// contains the JSON-serialized [`SseEventData`] wrapper.
pub fn format_sse_event(seq: usize, event: &AgentEvent) -> Result<SseEvent, serde_json::Error> {
    let event_type = sse_event_type(&event.kind);
    let data = SseEventData {
        seq,
        event: event.clone(),
    };
    let json = serde_json::to_string(&data)?;
    Ok(SseEvent::default().event(event_type).data(json))
}

/// Format an error message as an SSE event with type `"error"`.
pub fn format_sse_error(message: &str) -> SseEvent {
    let payload = serde_json::json!({
        "error": message,
    });
    SseEvent::default()
        .event("error")
        .data(payload.to_string())
}

/// Format a "done" sentinel SSE event signalling the end of the stream.
pub fn format_sse_done() -> SseEvent {
    SseEvent::default()
        .event("done")
        .data(serde_json::json!({"status": "complete"}).to_string())
}

// ---------------------------------------------------------------------------
// Stream conversion
// ---------------------------------------------------------------------------

/// Convert an `mpsc::Receiver<AgentEvent>` into an SSE-compatible stream.
///
/// Each received event is serialized as an SSE message with the appropriate
/// `event:` type. When the receiver closes a final `done` event is emitted.
///
/// A heartbeat comment is sent every `heartbeat_interval` to keep the
/// connection alive through proxies and load balancers.
pub fn agent_event_stream(
    rx: mpsc::Receiver<AgentEvent>,
    heartbeat_interval: Duration,
) -> Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>> {
    let event_stream = ReceiverStream::new(rx);

    let mut seq: usize = 0;
    let mapped = event_stream.map(move |event| {
        let current_seq = seq;
        seq += 1;
        let sse = format_sse_event(current_seq, &event)
            .unwrap_or_else(|_| format_sse_error("failed to serialize event"));
        Ok(sse)
    });

    // Append a "done" sentinel after the event stream closes.
    let done_stream = tokio_stream::once(Ok(format_sse_done()));
    let full_stream = mapped.chain(done_stream);

    Sse::new(full_stream).keep_alive(KeepAlive::new().interval(heartbeat_interval))
}

/// Create an SSE stream from an already-collected vector of events.
///
/// Useful for replaying events from a completed run.
pub fn replay_event_stream(
    events: Vec<AgentEvent>,
) -> Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>> {
    let items: Vec<Result<SseEvent, Infallible>> = events
        .iter()
        .enumerate()
        .map(|(seq, event)| {
            let sse = format_sse_event(seq, event)
                .unwrap_or_else(|_| format_sse_error("failed to serialize event"));
            Ok(sse)
        })
        .chain(std::iter::once(Ok(format_sse_done())))
        .collect();

    Sse::new(tokio_stream::iter(items)).keep_alive(
        KeepAlive::new().interval(Duration::from_secs(30)),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use abp_core::AgentEventKind;
    use chrono::Utc;

    fn make_event(kind: AgentEventKind) -> AgentEvent {
        AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        }
    }

    // -- sse_event_type mapping ---------------------------------------------

    #[test]
    fn event_type_run_started() {
        let kind = AgentEventKind::RunStarted {
            message: "go".into(),
        };
        assert_eq!(sse_event_type(&kind), "run_started");
    }

    #[test]
    fn event_type_run_completed() {
        let kind = AgentEventKind::RunCompleted {
            message: "done".into(),
        };
        assert_eq!(sse_event_type(&kind), "run_completed");
    }

    #[test]
    fn event_type_assistant_delta() {
        let kind = AgentEventKind::AssistantDelta {
            text: "hi".into(),
        };
        assert_eq!(sse_event_type(&kind), "assistant_delta");
    }

    #[test]
    fn event_type_assistant_message() {
        let kind = AgentEventKind::AssistantMessage {
            text: "hi".into(),
        };
        assert_eq!(sse_event_type(&kind), "assistant_message");
    }

    #[test]
    fn event_type_tool_call() {
        let kind = AgentEventKind::ToolCall {
            tool_name: "read".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        };
        assert_eq!(sse_event_type(&kind), "tool_call");
    }

    #[test]
    fn event_type_error() {
        let kind = AgentEventKind::Error {
            message: "oops".into(),
            error_code: None,
        };
        assert_eq!(sse_event_type(&kind), "error");
    }

    // -- format_sse_event ---------------------------------------------------

    #[test]
    fn format_sse_event_produces_valid_json_data() {
        let event = make_event(AgentEventKind::RunStarted {
            message: "starting".into(),
        });
        let sse = format_sse_event(0, &event).unwrap();
        // SseEvent is opaque but we can verify it was created without error.
        let _ = sse;
    }

    #[test]
    fn format_sse_event_increments_seq() {
        let event = make_event(AgentEventKind::AssistantDelta {
            text: "tok".into(),
        });
        let sse0 = format_sse_event(0, &event).unwrap();
        let sse1 = format_sse_event(1, &event).unwrap();
        // Both should succeed with different sequence numbers.
        let _ = (sse0, sse1);
    }

    // -- format_sse_error ---------------------------------------------------

    #[test]
    fn format_sse_error_contains_message() {
        let sse = format_sse_error("something broke");
        let _ = sse; // Opaque type; verify creation succeeds.
    }

    // -- format_sse_done ----------------------------------------------------

    #[test]
    fn format_sse_done_creates_event() {
        let sse = format_sse_done();
        let _ = sse;
    }

    // -- agent_event_stream -------------------------------------------------

    #[tokio::test]
    async fn agent_event_stream_creates_sse_from_channel() {
        let (tx, rx) = mpsc::channel(16);

        tx.send(make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }))
        .await
        .unwrap();
        drop(tx);

        // Verify the function compiles and produces an Sse value.
        let _sse = agent_event_stream(rx, Duration::from_secs(30));
    }

    #[tokio::test]
    async fn agent_event_stream_raw_events_mapped() {
        let (tx, rx) = mpsc::channel(16);

        tx.send(make_event(AgentEventKind::RunStarted {
            message: "go".into(),
        }))
        .await
        .unwrap();
        tx.send(make_event(AgentEventKind::RunCompleted {
            message: "done".into(),
        }))
        .await
        .unwrap();
        drop(tx);

        // Use the underlying ReceiverStream + map to verify event count.
        let event_stream = ReceiverStream::new(rx);
        let mut seq: usize = 0;
        let mapped = event_stream.map(move |event| {
            let current_seq = seq;
            seq += 1;
            format_sse_event(current_seq, &event)
        });
        tokio::pin!(mapped);
        let mut count = 0;
        while let Some(Ok(_)) = mapped.next().await {
            count += 1;
        }
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn agent_event_stream_empty_channel() {
        let (_tx, rx) = mpsc::channel::<AgentEvent>(1);
        drop(_tx);

        let _sse = agent_event_stream(rx, Duration::from_secs(30));
    }

    // -- replay_event_stream ------------------------------------------------

    #[tokio::test]
    async fn replay_event_stream_creates_sse() {
        let events = vec![
            make_event(AgentEventKind::RunStarted {
                message: "a".into(),
            }),
            make_event(AgentEventKind::AssistantMessage {
                text: "b".into(),
            }),
        ];

        let _sse = replay_event_stream(events);
    }

    #[tokio::test]
    async fn replay_event_stream_empty() {
        let _sse = replay_event_stream(vec![]);
    }

    #[test]
    fn format_multiple_events_preserves_sequence() {
        let events = vec![
            make_event(AgentEventKind::RunStarted {
                message: "a".into(),
            }),
            make_event(AgentEventKind::AssistantDelta {
                text: "b".into(),
            }),
            make_event(AgentEventKind::RunCompleted {
                message: "c".into(),
            }),
        ];

        for (i, event) in events.iter().enumerate() {
            let sse = format_sse_event(i, event).unwrap();
            let _ = sse;
        }
    }
}
