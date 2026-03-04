// SPDX-License-Identifier: MIT OR Apache-2.0
//! SSE-compatible streaming adapter for Claude message events.
//!
//! Provides [`MessageStream`] for consuming streaming responses and
//! [`SseParser`] for parsing raw SSE text into typed [`StreamEvent`]s.

use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio_stream::Stream;

use crate::error::ClaudeShimError;
use crate::types::{MessagesResponse, StreamEvent};

// ---------------------------------------------------------------------------
// MessageStream — typed stream of StreamEvents
// ---------------------------------------------------------------------------

/// A stream of [`StreamEvent`] items from a Claude Messages API streaming response.
///
/// Implements `tokio_stream::Stream` for async iteration and provides
/// convenience methods for collecting events.
#[derive(Debug)]
pub struct MessageStream {
    events: Vec<StreamEvent>,
    index: usize,
}

impl MessageStream {
    /// Create a stream from a pre-built event list.
    #[must_use]
    pub fn from_vec(events: Vec<StreamEvent>) -> Self {
        Self { events, index: 0 }
    }

    /// Create an empty stream.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            events: Vec::new(),
            index: 0,
        }
    }

    /// Collect all remaining events into a `Vec`.
    pub async fn collect_all(mut self) -> Vec<StreamEvent> {
        use tokio_stream::StreamExt;
        let mut out = Vec::new();
        while let Some(event) = StreamExt::next(&mut self).await {
            out.push(event);
        }
        out
    }

    /// Collect all text deltas from the stream into a single string.
    pub async fn collect_text(self) -> String {
        use crate::types::StreamDelta;
        let events = self.collect_all().await;
        let mut text = String::new();
        for event in events {
            if let StreamEvent::ContentBlockDelta {
                delta: StreamDelta::TextDelta { text: t },
                ..
            } = event
            {
                text.push_str(&t);
            }
        }
        text
    }

    /// Extract the final [`MessagesResponse`] from the stream's `message_start` event.
    ///
    /// Returns `None` if the stream has no `MessageStart` event.
    #[must_use]
    pub fn initial_message(&self) -> Option<&MessagesResponse> {
        self.events.iter().find_map(|e| match e {
            StreamEvent::MessageStart { message } => Some(message),
            _ => None,
        })
    }

    /// Return the total number of events in the stream.
    #[must_use]
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
}

impl Stream for MessageStream {
    type Item = StreamEvent;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.index < self.events.len() {
            let event = self.events[self.index].clone();
            self.index += 1;
            Poll::Ready(Some(event))
        } else {
            Poll::Ready(None)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.events.len() - self.index;
        (remaining, Some(remaining))
    }
}

// ---------------------------------------------------------------------------
// SSE parser
// ---------------------------------------------------------------------------

/// Server-Sent Events parser for Claude streaming responses.
///
/// Parses raw SSE text (with `event:` and `data:` lines) into typed
/// [`StreamEvent`] values.
pub struct SseParser {
    buffer: String,
    current_event_type: Option<String>,
    done: bool,
    events: VecDeque<Result<StreamEvent, ClaudeShimError>>,
}

impl SseParser {
    /// Create a new SSE parser.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            current_event_type: None,
            done: false,
            events: VecDeque::new(),
        }
    }

    /// Feed raw SSE text into the parser.
    pub fn feed(&mut self, data: &str) {
        self.buffer.push_str(data);
        self.parse_lines();
    }

    /// Signal that all data has been received.
    pub fn finish(&mut self) {
        self.done = true;
        // Process any remaining data in buffer
        let remaining = self.buffer.trim().to_string();
        self.buffer.clear();
        if !remaining.is_empty() {
            self.process_line(&remaining);
        }
    }

    /// Drain all parsed events.
    pub fn drain(&mut self) -> impl Iterator<Item = Result<StreamEvent, ClaudeShimError>> + '_ {
        self.events.drain(..)
    }

    /// Return `true` if the stream is finished.
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.done
    }

    fn parse_lines(&mut self) {
        while let Some(newline_pos) = self.buffer.find('\n') {
            let line = self.buffer[..newline_pos]
                .trim_end_matches('\r')
                .to_string();
            self.buffer = self.buffer[newline_pos + 1..].to_string();
            self.process_line(&line);
        }
    }

    fn process_line(&mut self, line: &str) {
        // Empty line = event boundary (in SSE spec); we emit on data: lines directly.
        if line.is_empty() {
            self.current_event_type = None;
            return;
        }

        // Comment lines
        if line.starts_with(':') {
            return;
        }

        // Event type line
        if let Some(event_type) = line.strip_prefix("event: ") {
            self.current_event_type = Some(event_type.trim().to_string());
            return;
        }

        // Data line
        if let Some(data) = line.strip_prefix("data: ") {
            let data = data.trim();
            if data == "[DONE]" {
                self.done = true;
                return;
            }
            match serde_json::from_str::<StreamEvent>(data) {
                Ok(event) => self.events.push_back(Ok(event)),
                Err(e) => self
                    .events
                    .push_back(Err(ClaudeShimError::Stream(format!(
                        "failed to parse SSE event: {e}"
                    )))),
            }
        }
    }
}

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Convenience functions
// ---------------------------------------------------------------------------

