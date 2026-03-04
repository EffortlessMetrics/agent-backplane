// SPDX-License-Identifier: MIT OR Apache-2.0
#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

use abp_core::{AgentEvent, AgentEventKind};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tokio::sync::mpsc;

pub mod aggregate;
pub mod backpressure;
pub mod buffer;
pub mod buffered;
pub mod collector;
pub mod merged;
pub mod metrics;
pub mod tee;
pub mod timeout;

pub use aggregate::{StreamAggregator, StreamSummary, ToolCallAggregate};
pub use backpressure::{BackpressurePolicy, BackpressuredSender, SendOutcome};
pub use buffer::StreamBuffer;
pub use buffered::BufferedStream;
pub use collector::EventCollector;
pub use merged::MergedStream;
pub use metrics::{MetricsSummary, StreamMetrics};
pub use tee::{StreamTee, TeeError};
pub use timeout::{StreamTimeout, TimeoutItem, TimeoutStream};

// ---------------------------------------------------------------------------
// EventFilter
// ---------------------------------------------------------------------------

/// Filters events by kind name, source, or a custom predicate.
#[derive(Clone)]
pub struct EventFilter {
    predicate: Arc<dyn Fn(&AgentEvent) -> bool + Send + Sync>,
}

impl std::fmt::Debug for EventFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventFilter").finish_non_exhaustive()
    }
}

impl EventFilter {
    /// Create a filter from a custom predicate.
    pub fn new<F>(predicate: F) -> Self
    where
        F: Fn(&AgentEvent) -> bool + Send + Sync + 'static,
    {
        Self {
            predicate: Arc::new(predicate),
        }
    }

    /// Filter events by their kind discriminant name.
    ///
    /// The `kind_name` is matched against the snake_case variant name
    /// (e.g. `"assistant_delta"`, `"tool_call"`, `"error"`).
    pub fn by_kind(kind_name: &str) -> Self {
        let kind_name = kind_name.to_string();
        Self::new(move |ev| event_kind_name(&ev.kind) == kind_name)
    }

    /// Filter to allow only error events.
    pub fn errors_only() -> Self {
        Self::new(|ev| matches!(ev.kind, AgentEventKind::Error { .. }))
    }

    /// Filter to exclude error events.
    pub fn exclude_errors() -> Self {
        Self::new(|ev| !matches!(ev.kind, AgentEventKind::Error { .. }))
    }

    /// Filter events matching any of the given kind names.
    pub fn by_kinds(kind_names: &[&str]) -> Self {
        let names: Vec<String> = kind_names.iter().map(|s| s.to_string()).collect();
        Self::new(move |ev| {
            let name = event_kind_name(&ev.kind);
            names.contains(&name)
        })
    }

    /// Combine two filters with logical AND — both must match.
    pub fn and(self, other: EventFilter) -> Self {
        let a = self.predicate;
        let b = other.predicate;
        Self {
            predicate: Arc::new(move |ev| a(ev) && b(ev)),
        }
    }

    /// Combine two filters with logical OR — either must match.
    pub fn or(self, other: EventFilter) -> Self {
        let a = self.predicate;
        let b = other.predicate;
        Self {
            predicate: Arc::new(move |ev| a(ev) || b(ev)),
        }
    }

    /// Negate a filter — passes events that the original rejects.
    #[allow(clippy::should_implement_trait)]
    pub fn not(self) -> Self {
        let inner = self.predicate;
        Self {
            predicate: Arc::new(move |ev| !inner(ev)),
        }
    }

    /// Returns `true` if the event passes the filter.
    pub fn matches(&self, event: &AgentEvent) -> bool {
        (self.predicate)(event)
    }
}

// ---------------------------------------------------------------------------
// EventTransform
// ---------------------------------------------------------------------------

/// Transforms events in-flight (e.g., add metadata, modify content).
#[derive(Clone)]
pub struct EventTransform {
    transform: Arc<dyn Fn(AgentEvent) -> AgentEvent + Send + Sync>,
}

impl std::fmt::Debug for EventTransform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventTransform").finish_non_exhaustive()
    }
}

impl EventTransform {
    /// Create a transform from a closure.
    pub fn new<F>(transform: F) -> Self
    where
        F: Fn(AgentEvent) -> AgentEvent + Send + Sync + 'static,
    {
        Self {
            transform: Arc::new(transform),
        }
    }

    /// Identity transform — passes events through unchanged.
    pub fn identity() -> Self {
        Self::new(|ev| ev)
    }

    /// Transform the text content of `AssistantDelta` and `AssistantMessage`
    /// events, leaving all other events unchanged.
    pub fn map_text<F>(f: F) -> Self
    where
        F: Fn(&str) -> String + Send + Sync + 'static,
    {
        Self::new(move |mut ev| {
            ev.kind = match ev.kind {
                AgentEventKind::AssistantDelta { text } => {
                    AgentEventKind::AssistantDelta { text: f(&text) }
                }
                AgentEventKind::AssistantMessage { text } => {
                    AgentEventKind::AssistantMessage { text: f(&text) }
                }
                other => other,
            };
            ev
        })
    }

    /// Add a metadata key-value pair to every event's `ext` map.
    pub fn add_metadata(key: impl Into<String>, value: serde_json::Value) -> Self {
        let key = key.into();
        Self::new(move |mut ev| {
            let ext = ev.ext.get_or_insert_with(std::collections::BTreeMap::new);
            ext.insert(key.clone(), value.clone());
            ev
        })
    }

    /// Chain two transforms: apply `self` first, then `other`.
    pub fn chain(self, other: EventTransform) -> Self {
        let a = self.transform;
        let b = other.transform;
        Self {
            transform: Arc::new(move |ev| b(a(ev))),
        }
    }

    /// Apply the transform to an event.
    pub fn apply(&self, event: AgentEvent) -> AgentEvent {
        (self.transform)(event)
    }
}

// ---------------------------------------------------------------------------
// EventRecorder
// ---------------------------------------------------------------------------

/// Records all events for later replay or inspection.
#[derive(Debug, Clone, Default)]
pub struct EventRecorder {
    events: Arc<Mutex<Vec<AgentEvent>>>,
}

