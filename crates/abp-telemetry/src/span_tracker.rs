// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(dead_code)]
//! [`SpanTracker`] — tracks individual request spans with timing and metadata.
//!
//! Unlike the simpler [`super::spans::SpanRecorder`] which stores completed
//! spans, `SpanTracker` manages the full lifecycle: starting, annotating, and
//! finishing spans by ID.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

// ---------------------------------------------------------------------------
// SpanEntry
// ---------------------------------------------------------------------------

/// A single tracked span with timing and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanEntry {
    /// Unique span identifier.
    pub span_id: String,
    /// Human-readable name of the operation.
    pub name: String,
    /// Optional parent span for nesting.
    pub parent_id: Option<String>,
    /// Arbitrary key-value metadata (deterministic ordering).
    pub metadata: BTreeMap<String, String>,
    /// Whether the span has been finished.
    pub finished: bool,
    /// Duration in milliseconds, set when finished.
    pub duration_ms: Option<u64>,
    /// ISO-8601 start timestamp (for serialization).
    pub started_at: String,
    /// ISO-8601 end timestamp (for serialization).
    pub ended_at: Option<String>,
}

// ---------------------------------------------------------------------------
// SpanTracker
// ---------------------------------------------------------------------------

/// Thread-safe tracker for individual request spans with full lifecycle
/// management.
///
/// Spans are identified by unique string IDs and can be started, annotated
/// with metadata, and finished. The tracker records timing via [`Instant`]
/// for accurate duration computation.
#[derive(Debug, Clone, Default)]
pub struct SpanTracker {
    inner: Arc<Mutex<SpanTrackerInner>>,
}

#[derive(Debug, Default)]
struct SpanTrackerInner {
    spans: BTreeMap<String, SpanEntry>,
    instants: BTreeMap<String, Instant>,
}

impl SpanTracker {
    /// Create a new, empty tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a new span with the given ID and name.
    ///
    /// If a span with this ID already exists, it is overwritten.
    pub fn start_span(
        &self,
        span_id: impl Into<String>,
        name: impl Into<String>,
        started_at: impl Into<String>,
    ) {
        let id = span_id.into();
        let mut inner = self.inner.lock().expect("span tracker lock poisoned");
        inner.instants.insert(id.clone(), Instant::now());
        inner.spans.insert(
            id.clone(),
            SpanEntry {
                span_id: id,
                name: name.into(),
                parent_id: None,
                metadata: BTreeMap::new(),
                finished: false,
                duration_ms: None,
                started_at: started_at.into(),
                ended_at: None,
            },
        );
    }

    /// Start a child span with a parent reference.
    pub fn start_child_span(
        &self,
        span_id: impl Into<String>,
        name: impl Into<String>,
        parent_id: impl Into<String>,
        started_at: impl Into<String>,
    ) {
        let id = span_id.into();
        let mut inner = self.inner.lock().expect("span tracker lock poisoned");
        inner.instants.insert(id.clone(), Instant::now());
        inner.spans.insert(
            id.clone(),
            SpanEntry {
                span_id: id,
                name: name.into(),
                parent_id: Some(parent_id.into()),
                metadata: BTreeMap::new(),
                finished: false,
                duration_ms: None,
                started_at: started_at.into(),
                ended_at: None,
            },
        );
    }