/// Parse a single SSE data line into a [`StreamEvent`].
///
/// Returns `None` for `[DONE]` sentinel and non-data lines.
pub fn parse_sse_line(line: &str) -> Option<Result<StreamEvent, String>> {
    let data = line.strip_prefix("data: ")?.trim();
    if data == "[DONE]" {
        return None;
    }
    Some(serde_json::from_str(data).map_err(|e| format!("SSE parse error: {e}")))
}

/// Parse a complete SSE text block into a list of [`StreamEvent`]s.
pub fn parse_sse_text(text: &str) -> Vec<Result<StreamEvent, String>> {
    let mut parser = SseParser::new();
    parser.feed(text);
    parser.finish();
    parser
        .drain()
        .map(|r| r.map_err(|e| e.to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        ClaudeUsage, ContentBlock, MessageDeltaBody, MessagesResponse, StreamDelta, StreamEvent,
    };

    fn sample_message_start_json() -> String {
        serde_json::to_string(&StreamEvent::MessageStart {
            message: MessagesResponse {
                id: "msg_test".into(),
                type_field: "message".into(),
                role: "assistant".into(),
                content: vec![],
                model: "claude-sonnet-4-20250514".into(),
                stop_reason: None,
                usage: ClaudeUsage {
                    input_tokens: 10,
                    output_tokens: 0,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                },
            },
        })
        .unwrap()
    }

    fn sample_text_delta_json() -> String {
        serde_json::to_string(&StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::TextDelta {
                text: "Hello".into(),
            },
        })
        .unwrap()
    }

    fn sample_message_stop_json() -> String {
        serde_json::to_string(&StreamEvent::MessageStop {}).unwrap()
    }

    // ── MessageStream tests ─────────────────────────────────────────────

    #[tokio::test]
    async fn message_stream_collect_all() {
        let events = vec![
            StreamEvent::Ping {},
            StreamEvent::MessageStop {},
        ];
        let stream = MessageStream::from_vec(events);
        let collected = stream.collect_all().await;
        assert_eq!(collected.len(), 2);
    }

    #[tokio::test]
    async fn message_stream_collect_text() {
        let events = vec![
            StreamEvent::ContentBlockDelta {
                index: 0,
                delta: StreamDelta::TextDelta {
                    text: "Hello ".into(),
                },
            },
            StreamEvent::ContentBlockDelta {
                index: 0,
                delta: StreamDelta::TextDelta {
                    text: "world".into(),
                },
            },
        ];
        let stream = MessageStream::from_vec(events);
        let text = stream.collect_text().await;
        assert_eq!(text, "Hello world");
    }

    #[test]
    fn message_stream_initial_message() {
        let resp = MessagesResponse {
            id: "msg_1".into(),
            type_field: "message".into(),
            role: "assistant".into(),
            content: vec![],
            model: "claude-sonnet-4-20250514".into(),
            stop_reason: None,
            usage: ClaudeUsage {
                input_tokens: 10,
                output_tokens: 0,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        };
        let events = vec![StreamEvent::MessageStart {
            message: resp.clone(),
        }];
        let stream = MessageStream::from_vec(events);
        let msg = stream.initial_message().unwrap();
        assert_eq!(msg.id, "msg_1");
    }

    #[test]
    fn message_stream_empty() {
        let stream = MessageStream::empty();
        assert_eq!(stream.event_count(), 0);
        assert!(stream.initial_message().is_none());
    }

    #[test]
    fn message_stream_size_hint() {
        let stream = MessageStream::from_vec(vec![StreamEvent::Ping {}, StreamEvent::Ping {}]);
        assert_eq!(stream.size_hint(), (2, Some(2)));
    }

    // ── SseParser tests ─────────────────────────────────────────────────

    #[test]
    fn sse_parser_single_event() {
        let data = format!("event: message_start\ndata: {}\n\n", sample_message_start_json());
        let mut parser = SseParser::new();
        parser.feed(&data);
        let events: Vec<_> = parser.drain().collect();
        assert_eq!(events.len(), 1);
        assert!(events[0].is_ok());
    }

    #[test]
    fn sse_parser_multiple_events() {
        let data = format!(
            "event: message_start\ndata: {}\n\nevent: content_block_delta\ndata: {}\n\nevent: message_stop\ndata: {}\n\n",
            sample_message_start_json(),
            sample_text_delta_json(),
            sample_message_stop_json(),
        );
        let mut parser = SseParser::new();
        parser.feed(&data);
        let events: Vec<_> = parser.drain().collect();
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn sse_parser_done_sentinel() {
        let data = format!(
            "data: {}\n\ndata: [DONE]\n\n",
            sample_message_stop_json()
        );
        let mut parser = SseParser::new();
        parser.feed(&data);
        assert!(parser.is_done());
        let events: Vec<_> = parser.drain().collect();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn sse_parser_skips_comments() {
        let data = format!(
            ": keep-alive\ndata: {}\n\n",
            sample_text_delta_json()
        );
        let mut parser = SseParser::new();
        parser.feed(&data);
        let events: Vec<_> = parser.drain().collect();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn sse_parser_incremental_feed() {
        let json = sample_text_delta_json();
        let full = format!("data: {json}\n\n");

        let mut parser = SseParser::new();
        // Feed in chunks
        let (first, second) = full.split_at(full.len() / 2);
        parser.feed(first);
        assert_eq!(parser.drain().count(), 0);
        parser.feed(second);
        let events: Vec<_> = parser.drain().collect();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn sse_parser_invalid_json() {
        let data = "data: {not valid json}\n\n";
        let mut parser = SseParser::new();
        parser.feed(data);
        let events: Vec<_> = parser.drain().collect();
        assert_eq!(events.len(), 1);
        assert!(events[0].is_err());
    }

    #[test]
    fn sse_parser_ping_event() {
        let ping_json = serde_json::to_string(&StreamEvent::Ping {}).unwrap();
        let data = format!("event: ping\ndata: {ping_json}\n\n");
        let mut parser = SseParser::new();
        parser.feed(&data);
        let events: Vec<_> = parser.drain().collect();
        assert_eq!(events.len(), 1);
        match events[0].as_ref().unwrap() {
            StreamEvent::Ping {} => {}
            other => panic!("expected Ping, got {other:?}"),
        }
    }

    // ── parse_sse_line tests ────────────────────────────────────────────

    #[test]
    fn parse_sse_line_data() {
        let json = sample_text_delta_json();
        let line = format!("data: {json}");
        let result = parse_sse_line(&line).unwrap().unwrap();
        match result {
            StreamEvent::ContentBlockDelta {
                delta: StreamDelta::TextDelta { text },
                ..
            } => assert_eq!(text, "Hello"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_sse_line_done() {
        assert!(parse_sse_line("data: [DONE]").is_none());
    }

    #[test]
    fn parse_sse_line_non_data() {
        assert!(parse_sse_line("event: message_start").is_none());
        assert!(parse_sse_line(": comment").is_none());
        assert!(parse_sse_line("").is_none());
    }

    // ── parse_sse_text tests ────────────────────────────────────────────

    #[test]
    fn parse_sse_text_full_stream() {
        let msg_start = sample_message_start_json();
        let cb_start = serde_json::to_string(&StreamEvent::ContentBlockStart {
            index: 0,
            content_block: ContentBlock::Text {
                text: String::new(),
            },
        })
        .unwrap();
        let text_delta = sample_text_delta_json();
        let cb_stop = serde_json::to_string(&StreamEvent::ContentBlockStop { index: 0 }).unwrap();
        let msg_delta = serde_json::to_string(&StreamEvent::MessageDelta {
            delta: MessageDeltaBody {
                stop_reason: Some("end_turn".into()),
                stop_sequence: None,
            },
            usage: None,
        })
        .unwrap();
        let msg_stop = sample_message_stop_json();

        let sse = format!(
            "event: message_start\ndata: {msg_start}\n\n\
             event: content_block_start\ndata: {cb_start}\n\n\
             event: content_block_delta\ndata: {text_delta}\n\n\
             event: content_block_stop\ndata: {cb_stop}\n\n\
             event: message_delta\ndata: {msg_delta}\n\n\
             event: message_stop\ndata: {msg_stop}\n\n"
        );

        let results = parse_sse_text(&sse);
        assert_eq!(results.len(), 6);
        assert!(results.iter().all(|r| r.is_ok()));
    }
}