impl EventRecorder {
    /// Create a new empty recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an event.
    pub fn record(&self, event: &AgentEvent) {
        self.events
            .lock()
            .expect("recorder lock poisoned")
            .push(event.clone());
    }

    /// Return a snapshot of all recorded events.
    pub fn events(&self) -> Vec<AgentEvent> {
        self.events.lock().expect("recorder lock poisoned").clone()
    }

    /// Number of recorded events.
    pub fn len(&self) -> usize {
        self.events.lock().expect("recorder lock poisoned").len()
    }

    /// Whether no events have been recorded.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clear all recorded events.
    pub fn clear(&self) {
        self.events.lock().expect("recorder lock poisoned").clear();
    }
}

// ---------------------------------------------------------------------------
// EventStats
// ---------------------------------------------------------------------------

/// Tracks event statistics: count by kind, total tokens, timing.
#[derive(Debug, Clone, Default)]
pub struct EventStats {
    inner: Arc<Mutex<StatsInner>>,
}

#[derive(Debug, Default)]
struct StatsInner {
    counts: HashMap<String, u64>,
    total_events: u64,
    total_delta_bytes: u64,
    error_count: u64,
    first_event_at: Option<std::time::Instant>,
    last_event_at: Option<std::time::Instant>,
}

impl EventStats {
    /// Create a new empty stats tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an event's statistics.
    pub fn observe(&self, event: &AgentEvent) {
        let mut inner = self.inner.lock().expect("stats lock poisoned");
        let now = std::time::Instant::now();
        if inner.first_event_at.is_none() {
            inner.first_event_at = Some(now);
        }
        inner.last_event_at = Some(now);

        let name = event_kind_name(&event.kind);
        *inner.counts.entry(name).or_insert(0) += 1;
        inner.total_events += 1;

        if let AgentEventKind::AssistantDelta { ref text } = event.kind {
            inner.total_delta_bytes += text.len() as u64;
        }
        if matches!(event.kind, AgentEventKind::Error { .. }) {
            inner.error_count += 1;
        }
    }

    /// Total number of events observed.
    pub fn total_events(&self) -> u64 {
        self.inner.lock().expect("stats lock poisoned").total_events
    }

    /// Count of events for a given kind name.
    pub fn count_for(&self, kind_name: &str) -> u64 {
        self.inner
            .lock()
            .expect("stats lock poisoned")
            .counts
            .get(kind_name)
            .copied()
            .unwrap_or(0)
    }

    /// Total bytes from `AssistantDelta` text payloads.
    pub fn total_delta_bytes(&self) -> u64 {
        self.inner
            .lock()
            .expect("stats lock poisoned")
            .total_delta_bytes
    }

    /// Number of error events observed.
    pub fn error_count(&self) -> u64 {
        self.inner.lock().expect("stats lock poisoned").error_count
    }

    /// Return a snapshot of per-kind counts.
    pub fn kind_counts(&self) -> HashMap<String, u64> {
        self.inner
            .lock()
            .expect("stats lock poisoned")
            .counts
            .clone()
    }

    /// Reset all statistics.
    pub fn reset(&self) {
        let mut inner = self.inner.lock().expect("stats lock poisoned");
        inner.counts.clear();
        inner.total_events = 0;
        inner.total_delta_bytes = 0;
        inner.error_count = 0;
        inner.first_event_at = None;
        inner.last_event_at = None;
    }

    /// Elapsed time between the first and last observed event.
    ///
    /// Returns [`std::time::Duration::ZERO`] if fewer than two events have
    /// been recorded.
    pub fn elapsed(&self) -> std::time::Duration {
        let inner = self.inner.lock().expect("stats lock poisoned");
        match (inner.first_event_at, inner.last_event_at) {
            (Some(first), Some(last)) => last.duration_since(first),
            _ => std::time::Duration::ZERO,
        }
    }

    /// Events per second based on the time between first and last event.
    ///
    /// Returns `0.0` if fewer than two events or no measurable time.
    pub fn events_per_second(&self) -> f64 {
        let inner = self.inner.lock().expect("stats lock poisoned");
        match (inner.first_event_at, inner.last_event_at) {
            (Some(first), Some(last)) => {
                let secs = last.duration_since(first).as_secs_f64();
                if secs > 0.0 {
                    inner.total_events as f64 / secs
                } else {
                    0.0
                }
            }
            _ => 0.0,
        }
    }

    /// Return a [`StatsSnapshot`] of the current state.
    pub fn snapshot(&self) -> StatsSnapshot {
        let inner = self.inner.lock().expect("stats lock poisoned");
        StatsSnapshot {
            total_events: inner.total_events,
            total_delta_bytes: inner.total_delta_bytes,
            error_count: inner.error_count,
            kind_counts: inner.counts.clone(),
            elapsed: match (inner.first_event_at, inner.last_event_at) {
                (Some(first), Some(last)) => last.duration_since(first),
                _ => std::time::Duration::ZERO,
            },
        }
    }
}

/// Point-in-time snapshot of [`EventStats`].
#[derive(Debug, Clone)]
pub struct StatsSnapshot {
    /// Total number of events observed.
    pub total_events: u64,
    /// Total bytes from `AssistantDelta` text payloads.
    pub total_delta_bytes: u64,
    /// Number of error events.
    pub error_count: u64,
    /// Per-kind event counts.
    pub kind_counts: HashMap<String, u64>,
    /// Elapsed time between first and last event.
    pub elapsed: std::time::Duration,
}

// ---------------------------------------------------------------------------
// EventStream
// ---------------------------------------------------------------------------

/// Wrapper around `mpsc::Receiver<AgentEvent>` providing stream processing.
///
/// Implements [`futures_core::Stream`] so it can be used with combinators,
/// [`EventCollector`], [`TimeoutStream`], etc.
pub struct EventStream {
    rx: mpsc::Receiver<AgentEvent>,
}

impl EventStream {
    /// Wrap an existing receiver.
    pub fn new(rx: mpsc::Receiver<AgentEvent>) -> Self {
        Self { rx }
    }

    /// Receive the next event, returning `None` when the channel closes.
    pub async fn recv(&mut self) -> Option<AgentEvent> {
        self.rx.recv().await
    }

