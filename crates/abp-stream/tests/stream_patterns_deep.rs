// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive stream combinator pattern tests covering construction,
//! transformation, filtering, take/skip, merge, timeout, buffer, collect,
//! count, first/last, fold/reduce, text aggregation, tool call extraction,
//! error propagation, and empty stream edge cases.

use std::collections::BTreeMap;
use std::pin::Pin;
use std::time::Duration;

use abp_core::{AgentEvent, AgentEventKind};
use abp_stream::{
    BufferedStream, EventCollector, EventFilter, EventRecorder, EventStats, EventStream,
    EventTransform, MergedStream, StreamAggregator, StreamBuffer, StreamMetrics,
    StreamPipelineBuilder, TimeoutStream,
};
use chrono::Utc;
use futures_core::Stream;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

// ===========================================================================
// Helpers
// ===========================================================================

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

fn delta(text: &str) -> AgentEvent {
    make_event(AgentEventKind::AssistantDelta {
        text: text.to_string(),
    })
}

fn message(text: &str) -> AgentEvent {
    make_event(AgentEventKind::AssistantMessage {
        text: text.to_string(),
    })
}

fn error_ev(msg: &str) -> AgentEvent {
    make_event(AgentEventKind::Error {
        message: msg.to_string(),
        error_code: None,
    })
}

fn warning_ev(msg: &str) -> AgentEvent {
    make_event(AgentEventKind::Warning {
        message: msg.to_string(),
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

fn tool_call(name: &str, id: &str) -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: name.to_string(),
        tool_use_id: Some(id.to_string()),
        parent_tool_use_id: None,
        input: serde_json::json!({"arg": "val"}),
    })
}

fn tool_result(name: &str, id: &str, output: &str) -> AgentEvent {
    make_event(AgentEventKind::ToolResult {
        tool_name: name.to_string(),
        tool_use_id: Some(id.to_string()),
        output: serde_json::json!(output),
        is_error: false,
    })
}

fn file_changed(path: &str) -> AgentEvent {
    make_event(AgentEventKind::FileChanged {
        path: path.to_string(),
        summary: "modified".to_string(),
    })
}

fn command_executed(cmd: &str, code: i32) -> AgentEvent {
    make_event(AgentEventKind::CommandExecuted {
        command: cmd.to_string(),
        exit_code: Some(code),
        output_preview: Some("output".to_string()),
    })
}

fn vec_stream(events: Vec<AgentEvent>) -> tokio_stream::Iter<std::vec::IntoIter<AgentEvent>> {
    tokio_stream::iter(events)
}

fn channel_stream(events: Vec<AgentEvent>) -> EventStream {
    let (tx, rx) = mpsc::channel(events.len().max(1));
    tokio::spawn(async move {
        for ev in events {
            let _ = tx.send(ev).await;
        }
    });
    EventStream::new(rx)
}

fn boxed_stream(events: Vec<AgentEvent>) -> Pin<Box<dyn Stream<Item = AgentEvent> + Send>> {
    Box::pin(vec_stream(events))
}

// ===========================================================================
// 1. Event Stream Construction
// ===========================================================================

#[tokio::test]
async fn construct_stream_from_vec() {
    let events = vec![delta("a"), delta("b"), delta("c")];
    let stream = vec_stream(events);
    let collected: Vec<_> = stream.collect().await;
    assert_eq!(collected.len(), 3);
}

#[tokio::test]
async fn construct_stream_from_channel() {
    let events = vec![delta("x"), delta("y")];
    let stream = channel_stream(events);
    let collected = stream.collect_all().await;
    assert_eq!(collected.len(), 2);
}

#[tokio::test]
async fn construct_stream_from_channel_recv() {
    let (tx, rx) = mpsc::channel(4);
    let mut stream = EventStream::new(rx);
    tx.send(delta("one")).await.unwrap();
    tx.send(delta("two")).await.unwrap();
    drop(tx);
    assert!(stream.recv().await.is_some());
    assert!(stream.recv().await.is_some());
    assert!(stream.recv().await.is_none());
}

#[tokio::test]
async fn construct_event_stream_as_futures_stream() {
    let (tx, rx) = mpsc::channel(4);
    tx.send(delta("hi")).await.unwrap();
    drop(tx);
    let stream = EventStream::new(rx);
    let mut pinned = Box::pin(stream);
    let item = pinned.next().await;
    assert!(item.is_some());
    assert!(pinned.next().await.is_none());
}

#[tokio::test]
async fn construct_stream_from_iterator() {
    let items = (0..5).map(|i| delta(&format!("item_{i}")));
    let stream = tokio_stream::iter(items);
    let collected: Vec<_> = stream.collect().await;
    assert_eq!(collected.len(), 5);
}

// ===========================================================================
// 2. Map (EventTransform)
// ===========================================================================

#[test]
fn map_transform_adds_metadata() {
    let transform = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("mapped".into(), serde_json::json!(true));
        ev
    });
    let result = transform.apply(delta("hello"));
    assert_eq!(
        result.ext.unwrap().get("mapped").unwrap(),
        &serde_json::json!(true)
    );
}

