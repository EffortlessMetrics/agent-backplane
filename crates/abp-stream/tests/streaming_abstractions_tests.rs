#![allow(clippy::all)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the new streaming abstractions: EventStream (as Stream),
//! EventCollector, MergedStream, TimeoutStream, BufferedStream, and StreamMetrics.

use std::pin::Pin;
use std::time::Duration;

use abp_core::{AgentEvent, AgentEventKind};
use abp_stream::{
    BufferedStream, EventCollector, EventFilter, EventStream, EventTransform, MergedStream,
    StreamMetrics, StreamTimeout, TimeoutStream,
};
use chrono::Utc;
use futures_core::Stream;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_event(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn delta(text: &str) -> AgentEvent {
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

fn run_started() -> AgentEvent {
    make_event(AgentEventKind::RunStarted {
        message: "started".to_string(),
    })
}

fn run_completed() -> AgentEvent {
    make_event(AgentEventKind::RunCompleted {
        message: "done".to_string(),
    })
}

fn tool_call(name: &str) -> AgentEvent {
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

/// Helper to feed events and close a sender.
async fn send_events(tx: mpsc::Sender<AgentEvent>, events: Vec<AgentEvent>) {
    for ev in events {
        tx.send(ev).await.unwrap();
    }
    // tx dropped — closes channel
}

// ===========================================================================
// EventStream as futures_core::Stream
// ===========================================================================

#[tokio::test]
async fn event_stream_implements_stream_trait() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    // Use StreamExt (from tokio-stream) which requires futures_core::Stream
    let events: Vec<_> = stream.collect().await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn event_stream_yields_none_on_close() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(tx);

    let mut stream = EventStream::new(rx);
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn event_stream_single_event() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("only")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let events: Vec<_> = stream.collect().await;
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn event_stream_rapid_events() {
    let (tx, rx) = mpsc::channel(256);
    for i in 0..100 {
        tx.send(delta(&format!("ev{i}"))).await.unwrap();
    }
    drop(tx);

    let stream = EventStream::new(rx);
    let events: Vec<_> = stream.collect().await;
    assert_eq!(events.len(), 100);
}

#[tokio::test]
async fn event_stream_empty_stream() {
    let (_tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(_tx);
    let stream = EventStream::new(rx);
    let events: Vec<_> = stream.collect().await;
    assert!(events.is_empty());
}

// ===========================================================================
// EventCollector
// ===========================================================================

#[tokio::test]
async fn collector_captures_all_events() {
    let (tx, rx) = mpsc::channel(16);
    let events = vec![delta("a"), delta("b"), error_event("e")];
    tokio::spawn(send_events(tx, events));

    let stream = EventStream::new(rx);
    let collector = EventCollector::new(stream);
    let handle = collector.collected();

    // Consume the collector as a stream.
    let forwarded: Vec<_> = collector.collect().await;
    assert_eq!(forwarded.len(), 3);

    // The internal buffer should also have 3 events.
    let collected = handle.lock().unwrap();
    assert_eq!(collected.len(), 3);
}

#[tokio::test]
async fn collector_snapshot_mid_stream() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("first")).await.unwrap();
    tx.send(delta("second")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let mut collector = EventCollector::new(stream);

    // Consume one event.
    let _ = collector.next().await;
    assert_eq!(collector.len(), 1);
    assert!(!collector.is_empty());

    // Consume the rest.
    let _ = collector.next().await;
    assert_eq!(collector.snapshot().len(), 2);
}

#[tokio::test]
async fn collector_empty_stream() {
    let (_tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(_tx);

    let stream = EventStream::new(rx);
    let collector = EventCollector::new(stream);
    assert!(collector.is_empty());

    let forwarded: Vec<_> = collector.collect().await;
    assert!(forwarded.is_empty());
}

#[tokio::test]
async fn collector_preserves_event_order() {
    let (tx, rx) = mpsc::channel(16);
    for i in 0..5 {
        tx.send(delta(&format!("{i}"))).await.unwrap();
    }
    drop(tx);

    let stream = EventStream::new(rx);
    let collector = EventCollector::new(stream);
    let forwarded: Vec<_> = collector.collect().await;

    for (i, ev) in forwarded.iter().enumerate() {
        assert!(matches!(
            &ev.kind,
            AgentEventKind::AssistantDelta { text } if text == &format!("{i}")
        ));
    }
}

#[tokio::test]
async fn collector_single_event() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(run_started()).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let collector = EventCollector::new(stream);
    let forwarded: Vec<_> = collector.collect().await;
    assert_eq!(forwarded.len(), 1);
}

// ===========================================================================
// EventFilter (additional tests for stream-based usage)
// ===========================================================================

#[tokio::test]
async fn filter_only_tool_calls_via_stream() {
    let (tx, rx) = mpsc::channel(16);
    let events = vec![
        run_started(),
        delta("text"),
        tool_call("read_file"),
        tool_call("write_file"),
        delta("more"),
        run_completed(),
    ];
    tokio::spawn(send_events(tx, events));

    let stream = EventStream::new(rx);
    let filter = EventFilter::by_kind("tool_call");
    let filtered: Vec<_> = stream.filter(|ev| filter.matches(ev)).collect().await;
    assert_eq!(filtered.len(), 2);
}

#[tokio::test]
async fn filter_only_text_output() {
    let (tx, rx) = mpsc::channel(16);
    let events = vec![
        delta("hello"),
        tool_call("read"),
        delta("world"),
        error_event("err"),
    ];
    tokio::spawn(send_events(tx, events));

    let stream = EventStream::new(rx);
    let filter = EventFilter::by_kind("assistant_delta");
    let filtered: Vec<_> = stream.filter(|ev| filter.matches(ev)).collect().await;
    assert_eq!(filtered.len(), 2);
}

#[tokio::test]
async fn filter_selects_correctly_mixed_kinds() {
    let (tx, rx) = mpsc::channel(16);
    let events = vec![
        run_started(),
        delta("a"),
        warning_event("w"),
        error_event("e"),
        run_completed(),
    ];
    tokio::spawn(send_events(tx, events));

    let stream = EventStream::new(rx);
    let filter = EventFilter::errors_only();
    let errors: Vec<_> = stream.filter(|ev| filter.matches(ev)).collect().await;
    assert_eq!(errors.len(), 1);
}

// ===========================================================================
// EventTransform (stream-based usage)
// ===========================================================================

#[tokio::test]
async fn transform_applies_mapping_via_stream() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("hello")).await.unwrap();
    tx.send(delta("world")).await.unwrap();
    drop(tx);

    let transform = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(std::collections::BTreeMap::new);
        ext.insert("tagged".to_string(), serde_json::json!(true));
        ev
    });

    let stream = EventStream::new(rx);
    let mapped: Vec<_> = stream.map(|ev| transform.apply(ev)).collect().await;
    assert_eq!(mapped.len(), 2);
    for ev in &mapped {
        assert_eq!(
            ev.ext.as_ref().unwrap().get("tagged").unwrap(),
            &serde_json::json!(true)
        );
    }
}

