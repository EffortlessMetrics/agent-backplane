// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Testing utilities for sidecar implementations.
//!
//! Provides mock I/O types and assertion helpers for testing sidecars
//! that speak the ABP JSONL protocol.
//!
//! # Example
//! ```
//! use sidecar_kit::test_utils::{MockStdout, assert_valid_hello, assert_valid_final};
//! use sidecar_kit::protocol_helpers::{send_hello, send_final};
//! use sidecar_kit::receipt_builder::TypedReceiptBuilder;
//! use abp_core::CapabilityManifest;
//!
//! let mut mock = MockStdout::new();
//! send_hello(&mut mock, "test", &CapabilityManifest::new()).unwrap();
//! let receipt = TypedReceiptBuilder::new("test").build();
//! send_final(&mut mock, "run-1", &receipt).unwrap();
//!
//! let lines = mock.lines();
//! assert_valid_hello(&lines[0]);
//! assert_valid_final(&lines[1]);
//! ```

use std::io::{self, BufRead, Read, Write};

use serde_json::Value;

use crate::codec::JsonlCodec;
use crate::frame::Frame;

// ── MockStdin ───────────────────────────────────────────────────────

/// Mock stdin that provides predefined JSONL input.
///
/// Construct with a sequence of JSONL lines. Implements [`Read`] and
/// [`BufRead`] so it can be passed to protocol reader functions.
///
/// # Example
/// ```
/// use sidecar_kit::test_utils::MockStdin;
/// use std::io::BufRead;
///
/// let input = r#"{"t":"fatal","ref_id":null,"error":"boom"}"#;
/// let mut mock = MockStdin::from_lines(&[input]);
/// let mut line = String::new();
/// mock.read_line(&mut line).unwrap();
/// assert!(line.contains("boom"));
/// ```
#[derive(Debug, Clone)]
pub struct MockStdin {
    data: io::Cursor<Vec<u8>>,
}

impl MockStdin {
    /// Create a mock stdin from raw bytes.
    #[must_use]
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            data: io::Cursor::new(data),
        }
    }

    /// Create a mock stdin from a slice of JSONL lines.
    ///
    /// Each line gets a newline appended automatically.
    #[must_use]
    pub fn from_lines(lines: &[&str]) -> Self {
        let mut buf = String::new();
        for line in lines {
            buf.push_str(line);
            buf.push('\n');
        }
        Self {
            data: io::Cursor::new(buf.into_bytes()),
        }
    }

    /// Create a mock stdin from a sequence of [`Frame`]s.
    ///
    /// Each frame is serialized to a JSONL line.
    #[must_use]
    pub fn from_frames(frames: &[Frame]) -> Self {
        let mut buf = Vec::new();
        for frame in frames {
            if let Ok(line) = JsonlCodec::encode(frame) {
                buf.extend_from_slice(line.as_bytes());
            }
        }
        Self {
            data: io::Cursor::new(buf),
        }
    }
}

impl Read for MockStdin {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.data.read(buf)
    }
}

impl BufRead for MockStdin {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.data.fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        self.data.consume(amt);
    }
}

// ── MockStdout ──────────────────────────────────────────────────────

/// Mock stdout that captures JSONL output for assertions.
///
/// Implements [`Write`] and provides methods to inspect the captured output.
///
/// # Example
/// ```
/// use sidecar_kit::test_utils::MockStdout;
/// use std::io::Write;
///
/// let mut mock = MockStdout::new();
/// write!(mock, "hello\n").unwrap();
/// assert_eq!(mock.lines(), vec!["hello"]);
/// ```
#[derive(Debug, Clone, Default)]
pub struct MockStdout {
    buf: Vec<u8>,
}

