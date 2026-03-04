// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code)]
//! Tracing integration: timed spans with auto-duration recording.
//!
//! Provides [`TracingSpan`] for timing operations and [`span_guard`] for
//! automatic duration recording into a [`Histogram`].

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::metrics::Histogram;

// ---------------------------------------------------------------------------
// TracingSpan
// ---------------------------------------------------------------------------

/// A timed span that records its duration on completion.
///
/// Use [`TracingSpan::start`] to begin timing and [`TracingSpan::finish`] to
/// stop and record the elapsed time.  If the span is dropped without calling
/// `finish`, the duration is still recorded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracingSpan {
    /// Human-readable name of the operation.
    pub name: String,
    /// When the span started (milliseconds since an epoch).
    pub start_time_ms: u64,
    /// When the span ended, if closed.
    pub end_time_ms: Option<u64>,
    /// Duration in milliseconds, set on finish.
    pub duration_ms: Option<f64>,
    /// Key-value attributes.
    pub attributes: BTreeMap<String, String>,
}

impl TracingSpan {
    /// Create a completed span from pre-computed values.
    pub fn completed(name: impl Into<String>, duration_ms: f64) -> Self {
        Self {
            name: name.into(),
            start_time_ms: 0,
            end_time_ms: Some(duration_ms as u64),
            duration_ms: Some(duration_ms),
            attributes: BTreeMap::new(),
        }
    }

    /// Whether the span has been closed.
    pub fn is_closed(&self) -> bool {
        self.end_time_ms.is_some()
    }

    /// Attach an attribute.
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }
}

// ---------------------------------------------------------------------------
// SpanGuard
// ---------------------------------------------------------------------------

/// RAII guard that records elapsed time to a [`Histogram`] on drop.
///
/// Created by [`span_guard`].  Call [`stop`](Self::stop) for explicit
/// completion, or let the guard drop for automatic recording.
#[derive(Debug)]
pub struct SpanGuard {
    name: String,
    start: Instant,
    histogram: Histogram,
    stopped: bool,
    attributes: BTreeMap<String, String>,
    recorder: Option<SpanRecorder>,
}

impl SpanGuard {
    /// Elapsed time in milliseconds so far, without stopping.
    pub fn elapsed_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }

    /// Name of this span.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Stop timing, record to histogram, and return the elapsed milliseconds.
    pub fn stop(mut self) -> f64 {
        let ms = self.start.elapsed().as_secs_f64() * 1000.0;
        self.histogram.observe(ms);
        if let Some(ref rec) = self.recorder {
            rec.record(TracingSpan::completed(&self.name, ms));
        }
        self.stopped = true;
        ms
    }
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        if !self.stopped {
            let ms = self.start.elapsed().as_secs_f64() * 1000.0;
            self.histogram.observe(ms);
            if let Some(ref rec) = self.recorder {
                rec.record(TracingSpan::completed(&self.name, ms));
            }
        }
    }
}

/// Start a timed span that records its duration to `histogram` on completion.
///
/// The returned [`SpanGuard`] records elapsed milliseconds when
/// [`stop`](SpanGuard::stop) is called or when the guard is dropped.
pub fn span_guard(name: impl Into<String>, histogram: &Histogram) -> SpanGuard {
    SpanGuard {
        name: name.into(),
        start: Instant::now(),
        histogram: histogram.clone(),
        stopped: false,
        attributes: BTreeMap::new(),
        recorder: None,
    }
}

/// Start a timed span that records to both a histogram and a [`SpanRecorder`].
pub fn span_guard_with_recorder(
    name: impl Into<String>,
    histogram: &Histogram,
    recorder: &SpanRecorder,
) -> SpanGuard {
    SpanGuard {
        name: name.into(),
        start: Instant::now(),
        histogram: histogram.clone(),
        stopped: false,
        attributes: BTreeMap::new(),
        recorder: Some(recorder.clone()),
    }
}

// ---------------------------------------------------------------------------
// SpanRecorder
// ---------------------------------------------------------------------------

/// Thread-safe collector of completed [`TracingSpan`]s.
#[derive(Debug, Clone, Default)]
pub struct SpanRecorder {
    spans: Arc<Mutex<Vec<TracingSpan>>>,
}

impl SpanRecorder {
    /// Create an empty recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a completed span.
    pub fn record(&self, span: TracingSpan) {
        let mut guard = self.spans.lock().expect("span recorder lock poisoned");
        guard.push(span);
    }

    /// All recorded spans.
    pub fn spans(&self) -> Vec<TracingSpan> {
        let guard = self.spans.lock().expect("span recorder lock poisoned");
        guard.clone()
    }

