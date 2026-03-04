#![allow(clippy::all)]
#![allow(unknown_lints)]

//! Comprehensive tests for the `abp-stream` crate covering stream creation,
//! filtering, mapping, multiplexing, backpressure, completion, cancellation,
//! error propagation, ordering guarantees, and edge cases.

use std::collections::BTreeMap;
use std::pin::Pin;
use std::time::Duration;

use abp_core::{AgentEvent, AgentEventKind};
use chrono::{DateTime, Utc};
use futures_core::Stream;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

use abp_stream::*;

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

fn make_event_with_ts(kind: AgentEventKind, ts: DateTime<Utc>) -> AgentEvent {
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

fn error_ev(msg: &str) -> AgentEvent {
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

fn tool_call_with_id(name: &str, id: &str) -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: name.to_string(),
        tool_use_id: Some(id.to_string()),
        parent_tool_use_id: None,
        input: serde_json::json!({"arg": "value"}),
    })
}

fn tool_result(name: &str, id: &str) -> AgentEvent {
    make_event(AgentEventKind::ToolResult {
        tool_name: name.to_string(),
        tool_use_id: Some(id.to_string()),
        output: serde_json::json!("result"),
        is_error: false,
    })
}

fn tool_result_error(name: &str, id: &str) -> AgentEvent {
    make_event(AgentEventKind::ToolResult {
        tool_name: name.to_string(),
        tool_use_id: Some(id.to_string()),
        output: serde_json::json!("error result"),
        is_error: true,
    })
}

fn warning_ev(msg: &str) -> AgentEvent {
    make_event(AgentEventKind::Warning {
        message: msg.to_string(),
    })
}

fn file_changed(path: &str) -> AgentEvent {
    make_event(AgentEventKind::FileChanged {
        path: path.to_string(),
        summary: "modified".to_string(),
    })
}

fn command_executed(cmd: &str) -> AgentEvent {
    make_event(AgentEventKind::CommandExecuted {
        command: cmd.to_string(),
        exit_code: Some(0),
        output_preview: None,
    })
}

fn assistant_message(text: &str) -> AgentEvent {
    make_event(AgentEventKind::AssistantMessage {
        text: text.to_string(),
    })
}

// ===========================================================================
// 1. EventFilter tests
// ===========================================================================

#[test]
fn filter_by_kind_assistant_delta() {
    let f = EventFilter::by_kind("assistant_delta");
    assert!(f.matches(&delta("hello")));
    assert!(!f.matches(&error_ev("err")));
    assert!(!f.matches(&tool_call("read")));
}

#[test]
fn filter_by_kind_run_started() {
    let f = EventFilter::by_kind("run_started");
    assert!(f.matches(&run_started()));
    assert!(!f.matches(&run_completed()));
}

#[test]
fn filter_by_kind_run_completed() {
    let f = EventFilter::by_kind("run_completed");
    assert!(f.matches(&run_completed()));
    assert!(!f.matches(&run_started()));
}

#[test]
fn filter_by_kind_tool_call() {
    let f = EventFilter::by_kind("tool_call");
    assert!(f.matches(&tool_call("write_file")));
    assert!(!f.matches(&delta("x")));
}

#[test]
fn filter_by_kind_tool_result() {
    let f = EventFilter::by_kind("tool_result");
    assert!(f.matches(&tool_result("r", "id1")));
    assert!(!f.matches(&tool_call("x")));
}

#[test]
fn filter_by_kind_file_changed() {
    let f = EventFilter::by_kind("file_changed");
    assert!(f.matches(&file_changed("src/main.rs")));
    assert!(!f.matches(&delta("x")));
}

#[test]
fn filter_by_kind_command_executed() {
    let f = EventFilter::by_kind("command_executed");
    assert!(f.matches(&command_executed("ls")));
    assert!(!f.matches(&delta("x")));
}

#[test]
fn filter_by_kind_warning() {
    let f = EventFilter::by_kind("warning");
    assert!(f.matches(&warning_ev("caution")));
    assert!(!f.matches(&error_ev("err")));
}

#[test]
fn filter_by_kind_assistant_message() {
    let f = EventFilter::by_kind("assistant_message");
    assert!(f.matches(&assistant_message("hello")));
    assert!(!f.matches(&delta("x")));
}

#[test]
fn filter_errors_only() {
    let f = EventFilter::errors_only();
    assert!(f.matches(&error_ev("bad")));
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
    assert!(f.matches(&run_completed()));
}

#[test]
fn filter_custom_predicate_text_length() {
    let f = EventFilter::new(
        |ev| matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text.len() > 5),
    );
    assert!(f.matches(&delta("long text")));
    assert!(!f.matches(&delta("hi")));
    assert!(!f.matches(&error_ev("err")));
}

#[test]
fn filter_custom_predicate_by_ext() {
    let f = EventFilter::new(|ev| ev.ext.is_some());
    let mut ev = delta("x");
    assert!(!f.matches(&ev));
    ev.ext = Some(BTreeMap::new());
    assert!(f.matches(&ev));
}

#[test]
fn filter_clone_works() {
    let f = EventFilter::by_kind("error");
    let f2 = f.clone();
    assert!(f2.matches(&error_ev("e")));
    assert!(!f2.matches(&delta("d")));
}

#[test]
fn filter_debug_format() {
    let f = EventFilter::by_kind("error");
    let debug = format!("{:?}", f);
    assert!(debug.contains("EventFilter"));
}

// ===========================================================================
// 2. EventTransform tests
// ===========================================================================

#[test]
fn transform_identity_returns_same_kind() {
    let t = EventTransform::identity();
    let ev = delta("hello");
    let result = t.apply(ev.clone());
    assert_eq!(format!("{:?}", result.kind), format!("{:?}", ev.kind));
}

#[test]
fn transform_add_ext_metadata() {
    let t = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("source".to_string(), serde_json::json!("test"));
        ev
    });
    let result = t.apply(delta("hello"));
    assert_eq!(
        result.ext.unwrap().get("source").unwrap(),
        &serde_json::json!("test")
    );
}

#[test]
fn transform_chain_two_transforms() {
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
    let ev = t2.apply(t1.apply(delta("x")));
    assert_eq!(ev.ext.unwrap().get("step").unwrap(), &serde_json::json!(2));
}

#[test]
fn transform_chain_preserves_earlier_keys() {
    let t1 = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("a".to_string(), serde_json::json!(1));
        ev
    });
    let t2 = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("b".to_string(), serde_json::json!(2));
        ev
    });
    let ev = t2.apply(t1.apply(delta("x")));
    let ext = ev.ext.unwrap();
    assert_eq!(ext.get("a").unwrap(), &serde_json::json!(1));
    assert_eq!(ext.get("b").unwrap(), &serde_json::json!(2));
}