    /// Attach metadata to an existing span.
    ///
    /// Returns `false` if the span does not exist.
    pub fn annotate(
        &self,
        span_id: &str,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> bool {
        let mut inner = self.inner.lock().expect("span tracker lock poisoned");
        if let Some(entry) = inner.spans.get_mut(span_id) {
            entry.metadata.insert(key.into(), value.into());
            true
        } else {
            false
        }
    }

    /// Finish a span, recording its duration and end timestamp.
    ///
    /// Returns the duration in milliseconds, or `None` if the span doesn't
    /// exist or was already finished.
    pub fn finish_span(&self, span_id: &str, ended_at: impl Into<String>) -> Option<u64> {
        let mut inner = self.inner.lock().expect("span tracker lock poisoned");
        let instant = inner.instants.get(span_id)?;
        let duration_ms = instant.elapsed().as_millis() as u64;
        let entry = inner.spans.get_mut(span_id)?;
        if entry.finished {
            return None;
        }
        entry.finished = true;
        entry.duration_ms = Some(duration_ms);
        entry.ended_at = Some(ended_at.into());
        Some(duration_ms)
    }

    /// Get a snapshot of a single span.
    pub fn get_span(&self, span_id: &str) -> Option<SpanEntry> {
        let inner = self.inner.lock().expect("span tracker lock poisoned");
        inner.spans.get(span_id).cloned()
    }

    /// Get all tracked spans.
    pub fn all_spans(&self) -> Vec<SpanEntry> {
        let inner = self.inner.lock().expect("span tracker lock poisoned");
        inner.spans.values().cloned().collect()
    }

    /// Get only finished spans.
    pub fn finished_spans(&self) -> Vec<SpanEntry> {
        let inner = self.inner.lock().expect("span tracker lock poisoned");
        inner
            .spans
            .values()
            .filter(|s| s.finished)
            .cloned()
            .collect()
    }

    /// Get only active (unfinished) spans.
    pub fn active_spans(&self) -> Vec<SpanEntry> {
        let inner = self.inner.lock().expect("span tracker lock poisoned");
        inner
            .spans
            .values()
            .filter(|s| !s.finished)
            .cloned()
            .collect()
    }

    /// Get child spans for a given parent.
    pub fn children_of(&self, parent_id: &str) -> Vec<SpanEntry> {
        let inner = self.inner.lock().expect("span tracker lock poisoned");
        inner
            .spans
            .values()
            .filter(|s| s.parent_id.as_deref() == Some(parent_id))
            .cloned()
            .collect()
    }

    /// Total number of tracked spans (active + finished).
    pub fn len(&self) -> usize {
        let inner = self.inner.lock().expect("span tracker lock poisoned");
        inner.spans.len()
    }

    /// Whether the tracker has no spans.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Number of active (unfinished) spans.
    pub fn active_count(&self) -> usize {
        let inner = self.inner.lock().expect("span tracker lock poisoned");
        inner.spans.values().filter(|s| !s.finished).count()
    }

    /// Number of finished spans.
    pub fn finished_count(&self) -> usize {
        let inner = self.inner.lock().expect("span tracker lock poisoned");
        inner.spans.values().filter(|s| s.finished).count()
    }

    /// Clear all tracked spans.
    pub fn clear(&self) {
        let mut inner = self.inner.lock().expect("span tracker lock poisoned");
        inner.spans.clear();
        inner.instants.clear();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_new_is_empty() {
        let t = SpanTracker::new();
        assert!(t.is_empty());
        assert_eq!(t.len(), 0);
        assert_eq!(t.active_count(), 0);
        assert_eq!(t.finished_count(), 0);
    }

    #[test]
    fn tracker_start_span() {
        let t = SpanTracker::new();
        t.start_span("s1", "operation", "2025-01-01T00:00:00Z");
        assert_eq!(t.len(), 1);
        assert_eq!(t.active_count(), 1);
        assert_eq!(t.finished_count(), 0);

        let span = t.get_span("s1").unwrap();
        assert_eq!(span.name, "operation");
        assert!(!span.finished);
        assert!(span.duration_ms.is_none());
        assert!(span.parent_id.is_none());
    }

    #[test]
    fn tracker_start_child_span() {
        let t = SpanTracker::new();
        t.start_span("parent", "root_op", "t");
        t.start_child_span("child", "child_op", "parent", "t");
        assert_eq!(t.len(), 2);

        let child = t.get_span("child").unwrap();
        assert_eq!(child.parent_id.as_deref(), Some("parent"));
    }

    #[test]
    fn tracker_finish_span() {
        let t = SpanTracker::new();
        t.start_span("s1", "op", "t");
        std::thread::sleep(std::time::Duration::from_millis(2));
        let dur = t.finish_span("s1", "t2");
        assert!(dur.is_some());

        let span = t.get_span("s1").unwrap();
        assert!(span.finished);
        assert!(span.duration_ms.is_some());
        assert_eq!(span.ended_at.as_deref(), Some("t2"));
        assert_eq!(t.active_count(), 0);
        assert_eq!(t.finished_count(), 1);
    }

    #[test]
    fn tracker_finish_nonexistent_returns_none() {
        let t = SpanTracker::new();
        assert!(t.finish_span("nonexistent", "t").is_none());
    }

    #[test]
    fn tracker_double_finish_returns_none() {
        let t = SpanTracker::new();
        t.start_span("s1", "op", "t");
        assert!(t.finish_span("s1", "t2").is_some());
        assert!(t.finish_span("s1", "t3").is_none());
    }

    #[test]
    fn tracker_annotate() {
        let t = SpanTracker::new();
        t.start_span("s1", "op", "t");
        assert!(t.annotate("s1", "backend", "mock"));
        assert!(t.annotate("s1", "dialect", "openai"));

        let span = t.get_span("s1").unwrap();
        assert_eq!(span.metadata["backend"], "mock");
        assert_eq!(span.metadata["dialect"], "openai");
    }

    #[test]
    fn tracker_annotate_nonexistent_returns_false() {
        let t = SpanTracker::new();
        assert!(!t.annotate("nonexistent", "key", "value"));
    }

    #[test]
    fn tracker_get_nonexistent_span() {
        let t = SpanTracker::new();
        assert!(t.get_span("nonexistent").is_none());
    }

    #[test]
    fn tracker_all_spans() {
        let t = SpanTracker::new();
        t.start_span("s1", "op1", "t");
        t.start_span("s2", "op2", "t");
        t.finish_span("s1", "t2");
        assert_eq!(t.all_spans().len(), 2);
    }

    #[test]
    fn tracker_finished_spans() {
        let t = SpanTracker::new();
        t.start_span("s1", "op1", "t");
        t.start_span("s2", "op2", "t");
        t.finish_span("s1", "t2");
        let finished = t.finished_spans();
        assert_eq!(finished.len(), 1);
        assert_eq!(finished[0].span_id, "s1");
    }

    #[test]
    fn tracker_active_spans() {
        let t = SpanTracker::new();
        t.start_span("s1", "op1", "t");
        t.start_span("s2", "op2", "t");
        t.finish_span("s1", "t2");
        let active = t.active_spans();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].span_id, "s2");
    }

