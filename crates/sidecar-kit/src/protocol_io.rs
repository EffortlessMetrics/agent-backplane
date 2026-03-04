// SPDX-License-Identifier: MIT OR Apache-2.0
//! Async JSONL protocol reader/writer over [`tokio::io`] streams.
//!
//! [`ProtocolReader`] and [`ProtocolWriter`] handle JSONL framing with
//! automatic newline management, partial-read buffering, and malformed-line
//! detection.
//!
//! # Example
//! ```no_run
//! # async fn example() {
//! use sidecar_kit::protocol_io::{ProtocolReader, ProtocolWriter};
//! use sidecar_kit::Frame;
//!
//! let mut buf = Vec::new();
//! let mut writer = ProtocolWriter::new(&mut buf);
//! let hello = Frame::Hello {
//!     contract_version: "abp/v0.1".into(),
//!     backend: serde_json::json!({"id": "test"}),
//!     capabilities: serde_json::json!({}),
//!     mode: serde_json::Value::Null,
//! };
//! writer.write_frame(&hello).await.unwrap();
//!
//! let mut reader = ProtocolReader::new(buf.as_slice());
//! let frame = reader.read_frame().await.unwrap().unwrap();
//! assert!(matches!(frame, Frame::Hello { .. }));
//! # }
//! ```

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

use crate::error::SidecarError;
use crate::frame::Frame;

/// Default maximum line size in bytes (16 MiB).
pub const DEFAULT_MAX_LINE_SIZE: usize = 16 * 1024 * 1024;

// ── ProtocolReader ──────────────────────────────────────────────────

/// Async JSONL frame reader.
///
/// Wraps any [`AsyncRead`] with buffered line reading, skips blank lines,
/// and enforces a maximum line size to guard against unbounded memory use.
pub struct ProtocolReader<R: AsyncRead + Unpin> {
    reader: BufReader<R>,
    max_line_size: usize,
    frames_read: u64,
}

impl<R: AsyncRead + Unpin> ProtocolReader<R> {
    /// Create a new reader with the default maximum line size.
    pub fn new(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
            max_line_size: DEFAULT_MAX_LINE_SIZE,
            frames_read: 0,
        }
    }

    /// Create a new reader with a custom maximum line size.
    pub fn with_max_line_size(reader: R, max_line_size: usize) -> Self {
        Self {
            reader: BufReader::new(reader),
            max_line_size,
            frames_read: 0,
        }
    }

    /// Read the next [`Frame`], skipping blank lines.
    ///
    /// Returns `Ok(None)` on EOF, `Err` on malformed JSON or oversized lines.
    pub async fn read_frame(&mut self) -> Result<Option<Frame>, SidecarError> {
        let mut line = String::new();
        loop {
            line.clear();
            let n = self
                .reader
                .read_line(&mut line)
                .await
                .map_err(SidecarError::Stdout)?;
            if n == 0 {
                return Ok(None);
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.len() > self.max_line_size {
                return Err(SidecarError::Protocol(format!(
                    "line size {} exceeds limit {}",
                    trimmed.len(),
                    self.max_line_size
                )));
            }
            let frame: Frame = serde_json::from_str(trimmed).map_err(SidecarError::Deserialize)?;
            self.frames_read += 1;
            return Ok(Some(frame));
        }
    }

    /// Number of frames successfully read so far.
    #[must_use]
    pub fn frames_read(&self) -> u64 {
        self.frames_read
    }

    /// Read all remaining frames until EOF.
    pub async fn read_all(&mut self) -> Result<Vec<Frame>, SidecarError> {
        let mut frames = Vec::new();
        while let Some(frame) = self.read_frame().await? {
            frames.push(frame);
        }
        Ok(frames)
    }
}

// ── ProtocolWriter ──────────────────────────────────────────────────

/// Async JSONL frame writer.
///
/// Serializes [`Frame`]s to newline-terminated JSON and writes them to
/// the underlying [`AsyncWrite`] sink.
pub struct ProtocolWriter<W: AsyncWrite + Unpin> {
    writer: W,
    frames_written: u64,
}