#[test]
fn transform_clone_works() {
    let t = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("cloned".to_string(), serde_json::json!(true));
        ev
    });
    let t2 = t.clone();
    let result = t2.apply(delta("x"));
    assert!(result.ext.unwrap().contains_key("cloned"));
}

#[test]
fn transform_debug_format() {
    let t = EventTransform::identity();
    let debug = format!("{:?}", t);
    assert!(debug.contains("EventTransform"));
}

// ===========================================================================
// 3. EventRecorder tests
// ===========================================================================

#[test]
fn recorder_new_is_empty() {
    let r = EventRecorder::new();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
    assert!(r.events().is_empty());
}

#[test]
fn recorder_records_events_in_order() {
    let r = EventRecorder::new();
    r.record(&delta("a"));
    r.record(&delta("b"));
    r.record(&delta("c"));
    let events = r.events();
    assert_eq!(events.len(), 3);
    if let AgentEventKind::AssistantDelta { ref text } = events[0].kind {
        assert_eq!(text, "a");
    } else {
        panic!("wrong kind");
    }
}

#[test]
fn recorder_clear_and_reuse() {
    let r = EventRecorder::new();
    r.record(&delta("a"));
    r.record(&delta("b"));
    assert_eq!(r.len(), 2);
    r.clear();
    assert!(r.is_empty());
    r.record(&delta("c"));
    assert_eq!(r.len(), 1);
}

#[test]
fn recorder_clone_shares_state() {
    let r = EventRecorder::new();
    let r2 = r.clone();
    r.record(&delta("x"));
    assert_eq!(r2.len(), 1);
    r2.record(&delta("y"));
    assert_eq!(r.len(), 2);
}

#[test]
fn recorder_mixed_event_types() {
    let r = EventRecorder::new();
    r.record(&run_started());
    r.record(&delta("a"));
    r.record(&tool_call("read_file"));
    r.record(&error_ev("oops"));
    r.record(&run_completed());
    assert_eq!(r.len(), 5);
}

// ===========================================================================
// 4. EventStats tests
// ===========================================================================

#[test]
fn stats_new_is_zero() {
    let s = EventStats::new();
    assert_eq!(s.total_events(), 0);
    assert_eq!(s.total_delta_bytes(), 0);
    assert_eq!(s.error_count(), 0);
    assert!(s.kind_counts().is_empty());
}

#[test]
fn stats_counts_by_kind() {
    let s = EventStats::new();
    s.observe(&delta("a"));
    s.observe(&delta("bb"));
    s.observe(&error_ev("e"));
    s.observe(&tool_call("read"));
    s.observe(&warning_ev("w"));
    assert_eq!(s.total_events(), 5);
    assert_eq!(s.count_for("assistant_delta"), 2);
    assert_eq!(s.count_for("error"), 1);
    assert_eq!(s.count_for("tool_call"), 1);
    assert_eq!(s.count_for("warning"), 1);
}

#[test]
fn stats_tracks_delta_bytes() {
    let s = EventStats::new();
    s.observe(&delta("hello")); // 5
    s.observe(&delta("world!")); // 6
    s.observe(&error_ev("x")); // not delta
    assert_eq!(s.total_delta_bytes(), 11);
}

#[test]
fn stats_error_count() {
    let s = EventStats::new();
    s.observe(&error_ev("e1"));
    s.observe(&error_ev("e2"));
    s.observe(&delta("ok"));
    assert_eq!(s.error_count(), 2);
}

#[test]
fn stats_reset_clears_everything() {
    let s = EventStats::new();
    s.observe(&delta("a"));
    s.observe(&error_ev("e"));
    s.reset();
    assert_eq!(s.total_events(), 0);
    assert_eq!(s.total_delta_bytes(), 0);
    assert_eq!(s.error_count(), 0);
    assert!(s.kind_counts().is_empty());
}

#[test]
fn stats_count_for_unknown_kind_is_zero() {
    let s = EventStats::new();
    s.observe(&delta("a"));
    assert_eq!(s.count_for("nonexistent_kind"), 0);
}

#[test]
fn stats_kind_counts_snapshot() {
    let s = EventStats::new();
    s.observe(&run_started());
    s.observe(&delta("x"));
    s.observe(&run_completed());
    let counts = s.kind_counts();
    assert_eq!(counts.len(), 3);
    assert_eq!(counts.get("run_started"), Some(&1));
    assert_eq!(counts.get("assistant_delta"), Some(&1));
    assert_eq!(counts.get("run_completed"), Some(&1));
}

#[test]
fn stats_clone_shares_state() {
    let s = EventStats::new();
    let s2 = s.clone();
    s.observe(&delta("a"));
    assert_eq!(s2.total_events(), 1);
}

// ===========================================================================
// 5. EventStream tests (creation, recv, collect)
// ===========================================================================

#[tokio::test]
async fn stream_new_and_recv() {
    let (tx, rx) = mpsc::channel(16);
    let mut stream = EventStream::new(rx);
    tx.send(delta("hello")).await.unwrap();
    drop(tx);
    let ev = stream.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { .. }));
    assert!(stream.recv().await.is_none());
}

#[tokio::test]
async fn stream_collect_all_empty() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(tx);
    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn stream_collect_all_multiple() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    tx.send(delta("c")).await.unwrap();
    drop(tx);
    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    assert_eq!(events.len(), 3);
}

#[tokio::test]
async fn stream_collect_filtered_errors_only() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(error_ev("e1")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    tx.send(error_ev("e2")).await.unwrap();
    drop(tx);
    let stream = EventStream::new(rx);
    let filter = EventFilter::errors_only();
    let events = stream.collect_filtered(&filter).await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn stream_collect_filtered_none_pass() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);
    let stream = EventStream::new(rx);
    let filter = EventFilter::errors_only();
    let events = stream.collect_filtered(&filter).await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn stream_collect_filtered_all_pass() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);
    let stream = EventStream::new(rx);
    let filter = EventFilter::by_kind("assistant_delta");
    let events = stream.collect_filtered(&filter).await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn stream_into_inner() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    drop(tx);
    let stream = EventStream::new(rx);
    let mut inner = stream.into_inner();
    let ev = inner.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { .. }));
}

#[tokio::test]
async fn stream_implements_futures_stream() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);
    let stream = EventStream::new(rx);
    let events: Vec<_> = stream.collect().await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn stream_single_element() {
    let (tx, rx) = mpsc::channel(1);
    tx.send(delta("only")).await.unwrap();
    drop(tx);
    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    assert_eq!(events.len(), 1);
}