#[test]
fn map_identity_preserves_kind() {
    let transform = EventTransform::identity();
    let ev = error_ev("oops");
    let result = transform.apply(ev.clone());
    assert!(matches!(result.kind, AgentEventKind::Error { .. }));
    assert_eq!(result.ts, ev.ts);
}

#[test]
fn map_chain_transforms_compose() {
    let t1 = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("step".into(), serde_json::json!(1));
        ev
    });
    let t2 = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("step".into(), serde_json::json!(2));
        ev
    });
    let result = t2.apply(t1.apply(delta("x")));
    assert_eq!(
        result.ext.unwrap().get("step").unwrap(),
        &serde_json::json!(2)
    );
}

#[tokio::test]
async fn map_transform_in_pipeline_modifies_all() {
    let pipeline = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("tagged".into(), serde_json::json!("yes"));
            ev
        }))
        .build();

    let events = vec![delta("a"), error_ev("b"), run_started()];
    for ev in events {
        let result = pipeline.process(ev).unwrap();
        assert_eq!(
            result.ext.unwrap().get("tagged").unwrap(),
            &serde_json::json!("yes")
        );
    }
}

#[test]
fn map_transform_preserves_ext_fields() {
    let mut ev = delta("test");
    let mut ext = BTreeMap::new();
    ext.insert("existing".into(), serde_json::json!(42));
    ev.ext = Some(ext);

    let transform = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("new_field".into(), serde_json::json!("added"));
        ev
    });
    let result = transform.apply(ev);
    let ext = result.ext.unwrap();
    assert_eq!(ext.get("existing").unwrap(), &serde_json::json!(42));
    assert_eq!(ext.get("new_field").unwrap(), &serde_json::json!("added"));
}

// ===========================================================================
// 3. Filter
// ===========================================================================

#[test]
fn filter_by_kind_assistant_delta() {
    let f = EventFilter::by_kind("assistant_delta");
    assert!(f.matches(&delta("hi")));
    assert!(!f.matches(&error_ev("bad")));
    assert!(!f.matches(&tool_call("read", "t1")));
}

#[test]
fn filter_by_kind_tool_call() {
    let f = EventFilter::by_kind("tool_call");
    assert!(f.matches(&tool_call("write", "tc1")));
    assert!(!f.matches(&delta("text")));
}

#[test]
fn filter_errors_only() {
    let f = EventFilter::errors_only();
    assert!(f.matches(&error_ev("oops")));
    assert!(!f.matches(&delta("ok")));
    assert!(!f.matches(&warning_ev("warn")));
    assert!(!f.matches(&run_started()));
}

#[test]
fn filter_exclude_errors() {
    let f = EventFilter::exclude_errors();
    assert!(!f.matches(&error_ev("bad")));
    assert!(f.matches(&delta("ok")));
    assert!(f.matches(&warning_ev("warn")));
    assert!(f.matches(&run_started()));
}

#[test]
fn filter_custom_predicate_text_length() {
    let f = EventFilter::new(
        |ev| matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text.len() > 5),
    );
    assert!(f.matches(&delta("longer text")));
    assert!(!f.matches(&delta("hi")));
}

#[tokio::test]
async fn filter_in_pipeline_removes_matching() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .build();

    assert!(pipeline.process(delta("ok")).is_some());
    assert!(pipeline.process(error_ev("bad")).is_none());
    assert!(pipeline.process(warning_ev("w")).is_some());
}

#[tokio::test]
async fn filter_collect_filtered_on_stream() {
    let (tx, rx) = mpsc::channel(16);
    for ev in [delta("a"), error_ev("e"), delta("b"), warning_ev("w")] {
        tx.send(ev).await.unwrap();
    }
    drop(tx);

    let stream = EventStream::new(rx);
    let filter = EventFilter::by_kind("assistant_delta");
    let result = stream.collect_filtered(&filter).await;
    assert_eq!(result.len(), 2);
}

#[test]
fn filter_multiple_filters_all_must_pass() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .filter(EventFilter::by_kind("assistant_delta"))
        .build();

    assert!(pipeline.process(delta("ok")).is_some());
    assert!(pipeline.process(error_ev("bad")).is_none());
    assert!(pipeline.process(tool_call("r", "t1")).is_none());
}

// ===========================================================================
// 4. Take / Skip
// ===========================================================================

#[tokio::test]
async fn take_first_n_events() {
    let events = vec![delta("a"), delta("b"), delta("c"), delta("d"), delta("e")];
    let stream = vec_stream(events);
    let taken: Vec<_> = stream.take(3).collect().await;
    assert_eq!(taken.len(), 3);
}

#[tokio::test]
async fn take_more_than_available() {
    let events = vec![delta("a"), delta("b")];
    let stream = vec_stream(events);
    let taken: Vec<_> = stream.take(10).collect().await;
    assert_eq!(taken.len(), 2);
}

#[tokio::test]
async fn take_zero_yields_nothing() {
    let events = vec![delta("a"), delta("b")];
    let stream = vec_stream(events);
    let taken: Vec<_> = stream.take(0).collect().await;
    assert!(taken.is_empty());
}