    /// Number of recorded spans.
    pub fn len(&self) -> usize {
        let guard = self.spans.lock().expect("span recorder lock poisoned");
        guard.len()
    }

    /// Whether the recorder is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Return spans matching a given name.
    pub fn spans_named(&self, name: &str) -> Vec<TracingSpan> {
        let guard = self.spans.lock().expect("span recorder lock poisoned");
        guard.iter().filter(|s| s.name == name).cloned().collect()
    }

    /// Clear all recorded spans.
    pub fn clear(&self) {
        let mut guard = self.spans.lock().expect("span recorder lock poisoned");
        guard.clear();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracing_span_completed() {
        let span = TracingSpan::completed("test_op", 42.5);
        assert_eq!(span.name, "test_op");
        assert!(span.is_closed());
        assert!((span.duration_ms.unwrap() - 42.5).abs() < 0.001);
    }

    #[test]
    fn tracing_span_with_attributes() {
        let span = TracingSpan::completed("op", 10.0)
            .with_attribute("backend", "mock")
            .with_attribute("dialect", "openai");
        assert_eq!(span.attributes["backend"], "mock");
        assert_eq!(span.attributes["dialect"], "openai");
    }

    #[test]
    fn tracing_span_serde_roundtrip() {
        let span = TracingSpan::completed("test", 50.0)
            .with_attribute("key", "value");
        let json = serde_json::to_string(&span).unwrap();
        let back: TracingSpan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "test");
        assert_eq!(back.attributes["key"], "value");
    }

    #[test]
    fn span_guard_records_to_histogram() {
        let h = Histogram::new();
        let guard = span_guard("test_op", &h);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let ms = guard.stop();
        assert!(ms >= 1.0, "elapsed={ms}ms");
        assert_eq!(h.count(), 1);
    }

    #[test]
    fn span_guard_auto_records_on_drop() {
        let h = Histogram::new();
        {
            let _guard = span_guard("test_op", &h);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(h.count(), 1);
    }

    #[test]
    fn span_guard_elapsed_before_stop() {
        let h = Histogram::new();
        let guard = span_guard("test_op", &h);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let e = guard.elapsed_ms();
        assert!(e >= 1.0);
        // guard drops here, records to histogram
    }

    #[test]
    fn span_guard_name() {
        let h = Histogram::new();
        let guard = span_guard("my_operation", &h);
        assert_eq!(guard.name(), "my_operation");
        drop(guard);
    }

    #[test]
    fn span_guard_with_recorder_records_both() {
        let h = Histogram::new();
        let rec = SpanRecorder::new();
        let guard = span_guard_with_recorder("test_op", &h, &rec);
        std::thread::sleep(std::time::Duration::from_millis(2));
        guard.stop();
        assert_eq!(h.count(), 1);
        assert_eq!(rec.len(), 1);
        assert_eq!(rec.spans()[0].name, "test_op");
    }

    #[test]
    fn span_guard_with_recorder_drop_records_both() {
        let h = Histogram::new();
        let rec = SpanRecorder::new();
        {
            let _guard = span_guard_with_recorder("op", &h, &rec);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(h.count(), 1);
        assert_eq!(rec.len(), 1);
    }

    #[test]
    fn span_recorder_basic() {
        let rec = SpanRecorder::new();
        assert!(rec.is_empty());
        rec.record(TracingSpan::completed("op", 10.0));
        assert_eq!(rec.len(), 1);
        assert!(!rec.is_empty());
    }

    #[test]
    fn span_recorder_filter_by_name() {
        let rec = SpanRecorder::new();
        rec.record(TracingSpan::completed("alpha", 1.0));
        rec.record(TracingSpan::completed("beta", 2.0));
        rec.record(TracingSpan::completed("alpha", 3.0));
        assert_eq!(rec.spans_named("alpha").len(), 2);
        assert_eq!(rec.spans_named("beta").len(), 1);
        assert_eq!(rec.spans_named("gamma").len(), 0);
    }

    #[test]
    fn span_recorder_clear() {
        let rec = SpanRecorder::new();
        rec.record(TracingSpan::completed("op", 1.0));
        rec.clear();
        assert!(rec.is_empty());
    }

    #[test]
    fn span_recorder_thread_safety() {
        let rec = SpanRecorder::new();
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let rec = rec.clone();
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        rec.record(TracingSpan::completed("op", 1.0));
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(rec.len(), 200);
    }

    #[test]
    fn multiple_span_guards_to_same_histogram() {
        let h = Histogram::new();
        for _ in 0..5 {
            let _guard = span_guard("op", &h);
        }
        assert_eq!(h.count(), 5);
    }
}