impl<W: AsyncWrite + Unpin> ProtocolWriter<W> {
    /// Create a new writer.
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            frames_written: 0,
        }
    }

    /// Write a single [`Frame`] as a newline-terminated JSON line.
    pub async fn write_frame(&mut self, frame: &Frame) -> Result<(), SidecarError> {
        let json = serde_json::to_string(frame).map_err(SidecarError::Serialize)?;
        self.writer
            .write_all(json.as_bytes())
            .await
            .map_err(SidecarError::Stdin)?;
        self.writer
            .write_all(b"\n")
            .await
            .map_err(SidecarError::Stdin)?;
        self.frames_written += 1;
        Ok(())
    }

    /// Flush the underlying writer.
    pub async fn flush(&mut self) -> Result<(), SidecarError> {
        self.writer.flush().await.map_err(SidecarError::Stdin)
    }

    /// Write a frame and immediately flush.
    pub async fn write_frame_flush(&mut self, frame: &Frame) -> Result<(), SidecarError> {
        self.write_frame(frame).await?;
        self.flush().await
    }

    /// Number of frames successfully written so far.
    #[must_use]
    pub fn frames_written(&self) -> u64 {
        self.frames_written
    }

    /// Consume self and return the underlying writer.
    pub fn into_inner(self) -> W {
        self.writer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hello_frame() -> Frame {
        Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"id": "test"}),
            capabilities: json!({}),
            mode: serde_json::Value::Null,
        }
    }

    fn event_frame(text: &str) -> Frame {
        Frame::Event {
            ref_id: "run-1".into(),
            event: json!({"type": "assistant_delta", "text": text, "ts": "2025-01-01T00:00:00Z"}),
        }
    }

    #[tokio::test]
    async fn round_trip_single_frame() {
        let mut buf = Vec::new();
        let mut writer = ProtocolWriter::new(&mut buf);
        writer.write_frame(&hello_frame()).await.unwrap();
        assert_eq!(writer.frames_written(), 1);

        let mut reader = ProtocolReader::new(buf.as_slice());
        let frame = reader.read_frame().await.unwrap().unwrap();
        assert!(matches!(frame, Frame::Hello { .. }));
        assert_eq!(reader.frames_read(), 1);
    }

    #[tokio::test]
    async fn round_trip_multiple_frames() {
        let mut buf = Vec::new();
        let mut writer = ProtocolWriter::new(&mut buf);
        writer.write_frame(&hello_frame()).await.unwrap();
        writer.write_frame(&event_frame("a")).await.unwrap();
        writer.write_frame(&event_frame("b")).await.unwrap();

        let mut reader = ProtocolReader::new(buf.as_slice());
        let frames = reader.read_all().await.unwrap();
        assert_eq!(frames.len(), 3);
    }

    #[tokio::test]
    async fn skips_blank_lines() {
        let input = format!(
            "\n\n{}\n\n{}\n",
            serde_json::to_string(&hello_frame()).unwrap(),
            serde_json::to_string(&event_frame("x")).unwrap(),
        );
        let mut reader = ProtocolReader::new(input.as_bytes());
        let frames = reader.read_all().await.unwrap();
        assert_eq!(frames.len(), 2);
    }

    #[tokio::test]
    async fn eof_returns_none() {
        let mut reader = ProtocolReader::new("".as_bytes());
        assert!(reader.read_frame().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn malformed_json_returns_error() {
        let input = "not valid json\n";
        let mut reader = ProtocolReader::new(input.as_bytes());
        assert!(reader.read_frame().await.is_err());
    }

    #[tokio::test]
    async fn oversized_line_rejected() {
        let huge = format!(
            "{{\"t\":\"ping\",\"seq\":0,\"pad\":\"{}\"}}\n",
            "x".repeat(200)
        );
        let mut reader = ProtocolReader::with_max_line_size(huge.as_bytes(), 100);
        assert!(reader.read_frame().await.is_err());
    }

    #[tokio::test]
    async fn write_frame_flush() {
        let mut buf = Vec::new();
        let mut writer = ProtocolWriter::new(&mut buf);
        writer.write_frame_flush(&hello_frame()).await.unwrap();
        assert!(!buf.is_empty());
    }

    #[tokio::test]
    async fn output_ends_with_newline() {
        let mut buf = Vec::new();
        let mut writer = ProtocolWriter::new(&mut buf);
        writer.write_frame(&hello_frame()).await.unwrap();
        assert!(buf.ends_with(b"\n"));
    }

    #[tokio::test]
    async fn frames_counter_increments() {
        let mut buf = Vec::new();
        let mut writer = ProtocolWriter::new(&mut buf);
        assert_eq!(writer.frames_written(), 0);
        writer.write_frame(&hello_frame()).await.unwrap();
        writer.write_frame(&event_frame("a")).await.unwrap();
        assert_eq!(writer.frames_written(), 2);
    }
}