#[tokio::test]
async fn skip_first_n_events() {
    let events = vec![delta("a"), delta("b"), delta("c"), delta("d")];
    let stream = vec_stream(events);
    let remaining: Vec<_> = stream.skip(2).collect().await;
    assert_eq!(remaining.len(), 2);
    assert!(matches!(
        &remaining[0].kind,
        AgentEventKind::AssistantDelta { text } if text == "c"
    ));
}

#[tokio::test]
async fn skip_more_than_available() {
    let events = vec![delta("a")];
    let stream = vec_stream(events);
    let remaining: Vec<_> = stream.skip(10).collect().await;
    assert!(remaining.is_empty());
}

#[tokio::test]
async fn take_and_skip_combined() {
    let events: Vec<_> = (0..10).map(|i| delta(&format!("{i}"))).collect();
    let stream = vec_stream(events);
    // skip 3, take 4 → items 3,4,5,6
    let result: Vec<_> = stream.skip(3).take(4).collect().await;
    assert_eq!(result.len(), 4);
}

// ===========================================================================
// 5. Merge
// ===========================================================================

#[tokio::test]
async fn merge_two_streams_interleaves() {
    let s1 = boxed_stream(vec![delta("a1"), delta("a2")]);
    let s2 = boxed_stream(vec![delta("b1"), delta("b2")]);
    let merged = MergedStream::new(vec![s1, s2]);
    let result: Vec<_> = merged.collect().await;
    assert_eq!(result.len(), 4);
}

#[tokio::test]
async fn merge_empty_and_nonempty() {
    let s1: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = boxed_stream(vec![]);
    let s2 = boxed_stream(vec![delta("x"), delta("y")]);
    let merged = MergedStream::new(vec![s1, s2]);
    let result: Vec<_> = merged.collect().await;
    assert_eq!(result.len(), 2);
}

#[tokio::test]
async fn merge_three_streams() {
    let s1 = boxed_stream(vec![delta("a")]);
    let s2 = boxed_stream(vec![delta("b")]);
    let s3 = boxed_stream(vec![delta("c")]);
    let merged = MergedStream::new(vec![s1, s2, s3]);
    let result: Vec<_> = merged.collect().await;
    assert_eq!(result.len(), 3);
}

#[tokio::test]
async fn merge_preserves_all_event_types() {
    let s1 = boxed_stream(vec![delta("text"), error_ev("err")]);
    let s2 = boxed_stream(vec![tool_call("read", "t1"), run_completed()]);
    let merged = MergedStream::new(vec![s1, s2]);
    let result: Vec<_> = merged.collect().await;
    assert_eq!(result.len(), 4);

    let has_delta = result
        .iter()
        .any(|e| matches!(e.kind, AgentEventKind::AssistantDelta { .. }));
    let has_error = result
        .iter()
        .any(|e| matches!(e.kind, AgentEventKind::Error { .. }));
    let has_tool = result
        .iter()
        .any(|e| matches!(e.kind, AgentEventKind::ToolCall { .. }));
    let has_completed = result
        .iter()
        .any(|e| matches!(e.kind, AgentEventKind::RunCompleted { .. }));
    assert!(has_delta && has_error && has_tool && has_completed);
}

#[tokio::test]
async fn merge_remaining_count() {
    let s1 = boxed_stream(vec![delta("a")]);
    let s2 = boxed_stream(vec![delta("b")]);
    let merged = MergedStream::new(vec![s1, s2]);
    assert_eq!(merged.remaining(), 2);
}

// ===========================================================================
// 6. Timeout
// ===========================================================================

#[tokio::test]
async fn timeout_stream_yields_events_before_timeout() {
    let events = vec![delta("fast1"), delta("fast2")];
    let inner = vec_stream(events);
    let timeout = TimeoutStream::new(inner, Duration::from_secs(5));
    let results: Vec<_> = timeout.collect().await;
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.is_ok()));
}

#[tokio::test]
async fn timeout_stream_fires_on_slow_source() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(4);
    let stream = EventStream::new(rx);
    let timeout = TimeoutStream::new(stream, Duration::from_millis(50));

    // Don't send anything — the timeout should fire.
    let mut pinned = Box::pin(timeout);
    let first = pinned.next().await;
    assert!(first.is_some());
    assert!(first.unwrap().is_err());

    // Clean up
    drop(tx);
}

#[tokio::test]
async fn timeout_stream_completes_when_inner_completes() {
    let events = vec![delta("only")];
    let inner = vec_stream(events);
    let timeout = TimeoutStream::new(inner, Duration::from_secs(10));
    let results: Vec<_> = timeout.collect().await;
    assert_eq!(results.len(), 1);
    assert!(results[0].is_ok());
}

#[tokio::test]
async fn timeout_error_has_correct_duration() {
    let (_tx, rx) = mpsc::channel::<AgentEvent>(4);
    let stream = EventStream::new(rx);
    let dur = Duration::from_millis(25);
    let timeout = TimeoutStream::new(stream, dur);

    let mut pinned = Box::pin(timeout);
    let item = pinned.next().await.unwrap();
    let err = item.unwrap_err();
    assert_eq!(err.duration, dur);
    drop(_tx);
}

// ===========================================================================
// 7. Buffer
// ===========================================================================