    /// Collect all remaining events into a vec (consumes the stream).
    pub async fn collect_all(mut self) -> Vec<AgentEvent> {
        let mut out = Vec::new();
        while let Some(ev) = self.rx.recv().await {
            out.push(ev);
        }
        out
    }

    /// Collect events that pass the given filter.
    pub async fn collect_filtered(mut self, filter: &EventFilter) -> Vec<AgentEvent> {
        let mut out = Vec::new();
        while let Some(ev) = self.rx.recv().await {
            if filter.matches(&ev) {
                out.push(ev);
            }
        }
        out
    }

    /// Consume the stream, forwarding events through a pipeline.
    pub async fn pipe(mut self, pipeline: &StreamPipeline, tx: mpsc::Sender<AgentEvent>) {
        while let Some(ev) = self.rx.recv().await {
            if let Some(ev) = pipeline.process(ev)
                && tx.send(ev).await.is_err()
            {
                break;
            }
        }
    }

    /// Return the inner receiver.
    pub fn into_inner(self) -> mpsc::Receiver<AgentEvent> {
        self.rx
    }
}

impl futures_core::Stream for EventStream {
    type Item = AgentEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

// ---------------------------------------------------------------------------
// EventMultiplexer
// ---------------------------------------------------------------------------

/// Combines multiple event streams into one, maintaining ordering by timestamp.
pub struct EventMultiplexer {
    receivers: Vec<mpsc::Receiver<AgentEvent>>,
}

impl EventMultiplexer {
    /// Create a new multiplexer from multiple receivers.
    pub fn new(receivers: Vec<mpsc::Receiver<AgentEvent>>) -> Self {
        Self { receivers }
    }

    /// Drain all streams and return events sorted by timestamp.
    ///
    /// Each stream is drained concurrently; the final output is sorted.
    pub async fn collect_sorted(self) -> Vec<AgentEvent> {
        let mut all = Vec::new();
        for mut rx in self.receivers {
            while let Some(ev) = rx.recv().await {
                all.push(ev);
            }
        }
        all.sort_by_key(|ev| ev.ts);
        all
    }

    /// Merge streams into a single output channel using true interleaved
    /// fan-in. Events are forwarded as they arrive from any source stream,
    /// without waiting for all streams to close first.
    ///
    /// Returns the receiving end of the merged stream.
    pub fn merge(self, buffer: usize) -> mpsc::Receiver<AgentEvent> {
        let (tx, rx) = mpsc::channel(buffer);

        for mut r in self.receivers {
            let tx = tx.clone();
            tokio::spawn(async move {
                while let Some(ev) = r.recv().await {
                    if tx.send(ev).await.is_err() {
                        break;
                    }
                }
            });
        }
        // Drop the original sender so the channel closes when all tasks finish.
        drop(tx);

        rx
    }

    /// Number of source streams.
    pub fn stream_count(&self) -> usize {
        self.receivers.len()
    }
}

// ---------------------------------------------------------------------------
// StreamPipeline
// ---------------------------------------------------------------------------

/// A composed pipeline of filters, transforms, recording, and statistics.
#[derive(Debug, Clone, Default)]
pub struct StreamPipeline {
    filters: Vec<EventFilter>,
    transforms: Vec<EventTransform>,
    recorder: Option<EventRecorder>,
    stats: Option<EventStats>,
}

impl StreamPipeline {
    /// Create an empty pipeline (pass-through).
    pub fn new() -> Self {
        Self::default()
    }

    /// Process a single event through the pipeline.
    ///
    /// Returns `None` if the event was filtered out.
    pub fn process(&self, mut event: AgentEvent) -> Option<AgentEvent> {
        // Filters
        for f in &self.filters {
            if !f.matches(&event) {
                return None;
            }
        }

        // Transforms
        for t in &self.transforms {
            event = t.apply(event);
        }

        // Stats
        if let Some(ref stats) = self.stats {
            stats.observe(&event);
        }

        // Record
        if let Some(ref recorder) = self.recorder {
            recorder.record(&event);
        }

        Some(event)
    }

    /// Return the pipeline's recorder, if any.
    pub fn recorder(&self) -> Option<&EventRecorder> {
        self.recorder.as_ref()
    }

    /// Return the pipeline's stats tracker, if any.
    pub fn stats(&self) -> Option<&EventStats> {
        self.stats.as_ref()
    }
}

// ---------------------------------------------------------------------------
// StreamPipelineBuilder
// ---------------------------------------------------------------------------

/// Builder for constructing a [`StreamPipeline`].
#[derive(Debug, Default)]
pub struct StreamPipelineBuilder {
    filters: Vec<EventFilter>,
    transforms: Vec<EventTransform>,
    recorder: Option<EventRecorder>,
    stats: Option<EventStats>,
}

impl StreamPipelineBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a filter stage.
    pub fn filter(mut self, filter: EventFilter) -> Self {
        self.filters.push(filter);
        self
    }

    /// Add a transform stage.
    pub fn transform(mut self, transform: EventTransform) -> Self {
        self.transforms.push(transform);
        self
    }

    /// Enable recording.
    pub fn record(mut self) -> Self {
        self.recorder = Some(EventRecorder::new());
        self
    }

    /// Attach an external recorder.
    pub fn with_recorder(mut self, recorder: EventRecorder) -> Self {
        self.recorder = Some(recorder);
        self
    }

    /// Enable statistics tracking.
    pub fn with_stats(mut self, stats: EventStats) -> Self {
        self.stats = Some(stats);
        self
    }

