// SPDX-License-Identifier: MIT OR Apache-2.0
//! SSE (Server-Sent Events) parser for OpenAI streaming responses.
//!
//! Handles the `text/event-stream` format used by OpenAI's streaming API,
//! including `data: [DONE]` termination and incremental `ChatCompletionChunk`
//! parsing.

use crate::openai_types::ChatCompletionChunk;

/// A parsed SSE event from the OpenAI streaming API.
#[derive(Debug, Clone, PartialEq)]
pub enum SseEvent {
    /// A successfully parsed chunk.
    Chunk(ChatCompletionChunk),
    /// The stream termination signal (`data: [DONE]`).
    Done,
    /// A line that could not be parsed as a chunk.
    ParseError {
        /// The raw data line that failed to parse.
        raw: String,
        /// The error message.
        error: String,
    },
}

/// Parser for SSE streams from the OpenAI API.
///
/// Handles partial line buffering, blank-line delimiters, and the
/// `data: [DONE]` sentinel.
#[derive(Debug, Default)]
pub struct SseParser {
    buffer: String,
    done: bool,
}

impl SseParser {
    /// Create a new parser.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if the `[DONE]` sentinel has been received.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Feed a raw byte chunk into the parser and return any complete events.
    ///
    /// The input may contain partial lines; buffering is handled internally.
    pub fn feed(&mut self, input: &str) -> Vec<SseEvent> {
        self.buffer.push_str(input);
        let mut events = Vec::new();

        // Process complete lines
        while let Some(newline_pos) = self.buffer.find('\n') {
            let line = self.buffer[..newline_pos]
                .trim_end_matches('\r')
                .to_string();
            self.buffer = self.buffer[newline_pos + 1..].to_string();

            if let Some(event) = self.process_line(&line) {
                events.push(event);
            }
        }

        events
    }

    /// Flush any remaining data in the buffer (for stream end).
    pub fn flush(&mut self) -> Vec<SseEvent> {
        if self.buffer.is_empty() {
            return Vec::new();
        }
        let remaining = std::mem::take(&mut self.buffer);
        let line = remaining.trim_end_matches('\r');
        if let Some(event) = self.process_line(line) {
            vec![event]
        } else {
            Vec::new()
        }
    }

    fn process_line(&mut self, line: &str) -> Option<SseEvent> {
        // Skip blank lines (SSE event delimiter)
        if line.is_empty() {
            return None;
        }

        // Skip SSE comments
        if line.starts_with(':') {
            return None;
        }

        // Extract data field
        let data = if let Some(stripped) = line.strip_prefix("data: ") {
            stripped
        } else if let Some(stripped) = line.strip_prefix("data:") {
            stripped
        } else {
            // Not a data line (could be event:, id:, retry:) — skip
            return None;
        };

        let data = data.trim();

        // Check for termination
        if data == "[DONE]" {
            self.done = true;
            return Some(SseEvent::Done);
        }

        // Empty data lines
        if data.is_empty() {
            return None;
        }

        // Parse as JSON chunk
        match serde_json::from_str::<ChatCompletionChunk>(data) {
            Ok(chunk) => Some(SseEvent::Chunk(chunk)),
            Err(e) => Some(SseEvent::ParseError {
                raw: data.to_string(),
                error: e.to_string(),
            }),
        }
    }
}