#[test]
fn buffer_push_and_recent() {
    let mut buf = StreamBuffer::new(5);
    buf.push(delta("a"));
    buf.push(delta("b"));
    buf.push(delta("c"));
    assert_eq!(buf.len(), 3);
    assert_eq!(buf.recent(2).len(), 2);
    assert!(!buf.is_full());
}

#[test]
fn buffer_evicts_oldest_when_full() {
    let mut buf = StreamBuffer::new(2);
    buf.push(delta("a"));
    buf.push(delta("b"));
    assert!(buf.is_full());
    buf.push(delta("c"));
    assert_eq!(buf.len(), 2);
    let recent = buf.recent(2);
    // "a" was evicted; "b" and "c" remain
    assert!(matches!(
        &recent[0].kind,
        AgentEventKind::AssistantDelta { text } if text == "b"
    ));
    assert!(matches!(
        &recent[1].kind,
        AgentEventKind::AssistantDelta { text } if text == "c"
    ));
}

#[test]
fn buffer_drain_empties() {
    let mut buf = StreamBuffer::new(10);
    buf.push(delta("a"));
    buf.push(delta("b"));
    let drained = buf.drain();
    assert_eq!(drained.len(), 2);
    assert!(buf.is_empty());
}

#[test]
fn buffer_capacity_reported() {
    let buf = StreamBuffer::new(42);
    assert_eq!(buf.capacity(), 42);
}

#[tokio::test]
async fn buffered_stream_emits_batches() {
    let events = vec![delta("a"), delta("b"), delta("c"), delta("d"), delta("e")];
    let inner = vec_stream(events);
    let buffered = BufferedStream::new(inner, 2);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;
    // 5 events / 2 per batch = 2 full + 1 partial
    assert_eq!(batches.len(), 3);
    assert_eq!(batches[0].len(), 2);
    assert_eq!(batches[1].len(), 2);
    assert_eq!(batches[2].len(), 1);
}

#[tokio::test]
async fn buffered_stream_exact_batch() {
    let events = vec![delta("a"), delta("b"), delta("c"), delta("d")];
    let inner = vec_stream(events);
    let buffered = BufferedStream::new(inner, 2);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;
    assert_eq!(batches.len(), 2);
    assert!(batches.iter().all(|b| b.len() == 2));
}

#[tokio::test]
async fn buffered_stream_single_batch() {
    let events = vec![delta("a"), delta("b")];
    let inner = vec_stream(events);
    let buffered = BufferedStream::new(inner, 10);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 2);
}

// ===========================================================================
// 8. Collect
// ===========================================================================

#[tokio::test]
async fn collect_all_from_event_stream() {
    let (tx, rx) = mpsc::channel(8);
    for ev in [delta("a"), delta("b"), error_ev("e")] {
        tx.send(ev).await.unwrap();
    }
    drop(tx);
    let stream = EventStream::new(rx);
    let all = stream.collect_all().await;
    assert_eq!(all.len(), 3);
}

#[tokio::test]
async fn collect_via_collector_records_snapshot() {
    let events = vec![delta("x"), warning_ev("w"), delta("y")];
    let collector = EventCollector::new(vec_stream(events));
    let handle = collector.collected();
    let _: Vec<_> = collector.collect().await;
    let snapshot = handle.lock().unwrap();
    assert_eq!(snapshot.len(), 3);
}

#[tokio::test]
async fn collect_filtered_only_matching() {
    let (tx, rx) = mpsc::channel(16);
    for ev in [
        run_started(),
        delta("text"),
        tool_call("read", "t1"),
        error_ev("err"),
        run_completed(),
    ] {
        tx.send(ev).await.unwrap();
    }
    drop(tx);
    let stream = EventStream::new(rx);
    let result = stream
        .collect_filtered(&EventFilter::by_kind("tool_call"))
        .await;
    assert_eq!(result.len(), 1);
}

#[tokio::test]
async fn collect_into_vec_via_stream_ext() {
    let events = vec![delta("a"), delta("b"), delta("c")];
    let stream = vec_stream(events);
    let collected: Vec<AgentEvent> = stream.collect().await;
    assert_eq!(collected.len(), 3);
}

// ===========================================================================
// 9. Count (EventStats)
// ===========================================================================

#[test]
fn count_events_by_kind() {
    let stats = EventStats::new();
    stats.observe(&delta("a"));
    stats.observe(&delta("b"));
    stats.observe(&error_ev("e"));
    stats.observe(&tool_call("read", "t1"));
    stats.observe(&warning_ev("w"));
    assert_eq!(stats.total_events(), 5);
    assert_eq!(stats.count_for("assistant_delta"), 2);
    assert_eq!(stats.count_for("error"), 1);
    assert_eq!(stats.count_for("tool_call"), 1);
    assert_eq!(stats.count_for("warning"), 1);
}

#[test]
fn count_delta_bytes() {
    let stats = EventStats::new();
    stats.observe(&delta("hello")); // 5
    stats.observe(&delta("world!!")); // 7
    stats.observe(&error_ev("ignored"));
    assert_eq!(stats.total_delta_bytes(), 12);
}

