// SPDX-License-Identifier: MIT OR Apache-2.0
//! Observability primitives: distributed-style tracing spans and a runtime observer
//! that aggregates metrics and trace data.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Status of a completed or in-progress span.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum SpanStatus {
    /// The operation completed successfully.
    Ok,
    /// The operation ended with an error.
    Error {
        /// Human-readable error description.
        message: String,
    },
    /// Status has not been explicitly set.
    Unset,
}

/// A single trace span representing a unit of work.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Span {
    /// Unique identifier for this span.
    pub id: String,
    /// Human-readable name describing the operation.
    pub name: String,
    /// ID of the parent span, if any.
    pub parent_id: Option<String>,
    /// ISO-8601 timestamp when the span started.
    pub start_time: String,
    /// ISO-8601 timestamp when the span ended, or `None` if still active.
    pub end_time: Option<String>,
    /// Arbitrary key-value attributes attached to the span.
    pub attributes: BTreeMap<String, String>,
    /// Current status of the span.
    pub status: SpanStatus,
}

/// Collects and queries trace spans for a single runtime execution.
#[derive(Clone, Debug, Default)]
pub struct TraceCollector {
    spans: Vec<Span>,
}

impl TraceCollector {
    /// Create an empty trace collector.
    #[must_use]
    pub fn new() -> Self {
        Self { spans: Vec::new() }
    }

    /// Start a new root span and return its ID.
    pub fn start_span(&mut self, name: &str) -> String {
        let id = Uuid::new_v4().to_string();
        self.spans.push(Span {
            id: id.clone(),
            name: name.to_string(),
            parent_id: None,
            start_time: chrono::Utc::now().to_rfc3339(),
            end_time: None,
            attributes: BTreeMap::new(),
            status: SpanStatus::Unset,
        });
        id
    }

    /// Start a new child span under the given parent and return its ID.
    pub fn start_child_span(&mut self, name: &str, parent_id: &str) -> String {
        let id = Uuid::new_v4().to_string();
        self.spans.push(Span {
            id: id.clone(),
            name: name.to_string(),
            parent_id: Some(parent_id.to_string()),
            start_time: chrono::Utc::now().to_rfc3339(),
            end_time: None,
            attributes: BTreeMap::new(),
            status: SpanStatus::Unset,
        });
        id
    }

    /// Mark a span as ended by recording the current time.
    pub fn end_span(&mut self, id: &str) {
        if let Some(span) = self.spans.iter_mut().find(|s| s.id == id) {
            span.end_time = Some(chrono::Utc::now().to_rfc3339());
        }
    }

    /// Set the status of a span.
    pub fn set_status(&mut self, id: &str, status: SpanStatus) {
        if let Some(span) = self.spans.iter_mut().find(|s| s.id == id) {
            span.status = status;
        }
    }

    /// Attach a key-value attribute to a span.
    pub fn set_attribute(&mut self, id: &str, key: &str, value: &str) {
        if let Some(span) = self.spans.iter_mut().find(|s| s.id == id) {
            span.attributes.insert(key.to_string(), value.to_string());
        }
    }

    /// Return all collected spans.
    #[must_use]
    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// Return spans that have not yet been ended.
    #[must_use]
    pub fn active_spans(&self) -> Vec<&Span> {
        self.spans.iter().filter(|s| s.end_time.is_none()).collect()
    }

    /// Return spans with no parent (root spans).
    #[must_use]
    pub fn root_spans(&self) -> Vec<&Span> {
        self.spans
            .iter()
            .filter(|s| s.parent_id.is_none())
            .collect()
    }

    /// Return direct children of the given parent span.
    #[must_use]
    pub fn children(&self, parent_id: &str) -> Vec<&Span> {
        self.spans
            .iter()
            .filter(|s| s.parent_id.as_deref() == Some(parent_id))
            .collect()
    }

    /// Serialise all spans to a JSON string.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.spans).unwrap_or_else(|_| "[]".to_string())
    }
}

/// High-level summary of observability state.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObservabilitySummary {
    /// Total number of spans recorded.
    pub total_spans: usize,
    /// Number of spans still active (no end time).
    pub active_spans: usize,
    /// Number of spans with error status.
    pub error_spans: usize,
    /// Number of custom metrics recorded.
    pub metrics_count: usize,
}

/// Aggregates runtime metrics and trace data into a single observability handle.
#[derive(Clone, Debug, Default)]
pub struct RuntimeObserver {
    metrics: BTreeMap<String, f64>,
    trace_collector: TraceCollector,
}

impl RuntimeObserver {
    /// Create a new, empty observer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a named metric value (overwrites any previous value for the key).
    pub fn record_metric(&mut self, name: &str, value: f64) {
        self.metrics.insert(name.to_string(), value);
    }

    /// Return all recorded metrics.
    #[must_use]
    pub fn metrics(&self) -> &BTreeMap<String, f64> {
        &self.metrics
    }

    /// Return a mutable reference to the inner [`TraceCollector`].
    pub fn trace_collector(&mut self) -> &mut TraceCollector {
        &mut self.trace_collector
    }

    /// Produce a point-in-time summary of spans and metrics.
    #[must_use]
    pub fn summary(&self) -> ObservabilitySummary {
        let error_spans = self
            .trace_collector
            .spans()
            .iter()
            .filter(|s| matches!(s.status, SpanStatus::Error { .. }))
            .count();
        ObservabilitySummary {
            total_spans: self.trace_collector.spans().len(),
            active_spans: self.trace_collector.active_spans().len(),
            error_spans,
            metrics_count: self.metrics.len(),
        }
    }
}
