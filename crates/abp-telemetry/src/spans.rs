// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code, unused_imports)]
//! Structured tracing span helpers and custom span types for ABP operations.
//!
//! The first part of this module provides thin helpers that create pre-populated
//! [`tracing::Span`] values.  The second part provides standalone span types
//! ([`SpanContext`], [`TelemetrySpan`], [`SpanBuilder`], [`SpanRecorder`]) that
//! are independent of the `tracing` crate and can be exported or inspected
//! directly.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{Span, info_span};

/// Create a tracing span for processing a work-order request.
///
/// The returned span carries `work_order_id`, `task`, and `lane` fields
/// extracted from the provided parameters.
///
/// # Example
///
/// ```
/// let span = abp_telemetry::spans::request_span("wo-1", "refactor auth", "mapped");
/// let _guard = span.enter();
/// ```
pub fn request_span(work_order_id: &str, task: &str, lane: &str) -> Span {
    info_span!(
        "abp.request",
        work_order_id = %work_order_id,
        task = %task,
        lane = %lane,
    )
}

/// Create a tracing span for processing a single agent event.
///
/// The returned span carries `event_kind` and `sequence` fields.
///
/// # Example
///
/// ```
/// let span = abp_telemetry::spans::event_span("tool_call", 3);
/// let _guard = span.enter();
/// ```
pub fn event_span(event_kind: &str, sequence: u64) -> Span {
    info_span!(
        "abp.event",
        event_kind = %event_kind,
        sequence = sequence,
    )
}

/// Create a tracing span for a backend call.
///
/// The returned span carries the `backend` name.
///
/// # Example
///
/// ```
/// let span = abp_telemetry::spans::backend_span("sidecar:node");
/// let _guard = span.enter();
/// ```
pub fn backend_span(backend_name: &str) -> Span {
    info_span!(
        "abp.backend",
        backend = %backend_name,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_span_does_not_panic() {
        let span = request_span("wo-123", "do stuff", "mapped");
        let _guard = span.enter();
    }

    #[test]
    fn event_span_does_not_panic() {
        let span = event_span("tool_call", 7);
        let _guard = span.enter();
    }

    #[test]
    fn backend_span_does_not_panic() {
        let span = backend_span("mock");
        let _guard = span.enter();
    }
}

// ===========================================================================
// Standalone span types (tracing-independent)
// ===========================================================================

// ---------------------------------------------------------------------------
// SpanContext
// ---------------------------------------------------------------------------

/// Identifies a span within a distributed trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanContext {
    /// Trace identifier shared across all spans in a trace.
    pub trace_id: String,
    /// Unique identifier for this span.
    pub span_id: String,
    /// Identifier of the parent span, if any.
    pub parent_span_id: Option<String>,
}

impl SpanContext {
    /// Create a root span context (no parent).
    pub fn root(trace_id: impl Into<String>, span_id: impl Into<String>) -> Self {
        Self {
            trace_id: trace_id.into(),
            span_id: span_id.into(),
            parent_span_id: None,
        }
    }

    /// Create a child span context.
    pub fn child(
        trace_id: impl Into<String>,
        span_id: impl Into<String>,
        parent_span_id: impl Into<String>,
    ) -> Self {
        Self {
            trace_id: trace_id.into(),
            span_id: span_id.into(),
            parent_span_id: Some(parent_span_id.into()),
        }
    }
}

// ---------------------------------------------------------------------------
// TelemetrySpan
// ---------------------------------------------------------------------------

/// A completed or in-progress span with timing and attributes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySpan {
    /// Human-readable name.
    pub name: String,
    /// Trace/span identifiers.
    pub context: SpanContext,
    /// When the span started (milliseconds since some epoch).
    pub start_time_ms: u64,
    /// When the span ended, if it has.
    pub end_time_ms: Option<u64>,
    /// Key-value attributes (deterministic ordering).
    pub attributes: BTreeMap<String, serde_json::Value>,
}

impl TelemetrySpan {
    /// Duration in milliseconds, or `None` if the span is still open.
    pub fn duration_ms(&self) -> Option<u64> {
        self.end_time_ms.map(|end| end.saturating_sub(self.start_time_ms))
    }

    /// Whether the span has been closed.
    pub fn is_closed(&self) -> bool {
        self.end_time_ms.is_some()
    }
}

// ---------------------------------------------------------------------------
// SpanBuilder
// ---------------------------------------------------------------------------

/// Fluent builder for [`TelemetrySpan`].
#[derive(Debug)]
pub struct SpanBuilder {
    name: String,
    context: SpanContext,
    start: Instant,
    attributes: BTreeMap<String, serde_json::Value>,
    /// Epoch offset to convert Instant to absolute ms.
    epoch_offset_ms: u64,
}

impl SpanBuilder {
    /// Start building a new span. The clock starts immediately.
    pub fn new(name: impl Into<String>, context: SpanContext) -> Self {
        Self {
            name: name.into(),
            context,
            start: Instant::now(),
            attributes: BTreeMap::new(),
            epoch_offset_ms: 0,
        }
    }

    /// Set an epoch offset so that `start_time_ms` is relative to a known
    /// epoch rather than an arbitrary `Instant`.
    pub fn with_epoch_offset_ms(mut self, offset: u64) -> Self {
        self.epoch_offset_ms = offset;
        self
    }