#[test]
fn count_error_events() {
    let stats = EventStats::new();
    stats.observe(&error_ev("e1"));
    stats.observe(&error_ev("e2"));
    stats.observe(&delta("ok"));
    assert_eq!(stats.error_count(), 2);
}

#[test]
fn count_kind_counts_snapshot() {
    let stats = EventStats::new();
    stats.observe(&run_started());
    stats.observe(&delta("x"));
    stats.observe(&delta("y"));
    stats.observe(&run_completed());
    let counts = stats.kind_counts();
    assert_eq!(counts.get("run_started"), Some(&1));
    assert_eq!(counts.get("assistant_delta"), Some(&2));
    assert_eq!(counts.get("run_completed"), Some(&1));
}

#[test]
fn count_reset_clears() {
    let stats = EventStats::new();
    stats.observe(&delta("a"));
    stats.observe(&error_ev("e"));
    stats.reset();
    assert_eq!(stats.total_events(), 0);
    assert_eq!(stats.error_count(), 0);
    assert_eq!(stats.total_delta_bytes(), 0);
    assert!(stats.kind_counts().is_empty());
}

#[test]
fn count_via_stream_metrics() {
    let mut metrics = StreamMetrics::new();
    metrics.record_event(&delta("abc"));
    metrics.record_event(&error_ev("e"));
    metrics.record_event(&tool_call("write", "t1"));
    assert_eq!(metrics.event_count(), 3);
    assert_eq!(metrics.total_bytes(), 3); // "abc" = 3 bytes
    assert_eq!(metrics.event_type_counts().get("assistant_delta"), Some(&1));
    assert_eq!(metrics.event_type_counts().get("error"), Some(&1));
}

// ===========================================================================
// 10. First / Last
// ===========================================================================

#[tokio::test]
async fn first_event_of_stream() {
    let events = vec![delta("first"), delta("second"), delta("third")];
    let stream = vec_stream(events);
    let first = stream.take(1).collect::<Vec<_>>().await;
    assert_eq!(first.len(), 1);
    assert!(matches!(&first[0].kind, AgentEventKind::AssistantDelta { text } if text == "first"));
}

#[tokio::test]
async fn last_event_of_stream() {
    let events = vec![delta("first"), delta("second"), delta("last")];
    let mut stream = vec_stream(events);
    let mut last = None;
    while let Some(ev) = stream.next().await {
        last = Some(ev);
    }
    assert!(
        matches!(&last.unwrap().kind, AgentEventKind::AssistantDelta { text } if text == "last")
    );
}

#[tokio::test]
async fn first_event_of_specific_kind() {
    let events = vec![
        run_started(),
        delta("hello"),
        error_ev("oops"),
        delta("world"),
    ];
    let stream = vec_stream(events);
    let first_delta = stream
        .filter(|ev| matches!(ev.kind, AgentEventKind::AssistantDelta { .. }))
        .take(1)
        .collect::<Vec<_>>()
        .await;
    assert_eq!(first_delta.len(), 1);
    assert!(matches!(
        &first_delta[0].kind,
        AgentEventKind::AssistantDelta { text } if text == "hello"
    ));
}

#[tokio::test]
async fn last_error_event() {
    let events = vec![
        error_ev("err1"),
        delta("ok"),
        error_ev("err2"),
        delta("fine"),
    ];
    let stream = vec_stream(events);
    let mut last_err = None;
    let mut pinned = Box::pin(stream);
    while let Some(ev) = pinned.next().await {
        if matches!(ev.kind, AgentEventKind::Error { .. }) {
            last_err = Some(ev);
        }
    }
    assert!(matches!(
        &last_err.unwrap().kind,
        AgentEventKind::Error { message, .. } if message == "err2"
    ));
}

#[tokio::test]
async fn first_of_empty_is_none() {
    let events: Vec<AgentEvent> = vec![];
    let stream = vec_stream(events);
    let first = stream.take(1).collect::<Vec<_>>().await;
    assert!(first.is_empty());
}

// ===========================================================================
// 11. Fold / Reduce
// ===========================================================================

#[tokio::test]
async fn fold_count_events() {
    let events = vec![delta("a"), delta("b"), error_ev("e"), delta("c")];
    let stream = vec_stream(events);
    let count = stream.fold(0u32, |acc, _| acc + 1).await;
    assert_eq!(count, 4);
}

#[tokio::test]
async fn fold_concatenate_delta_text() {
    let events = vec![delta("hello"), delta(" "), delta("world")];
    let stream = vec_stream(events);
    let text = stream
        .fold(String::new(), |mut acc, ev| {
            if let AgentEventKind::AssistantDelta { text } = ev.kind {
                acc.push_str(&text);
            }
            acc
        })
        .await;
    assert_eq!(text, "hello world");
}

#[tokio::test]
async fn fold_sum_delta_bytes() {
    let events = vec![delta("abc"), delta("de"), delta("f")];
    let stream = vec_stream(events);
    let bytes = stream
        .fold(0usize, |acc, ev| {
            if let AgentEventKind::AssistantDelta { text } = ev.kind {
                acc + text.len()
            } else {
                acc
            }
        })
        .await;
    assert_eq!(bytes, 6);
}

