// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep enhancement tests for abp-stream: buffer, multiplexer, filter,
//! transform, replay, and metrics.

use abp_core::{AgentEvent, AgentEventKind};
use abp_stream::*;
use chrono::Utc;
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn delta(text: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantDelta {
            text: text.to_string(),
        },
        ext: None,
    }
}

fn error_ev(msg: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Error {
            message: msg.to_string(),
            error_code: None,
        },
        ext: None,
    }
}

fn warning_ev(msg: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::Warning {
            message: msg.to_string(),
        },
        ext: None,
    }
}

fn tool_call_ev(name: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::ToolCall {
            tool_name: name.to_string(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        },
        ext: None,
    }
}

fn run_started() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunStarted {
            message: "go".to_string(),
        },
        ext: None,
    }
}

fn run_completed() -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::RunCompleted {
            message: "done".to_string(),
        },
        ext: None,
    }
}

fn file_changed(path: &str) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::FileChanged {
            path: path.to_string(),
            summary: "modified".to_string(),
        },
        ext: None,
    }
}

fn extract_delta_text(ev: &AgentEvent) -> Option<&str> {
    match &ev.kind {
        AgentEventKind::AssistantDelta { text } => Some(text),
        _ => None,
    }
}

// =========================================================================
// 1. StreamBuffer — FlushableBuffer with configurable flush strategies
// =========================================================================

#[test]
fn flushable_buffer_when_full_strategy() {
    let mut buf = FlushableBuffer::new(3, FlushStrategy::WhenFull);
    assert!(!buf.push(delta("a")));
    assert!(!buf.push(delta("b")));
    assert!(buf.push(delta("c"))); // now full → should_flush
    assert_eq!(buf.len(), 3);
    let flushed = buf.flush();
    assert_eq!(flushed.len(), 3);
    assert!(buf.is_empty());
}

#[test]
fn flushable_buffer_at_threshold_strategy() {
    let mut buf = FlushableBuffer::new(10, FlushStrategy::AtThreshold(3));
    assert!(!buf.push(delta("a")));
    assert!(!buf.push(delta("b")));
    assert!(buf.push(delta("c"))); // threshold 3 reached
    let flushed = buf.flush();
    assert_eq!(flushed.len(), 3);
}

#[test]
fn flushable_buffer_evicts_at_capacity() {
    let mut buf = FlushableBuffer::new(2, FlushStrategy::WhenFull);
    buf.push(delta("a"));
    buf.push(delta("b"));
    buf.push(delta("c")); // evicts "a"
    assert_eq!(buf.len(), 2);
    let flushed = buf.flush();
    assert_eq!(extract_delta_text(&flushed[0]), Some("b"));
    assert_eq!(extract_delta_text(&flushed[1]), Some("c"));
}

#[test]
fn flushable_buffer_strategy_getter() {
    let buf = FlushableBuffer::new(5, FlushStrategy::AtThreshold(3));
    assert_eq!(buf.strategy(), FlushStrategy::AtThreshold(3));
    assert_eq!(buf.capacity(), 5);
}

#[test]
#[should_panic(expected = "capacity must be > 0")]
fn flushable_buffer_zero_capacity_panics() {
    let _ = FlushableBuffer::new(0, FlushStrategy::WhenFull);
}

#[test]
#[should_panic(expected = "threshold must be in 1..=capacity")]
fn flushable_buffer_threshold_exceeds_capacity_panics() {
    let _ = FlushableBuffer::new(3, FlushStrategy::AtThreshold(5));
}

#[test]
fn flushable_buffer_flush_then_refill() {
    let mut buf = FlushableBuffer::new(2, FlushStrategy::WhenFull);
    buf.push(delta("a"));
    buf.push(delta("b"));
    let first = buf.flush();
    assert_eq!(first.len(), 2);
    assert!(buf.is_empty());

    buf.push(delta("c"));
    assert_eq!(buf.len(), 1);
    assert!(!buf.should_flush());
}

// =========================================================================
// 2. StreamMultiplexer — fan-out with subscribe/unsubscribe
// =========================================================================