#[tokio::test]
async fn transform_identity_preserves_all() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("x")).await.unwrap();
    drop(tx);

    let t = EventTransform::identity();
    let stream = EventStream::new(rx);
    let mapped: Vec<_> = stream.map(|ev| t.apply(ev)).collect().await;
    assert_eq!(mapped.len(), 1);
    assert!(matches!(
        &mapped[0].kind,
        AgentEventKind::AssistantDelta { text } if text == "x"
    ));
}

// ===========================================================================
// MergedStream
// ===========================================================================

#[tokio::test]
async fn merged_stream_interleaves_from_two_sources() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    tx1.send(delta("a1")).await.unwrap();
    tx1.send(delta("a2")).await.unwrap();
    drop(tx1);

    tx2.send(delta("b1")).await.unwrap();
    tx2.send(delta("b2")).await.unwrap();
    drop(tx2);

    let s1: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = Box::pin(EventStream::new(rx1));
    let s2: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = Box::pin(EventStream::new(rx2));

    let merged = MergedStream::new(vec![s1, s2]);
    let events: Vec<_> = merged.collect().await;

    // All 4 events should appear.
    assert_eq!(events.len(), 4);
}

#[tokio::test]
async fn merged_stream_empty_sources() {
    let merged = MergedStream::new(vec![]);
    let events: Vec<_> = merged.collect().await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn merged_stream_single_source() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("only")).await.unwrap();
    drop(tx);

    let s: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = Box::pin(EventStream::new(rx));
    let merged = MergedStream::new(vec![s]);
    let events: Vec<_> = merged.collect().await;
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn merged_stream_one_empty_one_populated() {
    let (_tx1, rx1) = mpsc::channel::<AgentEvent>(16);
    drop(_tx1);

    let (tx2, rx2) = mpsc::channel(16);
    tx2.send(delta("b1")).await.unwrap();
    tx2.send(delta("b2")).await.unwrap();
    drop(tx2);

    let s1: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = Box::pin(EventStream::new(rx1));
    let s2: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = Box::pin(EventStream::new(rx2));

    let merged = MergedStream::new(vec![s1, s2]);
    let events: Vec<_> = merged.collect().await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn merged_stream_three_sources() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    let (tx3, rx3) = mpsc::channel(16);

    tx1.send(delta("s1")).await.unwrap();
    drop(tx1);
    tx2.send(delta("s2")).await.unwrap();
    drop(tx2);
    tx3.send(delta("s3")).await.unwrap();
    drop(tx3);

    let s1: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = Box::pin(EventStream::new(rx1));
    let s2: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = Box::pin(EventStream::new(rx2));
    let s3: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = Box::pin(EventStream::new(rx3));

    let merged = MergedStream::new(vec![s1, s2, s3]);
    assert_eq!(merged.remaining(), 3);
    let events: Vec<_> = merged.collect().await;
    assert_eq!(events.len(), 3);
}