#[tokio::test]
async fn fold_collect_tool_names() {
    let events = vec![
        tool_call("read_file", "t1"),
        delta("output"),
        tool_call("write_file", "t2"),
        tool_call("execute", "t3"),
    ];
    let stream = vec_stream(events);
    let names: Vec<String> = stream
        .fold(Vec::new(), |mut acc, ev| {
            if let AgentEventKind::ToolCall { tool_name, .. } = ev.kind {
                acc.push(tool_name);
            }
            acc
        })
        .await;
    assert_eq!(names, vec!["read_file", "write_file", "execute"]);
}

#[tokio::test]
async fn fold_empty_stream_returns_initial() {
    let events: Vec<AgentEvent> = vec![];
    let stream = vec_stream(events);
    let result = stream.fold(42u32, |acc, _| acc + 1).await;
    assert_eq!(result, 42);
}

// ===========================================================================
// 12. Text Aggregation
// ===========================================================================

#[test]
fn aggregator_concatenates_deltas() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("Hello"));
    agg.push(&delta(", "));
    agg.push(&delta("world!"));
    assert_eq!(agg.text(), "Hello, world!");
}

#[test]
fn aggregator_handles_assistant_message() {
    let mut agg = StreamAggregator::new();
    agg.push(&message("Complete message."));
    assert_eq!(agg.text(), "Complete message.");
}

#[test]
fn aggregator_mixes_deltas_and_messages() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("Part 1"));
    agg.push(&message(" Part 2"));
    assert_eq!(agg.text(), "Part 1 Part 2");
}

#[tokio::test]
async fn aggregator_from_stream_fold() {
    let events = vec![
        run_started(),
        delta("The answer"),
        delta(" is 42."),
        run_completed(),
    ];
    let stream = vec_stream(events);
    let agg = stream
        .fold(StreamAggregator::new(), |mut agg, ev| {
            agg.push(&ev);
            agg
        })
        .await;
    assert_eq!(agg.text(), "The answer is 42.");
    assert!(agg.is_complete());
    assert_eq!(agg.event_count(), 4);
}

#[test]
fn aggregator_summary_reports_text_length() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("12345"));
    agg.push(&delta("67890"));
    let summary = agg.to_summary();
    assert_eq!(summary.text_length, 10);
    assert_eq!(summary.total_events, 2);
    assert!(!summary.has_errors);
}

#[test]
fn aggregator_empty_text_on_no_deltas() {
    let mut agg = StreamAggregator::new();
    agg.push(&run_started());
    agg.push(&tool_call("read", "t1"));
    assert_eq!(agg.text(), "");
}

// ===========================================================================
// 13. Tool Call Extraction
// ===========================================================================

#[test]
fn aggregator_extracts_tool_calls() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call("read_file", "tc1"));
    agg.push(&tool_call("write_file", "tc2"));
    agg.push(&delta("some text"));

    let tools = agg.tool_calls();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].name, "read_file");
    assert_eq!(tools[0].id, "tc1");
    assert_eq!(tools[1].name, "write_file");
    assert_eq!(tools[1].id, "tc2");
}

#[test]
fn aggregator_matches_tool_results() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call("read_file", "tc1"));
    agg.push(&tool_result("read_file", "tc1", "file contents"));

    let tools = agg.tool_calls();
    assert_eq!(tools.len(), 1);
    assert!(tools[0].result.is_some());
    assert!(tools[0].result.as_ref().unwrap().contains("file contents"));
}

#[test]
fn aggregator_tool_call_without_result() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call("execute", "tc1"));

    let tools = agg.tool_calls();
    assert_eq!(tools.len(), 1);
    assert!(tools[0].result.is_none());
}

#[tokio::test]
async fn extract_tool_calls_via_fold() {
    let events = vec![
        run_started(),
        tool_call("read", "t1"),
        delta("text"),
        tool_call("write", "t2"),
        tool_call("exec", "t3"),
        run_completed(),
    ];
    let stream = vec_stream(events);
    let tool_names: Vec<String> = stream
        .fold(Vec::new(), |mut acc, ev| {
            if let AgentEventKind::ToolCall { tool_name, .. } = ev.kind {
                acc.push(tool_name);
            }
            acc
        })
        .await;
    assert_eq!(tool_names, vec!["read", "write", "exec"]);
}

#[tokio::test]
async fn extract_tool_calls_via_filter() {
    let events = vec![
        delta("hello"),
        tool_call("search", "t1"),
        error_ev("oops"),
        tool_call("grep", "t2"),
    ];
    let stream = vec_stream(events);
    let tool_events: Vec<_> = stream
        .filter(|ev| matches!(ev.kind, AgentEventKind::ToolCall { .. }))
        .collect()
        .await;
    assert_eq!(tool_events.len(), 2);
}

#[test]
fn aggregator_multiple_tool_calls_with_results() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call("read", "tc1"));
    agg.push(&tool_result("read", "tc1", "data1"));
    agg.push(&tool_call("write", "tc2"));
    agg.push(&tool_result("write", "tc2", "ok"));

    let tools = agg.tool_calls();
    assert_eq!(tools.len(), 2);
    assert!(tools[0].result.is_some());
    assert!(tools[1].result.is_some());
}