    #[test]
    fn tracker_children_of() {
        let t = SpanTracker::new();
        t.start_span("parent", "root", "t");
        t.start_child_span("c1", "child1", "parent", "t");
        t.start_child_span("c2", "child2", "parent", "t");
        t.start_span("other", "unrelated", "t");

        let children = t.children_of("parent");
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn tracker_clear() {
        let t = SpanTracker::new();
        t.start_span("s1", "op", "t");
        t.clear();
        assert!(t.is_empty());
    }

    #[test]
    fn tracker_overwrite_span() {
        let t = SpanTracker::new();
        t.start_span("s1", "original", "t");
        t.annotate("s1", "key", "value");
        t.start_span("s1", "replacement", "t2");

        let span = t.get_span("s1").unwrap();
        assert_eq!(span.name, "replacement");
        assert!(span.metadata.is_empty());
    }

    #[test]
    fn tracker_thread_safety() {
        let t = SpanTracker::new();
        let handles: Vec<_> = (0..4)
            .map(|i| {
                let t = t.clone();
                std::thread::spawn(move || {
                    for j in 0..25 {
                        let id = format!("s-{i}-{j}");
                        t.start_span(&id, "op", "t");
                        t.annotate(&id, "thread", i.to_string());
                        t.finish_span(&id, "t2");
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(t.len(), 100);
        assert_eq!(t.finished_count(), 100);
        assert_eq!(t.active_count(), 0);
    }

    #[test]
    fn span_entry_serde_roundtrip() {
        let entry = SpanEntry {
            span_id: "s1".into(),
            name: "op".into(),
            parent_id: Some("parent".into()),
            metadata: BTreeMap::from([("key".into(), "value".into())]),
            finished: true,
            duration_ms: Some(42),
            started_at: "2025-01-01T00:00:00Z".into(),
            ended_at: Some("2025-01-01T00:00:01Z".into()),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: SpanEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.span_id, "s1");
        assert_eq!(back.duration_ms, Some(42));
        assert_eq!(back.metadata["key"], "value");
    }
}