#[tokio::test]
async fn merged_stream_preserves_all_events() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    for i in 0..10 {
        tx1.send(delta(&format!("a{i}"))).await.unwrap();
    }
    drop(tx1);

    for i in 0..10 {
        tx2.send(delta(&format!("b{i}"))).await.unwrap();
    }
    drop(tx2);

    let s1: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = Box::pin(EventStream::new(rx1));
    let s2: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = Box::pin(EventStream::new(rx2));

    let merged = MergedStream::new(vec![s1, s2]);
    let events: Vec<_> = merged.collect().await;
    assert_eq!(events.len(), 20);
}

// ===========================================================================
// TimeoutStream
// ===========================================================================

#[tokio::test]
async fn timeout_stream_passes_through_fast_events() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("fast")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let ts = TimeoutStream::new(stream, Duration::from_secs(10));
    let mut ts = Box::pin(ts);
    let mut results = Vec::new();
    while let Some(item) = ts.next().await {
        results.push(item);
    }

    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[tokio::test]
async fn timeout_stream_fires_on_slow_events() {
    let (_tx, rx) = mpsc::channel::<AgentEvent>(16);
    // Don't drop tx — keep channel open so the stream blocks.

    let stream = EventStream::new(rx);
    let ts = TimeoutStream::new(stream, Duration::from_millis(50));
    let mut ts = Box::pin(ts);

    // First poll should timeout after ~50ms.
    let item = ts.next().await;
    assert!(item.is_some());
    let result = item.unwrap();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.duration, Duration::from_millis(50));
}

#[tokio::test]
async fn timeout_stream_none_when_inner_closes() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(tx);

    let stream = EventStream::new(rx);
    let ts = TimeoutStream::new(stream, Duration::from_secs(10));
    let mut ts = Box::pin(ts);
    assert!(ts.next().await.is_none());
}

#[tokio::test]
async fn timeout_stream_mixed_fast_and_slow() {
    let (tx, rx) = mpsc::channel(16);
    let tx_clone = tx.clone();

    tokio::spawn(async move {
        tx_clone.send(delta("fast1")).await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        tx_clone.send(delta("fast2")).await.unwrap();
        drop(tx_clone);
    });
    drop(tx);

    let stream = EventStream::new(rx);
    let ts = TimeoutStream::new(stream, Duration::from_secs(5));
    let mut ts = Box::pin(ts);
    let mut results = Vec::new();
    while let Some(item) = ts.next().await {
        results.push(item);
    }

    // Both should succeed (timeout is generous).
    let ok_count = results.iter().filter(|r| r.is_ok()).count();
    assert_eq!(ok_count, 2);
}