#[test]
fn aggregator_summary_tool_count() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call("a", "t1"));
    agg.push(&tool_call("b", "t2"));
    agg.push(&tool_call("c", "t3"));
    let summary = agg.to_summary();
    assert_eq!(summary.tool_call_count, 3);
}

// ===========================================================================
// 14. Error Propagation
// ===========================================================================

#[test]
fn error_events_pass_through_pipeline_without_filter() {
    let pipeline = StreamPipelineBuilder::new().build();
    let result = pipeline.process(error_ev("critical"));
    assert!(result.is_some());
    assert!(matches!(result.unwrap().kind, AgentEventKind::Error { .. }));
}

#[test]
fn error_events_filtered_by_errors_only() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .build();
    assert!(pipeline.process(error_ev("err")).is_some());
    assert!(pipeline.process(delta("ok")).is_none());
}

#[test]
fn error_events_tracked_by_stats() {
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();
    pipeline.process(error_ev("e1"));
    pipeline.process(error_ev("e2"));
    pipeline.process(delta("ok"));
    assert_eq!(stats.error_count(), 2);
    assert_eq!(stats.total_events(), 3);
}

#[test]
fn error_events_recorded_by_recorder() {
    let recorder = EventRecorder::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .build();
    pipeline.process(error_ev("e"));
    pipeline.process(delta("ok"));
    assert_eq!(recorder.len(), 2);
    let events = recorder.events();
    assert!(matches!(events[0].kind, AgentEventKind::Error { .. }));
}

#[test]
fn aggregator_collects_errors() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("text"));
    agg.push(&error_ev("oops"));
    agg.push(&error_ev("again"));
    assert_eq!(agg.errors().len(), 2);
    assert!(agg.to_summary().has_errors);
}

#[tokio::test]
async fn error_events_in_merged_stream() {
    let s1 = boxed_stream(vec![delta("ok"), error_ev("err1")]);
    let s2 = boxed_stream(vec![error_ev("err2"), delta("fine")]);
    let merged = MergedStream::new(vec![s1, s2]);
    let result: Vec<_> = merged.collect().await;
    let errors: Vec<_> = result
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::Error { .. }))
        .collect();
    assert_eq!(errors.len(), 2);
}

#[tokio::test]
async fn error_events_survive_filter_pipeline_pipe() {
    let (tx_in, rx_in) = mpsc::channel(16);
    let (tx_out, mut rx_out) = mpsc::channel(16);

    // Pipeline that passes everything
    let pipeline = StreamPipelineBuilder::new().build();

    for ev in [delta("a"), error_ev("oops"), delta("b")] {
        tx_in.send(ev).await.unwrap();
    }
    drop(tx_in);

    let stream = EventStream::new(rx_in);
    stream.pipe(&pipeline, tx_out).await;

    let mut results = Vec::new();
    while let Some(ev) = rx_out.recv().await {
        results.push(ev);
    }
    assert_eq!(results.len(), 3);
    assert!(matches!(results[1].kind, AgentEventKind::Error { .. }));
}

// ===========================================================================
// 15. Empty Streams
// ===========================================================================

#[tokio::test]
async fn empty_stream_collect_all() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(4);
    drop(tx);
    let stream = EventStream::new(rx);
    let result = stream.collect_all().await;
    assert!(result.is_empty());
}

#[tokio::test]
async fn empty_stream_recv_is_none() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(4);
    drop(tx);
    let mut stream = EventStream::new(rx);
    assert!(stream.recv().await.is_none());
}

#[tokio::test]
async fn empty_vec_stream_collects_nothing() {
    let events: Vec<AgentEvent> = vec![];
    let stream = vec_stream(events);
    let collected: Vec<_> = stream.collect().await;
    assert!(collected.is_empty());
}

#[tokio::test]
async fn empty_merged_stream() {
    let merged = MergedStream::new(vec![]);
    let result: Vec<_> = merged.collect().await;
    assert!(result.is_empty());
}

#[tokio::test]
async fn empty_stream_buffered() {
    let events: Vec<AgentEvent> = vec![];
    let inner = vec_stream(events);
    let buffered = BufferedStream::new(inner, 5);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;
    assert!(batches.is_empty());
}

#[tokio::test]
async fn empty_collector_snapshot() {
    let events: Vec<AgentEvent> = vec![];
    let collector = EventCollector::new(vec_stream(events));
    assert!(collector.is_empty());
    let _: Vec<_> = collector.collect().await;
}

#[test]
fn empty_aggregator_state() {
    let agg = StreamAggregator::new();
    assert_eq!(agg.text(), "");
    assert!(agg.tool_calls().is_empty());
    assert!(agg.errors().is_empty());
    assert!(!agg.is_complete());
    assert_eq!(agg.event_count(), 0);
    assert!(agg.thinking().is_none());
    let summary = agg.to_summary();
    assert_eq!(summary.total_events, 0);
    assert_eq!(summary.text_length, 0);
    assert!(!summary.has_errors);
}

#[test]
fn empty_stats() {
    let stats = EventStats::new();
    assert_eq!(stats.total_events(), 0);
    assert_eq!(stats.error_count(), 0);
    assert_eq!(stats.total_delta_bytes(), 0);
    assert!(stats.kind_counts().is_empty());
}