    /// Attach a string attribute.
    pub fn attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes
            .insert(key.into(), serde_json::Value::String(value.into()));
        self
    }

    /// Attach a numeric attribute.
    pub fn attribute_i64(mut self, key: impl Into<String>, value: i64) -> Self {
        self.attributes
            .insert(key.into(), serde_json::Value::Number(value.into()));
        self
    }

    /// Finish the span, recording the end time.
    pub fn finish(self) -> TelemetrySpan {
        let elapsed = self.start.elapsed().as_millis() as u64;
        TelemetrySpan {
            name: self.name,
            context: self.context,
            start_time_ms: self.epoch_offset_ms,
            end_time_ms: Some(self.epoch_offset_ms + elapsed),
            attributes: self.attributes,
        }
    }
}

// ---------------------------------------------------------------------------
// SpanRecorder
// ---------------------------------------------------------------------------

/// Thread-safe collector of completed [`TelemetrySpan`]s.
#[derive(Debug, Clone, Default)]
pub struct SpanRecorder {
    spans: Arc<Mutex<Vec<TelemetrySpan>>>,
}

impl SpanRecorder {
    /// Create an empty recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a completed span.
    pub fn record(&self, span: TelemetrySpan) {
        let mut guard = self.spans.lock().expect("span recorder lock poisoned");
        guard.push(span);
    }

    /// All recorded spans.
    pub fn spans(&self) -> Vec<TelemetrySpan> {
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
    pub fn spans_named(&self, name: &str) -> Vec<TelemetrySpan> {
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
// Tests (standalone span types)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod standalone_tests {
    use super::*;

    #[test]
    fn span_context_root() {
        let ctx = SpanContext::root("trace-1", "span-1");
        assert_eq!(ctx.trace_id, "trace-1");
        assert_eq!(ctx.span_id, "span-1");
        assert!(ctx.parent_span_id.is_none());
    }

    #[test]
    fn span_context_child() {
        let ctx = SpanContext::child("trace-1", "span-2", "span-1");
        assert_eq!(ctx.parent_span_id.as_deref(), Some("span-1"));
    }

    #[test]
    fn span_context_serde_roundtrip() {
        let ctx = SpanContext::child("t", "s", "p");
        let json = serde_json::to_string(&ctx).unwrap();
        let back: SpanContext = serde_json::from_str(&json).unwrap();
        assert_eq!(ctx, back);
    }

    #[test]
    fn span_builder_finish() {
        let ctx = SpanContext::root("t1", "s1");
        let span = SpanBuilder::new("test_op", ctx)
            .attribute("key", "value")
            .attribute_i64("count", 42)
            .finish();
        assert_eq!(span.name, "test_op");
        assert!(span.is_closed());
        assert!(span.duration_ms().is_some());
        assert_eq!(
            span.attributes.get("key"),
            Some(&serde_json::Value::String("value".into()))
        );
        assert_eq!(
            span.attributes.get("count"),
            Some(&serde_json::json!(42))
        );
    }

    #[test]
    fn span_builder_with_epoch_offset() {
        let ctx = SpanContext::root("t1", "s1");
        let span = SpanBuilder::new("op", ctx)
            .with_epoch_offset_ms(1000)
            .finish();
        assert!(span.start_time_ms >= 1000);
        assert!(span.end_time_ms.unwrap() >= 1000);
    }

    #[test]
    fn telemetry_span_duration() {
        let span = TelemetrySpan {
            name: "test".into(),
            context: SpanContext::root("t", "s"),
            start_time_ms: 100,
            end_time_ms: Some(250),
            attributes: BTreeMap::new(),
        };
        assert_eq!(span.duration_ms(), Some(150));
    }

    #[test]
    fn telemetry_span_open() {
        let span = TelemetrySpan {
            name: "test".into(),
            context: SpanContext::root("t", "s"),
            start_time_ms: 100,
            end_time_ms: None,
            attributes: BTreeMap::new(),
        };
        assert!(!span.is_closed());
        assert!(span.duration_ms().is_none());
    }

    #[test]
    fn telemetry_span_serde_roundtrip() {
        let span = TelemetrySpan {
            name: "op".into(),
            context: SpanContext::root("t", "s"),
            start_time_ms: 0,
            end_time_ms: Some(50),
            attributes: BTreeMap::new(),
        };
        let json = serde_json::to_string(&span).unwrap();
        let back: TelemetrySpan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "op");
        assert_eq!(back.duration_ms(), Some(50));
    }

    #[test]
    fn span_recorder_basic() {
        let rec = SpanRecorder::new();
        assert!(rec.is_empty());

        let span = SpanBuilder::new("op", SpanContext::root("t", "s")).finish();
        rec.record(span);
        assert_eq!(rec.len(), 1);
        assert!(!rec.is_empty());
    }

    #[test]
    fn span_recorder_filter_by_name() {
        let rec = SpanRecorder::new();
        rec.record(SpanBuilder::new("alpha", SpanContext::root("t", "s1")).finish());
        rec.record(SpanBuilder::new("beta", SpanContext::root("t", "s2")).finish());
        rec.record(SpanBuilder::new("alpha", SpanContext::root("t", "s3")).finish());
        assert_eq!(rec.spans_named("alpha").len(), 2);
        assert_eq!(rec.spans_named("beta").len(), 1);
        assert_eq!(rec.spans_named("gamma").len(), 0);
    }

    #[test]
    fn span_recorder_clear() {
        let rec = SpanRecorder::new();
        rec.record(SpanBuilder::new("op", SpanContext::root("t", "s")).finish());
        rec.clear();
        assert!(rec.is_empty());
    }

    #[test]
    fn span_recorder_thread_safety() {
        let rec = SpanRecorder::new();
        let handles: Vec<_> = (0..4)
            .map(|i| {
                let rec = rec.clone();
                std::thread::spawn(move || {
                    for j in 0..50 {
                        let ctx = SpanContext::root("t", format!("s-{i}-{j}"));
                        rec.record(SpanBuilder::new("op", ctx).finish());
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(rec.len(), 200);
    }
}