impl MockStdout {
    /// Create an empty mock stdout.
    #[must_use]
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Get the raw captured bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }

    /// Get the captured output as a string.
    #[must_use]
    pub fn as_string(&self) -> String {
        String::from_utf8_lossy(&self.buf).to_string()
    }

    /// Split captured output into individual lines.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let s = self.as_string();
        s.lines().map(String::from).collect()
    }

    /// Parse each captured line as a [`Frame`].
    ///
    /// Lines that fail to parse are silently skipped.
    #[must_use]
    pub fn frames(&self) -> Vec<Frame> {
        self.lines()
            .iter()
            .filter_map(|line| JsonlCodec::decode(line.trim()).ok())
            .collect()
    }

    /// Reset the captured output.
    pub fn clear(&mut self) {
        self.buf.clear();
    }
}

impl Write for MockStdout {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// ── Assertion helpers ───────────────────────────────────────────────

/// Assert that a JSONL line is a valid `hello` frame.
///
/// # Panics
///
/// Panics if the line is not a valid hello frame.
pub fn assert_valid_hello(line: &str) {
    let v: Value = serde_json::from_str(line).expect("valid JSON");
    assert_eq!(
        v["t"].as_str(),
        Some("hello"),
        "expected hello frame, got: {}",
        v["t"]
    );
    assert!(
        v["contract_version"].is_string(),
        "hello frame must have contract_version"
    );
    assert!(
        v["backend"].is_object(),
        "hello frame must have backend object"
    );
}

/// Assert that a JSONL line is a valid `final` frame.
///
/// # Panics
///
/// Panics if the line is not a valid final frame.
pub fn assert_valid_final(line: &str) {
    let v: Value = serde_json::from_str(line).expect("valid JSON");
    assert_eq!(
        v["t"].as_str(),
        Some("final"),
        "expected final frame, got: {}",
        v["t"]
    );
    assert!(
        v["ref_id"].is_string(),
        "final frame must have ref_id"
    );
    assert!(
        v["receipt"].is_object(),
        "final frame must have receipt object"
    );
}

/// Assert that a JSONL line is a valid `event` frame.
///
/// # Panics
///
/// Panics if the line is not a valid event frame.
pub fn assert_valid_event(line: &str) {
    let v: Value = serde_json::from_str(line).expect("valid JSON");
    assert_eq!(
        v["t"].as_str(),
        Some("event"),
        "expected event frame, got: {}",
        v["t"]
    );
    assert!(
        v["ref_id"].is_string(),
        "event frame must have ref_id"
    );
    assert!(
        v["event"].is_object(),
        "event frame must have event object"
    );
}

/// Assert that a JSONL line is a valid `fatal` frame.
///
/// # Panics
///
/// Panics if the line is not a valid fatal frame.
pub fn assert_valid_fatal(line: &str) {
    let v: Value = serde_json::from_str(line).expect("valid JSON");
    assert_eq!(
        v["t"].as_str(),
        Some("fatal"),
        "expected fatal frame, got: {}",
        v["t"]
    );
    assert!(
        v["error"].is_string(),
        "fatal frame must have error string"
    );
}

// ── SidecarTestHarness ──────────────────────────────────────────────

/// Test harness for sidecar implementations.
///
/// Captures the full JSONL protocol exchange and provides methods
/// to verify protocol correctness.
///
/// # Example
/// ```
/// use sidecar_kit::test_utils::SidecarTestHarness;
/// use sidecar_kit::protocol_helpers::{send_hello, send_event, send_final};
/// use sidecar_kit::events::text_event;
/// use sidecar_kit::receipt_builder::TypedReceiptBuilder;
/// use abp_core::CapabilityManifest;
///
/// let mut harness = SidecarTestHarness::new();
///
/// // Simulate sidecar output
/// send_hello(harness.writer(), "test", &CapabilityManifest::new()).unwrap();
/// send_event(harness.writer(), "run-1", &text_event("hi")).unwrap();
/// let receipt = TypedReceiptBuilder::new("test").build();
/// send_final(harness.writer(), "run-1", &receipt).unwrap();
///
/// // Verify
/// assert!(harness.has_hello());
/// assert_eq!(harness.event_count(), 1);
/// assert!(harness.has_final());
/// assert!(!harness.has_fatal());
/// ```
#[derive(Debug, Clone)]
pub struct SidecarTestHarness {
    output: MockStdout,
}

impl SidecarTestHarness {
    /// Create a new test harness.
    #[must_use]
    pub fn new() -> Self {
        Self {
            output: MockStdout::new(),
        }
    }