// ===========================================================================
// 6. EventStream::pipe tests
// ===========================================================================

#[tokio::test]
async fn pipe_passthrough_all() {
    let (tx_in, rx_in) = mpsc::channel(16);
    let (tx_out, mut rx_out) = mpsc::channel(16);
    let pipeline = StreamPipeline::new();
    tx_in.send(delta("a")).await.unwrap();
    tx_in.send(delta("b")).await.unwrap();
    drop(tx_in);
    let stream = EventStream::new(rx_in);
    stream.pipe(&pipeline, tx_out).await;
    let mut results = Vec::new();
    while let Some(ev) = rx_out.recv().await {
        results.push(ev);
    }
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn pipe_filters_events() {
    let (tx_in, rx_in) = mpsc::channel(16);
    let (tx_out, mut rx_out) = mpsc::channel(16);
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .build();
    tx_in.send(delta("a")).await.unwrap();
    tx_in.send(error_ev("e")).await.unwrap();
    tx_in.send(delta("b")).await.unwrap();
    drop(tx_in);
    let stream = EventStream::new(rx_in);
    stream.pipe(&pipeline, tx_out).await;
    let mut results = Vec::new();
    while let Some(ev) = rx_out.recv().await {
        results.push(ev);
    }
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn pipe_transforms_events() {
    let (tx_in, rx_in) = mpsc::channel(16);
    let (tx_out, mut rx_out) = mpsc::channel(16);
    let pipeline = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("piped".to_string(), serde_json::json!(true));
            ev
        }))
        .build();
    tx_in.send(delta("a")).await.unwrap();
    drop(tx_in);
    let stream = EventStream::new(rx_in);
    stream.pipe(&pipeline, tx_out).await;
    let ev = rx_out.recv().await.unwrap();
    assert_eq!(
        ev.ext.unwrap().get("piped").unwrap(),
        &serde_json::json!(true)
    );
}

#[tokio::test]
async fn pipe_stops_when_output_closed() {
    let (tx_in, rx_in) = mpsc::channel(16);
    let (tx_out, rx_out) = mpsc::channel(1);
    let pipeline = StreamPipeline::new();
    // Send many events but drop the receiver immediately
    for i in 0..10 {
        tx_in.send(delta(&format!("ev{}", i))).await.unwrap();
    }
    drop(tx_in);
    drop(rx_out); // close output
    let stream = EventStream::new(rx_in);
    // Should not panic—pipe breaks when send fails
    stream.pipe(&pipeline, tx_out).await;
}

#[tokio::test]
async fn pipe_records_and_tracks_stats() {
    let (tx_in, rx_in) = mpsc::channel(16);
    let (tx_out, _rx_out) = mpsc::channel(16);
    let recorder = EventRecorder::new();
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .with_stats(stats.clone())
        .build();
    tx_in.send(delta("a")).await.unwrap();
    tx_in.send(error_ev("e")).await.unwrap();
    drop(tx_in);
    let stream = EventStream::new(rx_in);
    stream.pipe(&pipeline, tx_out).await;
    assert_eq!(recorder.len(), 2);
    assert_eq!(stats.total_events(), 2);
    assert_eq!(stats.error_count(), 1);
}

// ===========================================================================
// 7. EventMultiplexer tests
// ===========================================================================

#[tokio::test]
async fn multiplexer_collect_sorted_empty() {
    let (tx, rx1) = mpsc::channel::<AgentEvent>(16);
    let (tx2, rx2) = mpsc::channel::<AgentEvent>(16);
    drop(tx);
    drop(tx2);
    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let events = mux.collect_sorted().await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn multiplexer_single_stream() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);
    let mux = EventMultiplexer::new(vec![rx]);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn multiplexer_preserves_timestamp_order() {
    let base = Utc::now();
    let ts1 = base;
    let ts2 = base + chrono::Duration::milliseconds(10);
    let ts3 = base + chrono::Duration::milliseconds(20);
    let ts4 = base + chrono::Duration::milliseconds(30);

    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    tx1.send(make_event_with_ts(
        AgentEventKind::AssistantDelta {
            text: "s1-a".into(),
        },
        ts1,
    ))
    .await
    .unwrap();
    tx1.send(make_event_with_ts(
        AgentEventKind::AssistantDelta {
            text: "s1-b".into(),
        },
        ts3,
    ))
    .await
    .unwrap();
    drop(tx1);

    tx2.send(make_event_with_ts(
        AgentEventKind::AssistantDelta {
            text: "s2-a".into(),
        },
        ts2,
    ))
    .await
    .unwrap();
    tx2.send(make_event_with_ts(
        AgentEventKind::AssistantDelta {
            text: "s2-b".into(),
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
async fn multiplexer_three_streams() {
    let base = Utc::now();

    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    let (tx3, rx3) = mpsc::channel(16);

    tx1.send(make_event_with_ts(
        AgentEventKind::AssistantDelta { text: "a".into() },
        base + chrono::Duration::milliseconds(0),
    ))
    .await
    .unwrap();
    tx2.send(make_event_with_ts(
        AgentEventKind::AssistantDelta { text: "b".into() },
        base + chrono::Duration::milliseconds(5),
    ))
    .await
    .unwrap();
    tx3.send(make_event_with_ts(
        AgentEventKind::AssistantDelta { text: "c".into() },
        base + chrono::Duration::milliseconds(10),
    ))
    .await
    .unwrap();
    drop(tx1);
    drop(tx2);
    drop(tx3);

    let mux = EventMultiplexer::new(vec![rx1, rx2, rx3]);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), 3);
    assert!(events[0].ts <= events[1].ts);
    assert!(events[1].ts <= events[2].ts);
}

#[tokio::test]
async fn multiplexer_merge_channel() {
    let base = Utc::now();
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    tx1.send(make_event_with_ts(
        AgentEventKind::RunStarted {
            message: "s1".into(),
        },
        base,
    ))
    .await
    .unwrap();
    tx2.send(make_event_with_ts(
        AgentEventKind::RunCompleted {
            message: "s2".into(),
        },
        base + chrono::Duration::milliseconds(5),
    ))
    .await
    .unwrap();
    drop(tx1);
    drop(tx2);

    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let mut merged = mux.merge(16);

    let first = merged.recv().await.unwrap();
    assert!(matches!(first.kind, AgentEventKind::RunStarted { .. }));
    let second = merged.recv().await.unwrap();
    assert!(matches!(second.kind, AgentEventKind::RunCompleted { .. }));
    assert!(merged.recv().await.is_none());
}

#[tokio::test]
async fn multiplexer_no_streams() {
    let mux = EventMultiplexer::new(vec![]);
    let events = mux.collect_sorted().await;
    assert!(events.is_empty());
}

// ===========================================================================
// 8. StreamPipeline & builder tests
// ===========================================================================

#[test]
fn pipeline_empty_passes_through() {
    let p = StreamPipeline::new();
    assert!(p.process(delta("x")).is_some());
    assert!(p.process(error_ev("e")).is_some());
}

#[test]
fn pipeline_single_filter() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::by_kind("assistant_delta"))
        .build();
    assert!(p.process(delta("ok")).is_some());
    assert!(p.process(error_ev("bad")).is_none());
    assert!(p.process(tool_call("r")).is_none());
}

