// SPDX-License-Identifier: MIT OR Apache-2.0
//! Enhanced JSONL codec with chunked reading, line-length limits, error
//! recovery, and throughput metrics.

use abp_protocol::{Envelope, JsonlCodec};
use tracing::warn;

/// Default maximum line length in bytes (10 MiB).
pub const DEFAULT_MAX_LINE_LEN: usize = 10 * 1024 * 1024;

/// Cumulative metrics tracked by [`StreamingCodec`].
#[derive(Debug, Clone, Default)]
pub struct CodecMetrics {
    /// Total bytes fed into the codec.
    pub bytes_read: u64,
    /// Lines successfully parsed into [`Envelope`] values.
    pub lines_parsed: u64,
    /// Malformed lines that were skipped.
    pub errors_skipped: u64,
}

/// Enhanced JSONL codec with chunked reading, line-length limits, error
/// recovery, and throughput metrics.
///
/// Unlike [`abp_protocol::stream::StreamParser`] this codec logs warnings
/// and skips malformed lines instead of propagating errors, which makes it
/// suitable for long-running sidecar connections where a single bad line
/// should not tear down the stream.
///
/// # Examples
///
/// ```
/// use abp_sidecar_utils::codec::StreamingCodec;
///
/// let mut codec = StreamingCodec::new();
/// let envelopes = codec.push(
///     b"{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"boom\"}\n"
/// );
/// assert_eq!(envelopes.len(), 1);
/// assert_eq!(codec.metrics().lines_parsed, 1);
/// ```
#[derive(Debug)]
pub struct StreamingCodec {
    buf: Vec<u8>,
    max_line_len: usize,
    metrics: CodecMetrics,
}