#[tokio::test]
async fn mux_broadcast_different_event_types() {
    let mux = StreamMultiplexer::new(16);
    let (_id, mut rx) = mux.subscribe().await;

    mux.broadcast(&delta("text")).await;
    mux.broadcast(&error_ev("err")).await;
    mux.broadcast(&tool_call_ev("bash")).await;

    let e1 = rx.recv().await.unwrap();
    let e2 = rx.recv().await.unwrap();
    let e3 = rx.recv().await.unwrap();
    assert!(matches!(e1.kind, AgentEventKind::AssistantDelta { .. }));
    assert!(matches!(e2.kind, AgentEventKind::Error { .. }));
    assert!(matches!(e3.kind, AgentEventKind::ToolCall { .. }));
}

#[tokio::test]
async fn mux_rapid_subscribe_unsubscribe() {
    let mux = StreamMultiplexer::new(16);
    let mut ids = Vec::new();
    for _ in 0..10 {
        let (id, _rx) = mux.subscribe().await;
        ids.push(id);
    }
    assert_eq!(mux.subscriber_count().await, 10);
    for id in &ids[..5] {
        mux.unsubscribe(*id).await;
    }
    assert_eq!(mux.subscriber_count().await, 5);
}

#[tokio::test]
async fn mux_broadcast_sequence_ordering() {
    let mux = StreamMultiplexer::new(32);
    let (_id, mut rx) = mux.subscribe().await;

    for i in 0..20 {
        mux.broadcast(&delta(&format!("{i}"))).await;
    }

    for i in 0..20 {
        let ev = rx.recv().await.unwrap();
        assert_eq!(extract_delta_text(&ev), Some(i.to_string().as_str()));
    }
}

// =========================================================================
// 3. StreamFilter — severity, text_contains, combinators
// =========================================================================

#[test]
fn filter_severity_errors_and_warnings() {
    let f = EventFilter::severity_errors_and_warnings();
    assert!(f.matches(&error_ev("e")));
    assert!(f.matches(&warning_ev("w")));
    assert!(!f.matches(&delta("text")));
    assert!(!f.matches(&tool_call_ev("bash")));
    assert!(!f.matches(&run_started()));
}

#[test]
fn filter_text_contains_matches_delta() {
    let f = EventFilter::text_contains("secret");
    assert!(f.matches(&delta("this is a secret")));
    assert!(!f.matches(&delta("nothing here")));
    // Does not match non-text events
    assert!(!f.matches(&error_ev("secret error")));
}

#[test]
fn filter_text_contains_matches_message() {
    let f = EventFilter::text_contains("hello");
    let ev = AgentEvent {
        ts: Utc::now(),
        kind: AgentEventKind::AssistantMessage {
            text: "hello world".to_string(),
        },
        ext: None,
    };
    assert!(f.matches(&ev));
}

#[test]
fn filter_severity_and_kind_composition() {
    // Only warnings (not errors)
    let f = EventFilter::severity_errors_and_warnings().and(EventFilter::by_kind("warning"));
    assert!(f.matches(&warning_ev("w")));
    assert!(!f.matches(&error_ev("e")));
    assert!(!f.matches(&delta("d")));
}

#[test]
fn filter_complex_multi_level_composition() {
    // Accept deltas with text > 5 chars, OR any error, but NOT warnings
    let long_delta = EventFilter::by_kind("assistant_delta").and(EventFilter::new(
        |ev| matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text.len() > 5),
    ));
    let f = long_delta
        .or(EventFilter::errors_only())
        .and(EventFilter::by_kind("warning").not());

    assert!(f.matches(&delta("long text here")));
    assert!(!f.matches(&delta("hi"))); // too short
    assert!(f.matches(&error_ev("any error")));
    assert!(!f.matches(&warning_ev("warn")));
}

// =========================================================================
// 4. StreamTransform — redact, enrich, chain
// =========================================================================

#[test]
fn transform_redact_text_replaces_content() {
    let t = EventTransform::redact_text("password123", "[REDACTED]");
    let ev = delta("my password123 is here");
    let result = t.apply(ev);
    assert_eq!(extract_delta_text(&result), Some("my [REDACTED] is here"));
}

#[test]
fn transform_redact_text_leaves_non_text_events() {
    let t = EventTransform::redact_text("secret", "***");
    let ev = error_ev("secret error");
    let result = t.apply(ev);
    // Error messages are not text events — they pass through unchanged
    match &result.kind {
        AgentEventKind::Error { message, .. } => assert_eq!(message, "secret error"),
        _ => panic!("expected Error"),
    }
}