#[test]
fn pipeline_multiple_filters_all_must_pass() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .filter(EventFilter::by_kind("assistant_delta"))
        .build();
    assert!(p.process(delta("ok")).is_some());
    assert!(p.process(error_ev("bad")).is_none());
    assert!(p.process(tool_call("r")).is_none());
}

#[test]
fn pipeline_transform_adds_metadata() {
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("pipeline".to_string(), serde_json::json!(true));
            ev
        }))
        .build();
    let result = p.process(delta("x")).unwrap();
    assert_eq!(
        result.ext.unwrap().get("pipeline").unwrap(),
        &serde_json::json!(true)
    );
}

#[test]
fn pipeline_filter_before_transform() {
    // Filtered events shouldn't be transformed
    let recorder = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("transformed".to_string(), serde_json::json!(true));
            ev
        }))
        .with_recorder(recorder.clone())
        .build();
    p.process(delta("ok"));
    p.process(error_ev("filtered_out"));
    assert_eq!(recorder.len(), 1);
    let ev = &recorder.events()[0];
    assert!(ev.ext.as_ref().unwrap().contains_key("transformed"));
}

#[test]
fn pipeline_recorder_only() {
    let recorder = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .build();
    p.process(delta("a"));
    p.process(delta("b"));
    assert_eq!(recorder.len(), 2);
}

#[test]
fn pipeline_stats_only() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();
    p.process(delta("a"));
    p.process(error_ev("e"));
    assert_eq!(stats.total_events(), 2);
    assert_eq!(stats.error_count(), 1);
}

#[test]
fn pipeline_full_composition() {
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

    p.process(delta("a"));
    p.process(error_ev("e")); // filtered
    p.process(tool_call("write"));
    p.process(warning_ev("w"));

    assert_eq!(stats.total_events(), 3);
    assert_eq!(recorder.len(), 3);
    for ev in recorder.events() {
        assert!(ev.ext.as_ref().unwrap().contains_key("processed"));
    }
}

#[test]
fn pipeline_accessor_methods() {
    let recorder = EventRecorder::new();
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .with_stats(stats.clone())
        .build();
    assert!(p.recorder().is_some());
    assert!(p.stats().is_some());

    let p2 = StreamPipeline::new();
    assert!(p2.recorder().is_none());
    assert!(p2.stats().is_none());
}

#[test]
fn pipeline_record_mode_builder() {
    let p = StreamPipelineBuilder::new().record().build();
    p.process(delta("a"));
    assert_eq!(p.recorder().unwrap().len(), 1);
}

#[test]
fn pipeline_clone_shares_recorder_stats() {
    let recorder = EventRecorder::new();
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .with_stats(stats.clone())
        .build();
    let p2 = p.clone();
    p.process(delta("a"));
    p2.process(delta("b"));
    assert_eq!(recorder.len(), 2);
    assert_eq!(stats.total_events(), 2);
}

// ===========================================================================
// 9. event_kind_name tests
// ===========================================================================

#[test]
fn kind_name_all_variants() {
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

// ===========================================================================
// 10. StreamBuffer tests
// ===========================================================================

#[test]
fn buffer_new_is_empty() {
    let buf = StreamBuffer::new(5);
    assert!(buf.is_empty());
    assert_eq!(buf.len(), 0);
    assert_eq!(buf.capacity(), 5);
    assert!(!buf.is_full());
}

#[test]
fn buffer_push_and_len() {
    let mut buf = StreamBuffer::new(3);
    buf.push(delta("a"));
    assert_eq!(buf.len(), 1);
    buf.push(delta("b"));
    assert_eq!(buf.len(), 2);
    buf.push(delta("c"));
    assert_eq!(buf.len(), 3);
    assert!(buf.is_full());
}

#[test]
fn buffer_evicts_oldest_on_overflow() {
    let mut buf = StreamBuffer::new(2);
    buf.push(delta("a"));
    buf.push(delta("b"));
    buf.push(delta("c")); // evicts "a"
    assert_eq!(buf.len(), 2);
    let recent = buf.recent(2);
    if let AgentEventKind::AssistantDelta { ref text } = recent[0].kind {
        assert_eq!(text, "b");
    }
    if let AgentEventKind::AssistantDelta { ref text } = recent[1].kind {
        assert_eq!(text, "c");
    }
}

#[test]
fn buffer_recent_fewer_than_n() {
    let mut buf = StreamBuffer::new(10);
    buf.push(delta("a"));
    let recent = buf.recent(5);
    assert_eq!(recent.len(), 1);
}

#[test]
fn buffer_recent_exact_n() {
    let mut buf = StreamBuffer::new(3);
    buf.push(delta("a"));
    buf.push(delta("b"));
    buf.push(delta("c"));
    let recent = buf.recent(3);
    assert_eq!(recent.len(), 3);
}

#[test]
fn buffer_drain() {
    let mut buf = StreamBuffer::new(5);
    buf.push(delta("a"));
    buf.push(delta("b"));
    let drained = buf.drain();
    assert_eq!(drained.len(), 2);
    assert!(buf.is_empty());
}

#[test]
fn buffer_drain_then_push() {
    let mut buf = StreamBuffer::new(2);
    buf.push(delta("a"));
    buf.drain();
    buf.push(delta("b"));
    assert_eq!(buf.len(), 1);
}

#[test]
#[should_panic(expected = "capacity must be > 0")]
fn buffer_zero_capacity_panics() {
    let _ = StreamBuffer::new(0);
}

#[test]
fn buffer_capacity_one() {
    let mut buf = StreamBuffer::new(1);
    buf.push(delta("a"));
    assert!(buf.is_full());
    buf.push(delta("b")); // evicts "a"
    assert_eq!(buf.len(), 1);
    if let AgentEventKind::AssistantDelta { ref text } = buf.recent(1)[0].kind {
        assert_eq!(text, "b");
    }
}

// ===========================================================================
// 11. StreamAggregator tests
// ===========================================================================

#[test]
fn aggregator_new_is_empty() {
    let agg = StreamAggregator::new();
    assert_eq!(agg.text(), "");
    assert!(agg.tool_calls().is_empty());
    assert!(agg.thinking().is_none());
    assert!(agg.errors().is_empty());
    assert!(!agg.is_complete());
    assert_eq!(agg.event_count(), 0);
}

#[test]
fn aggregator_assembles_delta_text() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("Hello "));
    agg.push(&delta("world!"));
    assert_eq!(agg.text(), "Hello world!");
    assert_eq!(agg.event_count(), 2);
}