    /// Build the pipeline.
    pub fn build(self) -> StreamPipeline {
        StreamPipeline {
            filters: self.filters,
            transforms: self.transforms,
            recorder: self.recorder,
            stats: self.stats,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return the snake_case discriminant name for an [`AgentEventKind`].
pub fn event_kind_name(kind: &AgentEventKind) -> String {
    match kind {
        AgentEventKind::RunStarted { .. } => "run_started".to_string(),
        AgentEventKind::RunCompleted { .. } => "run_completed".to_string(),
        AgentEventKind::AssistantDelta { .. } => "assistant_delta".to_string(),
        AgentEventKind::AssistantMessage { .. } => "assistant_message".to_string(),
        AgentEventKind::ToolCall { .. } => "tool_call".to_string(),
        AgentEventKind::ToolResult { .. } => "tool_result".to_string(),
        AgentEventKind::FileChanged { .. } => "file_changed".to_string(),
        AgentEventKind::CommandExecuted { .. } => "command_executed".to_string(),
        AgentEventKind::Warning { .. } => "warning".to_string(),
        AgentEventKind::Error { .. } => "error".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::collections::BTreeMap;

    fn make_event(kind: AgentEventKind) -> AgentEvent {
        AgentEvent {
            ts: Utc::now(),
            kind,
            ext: None,
        }
    }

    fn make_event_with_ts(kind: AgentEventKind, ts: chrono::DateTime<Utc>) -> AgentEvent {
        AgentEvent {
            ts,
            kind,
            ext: None,
        }
    }

    fn delta_event(text: &str) -> AgentEvent {
        make_event(AgentEventKind::AssistantDelta {
            text: text.to_string(),
        })
    }

    fn error_event(msg: &str) -> AgentEvent {
        make_event(AgentEventKind::Error {
            message: msg.to_string(),
            error_code: None,
        })
    }

    fn run_started_event() -> AgentEvent {
        make_event(AgentEventKind::RunStarted {
            message: "started".to_string(),
        })
    }

    fn run_completed_event() -> AgentEvent {
        make_event(AgentEventKind::RunCompleted {
            message: "done".to_string(),
        })
    }

    fn tool_call_event(name: &str) -> AgentEvent {
        make_event(AgentEventKind::ToolCall {
            tool_name: name.to_string(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        })
    }

    fn warning_event(msg: &str) -> AgentEvent {
        make_event(AgentEventKind::Warning {
            message: msg.to_string(),
        })
    }

    fn file_changed_event(path: &str) -> AgentEvent {
        make_event(AgentEventKind::FileChanged {
            path: path.to_string(),
            summary: "modified".to_string(),
        })
    }

    // -----------------------------------------------------------------------
    // EventFilter tests
    // -----------------------------------------------------------------------

    #[test]
    fn filter_by_kind_matches_assistant_delta() {
        let filter = EventFilter::by_kind("assistant_delta");
        assert!(filter.matches(&delta_event("hello")));
        assert!(!filter.matches(&error_event("oops")));
    }

    #[test]
    fn filter_by_kind_matches_tool_call() {
        let filter = EventFilter::by_kind("tool_call");
        assert!(filter.matches(&tool_call_event("read_file")));
        assert!(!filter.matches(&delta_event("text")));
    }

    #[test]
    fn filter_by_kind_matches_error() {
        let filter = EventFilter::by_kind("error");
        assert!(filter.matches(&error_event("fail")));
        assert!(!filter.matches(&run_started_event()));
    }

    #[test]
    fn filter_errors_only() {
        let filter = EventFilter::errors_only();
        assert!(filter.matches(&error_event("bad")));
        assert!(!filter.matches(&delta_event("ok")));
        assert!(!filter.matches(&warning_event("warn")));
    }

    #[test]
    fn filter_exclude_errors() {
        let filter = EventFilter::exclude_errors();
        assert!(!filter.matches(&error_event("bad")));
        assert!(filter.matches(&delta_event("ok")));
        assert!(filter.matches(&warning_event("warn")));
    }

    #[test]
    fn filter_custom_predicate() {
        let filter = EventFilter::new(
            |ev| matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text.len() > 3),
        );
        assert!(filter.matches(&delta_event("long text")));
        assert!(!filter.matches(&delta_event("hi")));
    }

    #[test]
    fn filter_by_kind_run_started() {
        let filter = EventFilter::by_kind("run_started");
        assert!(filter.matches(&run_started_event()));
        assert!(!filter.matches(&run_completed_event()));
    }

    // -----------------------------------------------------------------------
    // EventTransform tests
    // -----------------------------------------------------------------------

    #[test]
    fn transform_identity_preserves_event() {
        let t = EventTransform::identity();
        let ev = delta_event("hello");
        let result = t.apply(ev.clone());
        assert_eq!(result.ts, ev.ts);
    }

    #[test]
    fn transform_modifies_event_ext() {
        let t = EventTransform::new(|mut ev| {
            let mut ext = BTreeMap::new();
            ext.insert("tag".to_string(), serde_json::json!("transformed"));
            ev.ext = Some(ext);
            ev
        });
        let result = t.apply(delta_event("hello"));
        assert!(result.ext.is_some());
        assert_eq!(
            result.ext.unwrap().get("tag").unwrap(),
            &serde_json::json!("transformed")
        );
    }

    #[test]
    fn transform_chain_applies_in_order() {
        let t1 = EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("step".to_string(), serde_json::json!(1));
            ev
        });
        let t2 = EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("step".to_string(), serde_json::json!(2));
            ev
        });
        let ev = delta_event("x");
        let ev = t1.apply(ev);
        let ev = t2.apply(ev);
        assert_eq!(ev.ext.unwrap().get("step").unwrap(), &serde_json::json!(2));
    }

    // -----------------------------------------------------------------------
    // EventRecorder tests
    // -----------------------------------------------------------------------

    #[test]
    fn recorder_captures_all_events() {
        let r = EventRecorder::new();
        assert!(r.is_empty());
        r.record(&delta_event("a"));
        r.record(&delta_event("b"));
        r.record(&error_event("e"));
        assert_eq!(r.len(), 3);
        assert_eq!(r.events().len(), 3);
    }

    #[test]
    fn recorder_clear_resets() {
        let r = EventRecorder::new();
        r.record(&delta_event("a"));
        assert_eq!(r.len(), 1);
        r.clear();
        assert!(r.is_empty());
    }

    #[test]
    fn recorder_clone_shares_state() {
        let r = EventRecorder::new();
        let r2 = r.clone();
        r.record(&delta_event("a"));
        assert_eq!(r2.len(), 1);
    }

    // -----------------------------------------------------------------------
    // EventStats tests
    // -----------------------------------------------------------------------