#[test]
fn transform_enrich_with_stream_id() {
    let t = EventTransform::enrich_with_stream_id("stream-42");
    let result = t.apply(delta("hello"));
    let ext = result.ext.unwrap();
    assert_eq!(
        ext.get("stream_id").unwrap(),
        &serde_json::json!("stream-42")
    );
}

#[test]
fn transform_chain_redact_then_enrich() {
    let t = EventTransform::redact_text("token-abc", "[REDACTED]")
        .chain(EventTransform::enrich_with_stream_id("s1"));
    let result = t.apply(delta("auth token-abc granted"));
    assert_eq!(extract_delta_text(&result), Some("auth [REDACTED] granted"));
    assert_eq!(
        result.ext.unwrap().get("stream_id").unwrap(),
        &serde_json::json!("s1")
    );
}

#[test]
fn transform_multiple_redactions_chained() {
    let t = EventTransform::redact_text("password", "***")
        .chain(EventTransform::redact_text("secret", "###"));
    let result = t.apply(delta("password and secret"));
    assert_eq!(extract_delta_text(&result), Some("*** and ###"));
}

// =========================================================================
// 5. StreamReplay — StreamRecorder record and replay
// =========================================================================

#[tokio::test]
async fn recorder_record_and_replay_instant() {
    let mut rec = StreamRecorder::new();
    rec.record(delta("a"));
    rec.record(delta("b"));
    rec.record(delta("c"));
    assert_eq!(rec.len(), 3);

    let (tx, mut rx) = mpsc::channel(16);
    let count = rec.replay(0.0, &tx).await.unwrap(); // instant replay
    assert_eq!(count, 3);
    drop(tx);

    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }
    assert_eq!(events.len(), 3);
    assert_eq!(extract_delta_text(&events[0]), Some("a"));
    assert_eq!(extract_delta_text(&events[2]), Some("c"));
}

#[tokio::test]
async fn recorder_replay_filtered() {
    let mut rec = StreamRecorder::new();
    rec.record(delta("keep"));
    rec.record(error_ev("skip"));
    rec.record(delta("keep2"));

    let (tx, mut rx) = mpsc::channel(16);
    let filter = |ev: &AgentEvent| matches!(ev.kind, AgentEventKind::AssistantDelta { .. });
    let count = rec.replay_filtered(&filter, &tx).await.unwrap();
    assert_eq!(count, 2);
    drop(tx);

    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn recorder_empty_replay() {
    let rec = StreamRecorder::new();
    assert!(rec.is_empty());

    let (tx, _rx) = mpsc::channel(16);
    let count = rec.replay(0.0, &tx).await.unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn recorder_clear_resets() {
    let mut rec = StreamRecorder::new();
    rec.record(delta("a"));
    rec.record(delta("b"));
    assert_eq!(rec.len(), 2);
    rec.clear();
    assert!(rec.is_empty());
    assert_eq!(rec.events().len(), 0);
}

#[tokio::test]
async fn recorder_timing_offsets_increase() {
    let mut rec = StreamRecorder::new();
    rec.record(delta("first"));
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    rec.record(delta("second"));

    let events = rec.events();
    assert!(events[1].offset >= events[0].offset);
    // Second event should have a measurable offset from first
    assert!(events[1].offset >= std::time::Duration::from_millis(5));
}

// =========================================================================
// 6. StreamMetrics — per-stream tracking and backpressure stats
// =========================================================================

#[test]
fn per_stream_metrics_records_independently() {
    let mut psm = PerStreamMetrics::new();
    psm.record("stream-a", &delta("hello"));
    psm.record("stream-a", &delta("world"));
    psm.record("stream-b", &error_ev("oops"));

    assert_eq!(psm.stream_count(), 2);
    assert_eq!(psm.total_events(), 3);
    assert_eq!(psm.get("stream-a").unwrap().event_count(), 2);
    assert_eq!(psm.get("stream-b").unwrap().event_count(), 1);
    assert!(psm.get("stream-c").is_none());
}

#[test]
fn per_stream_metrics_stream_names() {
    let mut psm = PerStreamMetrics::new();
    psm.record("beta", &delta("b"));
    psm.record("alpha", &delta("a"));
    let names = psm.stream_names();
    // BTreeMap → sorted
    assert_eq!(names, vec!["alpha", "beta"]);
}

#[test]
fn per_stream_metrics_backpressure_counters() {
    let mut psm = PerStreamMetrics::new();
    assert_eq!(psm.backpressure_drops(), 0);
    assert_eq!(psm.backpressure_blocks(), 0);

    psm.record_drop();
    psm.record_drop();
    psm.record_block();
    assert_eq!(psm.backpressure_drops(), 2);
    assert_eq!(psm.backpressure_blocks(), 1);
}

#[test]
fn per_stream_metrics_bytes_tracked_per_stream() {
    let mut psm = PerStreamMetrics::new();
    psm.record("s1", &delta("hello")); // 5 bytes
    psm.record("s1", &delta("world!")); // 6 bytes
    psm.record("s2", &delta("ab")); // 2 bytes

    assert_eq!(psm.get("s1").unwrap().total_bytes(), 11);
    assert_eq!(psm.get("s2").unwrap().total_bytes(), 2);
}

#[test]
fn per_stream_metrics_event_type_counts() {
    let mut psm = PerStreamMetrics::new();
    psm.record("main", &delta("a"));
    psm.record("main", &error_ev("e"));
    psm.record("main", &tool_call_ev("read"));

    let m = psm.get("main").unwrap();
    let counts = m.event_type_counts();
    assert_eq!(counts.get("assistant_delta"), Some(&1));
    assert_eq!(counts.get("error"), Some(&1));
    assert_eq!(counts.get("tool_call"), Some(&1));
}

// =========================================================================
// StreamMetrics (basic) — latency tracking
// =========================================================================

#[test]
fn stream_metrics_latency_with_single_event() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("only one"));
    assert_eq!(m.latencies().len(), 0);
    assert_eq!(m.average_latency(), std::time::Duration::ZERO);
    assert_eq!(m.min_latency(), std::time::Duration::ZERO);
    assert_eq!(m.max_latency(), std::time::Duration::ZERO);
}