/// Parse a complete SSE stream body into events.
///
/// Convenience function for cases where the entire response body is available.
pub fn parse_sse_stream(body: &str) -> Vec<SseEvent> {
    let mut parser = SseParser::new();
    let mut events = parser.feed(body);
    events.extend(parser.flush());
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_chunk_line(id: &str, content: Option<&str>, finish: Option<&str>) -> String {
        let delta = if let Some(c) = content {
            format!(r#"{{"content":"{}"}}"#, c)
        } else {
            "{}".to_string()
        };
        let finish_json = match finish {
            Some(f) => format!(r#""{}""#, f),
            None => "null".to_string(),
        };
        format!(
            r#"data: {{"id":"{}","object":"chat.completion.chunk","created":0,"model":"gpt-4o","choices":[{{"index":0,"delta":{},"finish_reason":{}}}]}}"#,
            id, delta, finish_json
        )
    }

    // ── Basic parsing ──────────────────────────────────────────────

    #[test]
    fn parse_single_chunk() {
        let line = format!("{}\n\n", make_chunk_line("c1", Some("Hello"), None));
        let events = parse_sse_stream(&line);
        assert_eq!(events.len(), 1);
        match &events[0] {
            SseEvent::Chunk(c) => {
                assert_eq!(c.id, "c1");
                assert_eq!(c.choices[0].delta.content.as_deref(), Some("Hello"));
            }
            _ => panic!("expected Chunk"),
        }
    }

    #[test]
    fn parse_done_signal() {
        let body = "data: [DONE]\n\n";
        let events = parse_sse_stream(body);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], SseEvent::Done);
    }

    #[test]
    fn parse_complete_stream() {
        let body = format!(
            "{}\n\n{}\n\ndata: [DONE]\n\n",
            make_chunk_line("c1", Some("Hello"), None),
            make_chunk_line("c1", Some(" world"), Some("stop")),
        );
        let events = parse_sse_stream(&body);
        assert_eq!(events.len(), 3);
        assert!(matches!(&events[0], SseEvent::Chunk(_)));
        assert!(matches!(&events[1], SseEvent::Chunk(_)));
        assert_eq!(events[2], SseEvent::Done);
    }

    #[test]
    fn parse_empty_input() {
        let events = parse_sse_stream("");
        assert!(events.is_empty());
    }

    #[test]
    fn parse_blank_lines_only() {
        let events = parse_sse_stream("\n\n\n\n");
        assert!(events.is_empty());
    }

    #[test]
    fn parse_sse_comments() {
        let body = ": this is a comment\ndata: [DONE]\n\n";
        let events = parse_sse_stream(body);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], SseEvent::Done);
    }

    #[test]
    fn parse_error_on_invalid_json() {
        let body = "data: {not valid json}\n\n";
        let events = parse_sse_stream(body);
        assert_eq!(events.len(), 1);
        match &events[0] {
            SseEvent::ParseError { raw, error } => {
                assert!(raw.contains("not valid json"));
                assert!(!error.is_empty());
            }
            _ => panic!("expected ParseError"),
        }
    }

    // ── Incremental / partial feeding ──────────────────────────────

    #[test]
    fn incremental_feed_partial_lines() {
        let mut parser = SseParser::new();
        let full_line = make_chunk_line("c1", Some("Hi"), None);

        // Feed first half
        let half = full_line.len() / 2;
        let events1 = parser.feed(&full_line[..half]);
        assert!(events1.is_empty()); // no newline yet

        // Feed rest with newline
        let events2 = parser.feed(&format!("{}\n", &full_line[half..]));
        assert_eq!(events2.len(), 1);
        assert!(matches!(&events2[0], SseEvent::Chunk(_)));
    }

    #[test]
    fn incremental_feed_multiple_chunks() {
        let mut parser = SseParser::new();
        let line1 = format!("{}\n\n", make_chunk_line("c1", Some("A"), None));
        let line2 = format!("{}\n\n", make_chunk_line("c1", Some("B"), None));

        let events = parser.feed(&format!("{}{}", line1, line2));
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn parser_done_flag() {
        let mut parser = SseParser::new();
        assert!(!parser.is_done());
        parser.feed("data: [DONE]\n");
        assert!(parser.is_done());
    }

    #[test]
    fn flush_remaining_buffer() {
        let mut parser = SseParser::new();
        // Feed without trailing newline
        parser.feed("data: [DONE]");
        assert!(!parser.is_done());
        let events = parser.flush();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], SseEvent::Done);
    }

    // ── Edge cases ─────────────────────────────────────────────────

    #[test]
    fn data_without_space_after_colon() {
        let body = "data:[DONE]\n\n";
        let events = parse_sse_stream(body);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], SseEvent::Done);
    }

    #[test]
    fn crlf_line_endings() {
        let body = format!(
            "{}\r\n\r\ndata: [DONE]\r\n\r\n",
            make_chunk_line("c1", Some("Hi"), None)
        );
        let events = parse_sse_stream(&body);
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], SseEvent::Chunk(_)));
        assert_eq!(events[1], SseEvent::Done);
    }

    #[test]
    fn non_data_fields_ignored() {
        let body = "event: message\nid: 123\nretry: 5000\ndata: [DONE]\n\n";
        let events = parse_sse_stream(body);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], SseEvent::Done);
    }

    #[test]
    fn empty_data_field_skipped() {
        let body = "data: \n\ndata: [DONE]\n\n";
        let events = parse_sse_stream(body);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], SseEvent::Done);
    }

    #[test]
    fn stream_with_interleaved_comments() {
        let body = format!(
            ": keep-alive\n{}\n\n: another comment\ndata: [DONE]\n\n",
            make_chunk_line("c1", Some("X"), None)
        );
        let events = parse_sse_stream(&body);
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], SseEvent::Chunk(_)));
        assert_eq!(events[1], SseEvent::Done);
    }

    #[test]
    fn multiple_chunks_then_done() {
        let mut lines = String::new();
        for i in 0..5 {
            lines.push_str(&format!(
                "{}\n\n",
                make_chunk_line("c1", Some(&format!("word{}", i)), None)
            ));
        }
        lines.push_str("data: [DONE]\n\n");
        let events = parse_sse_stream(&lines);
        assert_eq!(events.len(), 6); // 5 chunks + DONE
    }

    #[test]
    fn chunk_with_finish_reason() {
        let body = format!("{}\n\n", make_chunk_line("c1", None, Some("stop")));
        let events = parse_sse_stream(&body);
        assert_eq!(events.len(), 1);
        match &events[0] {
            SseEvent::Chunk(c) => {
                assert_eq!(c.choices[0].finish_reason.as_deref(), Some("stop"));
            }
            _ => panic!("expected Chunk"),
        }
    }

    #[test]
    fn parser_new_equals_default() {
        let p1 = SseParser::new();
        let p2 = SseParser::default();
        assert_eq!(p1.is_done(), p2.is_done());
    }
}
