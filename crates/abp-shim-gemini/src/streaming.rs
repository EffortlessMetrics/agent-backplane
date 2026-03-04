// SPDX-License-Identifier: MIT OR Apache-2.0
//! Streaming adapter for Gemini `streamGenerateContent` responses.
//!
//! The Gemini streaming API returns a JSON array where each element is a
//! `GenerateContentResponse` chunk. This module provides parsers and
//! adapters that process that stream incrementally.

use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::Stream;

use crate::types::{StreamEvent, UsageMetadata};

// ── Stream parser ───────────────────────────────────────────────────────

/// Incremental parser for Gemini streaming responses.
///
/// Gemini's `streamGenerateContent` returns a JSON array:
/// ```json
/// [
///   {"candidates":[...]},
///   {"candidates":[...],"usageMetadata":{...}}
/// ]
/// ```
///
/// The parser accepts chunks of raw bytes and emits parsed [`StreamEvent`]s
/// as they become available.
#[derive(Debug)]
pub struct GeminiStreamParser {
    buffer: String,
    events: VecDeque<StreamEvent>,
    done: bool,
}

impl GeminiStreamParser {
    /// Create a new stream parser.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            events: VecDeque::new(),
            done: false,
        }
    }

    /// Feed raw bytes into the parser.
    pub fn feed(&mut self, data: &[u8]) {
        self.buffer.push_str(&String::from_utf8_lossy(data));
        self.parse();
    }

    /// Feed a string into the parser.
    pub fn feed_str(&mut self, data: &str) {
        self.buffer.push_str(data);
        self.parse();
    }

    /// Signal end of input. Parses any remaining buffered data.
    pub fn finish(&mut self) {
        self.done = true;
        // Try to parse any remaining valid JSON objects
        self.parse_remaining();
    }

    /// Drain all parsed events.
    pub fn drain(&mut self) -> impl Iterator<Item = StreamEvent> + '_ {
        self.events.drain(..)
    }

    /// Return `true` if the stream has been finished.
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Number of pending events ready to be drained.
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.events.len()
    }

    fn parse(&mut self) {
        // Look for complete JSON objects by tracking brace depth.
        loop {
            let trimmed = self.buffer.trim_start();
            // Skip array delimiters and commas
            if trimmed.starts_with('[') || trimmed.starts_with(',') {
                let skip = self.buffer.len() - trimmed.len() + 1;
                self.buffer = self.buffer[skip..].to_string();
                continue;
            }
            if trimmed.starts_with(']') {
                self.done = true;
                let skip = self.buffer.len() - trimmed.len() + 1;
                self.buffer = self.buffer[skip..].to_string();
                return;
            }

            if !trimmed.starts_with('{') {
                break;
            }

            // Find the matching closing brace
            if let Some(end) = find_json_object_end(trimmed) {
                let json_str = &trimmed[..end];
                let skip_total = self.buffer.len() - trimmed.len() + end;
                if let Ok(event) = serde_json::from_str::<StreamEvent>(json_str) {
                    self.events.push_back(event);
                }
                self.buffer = self.buffer[skip_total..].to_string();
            } else {
                break; // Incomplete object, wait for more data
            }
        }
    }

    fn parse_remaining(&mut self) {
        let trimmed = self.buffer.trim();
        if trimmed.is_empty() {
            return;
        }
        // Try to parse the remaining buffer as a single response or array
        if let Ok(events) = serde_json::from_str::<Vec<StreamEvent>>(trimmed) {
            self.events.extend(events);
            self.buffer.clear();
        } else if let Ok(event) = serde_json::from_str::<StreamEvent>(trimmed) {
            self.events.push_back(event);
            self.buffer.clear();
        }
    }
}