#[test]
fn stream_metrics_summary_display() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("hello"));
    m.record_event(&error_ev("e"));
    let summary = m.summary();
    assert_eq!(summary.event_count, 2);
    assert_eq!(summary.total_bytes, 5);
    let display = format!("{summary}");
    assert!(display.contains("Events: 2"));
    assert!(display.contains("Bytes: 5"));
}

// =========================================================================
// Pipeline integration with new features
// =========================================================================

#[test]
fn pipeline_redact_and_filter() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .transform(EventTransform::redact_text("secret", "[REDACTED]"))
        .build();

    let r1 = p.process(delta("my secret data")).unwrap();
    assert_eq!(extract_delta_text(&r1), Some("my [REDACTED] data"));
    assert!(p.process(error_ev("secret")).is_none()); // filtered
}

#[test]
fn pipeline_enrich_and_record() {
    let recorder = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::enrich_with_stream_id("test-stream"))
        .with_recorder(recorder.clone())
        .build();

    p.process(delta("a"));
    p.process(delta("b"));
    assert_eq!(recorder.len(), 2);
    for ev in recorder.events() {
        let ext = ev.ext.as_ref().unwrap();
        assert_eq!(
            ext.get("stream_id").unwrap(),
            &serde_json::json!("test-stream")
        );
    }
}

#[test]
fn pipeline_severity_filter_with_stats() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::severity_errors_and_warnings())
        .with_stats(stats.clone())
        .build();

    p.process(delta("ignored"));
    p.process(error_ev("captured"));
    p.process(warning_ev("also captured"));
    p.process(tool_call_ev("ignored"));

    assert_eq!(stats.total_events(), 2);
    assert_eq!(stats.error_count(), 1);
}

// =========================================================================
// EventMultiplexer (ordered) + EventStream integration
// =========================================================================

