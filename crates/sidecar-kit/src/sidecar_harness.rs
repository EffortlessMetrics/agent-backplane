// SPDX-License-Identifier: MIT OR Apache-2.0
//! Async test harness for sidecar integration testing.
//!
//! [`SidecarTestHarness`](AsyncSidecarTestHarness) spawns a sidecar command,
//! performs the JSONL handshake (hello → run → collect events → verify receipt),
//! and provides rich assertion helpers.
//!
//! For in-process (non-spawn) testing, see [`InProcessHarness`].
//!
//! # Example (in-process)
//! ```
//! use sidecar_kit::sidecar_harness::InProcessHarness;
//! use sidecar_kit::Frame;
//! use serde_json::json;
//!
//! let mut harness = InProcessHarness::new();
//! harness.feed_frame(&Frame::Hello {
//!     contract_version: "abp/v0.1".into(),
//!     backend: json!({"id": "test"}),
//!     capabilities: json!({}),
//!     mode: serde_json::Value::Null,
//! });
//! harness.feed_frame(&Frame::Event {
//!     ref_id: "r1".into(),
//!     event: json!({"type": "assistant_delta", "text": "hi", "ts": "2025-01-01T00:00:00Z"}),
//! });
//! harness.feed_frame(&Frame::Final {
//!     ref_id: "r1".into(),
//!     receipt: json!({"outcome": "complete"}),
//! });
//!
//! assert!(harness.has_hello());
//! assert_eq!(harness.event_count(), 1);
//! assert!(harness.has_final());
//! harness.assert_valid_sequence();
//! ```

use serde_json::Value;

use crate::codec::JsonlCodec;
use crate::frame::Frame;

// ── InProcessHarness ────────────────────────────────────────────────

/// In-process test harness that collects frames directly.
///
/// Use this for unit-testing sidecar handler logic without spawning a
/// real process. Feed frames via [`feed_frame`](Self::feed_frame) and
/// inspect via assertion helpers.
#[derive(Debug, Clone, Default)]
pub struct InProcessHarness {
    frames: Vec<Frame>,
}

impl InProcessHarness {
    /// Create a new empty harness.
    #[must_use]
    pub fn new() -> Self {
        Self { frames: Vec::new() }
    }

    /// Feed a frame into the harness.
    pub fn feed_frame(&mut self, frame: &Frame) {
        self.frames.push(frame.clone());
    }

    /// Feed a raw JSONL line into the harness.
    ///
    /// Returns `Err` if the line is not valid JSONL.
    pub fn feed_line(&mut self, line: &str) -> Result<(), String> {
        let frame: Frame =
            JsonlCodec::decode(line.trim()).map_err(|e| format!("invalid JSONL: {e}"))?;
        self.frames.push(frame);
        Ok(())
    }

    /// Get all collected frames.
    #[must_use]
    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    /// Check if the first frame is a Hello.
    #[must_use]
    pub fn has_hello(&self) -> bool {
        self.frames
            .first()
            .is_some_and(|f| matches!(f, Frame::Hello { .. }))
    }

    /// Check if any frame is a Final.
    #[must_use]
    pub fn has_final(&self) -> bool {
        self.frames.iter().any(|f| matches!(f, Frame::Final { .. }))
    }

    /// Check if any frame is a Fatal.
    #[must_use]
    pub fn has_fatal(&self) -> bool {
        self.frames.iter().any(|f| matches!(f, Frame::Fatal { .. }))
    }

    /// Count event frames.
    #[must_use]
    pub fn event_count(&self) -> usize {
        self.frames
            .iter()
            .filter(|f| matches!(f, Frame::Event { .. }))
            .count()
    }

    /// Extract all event payloads.
    #[must_use]
    pub fn event_payloads(&self) -> Vec<&Value> {
        self.frames
            .iter()
            .filter_map(|f| match f {
                Frame::Event { event, .. } => Some(event),
                _ => None,
            })
            .collect()
    }

    /// Get the receipt from the first Final frame.
    #[must_use]
    pub fn receipt(&self) -> Option<&Value> {
        self.frames.iter().find_map(|f| match f {
            Frame::Final { receipt, .. } => Some(receipt),
            _ => None,
        })
    }

    /// Get the error from the first Fatal frame.
    #[must_use]
    pub fn fatal_error(&self) -> Option<&str> {
        self.frames.iter().find_map(|f| match f {
            Frame::Fatal { error, .. } => Some(error.as_str()),
            _ => None,
        })
    }