#[test]
fn empty_recorder() {
    let recorder = EventRecorder::new();
    assert!(recorder.is_empty());
    assert_eq!(recorder.len(), 0);
    assert!(recorder.events().is_empty());
}

#[test]
fn empty_buffer() {
    let buf = StreamBuffer::new(10);
    assert!(buf.is_empty());
    assert_eq!(buf.len(), 0);
    assert!(!buf.is_full());
    assert_eq!(buf.recent(5).len(), 0);
}

#[test]
fn empty_metrics() {
    let metrics = StreamMetrics::new();
    assert_eq!(metrics.event_count(), 0);
    assert_eq!(metrics.total_bytes(), 0);
    assert_eq!(metrics.elapsed(), Duration::ZERO);
    assert_eq!(metrics.throughput(), 0.0);
    assert!(metrics.event_type_counts().is_empty());
}

#[tokio::test]
async fn empty_stream_fold_returns_initial() {
    let events: Vec<AgentEvent> = vec![];
    let result = vec_stream(events).fold(100u32, |acc, _| acc + 1).await;
    assert_eq!(result, 100);
}

// ===========================================================================
// Additional Edge Cases & Integration
// ===========================================================================

#[tokio::test]
async fn pipeline_pipe_with_filter_and_transform() {
    let (tx_in, rx_in) = mpsc::channel(16);
    let (tx_out, mut rx_out) = mpsc::channel(16);

    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::by_kind("assistant_delta"))
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("processed".into(), serde_json::json!(true));
            ev
        }))
        .build();

    for ev in [
        delta("a"),
        error_ev("skip"),
        delta("b"),
        tool_call("r", "t1"),
    ] {
        tx_in.send(ev).await.unwrap();
    }
    drop(tx_in);

    let stream = EventStream::new(rx_in);
    stream.pipe(&pipeline, tx_out).await;

    let mut results = Vec::new();
    while let Some(ev) = rx_out.recv().await {
        results.push(ev);
    }
    // Only deltas pass the filter
    assert_eq!(results.len(), 2);
    for ev in &results {
        assert!(ev.ext.as_ref().unwrap().contains_key("processed"));
    }
}

#[tokio::test]
async fn collector_and_filter_combined() {
    let events = vec![
        delta("a"),
        error_ev("e"),
        delta("b"),
        warning_ev("w"),
        delta("c"),
    ];
    let stream = vec_stream(events);
    let filtered = stream.filter(|ev| matches!(ev.kind, AgentEventKind::AssistantDelta { .. }));
    let collector = EventCollector::new(filtered);
    let handle = collector.collected();
    let out: Vec<_> = collector.collect().await;
    assert_eq!(out.len(), 3);
    assert_eq!(handle.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn recorder_clone_shares_state() {
    let recorder = EventRecorder::new();
    let clone = recorder.clone();
    recorder.record(&delta("a"));
    recorder.record(&delta("b"));
    assert_eq!(clone.len(), 2);
    clone.clear();
    assert!(recorder.is_empty());
}

#[test]
fn buffer_recent_more_than_stored() {
    let mut buf = StreamBuffer::new(10);
    buf.push(delta("only"));
    assert_eq!(buf.recent(100).len(), 1);
}

#[test]
fn aggregator_run_completed_marks_complete() {
    let mut agg = StreamAggregator::new();
    assert!(!agg.is_complete());
    agg.push(&run_started());
    assert!(!agg.is_complete());
    agg.push(&run_completed());
    assert!(agg.is_complete());
}

#[test]
fn aggregator_handles_file_changed_and_command() {
    let mut agg = StreamAggregator::new();
    agg.push(&file_changed("src/main.rs"));
    agg.push(&command_executed("cargo build", 0));
    // These don't add text or tool calls but do increment event count
    assert_eq!(agg.event_count(), 2);
    assert_eq!(agg.text(), "");
    assert!(agg.tool_calls().is_empty());
}

#[test]
fn metrics_summary_display() {
    let mut metrics = StreamMetrics::new();
    metrics.record_event(&delta("abc"));
    metrics.record_event(&error_ev("e"));
    let summary = metrics.summary();
    let display = format!("{summary}");
    assert!(display.contains("Events: 2"));
    assert!(display.contains("Bytes: 3"));
}

#[tokio::test]
async fn multiplexer_sorted_merge() {
    let ts_base = Utc::now();
    let (tx1, rx1) = mpsc::channel(4);
    let (tx2, rx2) = mpsc::channel(4);

    tx1.send(make_event_with_ts(
        AgentEventKind::AssistantDelta {
            text: "first".into(),
        },
        ts_base,
    ))
    .await
    .unwrap();
    tx2.send(make_event_with_ts(
        AgentEventKind::AssistantDelta {
            text: "second".into(),
        },
        ts_base + chrono::Duration::milliseconds(10),
    ))
    .await
    .unwrap();
    drop(tx1);
    drop(tx2);

    let mux = abp_stream::EventMultiplexer::new(vec![rx1, rx2]);
    let sorted = mux.collect_sorted().await;
    assert_eq!(sorted.len(), 2);
    assert!(sorted[0].ts <= sorted[1].ts);
}