#[test]
fn aggregator_assembles_assistant_message() {
    let mut agg = StreamAggregator::new();
    agg.push(&assistant_message("Full message"));
    assert_eq!(agg.text(), "Full message");
}

#[test]
fn aggregator_mixed_delta_and_message() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("Hello "));
    agg.push(&assistant_message("world!"));
    assert_eq!(agg.text(), "Hello world!");
}

#[test]
fn aggregator_tracks_tool_calls() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call_with_id("read_file", "tc1"));
    agg.push(&tool_result("read_file", "tc1"));
    assert_eq!(agg.tool_calls().len(), 1);
    assert_eq!(agg.tool_calls()[0].name, "read_file");
    assert!(agg.tool_calls()[0].result.is_some());
}

#[test]
fn aggregator_tool_call_without_id() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call("my_tool"));
    assert_eq!(agg.tool_calls().len(), 1);
    assert!(agg.tool_calls()[0].id.starts_with("my_tool_"));
}

#[test]
fn aggregator_multiple_tool_calls() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call_with_id("read_file", "tc1"));
    agg.push(&tool_call_with_id("write_file", "tc2"));
    agg.push(&tool_result("read_file", "tc1"));
    agg.push(&tool_result("write_file", "tc2"));
    assert_eq!(agg.tool_calls().len(), 2);
    assert!(agg.tool_calls()[0].result.is_some());
    assert!(agg.tool_calls()[1].result.is_some());
}

#[test]
fn aggregator_records_errors() {
    let mut agg = StreamAggregator::new();
    agg.push(&error_ev("e1"));
    agg.push(&error_ev("e2"));
    assert_eq!(agg.errors().len(), 2);
}

#[test]
fn aggregator_run_completed_marks_complete() {
    let mut agg = StreamAggregator::new();
    assert!(!agg.is_complete());
    agg.push(&run_completed());
    assert!(agg.is_complete());
}

#[test]
fn aggregator_to_summary() {
    let mut agg = StreamAggregator::new();
    agg.push(&run_started());
    agg.push(&delta("Hello "));
    agg.push(&delta("world!"));
    agg.push(&tool_call_with_id("read", "tc1"));
    agg.push(&error_ev("oops"));
    agg.push(&run_completed());

    let summary = agg.to_summary();
    assert_eq!(summary.total_events, 6);
    assert_eq!(summary.text_length, 12);
    assert_eq!(summary.tool_call_count, 1);
    assert!(!summary.has_thinking);
    assert!(summary.has_errors);
}

#[test]
fn aggregator_full_lifecycle() {
    let mut agg = StreamAggregator::new();
    agg.push(&run_started());
    agg.push(&delta("I will "));
    agg.push(&delta("read a file."));
    agg.push(&tool_call_with_id("read_file", "tc1"));
    agg.push(&tool_result("read_file", "tc1"));
    agg.push(&delta(" Done."));
    agg.push(&run_completed());

    assert_eq!(agg.text(), "I will read a file. Done.");
    assert_eq!(agg.tool_calls().len(), 1);
    assert!(agg.is_complete());
    assert!(agg.errors().is_empty());
}

// ===========================================================================
// 12. StreamMetrics tests
// ===========================================================================

#[test]
fn metrics_new_is_zero() {
    let m = StreamMetrics::new();
    assert_eq!(m.event_count(), 0);
    assert_eq!(m.total_bytes(), 0);
    assert!(m.event_type_counts().is_empty());
    assert_eq!(m.throughput(), 0.0);
}

#[test]
fn metrics_records_events() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("hello"));
    m.record_event(&error_ev("e"));
    assert_eq!(m.event_count(), 2);
    assert_eq!(m.total_bytes(), 5); // "hello"
}

#[test]
fn metrics_event_type_counts() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("a"));
    m.record_event(&delta("b"));
    m.record_event(&error_ev("e"));
    let counts = m.event_type_counts();
    assert_eq!(counts.get("assistant_delta"), Some(&2));
    assert_eq!(counts.get("error"), Some(&1));
}

#[test]
fn metrics_summary_display() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("hello"));
    let summary = m.summary();
    let display = format!("{}", summary);
    assert!(display.contains("Events: 1"));
    assert!(display.contains("Bytes: 5"));
}

#[test]
fn metrics_default() {
    let m = StreamMetrics::default();
    assert_eq!(m.event_count(), 0);
}

// ===========================================================================
// 13. StreamTee tests
// ===========================================================================

#[tokio::test]
async fn tee_broadcasts_to_all() {
    let (tx1, mut rx1) = mpsc::channel(16);
    let (tx2, mut rx2) = mpsc::channel(16);
    let tee = StreamTee::new(vec![tx1, tx2]);

    tee.send(&delta("hello")).await.unwrap();

    let ev1 = rx1.recv().await.unwrap();
    let ev2 = rx2.recv().await.unwrap();
    assert!(matches!(ev1.kind, AgentEventKind::AssistantDelta { .. }));
    assert!(matches!(ev2.kind, AgentEventKind::AssistantDelta { .. }));
}

#[tokio::test]
async fn tee_skips_closed_receiver() {
    let (tx1, mut rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    drop(rx2); // close one receiver
    let tee = StreamTee::new(vec![tx1, tx2]);

    // Should still succeed because tx1 is active
    tee.send(&delta("hello")).await.unwrap();
    let ev = rx1.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { .. }));
}

#[tokio::test]
async fn tee_all_closed_returns_error() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    drop(rx1);
    drop(rx2);
    let tee = StreamTee::new(vec![tx1, tx2]);

    let result = tee.send(&delta("hello")).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn tee_empty_senders_is_ok() {
    let tee = StreamTee::new(vec![]);
    let result = tee.send(&delta("hello")).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn tee_active_count() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, _rx2) = mpsc::channel(16);
    let tee = StreamTee::new(vec![tx1, tx2]);
    assert_eq!(tee.active_count(), 2);
    drop(rx1);
    // After dropping rx1, active_count should decrease
    // Small yield to let state propagate
    tokio::task::yield_now().await;
    assert_eq!(tee.active_count(), 1);
}