    #[test]
    fn stats_tracks_counts_by_kind() {
        let s = EventStats::new();
        s.observe(&delta_event("a"));
        s.observe(&delta_event("bb"));
        s.observe(&error_event("e"));
        s.observe(&tool_call_event("read"));
        assert_eq!(s.total_events(), 4);
        assert_eq!(s.count_for("assistant_delta"), 2);
        assert_eq!(s.count_for("error"), 1);
        assert_eq!(s.count_for("tool_call"), 1);
        assert_eq!(s.count_for("warning"), 0);
    }

    #[test]
    fn stats_tracks_delta_bytes() {
        let s = EventStats::new();
        s.observe(&delta_event("hello")); // 5 bytes
        s.observe(&delta_event("world!")); // 6 bytes
        assert_eq!(s.total_delta_bytes(), 11);
    }

    #[test]
    fn stats_tracks_error_count() {
        let s = EventStats::new();
        s.observe(&error_event("e1"));
        s.observe(&error_event("e2"));
        s.observe(&delta_event("ok"));
        assert_eq!(s.error_count(), 2);
    }

    #[test]
    fn stats_reset_clears_all() {
        let s = EventStats::new();
        s.observe(&delta_event("a"));
        s.observe(&error_event("e"));
        s.reset();
        assert_eq!(s.total_events(), 0);
        assert_eq!(s.total_delta_bytes(), 0);
        assert_eq!(s.error_count(), 0);
        assert!(s.kind_counts().is_empty());
    }

    #[test]
    fn stats_kind_counts_snapshot() {
        let s = EventStats::new();
        s.observe(&run_started_event());
        s.observe(&delta_event("x"));
        s.observe(&run_completed_event());
        let counts = s.kind_counts();
        assert_eq!(counts.get("run_started"), Some(&1));
        assert_eq!(counts.get("assistant_delta"), Some(&1));
        assert_eq!(counts.get("run_completed"), Some(&1));
    }

    // -----------------------------------------------------------------------
    // EventStream tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn stream_collect_all() {
        let (tx, rx) = mpsc::channel(16);
        tx.send(delta_event("a")).await.unwrap();
        tx.send(delta_event("b")).await.unwrap();
        drop(tx);