#[tokio::test]
async fn multiplexer_sorted_with_mixed_event_types() {
    let ts_base = Utc::now();
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    tx1.send(AgentEvent {
        ts: ts_base + chrono::Duration::milliseconds(20),
        kind: AgentEventKind::Error {
            message: "late error".to_string(),
            error_code: None,
        },
        ext: None,
    })
    .await
    .unwrap();
    tx2.send(AgentEvent {
        ts: ts_base,
        kind: AgentEventKind::AssistantDelta {
            text: "early delta".to_string(),
        },
        ext: None,
    })
    .await
    .unwrap();
    drop(tx1);
    drop(tx2);

    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), 2);
    // Early delta should come first
    assert!(matches!(
        events[0].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
    assert!(matches!(events[1].kind, AgentEventKind::Error { .. }));
}

// =========================================================================
// ReplayBuffer + StreamRecorder round-trip
// =========================================================================

#[tokio::test]
async fn replay_buffer_send_returns_subscriber_count() {
    let mut rb = ReplayBuffer::new(10, 16);
    let n = rb.send(&delta("no-subs"));
    assert_eq!(n, 0); // no subscribers

    let _sub = rb.subscribe();
    let n = rb.send(&delta("one-sub"));
    assert_eq!(n, 1);
}

#[tokio::test]
async fn recorder_replay_with_speed_does_not_lose_events() {
    let mut rec = StreamRecorder::new();
    for i in 0..5 {
        rec.record(delta(&format!("ev-{i}")));
    }

    let (tx, mut rx) = mpsc::channel(16);
    // 100x speed — effectively instant for test purposes
    let count = rec.replay(100.0, &tx).await.unwrap();
    assert_eq!(count, 5);
    drop(tx);

    let mut received = Vec::new();
    while let Some(ev) = rx.recv().await {
        received.push(ev);
    }
    assert_eq!(received.len(), 5);
}

// =========================================================================
// Full lifecycle: record → filter → replay → metrics
// =========================================================================

#[tokio::test]
async fn full_lifecycle_record_filter_replay_metrics() {
    // 1. Record a stream of mixed events
    let mut rec = StreamRecorder::new();
    rec.record(run_started());
    rec.record(delta("hello "));
    rec.record(delta("world"));
    rec.record(error_ev("transient"));
    rec.record(tool_call_ev("read_file"));
    rec.record(file_changed("src/main.rs"));
    rec.record(run_completed());
    assert_eq!(rec.len(), 7);

    // 2. Replay only deltas
    let (tx, mut rx) = mpsc::channel(16);
    let count = rec
        .replay_filtered(
            &|ev: &AgentEvent| matches!(ev.kind, AgentEventKind::AssistantDelta { .. }),
            &tx,
        )
        .await
        .unwrap();
    assert_eq!(count, 2);
    drop(tx);

    // 3. Feed replayed events through metrics
    let mut metrics = StreamMetrics::new();
    while let Some(ev) = rx.recv().await {
        metrics.record_event(&ev);
    }
    assert_eq!(metrics.event_count(), 2);
    assert_eq!(metrics.total_bytes(), 11); // "hello " + "world"
}

// =========================================================================
// Additional edge-case tests
// =========================================================================

#[test]
fn ring_buffer_iter_count_matches_len() {
    let mut ring = RingBuffer::new(5);
    for i in 0..8 {
        ring.push(delta(&format!("{i}")));
    }
    assert_eq!(ring.len(), 5);
    assert_eq!(ring.iter().count(), 5);
}

#[test]
fn event_buffer_max_size_getter() {
    let buf = EventBuffer::new(42);
    assert_eq!(buf.max_size(), 42);
}

#[test]
fn stream_buffer_recent_fewer_than_n() {
    let mut buf = StreamBuffer::new(10);
    buf.push(delta("a"));
    buf.push(delta("b"));
    let recent = buf.recent(100); // ask for more than available
    assert_eq!(recent.len(), 2);
}

#[test]
fn flushable_buffer_at_threshold_one() {
    let mut buf = FlushableBuffer::new(5, FlushStrategy::AtThreshold(1));
    // Every push should trigger flush
    assert!(buf.push(delta("a")));
    let flushed = buf.flush();
    assert_eq!(flushed.len(), 1);
    assert!(buf.push(delta("b")));
}

#[test]
fn per_stream_metrics_empty_total_zero() {
    let psm = PerStreamMetrics::new();
    assert_eq!(psm.total_events(), 0);
    assert_eq!(psm.stream_count(), 0);
    assert!(psm.stream_names().is_empty());
}

#[tokio::test]
async fn recorder_replay_all_event_types() {
    let mut rec = StreamRecorder::new();
    rec.record(run_started());
    rec.record(delta("text"));
    rec.record(error_ev("err"));
    rec.record(warning_ev("warn"));
    rec.record(tool_call_ev("bash"));
    rec.record(file_changed("f.rs"));
    rec.record(run_completed());

    let (tx, mut rx) = mpsc::channel(16);
    let count = rec.replay(0.0, &tx).await.unwrap();
    assert_eq!(count, 7);
    drop(tx);

    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }
    assert_eq!(events.len(), 7);
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        events[6].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}