#[tokio::test]
async fn timeout_error_display() {
    let err = StreamTimeout {
        duration: Duration::from_millis(100),
    };
    let msg = format!("{err}");
    assert!(msg.contains("timeout"));
    assert!(msg.contains("100ms"));
}

// ===========================================================================
// BufferedStream
// ===========================================================================

#[tokio::test]
async fn buffered_stream_emits_full_batches() {
    let (tx, rx) = mpsc::channel(16);
    for i in 0..6 {
        tx.send(delta(&format!("{i}"))).await.unwrap();
    }
    drop(tx);

    let stream = EventStream::new(rx);
    let buffered = BufferedStream::new(stream, 3);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;

    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].len(), 3);
    assert_eq!(batches[1].len(), 3);
}

#[tokio::test]
async fn buffered_stream_flushes_remainder() {
    let (tx, rx) = mpsc::channel(16);
    for i in 0..5 {
        tx.send(delta(&format!("{i}"))).await.unwrap();
    }
    drop(tx);

    let stream = EventStream::new(rx);
    let buffered = BufferedStream::new(stream, 3);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;

    // 3 + 2 = 5 events, so 2 batches.
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].len(), 3);
    assert_eq!(batches[1].len(), 2);
}

#[tokio::test]
async fn buffered_stream_single_event() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("only")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let buffered = BufferedStream::new(stream, 10);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;

    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 1);
}

#[tokio::test]
async fn buffered_stream_empty() {
    let (_tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(_tx);

    let stream = EventStream::new(rx);
    let buffered = BufferedStream::new(stream, 5);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;
    assert!(batches.is_empty());
}

#[tokio::test]
async fn buffered_stream_batch_size_one() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    tx.send(delta("c")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let buffered = BufferedStream::new(stream, 1);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;

    assert_eq!(batches.len(), 3);
    for batch in &batches {
        assert_eq!(batch.len(), 1);
    }
}

#[tokio::test]
async fn buffered_stream_exact_batch_size() {
    let (tx, rx) = mpsc::channel(16);
    for _ in 0..4 {
        tx.send(delta("x")).await.unwrap();
    }
    drop(tx);

    let stream = EventStream::new(rx);
    let buffered = BufferedStream::new(stream, 4);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;

    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 4);
}

#[tokio::test]
async fn buffered_stream_large_batch() {
    let (tx, rx) = mpsc::channel(256);
    for i in 0..100 {
        tx.send(delta(&format!("{i}"))).await.unwrap();
    }
    drop(tx);

    let stream = EventStream::new(rx);
    let buffered = BufferedStream::new(stream, 25);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;

    assert_eq!(batches.len(), 4);
    for batch in &batches {
        assert_eq!(batch.len(), 25);
    }
}

// ===========================================================================
// StreamMetrics (additional accuracy tests)
// ===========================================================================

#[test]
fn stream_metrics_accurate_counts() {
    let mut m = StreamMetrics::new();
    m.record_event(&run_started());
    m.record_event(&delta("hello"));
    m.record_event(&delta("world"));
    m.record_event(&tool_call("read"));
    m.record_event(&error_event("fail"));
    m.record_event(&run_completed());

    assert_eq!(m.event_count(), 6);
    assert_eq!(m.total_bytes(), 10); // "hello" + "world"

    let counts = m.event_type_counts();
    assert_eq!(counts.get("run_started"), Some(&1));
    assert_eq!(counts.get("assistant_delta"), Some(&2));
    assert_eq!(counts.get("tool_call"), Some(&1));
    assert_eq!(counts.get("error"), Some(&1));
    assert_eq!(counts.get("run_completed"), Some(&1));
}

#[test]
fn stream_metrics_empty_has_zero_elapsed() {
    let m = StreamMetrics::new();
    assert_eq!(m.elapsed(), Duration::ZERO);
    assert_eq!(m.throughput(), 0.0);
}