#[tokio::test]
async fn tee_error_display() {
    let err = TeeError::AllReceiversClosed;
    assert_eq!(format!("{}", err), "all receivers closed");
}

// ===========================================================================
// 14. EventCollector tests (Stream wrapper)
// ===========================================================================

#[tokio::test]
async fn collector_records_and_forwards() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let collector = EventCollector::new(stream);
    let collected = collector.collected();

    let events: Vec<_> = collector.collect().await;
    assert_eq!(events.len(), 2);
    assert_eq!(collected.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn collector_empty_stream() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(tx);

    let stream = EventStream::new(rx);
    let collector = EventCollector::new(stream);
    assert!(collector.is_empty());

    let events: Vec<_> = collector.collect().await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn collector_snapshot() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let mut collector = EventCollector::new(stream);
    // Pull one item via StreamExt
    let _item = StreamExt::next(&mut collector).await;
    assert_eq!(collector.len(), 1);
    let snap = collector.snapshot();
    assert_eq!(snap.len(), 1);
}

// ===========================================================================
// 15. BufferedStream tests
// ===========================================================================

#[tokio::test]
async fn buffered_stream_emits_full_batches() {
    let (tx, rx) = mpsc::channel(16);
    for i in 0..6 {
        tx.send(delta(&format!("ev{}", i))).await.unwrap();
    }
    drop(tx);

    let stream = EventStream::new(rx);
    let buffered = BufferedStream::new(stream, 3);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;
    assert_eq!(batches.len(), 2); // 6 events / 3 per batch = 2 batches
    assert_eq!(batches[0].len(), 3);
    assert_eq!(batches[1].len(), 3);
}

#[tokio::test]
async fn buffered_stream_flushes_remainder() {
    let (tx, rx) = mpsc::channel(16);
    for i in 0..5 {
        tx.send(delta(&format!("ev{}", i))).await.unwrap();
    }
    drop(tx);

    let stream = EventStream::new(rx);
    let buffered = BufferedStream::new(stream, 3);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].len(), 3);
    assert_eq!(batches[1].len(), 2); // remainder
}

#[tokio::test]
async fn buffered_stream_empty_input() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(tx);

    let stream = EventStream::new(rx);
    let buffered = BufferedStream::new(stream, 3);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;
    assert!(batches.is_empty());
}

#[tokio::test]
async fn buffered_stream_single_element() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("only")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let buffered = BufferedStream::new(stream, 5);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 1);
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

// ===========================================================================
// 16. TimeoutStream tests
// ===========================================================================

#[tokio::test]
async fn timeout_stream_passes_through_fast_events() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let timeout_stream = TimeoutStream::new(stream, Duration::from_secs(10));
    let items: Vec<_> = timeout_stream.collect().await;
    assert_eq!(items.len(), 2);
    assert!(items[0].is_ok());
    assert!(items[1].is_ok());
}

#[tokio::test]
async fn timeout_stream_empty() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(tx);

    let stream = EventStream::new(rx);
    let timeout_stream = TimeoutStream::new(stream, Duration::from_secs(1));
    let items: Vec<_> = timeout_stream.collect().await;
    assert!(items.is_empty());
}

#[tokio::test]
async fn timeout_error_display() {
    let err = StreamTimeout {
        duration: Duration::from_millis(500),
    };
    let msg = format!("{}", err);
    assert!(msg.contains("timeout"));
    assert!(msg.contains("500ms"));
}

// ===========================================================================
// 17. MergedStream tests
// ===========================================================================

#[tokio::test]
async fn merged_stream_empty() {
    let merged = MergedStream::new(vec![]);
    assert_eq!(merged.remaining(), 0);
    let items: Vec<AgentEvent> = merged.collect().await;
    assert!(items.is_empty());
}

#[tokio::test]
async fn merged_stream_single_source() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let boxed: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = Box::pin(stream);
    let merged = MergedStream::new(vec![boxed]);
    assert_eq!(merged.remaining(), 1);

    let items: Vec<AgentEvent> = merged.collect().await;
    assert_eq!(items.len(), 2);
}

#[tokio::test]
async fn merged_stream_multiple_sources() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    tx1.send(delta("a")).await.unwrap();
    tx2.send(delta("b")).await.unwrap();
    tx1.send(delta("c")).await.unwrap();
    tx2.send(delta("d")).await.unwrap();
    drop(tx1);
    drop(tx2);

    let s1: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = Box::pin(EventStream::new(rx1));
    let s2: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = Box::pin(EventStream::new(rx2));
    let merged = MergedStream::new(vec![s1, s2]);

    let items: Vec<AgentEvent> = merged.collect().await;
    assert_eq!(items.len(), 4);
}

#[tokio::test]
async fn merged_stream_one_source_empty() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel::<AgentEvent>(16);
    tx1.send(delta("a")).await.unwrap();
    drop(tx1);
    drop(tx2);

    let s1: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = Box::pin(EventStream::new(rx1));
    let s2: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = Box::pin(EventStream::new(rx2));
    let merged = MergedStream::new(vec![s1, s2]);

    let items: Vec<AgentEvent> = merged.collect().await;
    assert_eq!(items.len(), 1);
}

// ===========================================================================
// 18. Edge cases: long streams, ordering, etc.
// ===========================================================================

#[tokio::test]
async fn stream_large_volume() {
    let (tx, rx) = mpsc::channel(256);
    let count = 200;
    for i in 0..count {
        tx.send(delta(&format!("event-{}", i))).await.unwrap();
    }
    drop(tx);

    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    assert_eq!(events.len(), count);
}

#[tokio::test]
async fn multiplexer_large_volume_sorted() {
    let base = Utc::now();
    let (tx1, rx1) = mpsc::channel(128);
    let (tx2, rx2) = mpsc::channel(128);

    for i in 0..50 {
        let ts = base + chrono::Duration::milliseconds(i * 2);
        tx1.send(make_event_with_ts(
            AgentEventKind::AssistantDelta {
                text: format!("s1-{}", i),
            },
            ts,
        ))
        .await
        .unwrap();
    }
    for i in 0..50 {
        let ts = base + chrono::Duration::milliseconds(i * 2 + 1);
        tx2.send(make_event_with_ts(
            AgentEventKind::AssistantDelta {
                text: format!("s2-{}", i),
            },
            ts,
        ))
        .await
        .unwrap();
    }
    drop(tx1);
    drop(tx2);

    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), 100);
    // Verify sorted
    for i in 1..events.len() {
        assert!(events[i].ts >= events[i - 1].ts);
    }
}