    /// Validate the protocol sequence: hello first, final/fatal last.
    ///
    /// # Panics
    /// Panics if the sequence is invalid.
    pub fn assert_valid_sequence(&self) {
        assert!(!self.frames.is_empty(), "no frames captured");
        assert!(
            matches!(self.frames.first(), Some(Frame::Hello { .. })),
            "first frame must be hello"
        );
        let last = self.frames.last().unwrap();
        assert!(
            matches!(last, Frame::Final { .. } | Frame::Fatal { .. }),
            "last frame must be final or fatal, got: {last:?}"
        );
    }

    /// Assert that all event ref_ids match the expected run ID.
    ///
    /// # Panics
    /// Panics if any event has a different ref_id.
    pub fn assert_ref_ids(&self, expected_ref_id: &str) {
        for frame in &self.frames {
            match frame {
                Frame::Event { ref_id, .. } | Frame::Final { ref_id, .. } => {
                    assert_eq!(
                        ref_id, expected_ref_id,
                        "mismatched ref_id: expected {expected_ref_id}, got {ref_id}"
                    );
                }
                _ => {}
            }
        }
    }

    /// Reset the harness.
    pub fn reset(&mut self) {
        self.frames.clear();
    }
}

// ── AsyncSidecarTestHarness ─────────────────────────────────────────

/// Async harness that drives a sidecar over simulated stdio.
///
/// Builds a sequence of frames to send to the sidecar (run frame),
/// collects frames the sidecar emits, and provides assertion helpers.
///
/// This is the async counterpart to [`InProcessHarness`] — use it when
/// your test needs async I/O to talk to a real or mocked sidecar process.
pub struct AsyncSidecarTestHarness {
    /// Frames to be sent to the sidecar (typically a Run frame).
    outbound: Vec<Frame>,
    /// Frames received from the sidecar.
    collected: Vec<Frame>,
}

impl AsyncSidecarTestHarness {
    /// Create a new async harness.
    #[must_use]
    pub fn new() -> Self {
        Self {
            outbound: Vec::new(),
            collected: Vec::new(),
        }
    }

    /// Queue a frame to send to the sidecar.
    pub fn queue_frame(&mut self, frame: Frame) {
        self.outbound.push(frame);
    }

    /// Build the JSONL bytes from the queued outbound frames.
    #[must_use]
    pub fn outbound_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        for frame in &self.outbound {
            if let Ok(line) = JsonlCodec::encode(frame) {
                buf.extend_from_slice(line.as_bytes());
            }
        }
        buf
    }

    /// Collect a frame received from the sidecar.
    pub fn collect_frame(&mut self, frame: Frame) {
        self.collected.push(frame);
    }

    /// Parse output bytes into frames and collect them.
    pub fn collect_output(&mut self, output: &[u8]) -> Result<(), String> {
        let s = String::from_utf8_lossy(output);
        for line in s.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let frame: Frame =
                JsonlCodec::decode(trimmed).map_err(|e| format!("decode error: {e}"))?;
            self.collected.push(frame);
        }
        Ok(())
    }

    /// Get all collected frames.
    #[must_use]
    pub fn frames(&self) -> &[Frame] {
        &self.collected
    }

    /// Check if collected frames start with hello.
    #[must_use]
    pub fn has_hello(&self) -> bool {
        self.collected
            .first()
            .is_some_and(|f| matches!(f, Frame::Hello { .. }))
    }

    /// Check if collected frames contain a final.
    #[must_use]
    pub fn has_final(&self) -> bool {
        self.collected
            .iter()
            .any(|f| matches!(f, Frame::Final { .. }))
    }

    /// Count event frames.
    #[must_use]
    pub fn event_count(&self) -> usize {
        self.collected
            .iter()
            .filter(|f| matches!(f, Frame::Event { .. }))
            .count()
    }

    /// Get the receipt value.
    #[must_use]
    pub fn receipt(&self) -> Option<&Value> {
        self.collected.iter().find_map(|f| match f {
            Frame::Final { receipt, .. } => Some(receipt),
            _ => None,
        })
    }

    /// Validate the sequence.
    ///
    /// # Panics
    /// Panics if invalid.
    pub fn assert_valid_sequence(&self) {
        assert!(!self.collected.is_empty(), "no frames collected");
        assert!(
            matches!(self.collected.first(), Some(Frame::Hello { .. })),
            "first frame must be hello"
        );
        let last = self.collected.last().unwrap();
        assert!(
            matches!(last, Frame::Final { .. } | Frame::Fatal { .. }),
            "last frame must be final or fatal"
        );
    }

    /// Reset collected frames.
    pub fn reset(&mut self) {
        self.collected.clear();
        self.outbound.clear();
    }
}