    /// Get a mutable reference to the writer for protocol output.
    pub fn writer(&mut self) -> &mut MockStdout {
        &mut self.output
    }

    /// Get all captured output lines.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        self.output.lines()
    }

    /// Get all captured frames.
    #[must_use]
    pub fn frames(&self) -> Vec<Frame> {
        self.output.frames()
    }

    /// Check if the output starts with a hello frame.
    #[must_use]
    pub fn has_hello(&self) -> bool {
        self.frames()
            .first()
            .is_some_and(|f| matches!(f, Frame::Hello { .. }))
    }

    /// Check if the output ends with a final frame.
    #[must_use]
    pub fn has_final(&self) -> bool {
        self.frames()
            .last()
            .is_some_and(|f| matches!(f, Frame::Final { .. }))
    }

    /// Check if any frame is a fatal frame.
    #[must_use]
    pub fn has_fatal(&self) -> bool {
        self.frames().iter().any(|f| matches!(f, Frame::Fatal { .. }))
    }

    /// Count the number of event frames.
    #[must_use]
    pub fn event_count(&self) -> usize {
        self.frames()
            .iter()
            .filter(|f| matches!(f, Frame::Event { .. }))
            .count()
    }

    /// Extract all event payloads as JSON values.
    #[must_use]
    pub fn event_payloads(&self) -> Vec<Value> {
        self.frames()
            .into_iter()
            .filter_map(|f| match f {
                Frame::Event { event, .. } => Some(event),
                _ => None,
            })
            .collect()
    }

    /// Get the receipt from the final frame, if present.
    #[must_use]
    pub fn receipt(&self) -> Option<Value> {
        self.frames().into_iter().find_map(|f| match f {
            Frame::Final { receipt, .. } => Some(receipt),
            _ => None,
        })
    }

    /// Validate the full protocol sequence: hello → events → final.
    ///
    /// # Panics
    ///
    /// Panics with a descriptive message if the sequence is invalid.
    pub fn assert_valid_sequence(&self) {
        let frames = self.frames();
        assert!(!frames.is_empty(), "no frames captured");
        assert!(
            matches!(frames.first(), Some(Frame::Hello { .. })),
            "first frame must be hello"
        );
        let last = frames.last().unwrap();
        assert!(
            matches!(last, Frame::Final { .. } | Frame::Fatal { .. }),
            "last frame must be final or fatal"
        );
    }

    /// Reset the harness, clearing all captured output.
    pub fn reset(&mut self) {
        self.output.clear();
    }
}

impl Default for SidecarTestHarness {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::text_event;
    use crate::protocol_helpers::{send_event, send_fatal, send_final, send_hello};
    use crate::receipt_builder::TypedReceiptBuilder;
    use abp_core::CapabilityManifest;

    #[test]
    fn mock_stdin_from_lines() {
        let mock = MockStdin::from_lines(&["line1", "line2"]);
        let reader = io::BufReader::new(mock);
        let lines: Vec<String> = reader.lines().map(|l| l.unwrap()).collect();
        assert_eq!(lines, vec!["line1", "line2"]);
    }

