// SPDX-License-Identifier: MIT OR Apache-2.0
//! Message framing utilities for the ABP JSONL sidecar protocol.
//!
//! Provides [`FrameWriter`] for writing JSONL frames with proper line
//! termination, [`FrameReader`] for reading frames with size limits, and
//! [`validate_frame`] for pre-send validation.

use std::io::{self, BufRead, Write};

use serde_json::Value;

use super::error::SidecarError;
use super::frame::Frame;

/// Default maximum frame size in bytes (16 MiB).
pub const DEFAULT_MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;

// ── FrameWriter ─────────────────────────────────────────────────────

/// Writes JSONL [`Frame`]s to an underlying [`Write`] sink with proper
/// newline termination and optional size limits.
pub struct FrameWriter<W: Write> {
    writer: W,
    max_frame_size: usize,
    frames_written: u64,
}

impl<W: Write> FrameWriter<W> {
    /// Create a new writer wrapping `writer` with the default size limit.
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            max_frame_size: DEFAULT_MAX_FRAME_SIZE,
            frames_written: 0,
        }
    }

    /// Create a new writer with a custom maximum frame size in bytes.
    pub fn with_max_size(writer: W, max_frame_size: usize) -> Self {
        Self {
            writer,
            max_frame_size,
            frames_written: 0,
        }
    }

    /// Write a single [`Frame`] as a newline-terminated JSON line.
    ///
    /// Returns `Err` if serialization fails, the frame exceeds the size
    /// limit, or the underlying writer reports an I/O error.
    pub fn write_frame(&mut self, frame: &Frame) -> Result<(), SidecarError> {
        let json = serde_json::to_string(frame).map_err(SidecarError::Serialize)?;
        if json.len() > self.max_frame_size {
            return Err(SidecarError::Protocol(format!(
                "frame size {} exceeds limit {}",
                json.len(),
                self.max_frame_size
            )));
        }
        self.writer
            .write_all(json.as_bytes())
            .map_err(SidecarError::Stdin)?;
        self.writer.write_all(b"\n").map_err(SidecarError::Stdin)?;
        self.frames_written += 1;
        Ok(())
    }

    /// Flush the underlying writer.
    pub fn flush(&mut self) -> Result<(), SidecarError> {
        self.writer.flush().map_err(SidecarError::Stdin)
    }

    /// Number of frames successfully written so far.
    #[must_use]
    pub fn frames_written(&self) -> u64 {
        self.frames_written
    }

    /// Borrow the underlying writer.
    #[must_use]
    pub fn inner(&self) -> &W {
        &self.writer
    }

    /// Consume self and return the underlying writer.
    #[must_use]
    pub fn into_inner(self) -> W {
        self.writer
    }
}

// ── FrameReader ─────────────────────────────────────────────────────

/// Reads JSONL [`Frame`]s from an underlying [`BufRead`] source with
/// size limits and empty-line handling.
pub struct FrameReader<R: BufRead> {
    reader: R,
    max_frame_size: usize,
    frames_read: u64,
}

impl<R: BufRead> FrameReader<R> {
    /// Create a new reader wrapping `reader` with the default size limit.
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            max_frame_size: DEFAULT_MAX_FRAME_SIZE,
            frames_read: 0,
        }
    }

    /// Create a new reader with a custom maximum frame size in bytes.
    pub fn with_max_size(reader: R, max_frame_size: usize) -> Self {
        Self {
            reader,
            max_frame_size,
            frames_read: 0,
        }
    }

    /// Read the next frame, skipping blank lines.
    ///
    /// Returns `Ok(None)` on EOF and `Err` on I/O, size-limit, or
    /// deserialization failures.
    pub fn read_frame(&mut self) -> Result<Option<Frame>, SidecarError> {
        loop {
            let mut line = String::new();
            let n = self
                .reader
                .read_line(&mut line)
                .map_err(SidecarError::Stdout)?;
            if n == 0 {
                return Ok(None);
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.len() > self.max_frame_size {
                return Err(SidecarError::Protocol(format!(
                    "frame size {} exceeds limit {}",
                    trimmed.len(),
                    self.max_frame_size
                )));
            }
            let frame =
                serde_json::from_str::<Frame>(trimmed).map_err(SidecarError::Deserialize)?;
            self.frames_read += 1;
            return Ok(Some(frame));
        }
    }

    /// Number of frames successfully read so far.
    #[must_use]
    pub fn frames_read(&self) -> u64 {
        self.frames_read
    }

    /// Return an iterator that yields frames until EOF or error.
    pub fn frames(self) -> FrameIter<R> {
        FrameIter { reader: self }
    }
}

