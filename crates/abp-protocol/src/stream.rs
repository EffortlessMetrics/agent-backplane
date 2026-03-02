// SPDX-License-Identifier: MIT OR Apache-2.0
//! Incremental JSONL stream parser for the ABP protocol.
//!
//! [`StreamParser`] buffers incoming bytes and yields complete
//! [`Envelope`] messages as full lines become available. This is useful
//! when data arrives in arbitrary chunks (e.g. from async I/O) and lines
//! may be split across reads.
//!
//! # Examples
//!
//! ```
//! use abp_protocol::stream::StreamParser;
//! use abp_protocol::{Envelope, JsonlCodec};
//!
//! let mut parser = StreamParser::new();
//!
//! // Feed a partial line…
//! let line = JsonlCodec::encode(&Envelope::Fatal {
//!     ref_id: None,
//!     error: "boom".into(),
//!     error_code: None,
//! }).unwrap();
//! let (first, second) = line.as_bytes().split_at(10);
//!
//! assert!(parser.push(first).is_empty());
//! let envelopes = parser.push(second);
//! assert_eq!(envelopes.len(), 1);
//! ```

use crate::{Envelope, JsonlCodec, ProtocolError};

/// Incremental JSONL stream parser.
///
/// Accepts arbitrary byte chunks via [`push`](Self::push) and returns fully
/// parsed [`Envelope`] values once a complete newline-terminated line is
/// available. Handles partial lines, empty lines, and multi-line chunks.
#[derive(Debug, Clone)]
pub struct StreamParser {
    buf: Vec<u8>,
    /// Maximum allowed line length in bytes. Lines exceeding this limit
    /// produce a [`ProtocolError::Violation`].
    max_line_len: usize,
}

/// Default maximum line length (16 MiB).
const DEFAULT_MAX_LINE_LEN: usize = 16 * 1024 * 1024;

impl Default for StreamParser {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamParser {
    /// Create a new parser with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            max_line_len: DEFAULT_MAX_LINE_LEN,
        }
    }

    /// Create a new parser with a custom maximum line length.
    #[must_use]
    pub fn with_max_line_len(max_line_len: usize) -> Self {
        Self {
            buf: Vec::new(),
            max_line_len,
        }
    }

    /// Feed a chunk of bytes into the parser.
    ///
    /// Returns a `Vec` of results — one per complete line found in the
    /// accumulated buffer. Blank lines are silently skipped. Incomplete
    /// trailing data is kept in the internal buffer until the next call.
    pub fn push(&mut self, data: &[u8]) -> Vec<Result<Envelope, ProtocolError>> {
        self.buf.extend_from_slice(data);
        self.drain_lines()
    }

    /// Flush any remaining data in the buffer, treating it as the final
    /// (possibly unterminated) line.
    ///
    /// After calling this method the parser is empty and ready for reuse.
    pub fn finish(&mut self) -> Vec<Result<Envelope, ProtocolError>> {
        if !self.buf.is_empty() {
            // Ensure there is a trailing newline so drain_lines picks it up.
            if !self.buf.ends_with(b"\n") {
                self.buf.push(b'\n');
            }
        }
        self.drain_lines()
    }

    /// Return `true` if the internal buffer is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Number of buffered bytes not yet consumed.
    #[must_use]
    pub fn buffered_len(&self) -> usize {
        self.buf.len()
    }

    /// Reset the parser, discarding any buffered data.
    pub fn reset(&mut self) {
        self.buf.clear();
    }

    // -- internal ---------------------------------------------------------

    fn drain_lines(&mut self) -> Vec<Result<Envelope, ProtocolError>> {
        let mut results = Vec::new();
        while let Some(newline_pos) = self.buf.iter().position(|&b| b == b'\n') {
            // Extract the line (excluding the newline itself).
            let line_bytes: Vec<u8> = self.buf.drain(..=newline_pos).collect();
            let line_bytes = &line_bytes[..line_bytes.len() - 1]; // strip '\n'

            // Check max length.
            if line_bytes.len() > self.max_line_len {
                results.push(Err(ProtocolError::Violation(format!(
                    "line length {} exceeds maximum {}",
                    line_bytes.len(),
                    self.max_line_len
                ))));
                continue;
            }

            // Convert to UTF-8.
            let line = match std::str::from_utf8(line_bytes) {
                Ok(s) => s,
                Err(e) => {
                    results.push(Err(ProtocolError::Violation(format!("invalid UTF-8: {e}"))));
                    continue;
                }
            };

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            results.push(JsonlCodec::decode(trimmed));
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_trait() {
        let p = StreamParser::default();
        assert!(p.is_empty());
    }
}