    #[test]
    fn mock_stdin_from_frames() {
        let frames = vec![
            Frame::Hello {
                contract_version: "abp/v0.1".into(),
                backend: serde_json::json!({"id": "test"}),
                capabilities: serde_json::json!({}),
                mode: Value::Null,
            },
        ];
        let mock = MockStdin::from_frames(&frames);
        let reader = io::BufReader::new(mock);
        let lines: Vec<String> = reader.lines().map(|l| l.unwrap()).collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"t\":\"hello\""));
    }

    #[test]
    fn mock_stdout_captures_output() {
        let mut mock = MockStdout::new();
        write!(mock, "hello\n").unwrap();
        write!(mock, "world\n").unwrap();
        assert_eq!(mock.lines(), vec!["hello", "world"]);
    }

    #[test]
    fn mock_stdout_clear() {
        let mut mock = MockStdout::new();
        write!(mock, "data\n").unwrap();
        assert!(!mock.bytes().is_empty());
        mock.clear();
        assert!(mock.bytes().is_empty());
    }

    #[test]
    fn mock_stdout_frames() {
        let mut mock = MockStdout::new();
        send_hello(&mut mock, "test", &CapabilityManifest::new()).unwrap();
        let frames = mock.frames();
        assert_eq!(frames.len(), 1);
        assert!(matches!(frames[0], Frame::Hello { .. }));
    }

    #[test]
    fn assert_valid_hello_passes() {
        let mut buf = Vec::new();
        send_hello(&mut buf, "test", &CapabilityManifest::new()).unwrap();
        let line = String::from_utf8(buf).unwrap();
        assert_valid_hello(line.trim());
    }

    #[test]
    #[should_panic(expected = "expected hello frame")]
    fn assert_valid_hello_fails_on_wrong_type() {
        assert_valid_hello(r#"{"t":"fatal","ref_id":null,"error":"boom"}"#);
    }

    #[test]
    fn assert_valid_final_passes() {
        let mut buf = Vec::new();
        let receipt = TypedReceiptBuilder::new("test").build();
        send_final(&mut buf, "run-1", &receipt).unwrap();
        let line = String::from_utf8(buf).unwrap();
        assert_valid_final(line.trim());
    }

    #[test]
    fn assert_valid_event_passes() {
        let mut buf = Vec::new();
        send_event(&mut buf, "run-1", &text_event("hi")).unwrap();
        let line = String::from_utf8(buf).unwrap();
        assert_valid_event(line.trim());
    }

    #[test]
    fn assert_valid_fatal_passes() {
        let mut buf = Vec::new();
        send_fatal(&mut buf, Some("run-1"), "E500", "oops").unwrap();
        let line = String::from_utf8(buf).unwrap();
        assert_valid_fatal(line.trim());
    }

    #[test]
    fn harness_full_lifecycle() {
        let mut harness = SidecarTestHarness::new();

        send_hello(harness.writer(), "test-sidecar", &CapabilityManifest::new()).unwrap();
        send_event(harness.writer(), "run-1", &text_event("hello")).unwrap();
        send_event(harness.writer(), "run-1", &text_event("world")).unwrap();

        let receipt = TypedReceiptBuilder::new("test-sidecar").build();
        send_final(harness.writer(), "run-1", &receipt).unwrap();

        assert!(harness.has_hello());
        assert!(harness.has_final());
        assert!(!harness.has_fatal());
        assert_eq!(harness.event_count(), 2);
        assert!(harness.receipt().is_some());
        harness.assert_valid_sequence();
    }

    #[test]
    fn harness_fatal_flow() {
        let mut harness = SidecarTestHarness::new();

        send_hello(harness.writer(), "test", &CapabilityManifest::new()).unwrap();
        send_fatal(harness.writer(), Some("run-1"), "E500", "crashed").unwrap();

        assert!(harness.has_hello());
        assert!(!harness.has_final());
        assert!(harness.has_fatal());
        assert_eq!(harness.event_count(), 0);
        harness.assert_valid_sequence();
    }

    #[test]
    fn harness_reset() {
        let mut harness = SidecarTestHarness::new();
        send_hello(harness.writer(), "test", &CapabilityManifest::new()).unwrap();
        assert!(harness.has_hello());

        harness.reset();
        assert!(!harness.has_hello());
        assert_eq!(harness.frames().len(), 0);
    }

    #[test]
    fn harness_event_payloads() {
        let mut harness = SidecarTestHarness::new();
        send_hello(harness.writer(), "test", &CapabilityManifest::new()).unwrap();
        send_event(harness.writer(), "run-1", &text_event("payload1")).unwrap();
        send_event(harness.writer(), "run-1", &text_event("payload2")).unwrap();

        let payloads = harness.event_payloads();
        assert_eq!(payloads.len(), 2);
        assert!(payloads[0]["text"].as_str().unwrap().contains("payload1"));
        assert!(payloads[1]["text"].as_str().unwrap().contains("payload2"));
    }
}