impl Default for AsyncSidecarTestHarness {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hello() -> Frame {
        Frame::Hello {
            contract_version: "abp/v0.1".into(),
            backend: json!({"id": "test"}),
            capabilities: json!({}),
            mode: Value::Null,
        }
    }

    fn event(text: &str) -> Frame {
        Frame::Event {
            ref_id: "run-1".into(),
            event: json!({"type": "assistant_delta", "text": text, "ts": "2025-01-01T00:00:00Z"}),
        }
    }

    fn final_f() -> Frame {
        Frame::Final {
            ref_id: "run-1".into(),
            receipt: json!({"outcome": "complete"}),
        }
    }

    fn fatal_f() -> Frame {
        Frame::Fatal {
            ref_id: Some("run-1".into()),
            error: "boom".into(),
        }
    }

    // ── InProcessHarness tests ──────────────────────────────────────

    #[test]
    fn in_process_full_lifecycle() {
        let mut h = InProcessHarness::new();
        h.feed_frame(&hello());
        h.feed_frame(&event("hi"));
        h.feed_frame(&event("world"));
        h.feed_frame(&final_f());

        assert!(h.has_hello());
        assert!(h.has_final());
        assert!(!h.has_fatal());
        assert_eq!(h.event_count(), 2);
        assert!(h.receipt().is_some());
        h.assert_valid_sequence();
    }

    #[test]
    fn in_process_fatal_flow() {
        let mut h = InProcessHarness::new();
        h.feed_frame(&hello());
        h.feed_frame(&fatal_f());

        assert!(h.has_hello());
        assert!(h.has_fatal());
        assert_eq!(h.fatal_error(), Some("boom"));
        h.assert_valid_sequence();
    }

    #[test]
    fn in_process_ref_ids() {
        let mut h = InProcessHarness::new();
        h.feed_frame(&hello());
        h.feed_frame(&event("a"));
        h.feed_frame(&final_f());
        h.assert_ref_ids("run-1");
    }

    #[test]
    #[should_panic(expected = "no frames captured")]
    fn in_process_empty_panics() {
        let h = InProcessHarness::new();
        h.assert_valid_sequence();
    }

    #[test]
    fn in_process_reset() {
        let mut h = InProcessHarness::new();
        h.feed_frame(&hello());
        assert_eq!(h.frames().len(), 1);
        h.reset();
        assert_eq!(h.frames().len(), 0);
    }

    #[test]
    fn in_process_feed_line() {
        let mut h = InProcessHarness::new();
        let line = serde_json::to_string(&hello()).unwrap();
        h.feed_line(&line).unwrap();
        assert!(h.has_hello());
    }

    #[test]
    fn in_process_feed_line_invalid() {
        let mut h = InProcessHarness::new();
        assert!(h.feed_line("not json").is_err());
    }

    #[test]
    fn in_process_event_payloads() {
        let mut h = InProcessHarness::new();
        h.feed_frame(&hello());
        h.feed_frame(&event("payload1"));
        h.feed_frame(&final_f());
        let payloads = h.event_payloads();
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0]["text"], "payload1");
    }

    // ── AsyncSidecarTestHarness tests ───────────────────────────────

    #[test]
    fn async_harness_queue_and_outbound() {
        let mut h = AsyncSidecarTestHarness::new();
        h.queue_frame(Frame::Run {
            id: "run-1".into(),
            work_order: json!({"task": "hello"}),
        });
        let bytes = h.outbound_bytes();
        assert!(!bytes.is_empty());
        let s = String::from_utf8(bytes).unwrap();
        assert!(s.contains("run-1"));
    }

    #[test]
    fn async_harness_collect_output() {
        let mut h = AsyncSidecarTestHarness::new();
        let mut output = String::new();
        output.push_str(&serde_json::to_string(&hello()).unwrap());
        output.push('\n');
        output.push_str(&serde_json::to_string(&event("x")).unwrap());
        output.push('\n');
        output.push_str(&serde_json::to_string(&final_f()).unwrap());
        output.push('\n');

        h.collect_output(output.as_bytes()).unwrap();
        assert!(h.has_hello());
        assert!(h.has_final());
        assert_eq!(h.event_count(), 1);
        h.assert_valid_sequence();
    }

    #[test]
    fn async_harness_reset() {
        let mut h = AsyncSidecarTestHarness::new();
        h.collect_frame(hello());
        assert!(!h.frames().is_empty());
        h.reset();
        assert!(h.frames().is_empty());
    }

    #[test]
    fn async_harness_receipt() {
        let mut h = AsyncSidecarTestHarness::new();
        h.collect_frame(hello());
        h.collect_frame(final_f());
        assert!(h.receipt().is_some());
        assert_eq!(h.receipt().unwrap()["outcome"], "complete");
    }
}