#[test]
fn buffer_large_volume_with_eviction() {
    let mut buf = StreamBuffer::new(10);
    for i in 0..100 {
        buf.push(delta(&format!("ev-{}", i)));
    }
    assert_eq!(buf.len(), 10);
    assert!(buf.is_full());
    // Should have the last 10 events
    let recent = buf.recent(10);
    if let AgentEventKind::AssistantDelta { ref text } = recent[0].kind {
        assert_eq!(text, "ev-90");
    }
}

#[test]
fn aggregator_many_deltas() {
    let mut agg = StreamAggregator::new();
    for i in 0..100 {
        agg.push(&delta(&format!("chunk{} ", i)));
    }
    assert_eq!(agg.event_count(), 100);
    assert!(agg.text().starts_with("chunk0 chunk1 "));
    assert!(agg.text().ends_with("chunk99 "));
}

#[test]
fn stats_many_events() {
    let s = EventStats::new();
    for i in 0..1000 {
        if i % 10 == 0 {
            s.observe(&error_ev("e"));
        } else {
            s.observe(&delta(&format!("d{}", i)));
        }
    }
    assert_eq!(s.total_events(), 1000);
    assert_eq!(s.error_count(), 100);
    assert_eq!(s.count_for("assistant_delta"), 900);
}

// ===========================================================================
// 19. Error propagation through streams
// ===========================================================================

#[tokio::test]
async fn error_events_flow_through_stream() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(error_ev("e1")).await.unwrap();
    tx.send(error_ev("e2")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    assert_eq!(events.len(), 2);
    for ev in &events {
        assert!(matches!(ev.kind, AgentEventKind::Error { .. }));
    }
}

#[tokio::test]
async fn error_events_filtered_by_pipeline() {
    let (tx, rx) = mpsc::channel(16);
    let (tx_out, mut rx_out) = mpsc::channel(16);

    tx.send(delta("a")).await.unwrap();
    tx.send(error_ev("e1")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    tx.send(error_ev("e2")).await.unwrap();
    drop(tx);

    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .build();

    let stream = EventStream::new(rx);
    stream.pipe(&pipeline, tx_out).await;

    let mut results = Vec::new();
    while let Some(ev) = rx_out.recv().await {
        results.push(ev);
    }
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn tool_result_errors_tracked_by_aggregator() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call_with_id("read_file", "tc1"));
    agg.push(&tool_result_error("read_file", "tc1"));
    // ToolResult with is_error doesn't go into errors vec (that's for Error events)
    assert!(agg.errors().is_empty());
    assert!(agg.tool_calls()[0].result.is_some());
}

// ===========================================================================
// 20. Cancellation and completion patterns
// ===========================================================================

#[tokio::test]
async fn stream_recv_after_sender_drop() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("last")).await.unwrap();
    drop(tx);

    let mut stream = EventStream::new(rx);
    let ev = stream.recv().await;
    assert!(ev.is_some());
    let ev = stream.recv().await;
    assert!(ev.is_none());
}

#[tokio::test]
async fn multiplexer_merge_receiver_closed_early() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    tx1.send(delta("a")).await.unwrap();
    tx2.send(delta("b")).await.unwrap();
    drop(tx1);
    drop(tx2);

    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let mut merged = mux.merge(16);

    // Read one event then drop
    let _first = merged.recv().await.unwrap();
    drop(merged);
    // Should not panic
}

#[tokio::test]
async fn tee_send_after_partial_close() {
    let (tx1, mut rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    let (tx3, mut rx3) = mpsc::channel(16);
    drop(rx2); // close middle receiver

    let tee = StreamTee::new(vec![tx1, tx2, tx3]);
    tee.send(&delta("hello")).await.unwrap();

    assert!(rx1.recv().await.is_some());
    assert!(rx3.recv().await.is_some());
}

// ===========================================================================
// 21. Pipeline process returns None for all filtered
// ===========================================================================

#[test]
fn pipeline_filters_out_everything() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::new(|_| false))
        .build();
    assert!(p.process(delta("a")).is_none());
    assert!(p.process(error_ev("e")).is_none());
    assert!(p.process(run_started()).is_none());
}

#[test]
fn pipeline_allows_everything() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::new(|_| true))
        .build();
    assert!(p.process(delta("a")).is_some());
    assert!(p.process(error_ev("e")).is_some());
    assert!(p.process(run_started()).is_some());
}

// ===========================================================================
// 22. Diverse event kind processing
// ===========================================================================

#[test]
fn aggregator_ignores_non_text_events_in_text() {
    let mut agg = StreamAggregator::new();
    agg.push(&run_started());
    agg.push(&file_changed("src/main.rs"));
    agg.push(&command_executed("cargo build"));
    agg.push(&warning_ev("low disk"));
    assert_eq!(agg.text(), "");
    assert_eq!(agg.event_count(), 4);
}

#[test]
fn metrics_all_event_types_counted() {
    let mut m = StreamMetrics::new();
    m.record_event(&run_started());
    m.record_event(&delta("x"));
    m.record_event(&assistant_message("y"));
    m.record_event(&tool_call("t"));
    m.record_event(&tool_result("t", "id"));
    m.record_event(&file_changed("f.rs"));
    m.record_event(&command_executed("ls"));
    m.record_event(&warning_ev("w"));
    m.record_event(&error_ev("e"));
    m.record_event(&run_completed());

    assert_eq!(m.event_count(), 10);
    let counts = m.event_type_counts();
    assert_eq!(counts.len(), 10);
}

// ===========================================================================
// 23. BufferedStream with flush interval
// ===========================================================================

#[tokio::test]
async fn buffered_stream_with_flush_interval_exact_batch() {
    let (tx, rx) = mpsc::channel(16);
    for i in 0..4 {
        tx.send(delta(&format!("ev{}", i))).await.unwrap();
    }
    drop(tx);

    let stream = EventStream::new(rx);
    let buffered = BufferedStream::new(stream, 4).with_flush_interval(Duration::from_secs(60));
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 4);
}

// ===========================================================================
// 24. Additional integration tests
// ===========================================================================