#[test]
fn stream_metrics_single_event_zero_throughput() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("x"));
    // Single event: no time gap, throughput = 0.
    assert_eq!(m.throughput(), 0.0);
    assert_eq!(m.event_count(), 1);
}

// ===========================================================================
// Edge cases: drop/cancellation safety
// ===========================================================================

#[tokio::test]
async fn drop_event_stream_does_not_panic() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    let stream = EventStream::new(rx);
    drop(stream);
    // Sender should now fail since receiver was dropped.
    assert!(tx.send(delta("b")).await.is_err());
}

#[tokio::test]
async fn drop_collector_mid_stream() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();

    let stream = EventStream::new(rx);
    let mut collector = EventCollector::new(stream);

    // Consume one event.
    let _ = collector.next().await;
    assert_eq!(collector.len(), 1);

    // Drop without consuming the rest.
    drop(collector);
    // Should not panic; sender detects closed receiver.
    assert!(tx.send(delta("c")).await.is_err());
}

#[tokio::test]
async fn drop_merged_stream_partial_consumption() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    tx1.send(delta("a")).await.unwrap();
    tx2.send(delta("b")).await.unwrap();
    drop(tx1);
    drop(tx2);

    let s1: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = Box::pin(EventStream::new(rx1));
    let s2: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = Box::pin(EventStream::new(rx2));

    let mut merged = MergedStream::new(vec![s1, s2]);
    let _ = merged.next().await; // Consume one.
    drop(merged); // Drop with one remaining — no panic.
}

#[tokio::test]
async fn cancellation_safety_timeout_stream() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("safe")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let ts = TimeoutStream::new(stream, Duration::from_secs(10));
    let mut ts = Box::pin(ts);

    let first = ts.next().await;
    assert!(first.is_some());
    assert!(first.unwrap().is_ok());

    // Stream ended; next should be None.
    assert!(ts.next().await.is_none());
}

// ===========================================================================
// Integration: combining multiple abstractions
// ===========================================================================

#[tokio::test]
async fn collector_with_filter_and_transform() {
    let (tx, rx) = mpsc::channel(16);
    let events = vec![
        delta("keep"),
        error_event("drop"),
        delta("also_keep"),
        tool_call("drop_too"),
    ];
    tokio::spawn(send_events(tx, events));

    let stream = EventStream::new(rx);
    let filter = EventFilter::by_kind("assistant_delta");
    let transform = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(std::collections::BTreeMap::new);
        ext.insert("processed".to_string(), serde_json::json!(true));
        ev
    });

    // Filter → transform → collect
    let filtered = stream.filter(|ev| filter.matches(ev));
    let transformed = filtered.map(|ev| transform.apply(ev));
    let collector = EventCollector::new(transformed);

    let results: Vec<_> = collector.collect().await;
    assert_eq!(results.len(), 2);
    for ev in &results {
        assert!(ev.ext.as_ref().unwrap().contains_key("processed"));
    }
}

#[tokio::test]
async fn merged_into_buffered() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    for i in 0..3 {
        tx1.send(delta(&format!("a{i}"))).await.unwrap();
    }
    drop(tx1);

    for i in 0..3 {
        tx2.send(delta(&format!("b{i}"))).await.unwrap();
    }
    drop(tx2);

    let s1: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = Box::pin(EventStream::new(rx1));
    let s2: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = Box::pin(EventStream::new(rx2));

    let merged = MergedStream::new(vec![s1, s2]);
    let buffered = BufferedStream::new(merged, 2);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;

    let total: usize = batches.iter().map(|b| b.len()).sum();
    assert_eq!(total, 6);
}

#[tokio::test]
async fn collector_with_metrics() {
    let (tx, rx) = mpsc::channel(16);
    let events = vec![delta("a"), delta("bb"), error_event("e"), run_completed()];
    tokio::spawn(send_events(tx, events));

    let stream = EventStream::new(rx);
    let collector = EventCollector::new(stream);

    let mut metrics = StreamMetrics::new();
    let forwarded: Vec<_> = collector.collect().await;
    for ev in &forwarded {
        metrics.record_event(ev);
    }

    assert_eq!(metrics.event_count(), 4);
    assert_eq!(metrics.total_bytes(), 3); // "a" + "bb"
}