impl Default for StreamingCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamingCodec {
    /// Create a codec with default settings (10 MiB line limit).
    #[must_use]
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            max_line_len: DEFAULT_MAX_LINE_LEN,
            metrics: CodecMetrics::default(),
        }
    }

    /// Create a codec with a custom maximum line length in bytes.
    #[must_use]
    pub fn with_max_line_len(max_line_len: usize) -> Self {
        Self {
            buf: Vec::new(),
            max_line_len,
            metrics: CodecMetrics::default(),
        }
    }

    /// Current cumulative metrics.
    #[must_use]
    pub fn metrics(&self) -> &CodecMetrics {
        &self.metrics
    }

    /// Reset the internal buffer and metrics.
    pub fn reset(&mut self) {
        self.buf.clear();
        self.metrics = CodecMetrics::default();
    }

    /// Feed a chunk of bytes and return successfully parsed envelopes.
    ///
    /// Malformed lines are skipped with a warning log. Partial trailing
    /// data is buffered until the next call.
    pub fn push(&mut self, data: &[u8]) -> Vec<Envelope> {
        self.metrics.bytes_read += data.len() as u64;
        self.buf.extend_from_slice(data);
        self.drain_lines()
    }

    /// Flush any remaining buffered data, treating it as the final
    /// (possibly unterminated) line.
    pub fn finish(&mut self) -> Vec<Envelope> {
        if !self.buf.is_empty() && !self.buf.ends_with(b"\n") {
            self.buf.push(b'\n');
        }
        self.drain_lines()
    }

    /// Number of buffered bytes not yet consumed.
    #[must_use]
    pub fn buffered_len(&self) -> usize {
        self.buf.len()
    }

    // -- internal ---------------------------------------------------------

    fn drain_lines(&mut self) -> Vec<Envelope> {
        let mut results = Vec::new();

        while let Some(newline_pos) = self.buf.iter().position(|&b| b == b'\n') {
            let line_bytes: Vec<u8> = self.buf.drain(..=newline_pos).collect();
            let line_bytes = &line_bytes[..line_bytes.len() - 1]; // strip '\n'

            // Enforce line-length limit.
            if line_bytes.len() > self.max_line_len {
                warn!(
                    bytes = line_bytes.len(),
                    max = self.max_line_len,
                    "skipping oversized JSONL line"
                );
                self.metrics.errors_skipped += 1;
                continue;
            }

            // Convert to UTF-8.
            let line = match std::str::from_utf8(line_bytes) {
                Ok(s) => s,
                Err(e) => {
                    warn!("skipping non-UTF-8 JSONL line: {e}");
                    self.metrics.errors_skipped += 1;
                    continue;
                }
            };

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            match JsonlCodec::decode(trimmed) {
                Ok(env) => {
                    self.metrics.lines_parsed += 1;
                    results.push(env);
                }
                Err(e) => {
                    warn!("skipping malformed JSONL line: {e}");
                    self.metrics.errors_skipped += 1;
                }
            }
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fatal_line(msg: &str) -> String {
        format!("{{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"{msg}\"}}\n")
    }

    #[test]
    fn basic_push() {
        let mut codec = StreamingCodec::new();
        let line = fatal_line("boom");
        let envs = codec.push(line.as_bytes());
        assert_eq!(envs.len(), 1);
        assert!(matches!(&envs[0], Envelope::Fatal { error, .. } if error == "boom"));
        assert_eq!(codec.metrics().lines_parsed, 1);
        assert_eq!(codec.metrics().bytes_read, line.len() as u64);
    }

    #[test]
    fn chunked_reading() {
        let mut codec = StreamingCodec::new();
        let line = fatal_line("chunked");
        let (a, b) = line.as_bytes().split_at(10);
        assert!(codec.push(a).is_empty());
        let envs = codec.push(b);
        assert_eq!(envs.len(), 1);
        assert_eq!(codec.metrics().lines_parsed, 1);
    }

    #[test]
    fn multiple_lines_in_one_chunk() {
        let mut codec = StreamingCodec::new();
        let data = format!("{}{}", fatal_line("a"), fatal_line("b"));
        let envs = codec.push(data.as_bytes());
        assert_eq!(envs.len(), 2);
        assert_eq!(codec.metrics().lines_parsed, 2);
    }

    #[test]
    fn line_length_limit() {
        let mut codec = StreamingCodec::with_max_line_len(20);
        // Create a line longer than the limit.
        let long =
            "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"this is way too long\"}\n".to_string();
        let envs = codec.push(long.as_bytes());
        assert!(envs.is_empty());
        assert_eq!(codec.metrics().errors_skipped, 1);
    }

    #[test]
    fn error_recovery_skips_bad_lines() {
        let mut codec = StreamingCodec::new();
        let data = format!("not valid json\n{}", fatal_line("ok"));
        let envs = codec.push(data.as_bytes());
        assert_eq!(envs.len(), 1);
        assert_eq!(codec.metrics().errors_skipped, 1);
        assert_eq!(codec.metrics().lines_parsed, 1);
    }

    #[test]
    fn blank_lines_skipped() {
        let mut codec = StreamingCodec::new();
        let data = format!("\n\n{}\n\n", fatal_line("ok").trim_end());
        let envs = codec.push(data.as_bytes());
        assert_eq!(envs.len(), 1);
    }

    #[test]
    fn finish_flushes_partial() {
        let mut codec = StreamingCodec::new();
        // Push a line without a trailing newline.
        let line = "{\"t\":\"fatal\",\"ref_id\":null,\"error\":\"eof\"}";
        codec.push(line.as_bytes());
        assert_eq!(codec.buffered_len(), line.len());
        let envs = codec.finish();
        assert_eq!(envs.len(), 1);
    }

    #[test]
    fn non_utf8_skipped() {
        let mut codec = StreamingCodec::new();
        let mut data = vec![0xFF, 0xFE, b'\n'];
        data.extend_from_slice(fatal_line("ok").as_bytes());
        let envs = codec.push(&data);
        assert_eq!(envs.len(), 1);
        assert_eq!(codec.metrics().errors_skipped, 1);
    }

    #[test]
    fn reset_clears_state() {
        let mut codec = StreamingCodec::new();
        codec.push(fatal_line("x").as_bytes());
        codec.reset();
        assert_eq!(codec.buffered_len(), 0);
        assert_eq!(codec.metrics().lines_parsed, 0);
    }
}