/// Iterator adapter over [`FrameReader`].
pub struct FrameIter<R: BufRead> {
    reader: FrameReader<R>,
}

impl<R: BufRead> Iterator for FrameIter<R> {
    type Item = Result<Frame, SidecarError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.reader.read_frame() {
            Ok(Some(frame)) => Some(Ok(frame)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

// ── validate_frame ──────────────────────────────────────────────────

/// Validation outcome for [`validate_frame`].
#[derive(Debug, Clone)]
pub struct FrameValidation {
    /// Whether the frame passed all checks.
    pub valid: bool,
    /// Human-readable issues found.
    pub issues: Vec<String>,
}

/// Validate a [`Frame`] before sending, checking structural invariants.
///
/// Checks performed:
/// - `Hello`: `contract_version` is non-empty and starts with `"abp/v"`.
/// - `Run`: `id` is non-empty.
/// - `Event` / `Final`: `ref_id` is non-empty.
/// - `Fatal`: `error` is non-empty.
/// - Serialized size does not exceed `max_size`.
#[must_use]
pub fn validate_frame(frame: &Frame, max_size: usize) -> FrameValidation {
    let mut issues = Vec::new();

    match frame {
        Frame::Hello {
            contract_version,
            backend,
            ..
        } => {
            if contract_version.is_empty() {
                issues.push("contract_version is empty".into());
            } else if !contract_version.starts_with("abp/v") {
                issues.push(format!(
                    "contract_version \"{contract_version}\" does not start with \"abp/v\""
                ));
            }
            if backend
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .is_empty()
            {
                issues.push("backend.id is missing or empty".into());
            }
        }
        Frame::Run { id, .. } => {
            if id.is_empty() {
                issues.push("run id is empty".into());
            }
        }
        Frame::Event { ref_id, .. } => {
            if ref_id.is_empty() {
                issues.push("event ref_id is empty".into());
            }
        }
        Frame::Final { ref_id, .. } => {
            if ref_id.is_empty() {
                issues.push("final ref_id is empty".into());
            }
        }
        Frame::Fatal { error, .. } => {
            if error.is_empty() {
                issues.push("fatal error message is empty".into());
            }
        }
        Frame::Cancel { ref_id, .. } => {
            if ref_id.is_empty() {
                issues.push("cancel ref_id is empty".into());
            }
        }
        Frame::Ping { .. } | Frame::Pong { .. } => {}
    }

    // Size check.
    if let Ok(json) = serde_json::to_string(frame) {
        if json.len() > max_size {
            issues.push(format!(
                "serialized size {} exceeds limit {max_size}",
                json.len()
            ));
        }
    } else {
        issues.push("frame could not be serialized".into());
    }

    FrameValidation {
        valid: issues.is_empty(),
        issues,
    }
}

// ── Convenience I/O helpers ─────────────────────────────────────────

/// Write a series of frames to a writer, returning the bytes written.
///
/// This is a convenience wrapper around [`FrameWriter`] that flushes
/// after writing all frames.
pub fn write_frames(writer: impl Write, frames: &[Frame]) -> Result<u64, SidecarError> {
    let mut w = FrameWriter::new(writer);
    for f in frames {
        w.write_frame(f)?;
    }
    w.flush()?;
    Ok(w.frames_written())
}

/// Read all frames from a reader until EOF.
pub fn read_all_frames(reader: impl BufRead) -> Result<Vec<Frame>, SidecarError> {
    let r = FrameReader::new(reader);
    r.frames().collect()
}

/// Encode a single frame to a JSON string (without trailing newline).
///
/// Useful for tests and diagnostics.
pub fn frame_to_json(frame: &Frame) -> Result<String, SidecarError> {
    serde_json::to_string(frame).map_err(SidecarError::Serialize)
}

/// Decode a single JSON string into a [`Frame`].
pub fn json_to_frame(json: &str) -> Result<Frame, SidecarError> {
    serde_json::from_str(json).map_err(SidecarError::Deserialize)
}

/// Create a [`std::io::BufReader`] from a byte slice — handy for tests.
pub fn buf_reader_from_bytes(data: &[u8]) -> io::BufReader<&[u8]> {
    io::BufReader::new(data)
}