impl Default for GeminiStreamParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Find the end of a JSON object starting at position 0.
///
/// Tracks brace depth and string boundaries. Returns the byte position
/// just past the closing `}`, or `None` if the object is incomplete.
fn find_json_object_end(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;

    for (i, ch) in s.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

// ── StreamAdapter ───────────────────────────────────────────────────────

/// Wraps a [`GeminiStreamParser`] and a vec of events as a [`Stream`].
///
/// Useful for converting parsed stream events into a `Stream<Item = StreamEvent>`.
pub struct StreamAdapter {
    events: VecDeque<StreamEvent>,
}

impl StreamAdapter {
    /// Create a stream adapter from a vec of events.
    #[must_use]
    pub fn from_events(events: Vec<StreamEvent>) -> Self {
        Self {
            events: events.into(),
        }
    }
}

impl Stream for StreamAdapter {
    type Item = StreamEvent;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.events.pop_front() {
            Some(event) => Poll::Ready(Some(event)),
            None => Poll::Ready(None),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.events.len();
        (len, Some(len))
    }
}

// ── Convenience: parse full response ────────────────────────────────────

/// Parse a complete Gemini streaming response body into events.
///
/// The body should be a JSON array of response objects.
#[must_use]
pub fn parse_stream_body(body: &str) -> Vec<StreamEvent> {
    let mut parser = GeminiStreamParser::new();
    parser.feed_str(body);
    parser.finish();
    parser.drain().collect()
}

/// Accumulate text from a sequence of stream events.
#[must_use]
pub fn accumulate_text(events: &[StreamEvent]) -> String {
    events
        .iter()
        .filter_map(|e| e.text())
        .collect::<Vec<_>>()
        .join("")
}

/// Extract the final [`UsageMetadata`] from a sequence of stream events.
///
/// Typically found in the last chunk.
#[must_use]
pub fn final_usage(events: &[StreamEvent]) -> Option<&UsageMetadata> {
    events.iter().rev().find_map(|e| e.usage_metadata.as_ref())
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Candidate, Content, Part};
    use serde_json::json;

    fn make_text_event(text: &str) -> String {
        json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{"text": text}]
                }
            }]
        })
        .to_string()
    }

    fn make_usage_event(prompt: u64, candidates: u64) -> String {
        json!({
            "candidates": [],
            "usageMetadata": {
                "promptTokenCount": prompt,
                "candidatesTokenCount": candidates,
                "totalTokenCount": prompt + candidates
            }
        })
        .to_string()
    }

    #[test]
    fn parser_single_object() {
        let data = make_text_event("Hello");
        let mut parser = GeminiStreamParser::new();
        parser.feed_str(&data);
        let events: Vec<_> = parser.drain().collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].text(), Some("Hello"));
    }

    #[test]
    fn parser_json_array() {
        let body = format!(
            "[{},{}]",
            make_text_event("Hello "),
            make_text_event("world")
        );
        let mut parser = GeminiStreamParser::new();
        parser.feed_str(&body);
        assert!(parser.is_done());
        let events: Vec<_> = parser.drain().collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].text(), Some("Hello "));
        assert_eq!(events[1].text(), Some("world"));
    }

    #[test]
    fn parser_incremental_feed() {
        let obj = make_text_event("Hi");
        let body = format!("[{obj}]");

        let mut parser = GeminiStreamParser::new();
        // Feed one byte at a time
        for byte in body.as_bytes() {
            parser.feed(&[*byte]);
        }
        let events: Vec<_> = parser.drain().collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].text(), Some("Hi"));
    }

    #[test]
    fn parser_with_usage_metadata() {
        let body = format!(
            "[{},{}]",
            make_text_event("Answer"),
            make_usage_event(10, 5)
        );
        let events = parse_stream_body(&body);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].text(), Some("Answer"));
        let usage = events[1].usage_metadata.as_ref().unwrap();
        assert_eq!(usage.prompt_token_count, 10);
        assert_eq!(usage.candidates_token_count, 5);
        assert_eq!(usage.total_token_count, 15);
    }

    #[test]
    fn parser_empty_array() {
        let events = parse_stream_body("[]");
        assert!(events.is_empty());
    }

    #[test]
    fn parser_empty_string() {
        let events = parse_stream_body("");
        assert!(events.is_empty());
    }

    #[test]
    fn parser_finish_parses_remaining() {
        let data = make_text_event("leftover");
        let mut parser = GeminiStreamParser::new();
        parser.feed_str(&data);
        parser.finish();
        assert!(parser.is_done());
        let events: Vec<_> = parser.drain().collect();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn accumulate_text_helper() {
        let events = vec![
            StreamEvent {
                candidates: vec![Candidate {
                    content: Content::model(vec![Part::text("Hello ")]),
                    finish_reason: None,
                    safety_ratings: None,
                }],
                usage_metadata: None,
            },
            StreamEvent {
                candidates: vec![Candidate {
                    content: Content::model(vec![Part::text("world!")]),
                    finish_reason: None,
                    safety_ratings: None,
                }],
                usage_metadata: None,
            },
        ];
        assert_eq!(accumulate_text(&events), "Hello world!");
    }

    #[test]
    fn final_usage_helper() {
        let events = vec![
            StreamEvent {
                candidates: vec![Candidate {
                    content: Content::model(vec![Part::text("text")]),
                    finish_reason: None,
                    safety_ratings: None,
                }],
                usage_metadata: None,
            },
            StreamEvent {
                candidates: vec![],
                usage_metadata: Some(UsageMetadata {
                    prompt_token_count: 20,
                    candidates_token_count: 10,
                    total_token_count: 30,
                }),
            },
        ];
        let usage = final_usage(&events).unwrap();
        assert_eq!(usage.total_token_count, 30);
    }

    #[test]
    fn final_usage_none_when_empty() {
        let events: Vec<StreamEvent> = vec![];
        assert!(final_usage(&events).is_none());
    }

    #[test]
    fn stream_adapter_yields_all_events() {
        use tokio_stream::StreamExt;

        let events = vec![
            StreamEvent {
                candidates: vec![Candidate {
                    content: Content::model(vec![Part::text("a")]),
                    finish_reason: None,
                    safety_ratings: None,
                }],
                usage_metadata: None,
            },
            StreamEvent {
                candidates: vec![Candidate {
                    content: Content::model(vec![Part::text("b")]),
                    finish_reason: None,
                    safety_ratings: None,
                }],
                usage_metadata: None,
            },
        ];

        let mut adapter = StreamAdapter::from_events(events);
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut collected = Vec::new();
            while let Some(e) = adapter.next().await {
                collected.push(e);
            }
            assert_eq!(collected.len(), 2);
            assert_eq!(collected[0].text(), Some("a"));
            assert_eq!(collected[1].text(), Some("b"));
        });
    }

    #[test]
    fn stream_adapter_size_hint() {
        let adapter = StreamAdapter::from_events(vec![
            StreamEvent {
                candidates: vec![],
                usage_metadata: None,
            },
            StreamEvent {
                candidates: vec![],
                usage_metadata: None,
            },
        ]);
        assert_eq!(adapter.size_hint(), (2, Some(2)));
    }

    #[test]
    fn find_json_object_end_basic() {
        assert_eq!(find_json_object_end("{}"), Some(2));
        assert_eq!(find_json_object_end("{\"a\":1}"), Some(7));
        assert_eq!(find_json_object_end("{\"a\":{\"b\":2}}"), Some(13));
    }

    #[test]
    fn find_json_object_end_with_strings() {
        // Braces inside strings should be ignored
        assert_eq!(find_json_object_end(r#"{"a":"{}"}"#), Some(10));
        assert_eq!(find_json_object_end(r#"{"a":"\"}"}"#), Some(11));
    }

    #[test]
    fn find_json_object_end_incomplete() {
        assert_eq!(find_json_object_end("{\"a\":"), None);
        assert_eq!(find_json_object_end("{"), None);
    }

    #[test]
    fn parser_pending_count() {
        let mut parser = GeminiStreamParser::new();
        assert_eq!(parser.pending_count(), 0);
        parser.feed_str(&make_text_event("hi"));
        assert_eq!(parser.pending_count(), 1);
        let _ = parser.drain().count();
        assert_eq!(parser.pending_count(), 0);
    }

    #[test]
    fn parser_default_trait() {
        let parser = GeminiStreamParser::default();
        assert!(!parser.is_done());
        assert_eq!(parser.pending_count(), 0);
    }
}
