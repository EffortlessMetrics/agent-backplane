// SPDX-License-Identifier: MIT OR Apache-2.0
//! Server-Sent Events (SSE) parser for Claude streaming responses.
//!
//! The Claude Messages API streams responses using the SSE protocol. Each event
//! has an `event:` line and a `data:` line containing JSON. This module parses
//! the raw byte stream into typed [`StreamEvent`](crate::claude_types::StreamEvent)s.

use crate::claude_types::StreamEvent;
use crate::error::BridgeError;

/// Parse a single SSE data line (the JSON payload after `data: `) into a
/// [`StreamEvent`].
pub fn parse_event_data(data: &str) -> Result<StreamEvent, BridgeError> {
    serde_json::from_str(data).map_err(|e| {
        BridgeError::Run(format!(
            "failed to parse SSE event data: {e}: {data}"
        ))
    })
}

/// An incremental SSE line parser.
///
/// Feed it lines from the SSE stream and it will emit parsed [`StreamEvent`]s
/// when a complete event (event + data) has been received.
#[derive(Debug, Default)]
pub struct SseParser {
    event_type: Option<String>,
    data_buf: String,
}

impl SseParser {
    /// Create a new parser.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a single line from the SSE stream.
    ///
    /// Returns `Some(event)` when a complete SSE event has been assembled,
    /// or `None` if more data is needed. Blank lines signal the end of an
    /// event block in the SSE protocol.
    pub fn feed_line(&mut self, line: &str) -> Result<Option<StreamEvent>, BridgeError> {
        let line = line.trim_end_matches(['\r', '\n']);

        // Blank line = end of event block
        if line.is_empty() {
            return self.flush();
        }

        if let Some(rest) = line.strip_prefix("event:") {
            self.event_type = Some(rest.trim().to_string());
            return Ok(None);
        }

        if let Some(rest) = line.strip_prefix("data:") {
            let data = rest.trim_start();
            if !self.data_buf.is_empty() {
                self.data_buf.push('\n');
            }
            self.data_buf.push_str(data);
            return Ok(None);
        }

        // Comment lines (starting with `:`) and unknown fields are ignored per SSE spec.
        Ok(None)
    }

    /// Flush any buffered event data and reset the parser state.
    fn flush(&mut self) -> Result<Option<StreamEvent>, BridgeError> {
        if self.data_buf.is_empty() {
            self.event_type = None;
            return Ok(None);
        }

        let data = std::mem::take(&mut self.data_buf);
        let _event_type = self.event_type.take();

        parse_event_data(&data).map(Some)
    }

    /// Parse a complete SSE text blob (multiple events separated by blank lines).
    ///
    /// Useful for testing or processing a buffered SSE response.
    pub fn parse_all(text: &str) -> Result<Vec<StreamEvent>, BridgeError> {
        let mut parser = Self::new();
        let mut events = Vec::new();

        for line in text.lines() {
            if let Some(event) = parser.feed_line(line)? {
                events.push(event);
            }
        }

        // Flush any trailing event (no trailing blank line)
        if let Some(event) = parser.flush()? {
            events.push(event);
        }

        Ok(events)
    }
}