        let stream = EventStream::new(rx);
        let events = stream.collect_all().await;
        assert_eq!(events.len(), 2);
    }

    #[tokio::test]
    async fn stream_collect_filtered() {
        let (tx, rx) = mpsc::channel(16);
        tx.send(delta_event("a")).await.unwrap();
        tx.send(error_event("e")).await.unwrap();
        tx.send(delta_event("b")).await.unwrap();
        drop(tx);

        let stream = EventStream::new(rx);
        let filter = EventFilter::by_kind("assistant_delta");
        let events = stream.collect_filtered(&filter).await;
        assert_eq!(events.len(), 2);
    }

    #[tokio::test]
    async fn stream_recv_returns_none_on_close() {
        let (tx, rx) = mpsc::channel::<AgentEvent>(16);
        drop(tx);
        let mut stream = EventStream::new(rx);
        assert!(stream.recv().await.is_none());
    }

    #[tokio::test]
    async fn stream_empty_collect() {
        let (_tx, rx) = mpsc::channel::<AgentEvent>(16);
        drop(_tx);
        let stream = EventStream::new(rx);
        let events = stream.collect_all().await;
        assert!(events.is_empty());
    }

    // -----------------------------------------------------------------------
    // EventMultiplexer tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn multiplexer_preserves_timestamp_ordering() {
        let ts_base = Utc::now();
        let ts1 = ts_base;
        let ts2 = ts_base + chrono::Duration::milliseconds(10);
        let ts3 = ts_base + chrono::Duration::milliseconds(20);
        let ts4 = ts_base + chrono::Duration::milliseconds(30);

        let (tx1, rx1) = mpsc::channel(16);
        let (tx2, rx2) = mpsc::channel(16);

        // Stream 1: ts1, ts3
        tx1.send(make_event_with_ts(
            AgentEventKind::AssistantDelta {
                text: "s1-a".to_string(),
            },
            ts1,
        ))
        .await
        .unwrap();
        tx1.send(make_event_with_ts(
            AgentEventKind::AssistantDelta {
                text: "s1-b".to_string(),
            },
            ts3,
        ))
        .await
        .unwrap();
        drop(tx1);

        // Stream 2: ts2, ts4
        tx2.send(make_event_with_ts(
            AgentEventKind::AssistantDelta {
                text: "s2-a".to_string(),
            },
            ts2,
        ))
        .await
        .unwrap();
        tx2.send(make_event_with_ts(
            AgentEventKind::AssistantDelta {
                text: "s2-b".to_string(),
            },
            ts4,
        ))
        .await
        .unwrap();
        drop(tx2);

        let mux = EventMultiplexer::new(vec![rx1, rx2]);
        let events = mux.collect_sorted().await;

        assert_eq!(events.len(), 4);
        assert_eq!(events[0].ts, ts1);
        assert_eq!(events[1].ts, ts2);
        assert_eq!(events[2].ts, ts3);
        assert_eq!(events[3].ts, ts4);
    }

    #[tokio::test]
    async fn multiplexer_empty_streams() {
        let (_tx, rx1) = mpsc::channel::<AgentEvent>(16);
        let (_tx2, rx2) = mpsc::channel::<AgentEvent>(16);
        drop(_tx);
        drop(_tx2);

        let mux = EventMultiplexer::new(vec![rx1, rx2]);
        let events = mux.collect_sorted().await;
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn multiplexer_merge_channel() {
        let ts_base = Utc::now();
        let (tx1, rx1) = mpsc::channel(16);
        let (tx2, rx2) = mpsc::channel(16);

        tx1.send(make_event_with_ts(
            AgentEventKind::RunStarted {
                message: "a".to_string(),
            },
            ts_base,
        ))
        .await
        .unwrap();
        tx2.send(make_event_with_ts(
            AgentEventKind::RunCompleted {
                message: "b".to_string(),
            },
            ts_base + chrono::Duration::milliseconds(5),
        ))
        .await
        .unwrap();
        drop(tx1);
        drop(tx2);

        let mux = EventMultiplexer::new(vec![rx1, rx2]);
        let mut merged_rx = mux.merge(16);

        let first = merged_rx.recv().await.unwrap();
        assert!(matches!(first.kind, AgentEventKind::RunStarted { .. }));
        let second = merged_rx.recv().await.unwrap();
        assert!(matches!(second.kind, AgentEventKind::RunCompleted { .. }));
        assert!(merged_rx.recv().await.is_none());
    }

    // -----------------------------------------------------------------------
    // StreamPipeline tests
    // -----------------------------------------------------------------------

    #[test]
    fn pipeline_empty_passes_through() {
        let p = StreamPipeline::new();
        let ev = delta_event("test");
        let result = p.process(ev);
        assert!(result.is_some());
    }

    #[test]
    fn pipeline_filter_removes_events() {
        let p = StreamPipelineBuilder::new()
            .filter(EventFilter::by_kind("assistant_delta"))
            .build();

        assert!(p.process(delta_event("ok")).is_some());
        assert!(p.process(error_event("bad")).is_none());
    }

    #[test]
    fn pipeline_transform_modifies_events() {
        let p = StreamPipelineBuilder::new()
            .transform(EventTransform::new(|mut ev| {
                let ext = ev.ext.get_or_insert_with(BTreeMap::new);
                ext.insert("pipeline".to_string(), serde_json::json!(true));
                ev
            }))
            .build();

        let result = p.process(delta_event("x")).unwrap();
        assert_eq!(
            result.ext.unwrap().get("pipeline").unwrap(),
            &serde_json::json!(true)
        );
    }

    #[test]
    fn pipeline_records_events() {
        let recorder = EventRecorder::new();
        let p = StreamPipelineBuilder::new()
            .with_recorder(recorder.clone())
            .build();

        p.process(delta_event("a"));
        p.process(delta_event("b"));
        assert_eq!(recorder.len(), 2);
    }

    #[test]
    fn pipeline_tracks_stats() {
        let stats = EventStats::new();
        let p = StreamPipelineBuilder::new()
            .with_stats(stats.clone())
            .build();

        p.process(delta_event("a"));
        p.process(error_event("e"));
        assert_eq!(stats.total_events(), 2);
        assert_eq!(stats.error_count(), 1);
    }

    #[test]
    fn pipeline_filter_then_record_skips_filtered() {
        let recorder = EventRecorder::new();
        let p = StreamPipelineBuilder::new()
            .filter(EventFilter::exclude_errors())
            .with_recorder(recorder.clone())
            .build();

        p.process(delta_event("ok"));
        p.process(error_event("bad"));
        p.process(delta_event("also ok"));
        // Only non-error events should be recorded
        assert_eq!(recorder.len(), 2);
    }

    #[test]
    fn pipeline_compose_filter_transform_record_stats() {
        let recorder = EventRecorder::new();
        let stats = EventStats::new();
        let p = StreamPipelineBuilder::new()
            .filter(EventFilter::exclude_errors())
            .transform(EventTransform::new(|mut ev| {
                let ext = ev.ext.get_or_insert_with(BTreeMap::new);
                ext.insert("processed".to_string(), serde_json::json!(true));
                ev
            }))
            .with_recorder(recorder.clone())
            .with_stats(stats.clone())
            .build();

        p.process(delta_event("a"));
        p.process(error_event("e"));
        p.process(tool_call_event("write"));

        // Error filtered out
        assert_eq!(stats.total_events(), 2);
        assert_eq!(recorder.len(), 2);
        // All recorded events have the "processed" ext
        for ev in recorder.events() {
            assert_eq!(
                ev.ext.as_ref().unwrap().get("processed").unwrap(),
                &serde_json::json!(true)
            );
        }
    }

    #[test]
    fn pipeline_multiple_filters_all_must_pass() {
        let p = StreamPipelineBuilder::new()
            .filter(EventFilter::exclude_errors())
            .filter(EventFilter::by_kind("assistant_delta"))
            .build();

        assert!(p.process(delta_event("ok")).is_some());
        assert!(p.process(error_event("bad")).is_none());
        assert!(p.process(tool_call_event("read")).is_none());
    }

    #[test]
    fn pipeline_error_events_pass_without_filter() {
        let stats = EventStats::new();
        let p = StreamPipelineBuilder::new()
            .with_stats(stats.clone())
            .build();

        p.process(error_event("e1"));
        p.process(error_event("e2"));
        assert_eq!(stats.error_count(), 2);
        assert_eq!(stats.total_events(), 2);
    }

    #[tokio::test]
    async fn stream_pipe_through_pipeline() {
        let (tx_in, rx_in) = mpsc::channel(16);
        let (tx_out, mut rx_out) = mpsc::channel(16);

        let recorder = EventRecorder::new();
        let pipeline = StreamPipelineBuilder::new()
            .filter(EventFilter::exclude_errors())
            .with_recorder(recorder.clone())
            .build();

        tx_in.send(delta_event("a")).await.unwrap();
        tx_in.send(error_event("e")).await.unwrap();
        tx_in.send(delta_event("b")).await.unwrap();
        drop(tx_in);

        let stream = EventStream::new(rx_in);
        stream.pipe(&pipeline, tx_out).await;

        let mut results = Vec::new();
        while let Some(ev) = rx_out.recv().await {
            results.push(ev);
        }
        assert_eq!(results.len(), 2);
        assert_eq!(recorder.len(), 2);
    }

    // -----------------------------------------------------------------------
    // event_kind_name tests
    // -----------------------------------------------------------------------

    #[test]
    fn event_kind_name_all_variants() {
        assert_eq!(
            event_kind_name(&AgentEventKind::RunStarted {
                message: String::new()
            }),
            "run_started"
        );
        assert_eq!(
            event_kind_name(&AgentEventKind::RunCompleted {
                message: String::new()
            }),
            "run_completed"
        );
        assert_eq!(
            event_kind_name(&AgentEventKind::AssistantDelta {
                text: String::new()
            }),
            "assistant_delta"
        );
        assert_eq!(
            event_kind_name(&AgentEventKind::AssistantMessage {
                text: String::new()
            }),
            "assistant_message"
        );
        assert_eq!(
            event_kind_name(&AgentEventKind::ToolCall {
                tool_name: String::new(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!(null),
            }),
            "tool_call"
        );
        assert_eq!(
            event_kind_name(&AgentEventKind::ToolResult {
                tool_name: String::new(),
                tool_use_id: None,
                output: serde_json::json!(null),
                is_error: false,
            }),
            "tool_result"
        );
        assert_eq!(
            event_kind_name(&AgentEventKind::FileChanged {
                path: String::new(),
                summary: String::new(),
            }),
            "file_changed"
        );
        assert_eq!(
            event_kind_name(&AgentEventKind::CommandExecuted {
                command: String::new(),
                exit_code: None,
                output_preview: None,
            }),
            "command_executed"
        );
        assert_eq!(
            event_kind_name(&AgentEventKind::Warning {
                message: String::new()
            }),
            "warning"
        );
        assert_eq!(
            event_kind_name(&AgentEventKind::Error {
                message: String::new(),
                error_code: None,
            }),
            "error"
        );
    }

    #[test]
    fn filter_file_changed_events() {
        let filter = EventFilter::by_kind("file_changed");
        assert!(filter.matches(&file_changed_event("src/main.rs")));
        assert!(!filter.matches(&delta_event("text")));
    }

    // -----------------------------------------------------------------------
    // Back-pressure behavior test
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn back_pressure_bounded_channel() {
        // Verify that a full channel does not lose events — sender blocks.
        let (tx, rx) = mpsc::channel(2);

        tx.send(delta_event("1")).await.unwrap();
        tx.send(delta_event("2")).await.unwrap();

        // Channel is now full (capacity 2). Spawn a task that sends a third
        // event then drops the sender.
        let sender = tokio::spawn(async move {
            tx.send(delta_event("3")).await.unwrap();
            // tx is dropped here, closing the channel after the send.
        });

        // Drain the stream — this unblocks the sender.
        let stream = EventStream::new(rx);
        let events = stream.collect_all().await;
        sender.await.unwrap();

        assert_eq!(events.len(), 3);
    }

    // -----------------------------------------------------------------------
    // EventFilter combinator tests
    // -----------------------------------------------------------------------

    #[test]
    fn filter_by_kinds_matches_multiple() {
        let filter = EventFilter::by_kinds(&["assistant_delta", "error"]);
        assert!(filter.matches(&delta_event("hi")));
        assert!(filter.matches(&error_event("bad")));
        assert!(!filter.matches(&tool_call_event("read")));
        assert!(!filter.matches(&run_started_event()));
    }

    #[test]
    fn filter_by_kinds_empty_matches_nothing() {
        let filter = EventFilter::by_kinds(&[]);
        assert!(!filter.matches(&delta_event("hi")));
        assert!(!filter.matches(&error_event("bad")));
    }

    #[test]
    fn filter_and_both_must_pass() {
        let f = EventFilter::by_kind("assistant_delta").and(EventFilter::new(
            |ev| matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text.len() > 3),
        ));
        assert!(f.matches(&delta_event("long text")));
        assert!(!f.matches(&delta_event("hi"))); // too short
        assert!(!f.matches(&error_event("long error text"))); // wrong kind
    }

    #[test]
    fn filter_or_either_passes() {
        let f = EventFilter::by_kind("error").or(EventFilter::by_kind("warning"));
        assert!(f.matches(&error_event("e")));
        assert!(f.matches(&warning_event("w")));
        assert!(!f.matches(&delta_event("d")));
    }

    #[test]
    fn filter_not_inverts() {
        let f = EventFilter::by_kind("error").not();
        assert!(!f.matches(&error_event("e")));
        assert!(f.matches(&delta_event("d")));
        assert!(f.matches(&warning_event("w")));
    }

    #[test]
    fn filter_complex_composition() {
        // (delta OR tool_call) AND NOT error
        let f = EventFilter::by_kinds(&["assistant_delta", "tool_call"])
            .and(EventFilter::errors_only().not());
        assert!(f.matches(&delta_event("hi")));
        assert!(f.matches(&tool_call_event("read")));
        assert!(!f.matches(&error_event("e")));
        assert!(!f.matches(&warning_event("w")));
    }

    // -----------------------------------------------------------------------
    // EventTransform utility tests
    // -----------------------------------------------------------------------

    #[test]
    fn transform_map_text_modifies_delta() {
        let t = EventTransform::map_text(|s| s.to_uppercase());
        let result = t.apply(delta_event("hello"));
        match &result.kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "HELLO"),
            _ => panic!("expected AssistantDelta"),
        }
    }

    #[test]
    fn transform_map_text_modifies_message() {
        let t = EventTransform::map_text(|s| format!("[prefix] {s}"));
        let ev = make_event(AgentEventKind::AssistantMessage {
            text: "content".to_string(),
        });
        let result = t.apply(ev);
        match &result.kind {
            AgentEventKind::AssistantMessage { text } => {
                assert_eq!(text, "[prefix] content")
            }
            _ => panic!("expected AssistantMessage"),
        }
    }

    #[test]
    fn transform_map_text_leaves_other_events() {
        let t = EventTransform::map_text(|s| s.to_uppercase());
        let ev = error_event("oops");
        let result = t.apply(ev);
        match &result.kind {
            AgentEventKind::Error { message, .. } => assert_eq!(message, "oops"),
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn transform_add_metadata_sets_ext() {
        let t = EventTransform::add_metadata("source", serde_json::json!("test"));
        let result = t.apply(delta_event("x"));
        let ext = result.ext.unwrap();
        assert_eq!(ext.get("source").unwrap(), &serde_json::json!("test"));
    }

    #[test]
    fn transform_add_metadata_preserves_existing() {
        let t = EventTransform::add_metadata("new_key", serde_json::json!(42));
        let mut ev = delta_event("x");
        let mut ext = BTreeMap::new();
        ext.insert("existing".to_string(), serde_json::json!("value"));
        ev.ext = Some(ext);

        let result = t.apply(ev);
        let ext = result.ext.unwrap();
        assert_eq!(ext.get("existing").unwrap(), &serde_json::json!("value"));
        assert_eq!(ext.get("new_key").unwrap(), &serde_json::json!(42));
    }

    #[test]
    fn transform_chain_applies_both() {
        let t = EventTransform::map_text(|s| s.to_uppercase()).chain(EventTransform::add_metadata(
            "done",
            serde_json::json!(true),
        ));
        let result = t.apply(delta_event("hello"));
        match &result.kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "HELLO"),
            _ => panic!("expected AssistantDelta"),
        }
        assert_eq!(
            result.ext.unwrap().get("done").unwrap(),
            &serde_json::json!(true)
        );
    }

    // -----------------------------------------------------------------------
    // EventStats enhanced tests
    // -----------------------------------------------------------------------

    #[test]
    fn stats_snapshot_captures_state() {
        let s = EventStats::new();
        s.observe(&delta_event("hello"));
        s.observe(&error_event("e"));
        s.observe(&tool_call_event("read"));
        let snap = s.snapshot();
        assert_eq!(snap.total_events, 3);
        assert_eq!(snap.total_delta_bytes, 5);
        assert_eq!(snap.error_count, 1);
        assert_eq!(snap.kind_counts.get("assistant_delta"), Some(&1));
        assert_eq!(snap.kind_counts.get("error"), Some(&1));
        assert_eq!(snap.kind_counts.get("tool_call"), Some(&1));
    }

    #[test]
    fn stats_elapsed_zero_with_no_events() {
        let s = EventStats::new();
        assert_eq!(s.elapsed(), std::time::Duration::ZERO);
        assert_eq!(s.events_per_second(), 0.0);
    }

    #[test]
    fn stats_reset_clears_timestamps() {
        let s = EventStats::new();
        s.observe(&delta_event("a"));
        s.reset();
        assert_eq!(s.elapsed(), std::time::Duration::ZERO);
        assert_eq!(s.events_per_second(), 0.0);
        let snap = s.snapshot();
        assert_eq!(snap.total_events, 0);
    }

    // -----------------------------------------------------------------------
    // BackpressuredSender tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn backpressure_block_policy_sends_all() {
        use crate::backpressure::{BackpressurePolicy, BackpressuredSender, SendOutcome};

        let (tx, rx) = mpsc::channel(16);
        let mut sender = BackpressuredSender::new(tx, BackpressurePolicy::Block);

        assert_eq!(sender.send(delta_event("a")).await, SendOutcome::Sent);
        assert_eq!(sender.send(delta_event("b")).await, SendOutcome::Sent);
        assert_eq!(sender.dropped_count(), 0);
        assert_eq!(sender.policy(), BackpressurePolicy::Block);

        drop(sender);
        let stream = EventStream::new(rx);
        let events = stream.collect_all().await;
        assert_eq!(events.len(), 2);
    }

    #[tokio::test]
    async fn backpressure_drop_newest_on_full_channel() {
        use crate::backpressure::{BackpressurePolicy, BackpressuredSender, SendOutcome};

        let (tx, rx) = mpsc::channel(2);
        let mut sender = BackpressuredSender::new(tx, BackpressurePolicy::DropNewest);

        assert_eq!(sender.send(delta_event("1")).await, SendOutcome::Sent);
        assert_eq!(sender.send(delta_event("2")).await, SendOutcome::Sent);
        // Channel full — this should be dropped.
        assert_eq!(sender.send(delta_event("3")).await, SendOutcome::Dropped);
        assert_eq!(sender.dropped_count(), 1);

        drop(sender);
        let stream = EventStream::new(rx);
        let events = stream.collect_all().await;
        assert_eq!(events.len(), 2);
    }

    #[tokio::test]
    async fn backpressure_closed_channel_returns_closed() {
        use crate::backpressure::{BackpressurePolicy, BackpressuredSender, SendOutcome};

        let (tx, rx) = mpsc::channel(16);
        drop(rx);
        let mut sender = BackpressuredSender::new(tx, BackpressurePolicy::Block);

        assert_eq!(sender.send(delta_event("a")).await, SendOutcome::Closed);
        assert!(sender.is_closed());
    }

    // -----------------------------------------------------------------------
    // EventMultiplexer interleaved merge tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn multiplexer_merge_interleaved_fan_in() {
        let (tx1, rx1) = mpsc::channel(16);
        let (tx2, rx2) = mpsc::channel(16);
        let (tx3, rx3) = mpsc::channel(16);

        // Send events from 3 streams.
        for i in 0..3 {
            tx1.send(delta_event(&format!("s1-{i}"))).await.unwrap();
            tx2.send(delta_event(&format!("s2-{i}"))).await.unwrap();
            tx3.send(delta_event(&format!("s3-{i}"))).await.unwrap();
        }
        drop(tx1);
        drop(tx2);
        drop(tx3);

        let mux = EventMultiplexer::new(vec![rx1, rx2, rx3]);
        assert_eq!(mux.stream_count(), 3);
        let mut merged_rx = mux.merge(16);

        let mut events = Vec::new();
        while let Some(ev) = merged_rx.recv().await {
            events.push(ev);
        }
        // All 9 events should arrive.
        assert_eq!(events.len(), 9);
    }

    #[tokio::test]
    async fn multiplexer_merge_empty() {
        let mux = EventMultiplexer::new(vec![]);
        let mut merged_rx = mux.merge(16);
        assert!(merged_rx.recv().await.is_none());
    }

    // -----------------------------------------------------------------------
    // Pipeline integration with new combinators
    // -----------------------------------------------------------------------

    #[test]
    fn pipeline_with_map_text_transform() {
        let p = StreamPipelineBuilder::new()
            .transform(EventTransform::map_text(|s| s.to_uppercase()))
            .build();

        let result = p.process(delta_event("hello")).unwrap();
        match &result.kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, "HELLO"),
            _ => panic!("expected AssistantDelta"),
        }
    }

    #[test]
    fn pipeline_with_combined_filters() {
        let stats = EventStats::new();
        let p = StreamPipelineBuilder::new()
            .filter(EventFilter::by_kind("assistant_delta").or(EventFilter::by_kind("tool_call")))
            .with_stats(stats.clone())
            .build();

        p.process(delta_event("ok"));
        p.process(tool_call_event("read"));
        p.process(error_event("bad"));
        p.process(warning_event("w"));

        assert_eq!(stats.total_events(), 2);
    }
}