#[tokio::test]
async fn full_pipeline_with_multiplexer() {
    let base = Utc::now();
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    // Stream 1: deltas
    tx1.send(make_event_with_ts(
        AgentEventKind::AssistantDelta {
            text: "hello ".into(),
        },
        base,
    ))
    .await
    .unwrap();
    tx1.send(make_event_with_ts(
        AgentEventKind::AssistantDelta {
            text: "world".into(),
        },
        base + chrono::Duration::milliseconds(20),
    ))
    .await
    .unwrap();
    drop(tx1);

    // Stream 2: error + delta
    tx2.send(make_event_with_ts(
        AgentEventKind::Error {
            message: "oops".into(),
            error_code: None,
        },
        base + chrono::Duration::milliseconds(10),
    ))
    .await
    .unwrap();
    tx2.send(make_event_with_ts(
        AgentEventKind::AssistantDelta { text: "!".into() },
        base + chrono::Duration::milliseconds(30),
    ))
    .await
    .unwrap();
    drop(tx2);

    // Multiplex
    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let sorted = mux.collect_sorted().await;
    assert_eq!(sorted.len(), 4);

    // Filter through pipeline
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .with_stats(stats.clone())
        .build();

    let mut results = Vec::new();
    for ev in sorted {
        if let Some(processed) = pipeline.process(ev) {
            results.push(processed);
        }
    }

    assert_eq!(results.len(), 3); // 3 deltas, 1 error filtered
    assert_eq!(stats.total_events(), 3);
    assert_eq!(stats.error_count(), 0);
}

#[tokio::test]
async fn recorder_with_stream_and_tee() {
    let (tx, rx) = mpsc::channel(16);
    let (tee_tx1, mut tee_rx1) = mpsc::channel(16);
    let (tee_tx2, mut tee_rx2) = mpsc::channel(16);

    let tee = StreamTee::new(vec![tee_tx1, tee_tx2]);
    let recorder = EventRecorder::new();

    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);

    let mut stream = EventStream::new(rx);
    while let Some(ev) = stream.recv().await {
        recorder.record(&ev);
        tee.send(&ev).await.unwrap();
    }

    assert_eq!(recorder.len(), 2);
    assert!(tee_rx1.recv().await.is_some());
    assert!(tee_rx1.recv().await.is_some());
    assert!(tee_rx2.recv().await.is_some());
    assert!(tee_rx2.recv().await.is_some());
}

#[test]
fn aggregator_summary_no_events() {
    let agg = StreamAggregator::new();
    let summary = agg.to_summary();
    assert_eq!(summary.total_events, 0);
    assert_eq!(summary.text_length, 0);
    assert_eq!(summary.tool_call_count, 0);
    assert!(!summary.has_thinking);
    assert!(!summary.has_errors);
    assert!(summary.duration_ms.is_none());
}

#[test]
fn aggregator_default_trait() {
    let agg = StreamAggregator::default();
    assert_eq!(agg.event_count(), 0);
    assert!(agg.text().is_empty());
}

#[test]
fn tee_error_is_std_error() {
    let err = TeeError::AllReceiversClosed;
    let _: &dyn std::error::Error = &err;
    let debug = format!("{:?}", err);
    assert!(debug.contains("AllReceiversClosed"));
}

#[test]
fn stream_timeout_is_std_error() {
    let err = StreamTimeout {
        duration: Duration::from_secs(1),
    };
    let _: &dyn std::error::Error = &err;
    let debug = format!("{:?}", err);
    assert!(debug.contains("StreamTimeout"));
}

#[tokio::test]
async fn collector_with_filter_downstream() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(error_ev("e")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let collector = EventCollector::new(stream);
    let collected_handle = collector.collected();

    // Consume all from collector, then filter manually
    let all: Vec<_> = collector.collect().await;
    assert_eq!(all.len(), 3);

    // collected_handle also has all 3
    assert_eq!(collected_handle.lock().unwrap().len(), 3);
}

#[test]
fn buffer_recent_zero() {
    let mut buf = StreamBuffer::new(5);
    buf.push(delta("a"));
    let recent = buf.recent(0);
    assert!(recent.is_empty());
}

#[test]
fn buffer_clone() {
    let mut buf = StreamBuffer::new(3);
    buf.push(delta("a"));
    buf.push(delta("b"));
    let buf2 = buf.clone();
    assert_eq!(buf2.len(), 2);
    assert_eq!(buf2.capacity(), 3);
}

#[tokio::test]
async fn stream_mixed_event_types_collect() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(run_started()).await.unwrap();
    tx.send(delta("hello")).await.unwrap();
    tx.send(tool_call("read_file")).await.unwrap();
    tx.send(tool_result("read_file", "tc1")).await.unwrap();
    tx.send(file_changed("main.rs")).await.unwrap();
    tx.send(command_executed("cargo build")).await.unwrap();
    tx.send(warning_ev("low memory")).await.unwrap();
    tx.send(error_ev("fatal")).await.unwrap();
    tx.send(run_completed()).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    assert_eq!(events.len(), 9);
}

#[test]
fn multiple_transforms_accumulate() {
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("t1".to_string(), serde_json::json!(true));
            ev
        }))
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("t2".to_string(), serde_json::json!(true));
            ev
        }))
        .build();

    let result = p.process(delta("x")).unwrap();
    let ext = result.ext.unwrap();
    assert!(ext.contains_key("t1"));
    assert!(ext.contains_key("t2"));
}

#[test]
fn filter_by_kind_rejects_all_non_matching() {
    let f = EventFilter::by_kind("assistant_delta");
    assert!(!f.matches(&run_started()));
    assert!(!f.matches(&run_completed()));
    assert!(!f.matches(&tool_call("t")));
    assert!(!f.matches(&tool_result("t", "id")));
    assert!(!f.matches(&file_changed("f.rs")));
    assert!(!f.matches(&command_executed("ls")));
    assert!(!f.matches(&warning_ev("w")));
    assert!(!f.matches(&error_ev("e")));
    assert!(!f.matches(&assistant_message("m")));
}

#[test]
fn stats_observe_after_reset() {
    let s = EventStats::new();
    s.observe(&delta("a"));
    s.reset();
    s.observe(&delta("b"));
    assert_eq!(s.total_events(), 1);
    assert_eq!(s.count_for("assistant_delta"), 1);
}

#[tokio::test]
async fn tee_multiple_events_broadcast() {
    let (tx1, mut rx1) = mpsc::channel(16);
    let (tx2, mut rx2) = mpsc::channel(16);
    let tee = StreamTee::new(vec![tx1, tx2]);

    for i in 0..5 {
        tee.send(&delta(&format!("ev{}", i))).await.unwrap();
    }

    for _ in 0..5 {
        assert!(rx1.recv().await.is_some());
        assert!(rx2.recv().await.is_some());
    }
}
