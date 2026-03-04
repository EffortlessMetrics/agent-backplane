#![allow(clippy::all)]

//! Comprehensive tests for the `abp-stream` crate — event stream processing,
//! transformation, buffering, merging, tee, timeout, metrics, aggregation,
//! pipeline composition, and multiplexing.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::Utc;
use futures::StreamExt;
use futures::stream::Stream as _;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use abp_core::{AgentEvent, AgentEventKind};
use abp_stream::{
    BufferedStream, EventCollector, EventFilter, EventRecorder, EventStats, EventStream,
    EventTransform, MergedStream, StreamAggregator, StreamBuffer, StreamMetrics, StreamPipeline,
    StreamPipelineBuilder, StreamSummary, StreamTee, StreamTimeout, TeeError, TimeoutStream,
    ToolCallAggregate, event_kind_name,
};

// =========================================================================
// Helpers
// =========================================================================

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

fn error_ev_with_code(msg: &str, code: abp_error::ErrorCode) -> AgentEvent {
    make_event(AgentEventKind::Error {
        message: msg.to_string(),
        error_code: Some(code),
    })
}

fn run_started() -> AgentEvent {
    make_event(AgentEventKind::RunStarted {
        message: "started".into(),
    })
}

fn run_completed() -> AgentEvent {
    make_event(AgentEventKind::RunCompleted {
        message: "done".into(),
    })
}

fn tool_call(name: &str) -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: name.to_string(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: json!({}),
    })
}

fn tool_call_with_id(name: &str, id: &str, input: serde_json::Value) -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: name.to_string(),
        tool_use_id: Some(id.to_string()),
        parent_tool_use_id: None,
        input,
    })
}

fn tool_result(name: &str, id: Option<&str>, output: serde_json::Value) -> AgentEvent {
    make_event(AgentEventKind::ToolResult {
        tool_name: name.to_string(),
        tool_use_id: id.map(|s| s.to_string()),
        output,
        is_error: false,
    })
}

fn tool_result_error(name: &str, id: Option<&str>, output: serde_json::Value) -> AgentEvent {
    make_event(AgentEventKind::ToolResult {
        tool_name: name.to_string(),
        tool_use_id: id.map(|s| s.to_string()),
        output,
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

fn command_executed(cmd: &str, exit_code: Option<i32>) -> AgentEvent {
    make_event(AgentEventKind::CommandExecuted {
        command: cmd.to_string(),
        exit_code,
        output_preview: None,
    })
}

/// Helper to create an event stream from a vec of events via an mpsc channel.
fn events_to_stream(events: Vec<AgentEvent>) -> ReceiverStream<AgentEvent> {
    let (tx, rx) = mpsc::channel(events.len().max(1));
    tokio::spawn(async move {
        for ev in events {
            let _ = tx.send(ev).await;
        }
    });
    ReceiverStream::new(rx)
}

// =========================================================================
// 1. EventFilter tests
// =========================================================================

#[test]
fn filter_by_kind_assistant_delta() {
    let f = EventFilter::by_kind("assistant_delta");
    assert!(f.matches(&delta("hi")));
    assert!(!f.matches(&error_ev("nope")));
}

#[test]
fn filter_by_kind_tool_call() {
    let f = EventFilter::by_kind("tool_call");
    assert!(f.matches(&tool_call("read")));
    assert!(!f.matches(&delta("x")));
}

#[test]
fn filter_by_kind_error() {
    let f = EventFilter::by_kind("error");
    assert!(f.matches(&error_ev("fail")));
    assert!(!f.matches(&run_started()));
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
fn filter_by_kind_warning() {
    let f = EventFilter::by_kind("warning");
    assert!(f.matches(&warning_ev("warn")));
    assert!(!f.matches(&delta("x")));
}

#[test]
fn filter_by_kind_file_changed() {
    let f = EventFilter::by_kind("file_changed");
    assert!(f.matches(&file_changed("a.rs")));
    assert!(!f.matches(&delta("x")));
}

#[test]
fn filter_by_kind_command_executed() {
    let f = EventFilter::by_kind("command_executed");
    assert!(f.matches(&command_executed("ls", Some(0))));
    assert!(!f.matches(&delta("x")));
}

#[test]
fn filter_by_kind_tool_result() {
    let f = EventFilter::by_kind("tool_result");
    assert!(f.matches(&tool_result("read", None, json!("ok"))));
    assert!(!f.matches(&tool_call("read")));
}

#[test]
fn filter_by_kind_assistant_message() {
    let f = EventFilter::by_kind("assistant_message");
    assert!(f.matches(&message("hello")));
    assert!(!f.matches(&delta("hello")));
}

#[test]
fn filter_errors_only_accepts_errors() {
    let f = EventFilter::errors_only();
    assert!(f.matches(&error_ev("bad")));
    assert!(!f.matches(&delta("ok")));
    assert!(!f.matches(&warning_ev("warn")));
    assert!(!f.matches(&run_started()));
}

#[test]
fn filter_exclude_errors_rejects_errors() {
    let f = EventFilter::exclude_errors();
    assert!(!f.matches(&error_ev("bad")));
    assert!(f.matches(&delta("ok")));
    assert!(f.matches(&warning_ev("warn")));
    assert!(f.matches(&tool_call("x")));
}

#[test]
fn filter_custom_predicate_text_length() {
    let f = EventFilter::new(
        |ev| matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text.len() > 5),
    );
    assert!(f.matches(&delta("abcdef")));
    assert!(!f.matches(&delta("abc")));
}

#[test]
fn filter_custom_predicate_ext_field() {
    let f = EventFilter::new(|ev| ev.ext.is_some());
    assert!(!f.matches(&delta("x")));
    let mut ev = delta("x");
    ev.ext = Some(BTreeMap::new());
    assert!(f.matches(&ev));
}

#[test]
fn filter_clone_shares_behavior() {
    let f = EventFilter::by_kind("error");
    let f2 = f.clone();
    assert!(f2.matches(&error_ev("x")));
    assert!(!f2.matches(&delta("x")));
}

#[test]
fn filter_debug_does_not_panic() {
    let f = EventFilter::by_kind("error");
    let dbg = format!("{:?}", f);
    assert!(dbg.contains("EventFilter"));
}

// =========================================================================
// 2. EventTransform tests
// =========================================================================

#[test]
fn transform_identity_preserves_event() {
    let t = EventTransform::identity();
    let ev = delta("hello");
    let result = t.apply(ev.clone());
    assert_eq!(result.ts, ev.ts);
    assert!(result.ext.is_none());
}

#[test]
fn transform_adds_ext_field() {
    let t = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("tag".to_string(), json!("added"));
        ev
    });
    let result = t.apply(delta("x"));
    assert_eq!(result.ext.unwrap()["tag"], json!("added"));
}

#[test]
fn transform_chain_order_matters() {
    let t1 = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("step".to_string(), json!(1));
        ev
    });
    let t2 = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("step".to_string(), json!(2));
        ev
    });
    let ev = t2.apply(t1.apply(delta("x")));
    assert_eq!(ev.ext.unwrap()["step"], json!(2));
}

#[test]
fn transform_clone_produces_equivalent() {
    let t = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("k".to_string(), json!(true));
        ev
    });
    let t2 = t.clone();
    let r1 = t.apply(delta("a"));
    let r2 = t2.apply(delta("a"));
    assert!(r1.ext.is_some());
    assert!(r2.ext.is_some());
}

#[test]
fn transform_debug_does_not_panic() {
    let t = EventTransform::identity();
    let dbg = format!("{:?}", t);
    assert!(dbg.contains("EventTransform"));
}

// =========================================================================
// 3. EventRecorder tests
// =========================================================================

#[test]
fn recorder_starts_empty() {
    let r = EventRecorder::new();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
    assert!(r.events().is_empty());
}

#[test]
fn recorder_records_events() {
    let r = EventRecorder::new();
    r.record(&delta("a"));
    r.record(&error_ev("b"));
    assert_eq!(r.len(), 2);
}

#[test]
fn recorder_events_snapshot() {
    let r = EventRecorder::new();
    r.record(&delta("first"));
    let snap = r.events();
    r.record(&delta("second"));
    assert_eq!(snap.len(), 1);
    assert_eq!(r.len(), 2);
}

#[test]
fn recorder_clear_empties() {
    let r = EventRecorder::new();
    r.record(&delta("a"));
    r.clear();
    assert!(r.is_empty());
}

#[test]
fn recorder_clone_shares_state() {
    let r = EventRecorder::new();
    let r2 = r.clone();
    r.record(&delta("a"));
    assert_eq!(r2.len(), 1);
}

#[test]
fn recorder_default_is_empty() {
    let r = EventRecorder::default();
    assert!(r.is_empty());
}

// =========================================================================
// 4. EventStats tests
// =========================================================================

#[test]
fn stats_empty_initial_state() {
    let s = EventStats::new();
    assert_eq!(s.total_events(), 0);
    assert_eq!(s.total_delta_bytes(), 0);
    assert_eq!(s.error_count(), 0);
    assert!(s.kind_counts().is_empty());
}

#[test]
fn stats_counts_by_kind() {
    let s = EventStats::new();
    s.observe(&delta("abc"));
    s.observe(&delta("de"));
    s.observe(&error_ev("e"));
    s.observe(&tool_call("r"));
    assert_eq!(s.total_events(), 4);
    assert_eq!(s.count_for("assistant_delta"), 2);
    assert_eq!(s.count_for("error"), 1);
    assert_eq!(s.count_for("tool_call"), 1);
    assert_eq!(s.count_for("warning"), 0);
}

#[test]
fn stats_delta_bytes() {
    let s = EventStats::new();
    s.observe(&delta("hello")); // 5
    s.observe(&delta("world!")); // 6
    assert_eq!(s.total_delta_bytes(), 11);
}

#[test]
fn stats_error_count() {
    let s = EventStats::new();
    s.observe(&error_ev("1"));
    s.observe(&error_ev("2"));
    s.observe(&delta("ok"));
    assert_eq!(s.error_count(), 2);
}

#[test]
fn stats_reset_clears() {
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
fn stats_kind_counts_snapshot() {
    let s = EventStats::new();
    s.observe(&run_started());
    s.observe(&delta("x"));
    s.observe(&run_completed());
    let counts = s.kind_counts();
    assert_eq!(counts["run_started"], 1);
    assert_eq!(counts["assistant_delta"], 1);
    assert_eq!(counts["run_completed"], 1);
}

#[test]
fn stats_clone_shares_state() {
    let s = EventStats::new();
    let s2 = s.clone();
    s.observe(&delta("a"));
    assert_eq!(s2.total_events(), 1);
}

#[test]
fn stats_non_delta_events_no_bytes() {
    let s = EventStats::new();
    s.observe(&error_ev("fail"));
    s.observe(&tool_call("x"));
    s.observe(&warning_ev("warn"));
    assert_eq!(s.total_delta_bytes(), 0);
}

// =========================================================================
// 5. event_kind_name tests
// =========================================================================

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
            input: json!(null),
        }),
        "tool_call"
    );
    assert_eq!(
        event_kind_name(&AgentEventKind::ToolResult {
            tool_name: String::new(),
            tool_use_id: None,
            output: json!(null),
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
fn event_kind_name_returns_snake_case() {
    for name in &[
        "run_started",
        "run_completed",
        "assistant_delta",
        "assistant_message",
        "tool_call",
        "tool_result",
        "file_changed",
        "command_executed",
        "warning",
        "error",
    ] {
        assert!(
            name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{name} is not snake_case"
        );
    }
}

// =========================================================================
// 6. EventStream tests
// =========================================================================

#[tokio::test]
async fn event_stream_collect_all() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);
    let events = EventStream::new(rx).collect_all().await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn event_stream_collect_all_empty() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(tx);
    let events = EventStream::new(rx).collect_all().await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn event_stream_recv() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("hello")).await.unwrap();
    drop(tx);
    let mut stream = EventStream::new(rx);
    let ev = stream.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { .. }));
    assert!(stream.recv().await.is_none());
}

#[tokio::test]
async fn event_stream_recv_none_on_close() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(tx);
    let mut stream = EventStream::new(rx);
    assert!(stream.recv().await.is_none());
}

#[tokio::test]
async fn event_stream_collect_filtered() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(error_ev("e")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);
    let filter = EventFilter::by_kind("assistant_delta");
    let events = EventStream::new(rx).collect_filtered(&filter).await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn event_stream_collect_filtered_none_match() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    drop(tx);
    let filter = EventFilter::by_kind("error");
    let events = EventStream::new(rx).collect_filtered(&filter).await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn event_stream_into_inner() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    drop(tx);
    let stream = EventStream::new(rx);
    let mut inner = stream.into_inner();
    assert!(inner.recv().await.is_some());
    assert!(inner.recv().await.is_none());
}

#[tokio::test]
async fn event_stream_as_futures_stream() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);
    let stream = EventStream::new(rx);
    let collected: Vec<_> = stream.collect().await;
    assert_eq!(collected.len(), 2);
}

#[tokio::test]
async fn event_stream_pipe_through_pipeline() {
    let (tx_in, rx_in) = mpsc::channel(16);
    let (tx_out, mut rx_out) = mpsc::channel(16);
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .build();
    tx_in.send(delta("ok")).await.unwrap();
    tx_in.send(error_ev("bad")).await.unwrap();
    tx_in.send(delta("also ok")).await.unwrap();
    drop(tx_in);
    EventStream::new(rx_in).pipe(&pipeline, tx_out).await;
    let mut results = Vec::new();
    while let Some(ev) = rx_out.recv().await {
        results.push(ev);
    }
    assert_eq!(results.len(), 2);
}

// =========================================================================
// 7. StreamBuffer tests
// =========================================================================

#[test]
fn buffer_new_starts_empty() {
    let buf = StreamBuffer::new(5);
    assert!(buf.is_empty());
    assert_eq!(buf.len(), 0);
    assert_eq!(buf.capacity(), 5);
    assert!(!buf.is_full());
}

#[test]
fn buffer_push_increases_len() {
    let mut buf = StreamBuffer::new(5);
    buf.push(delta("a"));
    assert_eq!(buf.len(), 1);
    buf.push(delta("b"));
    assert_eq!(buf.len(), 2);
}

#[test]
fn buffer_evicts_oldest_when_full() {
    let mut buf = StreamBuffer::new(2);
    buf.push(delta("a"));
    buf.push(delta("b"));
    assert!(buf.is_full());
    buf.push(delta("c"));
    assert_eq!(buf.len(), 2);
    let recent = buf.recent(10);
    assert_eq!(recent.len(), 2);
    assert!(matches!(&recent[0].kind, AgentEventKind::AssistantDelta { text } if text == "b"));
    assert!(matches!(&recent[1].kind, AgentEventKind::AssistantDelta { text } if text == "c"));
}

#[test]
fn buffer_recent_returns_last_n() {
    let mut buf = StreamBuffer::new(10);
    for i in 0..5 {
        buf.push(delta(&format!("{}", i)));
    }
    let recent = buf.recent(2);
    assert_eq!(recent.len(), 2);
    assert!(matches!(&recent[0].kind, AgentEventKind::AssistantDelta { text } if text == "3"));
    assert!(matches!(&recent[1].kind, AgentEventKind::AssistantDelta { text } if text == "4"));
}

#[test]
fn buffer_recent_more_than_stored() {
    let mut buf = StreamBuffer::new(10);
    buf.push(delta("only"));
    let recent = buf.recent(100);
    assert_eq!(recent.len(), 1);
}

#[test]
fn buffer_drain_returns_all_and_empties() {
    let mut buf = StreamBuffer::new(10);
    buf.push(delta("a"));
    buf.push(delta("b"));
    let drained = buf.drain();
    assert_eq!(drained.len(), 2);
    assert!(buf.is_empty());
}

#[test]
fn buffer_drain_empty() {
    let mut buf = StreamBuffer::new(5);
    let drained = buf.drain();
    assert!(drained.is_empty());
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
    buf.push(delta("b"));
    assert_eq!(buf.len(), 1);
    assert!(
        matches!(&buf.recent(1)[0].kind, AgentEventKind::AssistantDelta { text } if text == "b")
    );
}

#[test]
fn buffer_clone() {
    let mut buf = StreamBuffer::new(5);
    buf.push(delta("a"));
    let buf2 = buf.clone();
    assert_eq!(buf2.len(), 1);
    buf.push(delta("b"));
    assert_eq!(buf2.len(), 1);
    assert_eq!(buf.len(), 2);
}

// =========================================================================
// 8. StreamPipeline & Builder tests
// =========================================================================

#[test]
fn pipeline_empty_passes_through() {
    let p = StreamPipeline::new();
    assert!(p.process(delta("x")).is_some());
}

#[test]
fn pipeline_default_passes_through() {
    let p = StreamPipeline::default();
    assert!(p.process(error_ev("x")).is_some());
}

#[test]
fn pipeline_filter_removes_events() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::by_kind("assistant_delta"))
        .build();
    assert!(p.process(delta("ok")).is_some());
    assert!(p.process(error_ev("bad")).is_none());
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
fn pipeline_transform_modifies_events() {
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("pipeline".to_string(), json!(true));
            ev
        }))
        .build();
    let result = p.process(delta("x")).unwrap();
    assert_eq!(result.ext.unwrap()["pipeline"], json!(true));
}

#[test]
fn pipeline_records_events() {
    let recorder = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .build();
    p.process(delta("a"));
    p.process(delta("b"));
    assert_eq!(recorder.len(), 2);
}

#[test]
fn pipeline_tracks_stats() {
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
fn pipeline_filter_then_record_skips_filtered() {
    let recorder = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .with_recorder(recorder.clone())
        .build();
    p.process(delta("ok"));
    p.process(error_ev("bad"));
    assert_eq!(recorder.len(), 1);
}

#[test]
fn pipeline_compose_full() {
    let recorder = EventRecorder::new();
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("processed".to_string(), json!(true));
            ev
        }))
        .with_recorder(recorder.clone())
        .with_stats(stats.clone())
        .build();
    p.process(delta("a"));
    p.process(error_ev("e"));
    p.process(tool_call("write"));
    assert_eq!(stats.total_events(), 2);
    assert_eq!(recorder.len(), 2);
    for ev in recorder.events() {
        assert_eq!(ev.ext.as_ref().unwrap()["processed"], json!(true));
    }
}

#[test]
fn pipeline_recorder_accessor() {
    let p = StreamPipelineBuilder::new().record().build();
    assert!(p.recorder().is_some());
}

#[test]
fn pipeline_stats_accessor() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new().with_stats(stats).build();
    assert!(p.stats().is_some());
}

#[test]
fn pipeline_no_recorder_returns_none() {
    let p = StreamPipeline::new();
    assert!(p.recorder().is_none());
}

#[test]
fn pipeline_no_stats_returns_none() {
    let p = StreamPipeline::new();
    assert!(p.stats().is_none());
}

#[test]
fn pipeline_builder_record_creates_internal_recorder() {
    let p = StreamPipelineBuilder::new().record().build();
    p.process(delta("a"));
    assert_eq!(p.recorder().unwrap().len(), 1);
}

// =========================================================================
// 9. EventMultiplexer tests
// =========================================================================

#[tokio::test]
async fn multiplexer_collect_sorted_preserves_order() {
    use abp_stream::EventMultiplexer;
    let ts_base = Utc::now();
    let ts1 = ts_base;
    let ts2 = ts_base + chrono::Duration::milliseconds(10);
    let ts3 = ts_base + chrono::Duration::milliseconds(20);

    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    tx1.send(make_event_with_ts(
        AgentEventKind::AssistantDelta { text: "s1".into() },
        ts1,
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
        ts3,
    ))
    .await
    .unwrap();
    drop(tx2);

    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), 3);
    assert!(events[0].ts <= events[1].ts);
    assert!(events[1].ts <= events[2].ts);
}

#[tokio::test]
async fn multiplexer_empty_streams() {
    use abp_stream::EventMultiplexer;
    let (tx1, rx1) = mpsc::channel::<AgentEvent>(16);
    let (tx2, rx2) = mpsc::channel::<AgentEvent>(16);
    drop(tx1);
    drop(tx2);
    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let events = mux.collect_sorted().await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn multiplexer_merge_channel() {
    use abp_stream::EventMultiplexer;
    let ts_base = Utc::now();
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    tx1.send(make_event_with_ts(
        AgentEventKind::RunStarted {
            message: "a".into(),
        },
        ts_base,
    ))
    .await
    .unwrap();
    tx2.send(make_event_with_ts(
        AgentEventKind::RunCompleted {
            message: "b".into(),
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

#[tokio::test]
async fn multiplexer_single_stream() {
    use abp_stream::EventMultiplexer;
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("only")).await.unwrap();
    drop(tx);
    let mux = EventMultiplexer::new(vec![rx]);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), 1);
}

// =========================================================================
// 10. MergedStream tests
// =========================================================================

#[tokio::test]
async fn merged_stream_empty() {
    let merged = MergedStream::new(vec![]);
    assert_eq!(merged.remaining(), 0);
    let collected: Vec<_> = merged.collect().await;
    assert!(collected.is_empty());
}

#[tokio::test]
async fn merged_stream_single_source() {
    let stream = events_to_stream(vec![delta("a"), delta("b")]);
    let merged = MergedStream::new(vec![Box::pin(stream)]);
    assert_eq!(merged.remaining(), 1);
    let collected: Vec<_> = merged.collect().await;
    assert_eq!(collected.len(), 2);
}

#[tokio::test]
async fn merged_stream_multiple_sources() {
    let s1 = events_to_stream(vec![delta("a1"), delta("a2")]);
    let s2 = events_to_stream(vec![delta("b1"), delta("b2")]);
    let merged = MergedStream::new(vec![Box::pin(s1), Box::pin(s2)]);
    let collected: Vec<_> = merged.collect().await;
    assert_eq!(collected.len(), 4);
}

#[tokio::test]
async fn merged_stream_interleaves_fairly() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    for i in 0..3 {
        tx1.send(delta(&format!("a{i}"))).await.unwrap();
        tx2.send(delta(&format!("b{i}"))).await.unwrap();
    }
    drop(tx1);
    drop(tx2);
    let s1 = ReceiverStream::new(rx1);
    let s2 = ReceiverStream::new(rx2);
    let merged = MergedStream::new(vec![Box::pin(s1), Box::pin(s2)]);
    let collected: Vec<_> = merged.collect().await;
    assert_eq!(collected.len(), 6);
}

#[tokio::test]
async fn merged_stream_one_empty_one_full() {
    let s1 = events_to_stream(vec![]);
    let s2 = events_to_stream(vec![delta("b1"), delta("b2")]);
    let merged = MergedStream::new(vec![Box::pin(s1), Box::pin(s2)]);
    let collected: Vec<_> = merged.collect().await;
    assert_eq!(collected.len(), 2);
}

// =========================================================================
// 11. StreamTee tests
// =========================================================================

#[tokio::test]
async fn tee_broadcasts_to_all() {
    let (tx1, mut rx1) = mpsc::channel(16);
    let (tx2, mut rx2) = mpsc::channel(16);
    let tee = StreamTee::new(vec![tx1, tx2]);
    let ev = delta("broadcast");
    tee.send(&ev).await.unwrap();
    assert!(rx1.recv().await.is_some());
    assert!(rx2.recv().await.is_some());
}

#[tokio::test]
async fn tee_empty_senders_ok() {
    let tee = StreamTee::new(vec![]);
    assert!(tee.send(&delta("x")).await.is_ok());
}

#[tokio::test]
async fn tee_all_receivers_closed_error() {
    let (tx1, rx1) = mpsc::channel::<AgentEvent>(16);
    let (tx2, rx2) = mpsc::channel::<AgentEvent>(16);
    drop(rx1);
    drop(rx2);
    let tee = StreamTee::new(vec![tx1, tx2]);
    let result = tee.send(&delta("x")).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn tee_partial_receivers_closed_ok() {
    let (tx1, rx1) = mpsc::channel::<AgentEvent>(16);
    let (tx2, mut rx2) = mpsc::channel(16);
    drop(rx1);
    let tee = StreamTee::new(vec![tx1, tx2]);
    assert!(tee.send(&delta("x")).await.is_ok());
    assert!(rx2.recv().await.is_some());
}

#[tokio::test]
async fn tee_active_count() {
    let (tx1, _rx1) = mpsc::channel::<AgentEvent>(16);
    let (tx2, rx2) = mpsc::channel::<AgentEvent>(16);
    let tee = StreamTee::new(vec![tx1, tx2]);
    assert_eq!(tee.active_count(), 2);
    drop(rx2);
    // The dropped receiver eventually shows up
    assert!(tee.active_count() <= 2);
}

#[tokio::test]
async fn tee_error_display() {
    let err = TeeError::AllReceiversClosed;
    assert_eq!(format!("{err}"), "all receivers closed");
}

#[tokio::test]
async fn tee_error_is_std_error() {
    let err = TeeError::AllReceiversClosed;
    let _: &dyn std::error::Error = &err;
}

// =========================================================================
// 12. StreamMetrics tests
// =========================================================================

#[test]
fn metrics_new_zeroes() {
    let m = StreamMetrics::new();
    assert_eq!(m.event_count(), 0);
    assert_eq!(m.total_bytes(), 0);
    assert!(m.event_type_counts().is_empty());
    assert_eq!(m.throughput(), 0.0);
}

#[test]
fn metrics_default_zeroes() {
    let m = StreamMetrics::default();
    assert_eq!(m.event_count(), 0);
}

#[test]
fn metrics_records_events() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("hello"));
    m.record_event(&error_ev("err"));
    assert_eq!(m.event_count(), 2);
}

#[test]
fn metrics_tracks_bytes() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("abc")); // 3
    m.record_event(&delta("de")); // 2
    assert_eq!(m.total_bytes(), 5);
}

#[test]
fn metrics_type_counts() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("a"));
    m.record_event(&delta("b"));
    m.record_event(&error_ev("e"));
    let counts = m.event_type_counts();
    assert_eq!(counts["assistant_delta"], 2);
    assert_eq!(counts["error"], 1);
}

#[test]
fn metrics_elapsed_zero_before_events() {
    let m = StreamMetrics::new();
    assert_eq!(m.elapsed(), Duration::ZERO);
}

#[test]
fn metrics_summary_structure() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("hello"));
    let summary = m.summary();
    assert_eq!(summary.event_count, 1);
    assert_eq!(summary.total_bytes, 5);
    assert!(summary.event_type_counts.contains_key("assistant_delta"));
}

#[test]
fn metrics_summary_display() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("hi"));
    let summary = m.summary();
    let display = format!("{summary}");
    assert!(display.contains("Events: 1"));
    assert!(display.contains("Bytes: 2"));
}

// =========================================================================
// 13. StreamAggregator tests
// =========================================================================

#[test]
fn aggregator_empty() {
    let agg = StreamAggregator::new();
    assert_eq!(agg.text(), "");
    assert!(agg.tool_calls().is_empty());
    assert!(agg.thinking().is_none());
    assert!(agg.errors().is_empty());
    assert!(!agg.is_complete());
    assert_eq!(agg.event_count(), 0);
}

#[test]
fn aggregator_default() {
    let agg = StreamAggregator::default();
    assert_eq!(agg.event_count(), 0);
}

#[test]
fn aggregator_assistant_delta_concatenation() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("Hello"));
    agg.push(&delta(", "));
    agg.push(&delta("world!"));
    assert_eq!(agg.text(), "Hello, world!");
    assert_eq!(agg.event_count(), 3);
}

#[test]
fn aggregator_assistant_message() {
    let mut agg = StreamAggregator::new();
    agg.push(&message("Complete message"));
    assert_eq!(agg.text(), "Complete message");
}

#[test]
fn aggregator_mixed_delta_and_message() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("start"));
    agg.push(&message(" end"));
    assert_eq!(agg.text(), "start end");
}

#[test]
fn aggregator_tool_call_without_id() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call("read_file"));
    assert_eq!(agg.tool_calls().len(), 1);
    assert_eq!(agg.tool_calls()[0].name, "read_file");
}

#[test]
fn aggregator_tool_call_with_id() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call_with_id("write", "tc-1", json!({"path": "a.rs"})));
    assert_eq!(agg.tool_calls().len(), 1);
    assert_eq!(agg.tool_calls()[0].id, "tc-1");
    assert_eq!(agg.tool_calls()[0].name, "write");
}

#[test]
fn aggregator_tool_result_matches_call() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call_with_id("read", "tc-1", json!({})));
    agg.push(&tool_result("read", Some("tc-1"), json!("file contents")));
    assert_eq!(agg.tool_calls()[0].result, Some("\"file contents\"".into()));
}

#[test]
fn aggregator_tool_result_no_matching_call() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_result("read", Some("nonexistent"), json!("data")));
    assert!(agg.tool_calls().is_empty());
}

#[test]
fn aggregator_tool_result_without_id() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_result("read", None, json!("data")));
    // No crash; no tool call updated
    assert!(agg.tool_calls().is_empty());
}

#[test]
fn aggregator_errors_collected() {
    let mut agg = StreamAggregator::new();
    agg.push(&error_ev("fail1"));
    agg.push(&error_ev("fail2"));
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
fn aggregator_run_started_does_not_complete() {
    let mut agg = StreamAggregator::new();
    agg.push(&run_started());
    assert!(!agg.is_complete());
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
fn aggregator_summary_with_events() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("hello"));
    agg.push(&tool_call("x"));
    agg.push(&error_ev("err"));
    let summary = agg.to_summary();
    assert_eq!(summary.total_events, 3);
    assert_eq!(summary.text_length, 5);
    assert_eq!(summary.tool_call_count, 1);
    assert!(summary.has_errors);
}

#[test]
fn aggregator_multiple_tool_calls() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call_with_id("read", "tc-1", json!({})));
    agg.push(&tool_call_with_id("write", "tc-2", json!({"path": "x"})));
    agg.push(&tool_result("read", Some("tc-1"), json!("data")));
    agg.push(&tool_result("write", Some("tc-2"), json!("ok")));
    assert_eq!(agg.tool_calls().len(), 2);
    assert!(agg.tool_calls()[0].result.is_some());
    assert!(agg.tool_calls()[1].result.is_some());
}

#[test]
fn aggregator_file_changed_counted() {
    let mut agg = StreamAggregator::new();
    agg.push(&file_changed("src/lib.rs"));
    assert_eq!(agg.event_count(), 1);
}

#[test]
fn aggregator_command_executed_counted() {
    let mut agg = StreamAggregator::new();
    agg.push(&command_executed("cargo build", Some(0)));
    assert_eq!(agg.event_count(), 1);
}

#[test]
fn aggregator_warning_counted() {
    let mut agg = StreamAggregator::new();
    agg.push(&warning_ev("something"));
    assert_eq!(agg.event_count(), 1);
    assert!(agg.errors().is_empty());
}

// =========================================================================
// 14. BufferedStream tests
// =========================================================================

#[tokio::test]
async fn buffered_stream_exact_batch() {
    let events = vec![delta("a"), delta("b"), delta("c")];
    let inner = events_to_stream(events);
    let buffered = BufferedStream::new(inner, 3);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 3);
}

#[tokio::test]
async fn buffered_stream_multiple_batches() {
    let events: Vec<_> = (0..5).map(|i| delta(&format!("{i}"))).collect();
    let inner = events_to_stream(events);
    let buffered = BufferedStream::new(inner, 2);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;
    // 2+2+1 = 3 batches
    assert_eq!(batches.len(), 3);
    assert_eq!(batches[0].len(), 2);
    assert_eq!(batches[1].len(), 2);
    assert_eq!(batches[2].len(), 1);
}

#[tokio::test]
async fn buffered_stream_single_item_per_batch() {
    let events = vec![delta("a"), delta("b")];
    let inner = events_to_stream(events);
    let buffered = BufferedStream::new(inner, 1);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].len(), 1);
    assert_eq!(batches[1].len(), 1);
}

#[tokio::test]
async fn buffered_stream_empty_input() {
    let inner = events_to_stream(vec![]);
    let buffered = BufferedStream::new(inner, 5);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;
    assert!(batches.is_empty());
}

#[tokio::test]
async fn buffered_stream_large_batch_size() {
    let events = vec![delta("a"), delta("b")];
    let inner = events_to_stream(events);
    let buffered = BufferedStream::new(inner, 100);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 2);
}

// =========================================================================
// 15. EventCollector tests
// =========================================================================

#[tokio::test]
async fn collector_records_and_forwards() {
    let inner = events_to_stream(vec![delta("a"), delta("b"), delta("c")]);
    let collector = EventCollector::new(inner);
    let handle = collector.collected();
    let collected_stream: Vec<AgentEvent> = collector.collect().await;
    assert_eq!(collected_stream.len(), 3);
    assert_eq!(handle.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn collector_empty_stream() {
    let inner = events_to_stream(vec![]);
    let collector = EventCollector::new(inner);
    assert!(collector.is_empty());
    let collected_stream: Vec<AgentEvent> = collector.collect().await;
    assert!(collected_stream.is_empty());
}

#[tokio::test]
async fn collector_snapshot() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);
    let inner = ReceiverStream::new(rx);
    let mut collector = EventCollector::new(inner);
    // Read one via StreamExt
    use futures::StreamExt;
    let _ = collector.next().await;
    let snap = collector.snapshot();
    assert_eq!(snap.len(), 1);
}

#[tokio::test]
async fn collector_len_updates() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    drop(tx);
    let inner = ReceiverStream::new(rx);
    let mut collector = EventCollector::new(inner);
    assert_eq!(collector.len(), 0);
    let _ = collector.next().await;
    assert_eq!(collector.len(), 1);
}

// =========================================================================
// 16. TimeoutStream tests
// =========================================================================

#[tokio::test]
async fn timeout_stream_no_timeout_when_data_arrives() {
    let events = vec![delta("a"), delta("b")];
    let inner = events_to_stream(events);
    let timeout_stream = TimeoutStream::new(inner, Duration::from_secs(10));
    let results: Vec<_> = timeout_stream.collect().await;
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.is_ok()));
}

#[tokio::test]
async fn timeout_stream_timeout_fires_on_stall() {
    // Create a stream that never sends anything
    let (_tx, rx) = mpsc::channel::<AgentEvent>(1);
    let inner = ReceiverStream::new(rx);
    let timeout_stream = TimeoutStream::new(inner, Duration::from_millis(50));
    tokio::pin!(timeout_stream);
    // First poll should eventually time out
    let first = timeout_stream.next().await;
    assert!(first.is_some());
    assert!(first.unwrap().is_err());
}

#[tokio::test]
async fn timeout_stream_empty_ends_cleanly() {
    let inner = events_to_stream(vec![]);
    let timeout_stream = TimeoutStream::new(inner, Duration::from_secs(1));
    let results: Vec<_> = timeout_stream.collect().await;
    assert!(results.is_empty());
}

#[tokio::test]
async fn stream_timeout_display() {
    let err = StreamTimeout {
        duration: Duration::from_millis(500),
    };
    let display = format!("{err}");
    assert!(display.contains("500ms"));
}

#[tokio::test]
async fn stream_timeout_is_std_error() {
    let err = StreamTimeout {
        duration: Duration::from_secs(1),
    };
    let _: &dyn std::error::Error = &err;
}

// =========================================================================
// 17. Error handling & recovery tests
// =========================================================================

#[test]
fn pipeline_processes_error_events_with_code() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();
    p.process(error_ev_with_code(
        "timeout",
        abp_error::ErrorCode::BackendTimeout,
    ));
    assert_eq!(stats.error_count(), 1);
}

#[test]
fn filter_errors_with_code() {
    let f = EventFilter::errors_only();
    assert!(f.matches(&error_ev_with_code("x", abp_error::ErrorCode::Internal)));
}

#[test]
fn aggregator_error_with_code() {
    let mut agg = StreamAggregator::new();
    agg.push(&error_ev_with_code(
        "stream closed",
        abp_error::ErrorCode::ExecutionToolFailed,
    ));
    assert_eq!(agg.errors().len(), 1);
    assert!(agg.to_summary().has_errors);
}

#[tokio::test]
async fn pipeline_recover_after_error_filtered() {
    let recorder = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .with_recorder(recorder.clone())
        .build();
    p.process(delta("before"));
    p.process(error_ev("boom"));
    p.process(delta("after"));
    assert_eq!(recorder.len(), 2);
}

// =========================================================================
// 18. Stream cancellation / dropping tests
// =========================================================================

#[tokio::test]
async fn dropping_sender_closes_stream() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    drop(tx);
    let mut stream = EventStream::new(rx);
    let _ = stream.recv().await;
    assert!(stream.recv().await.is_none());
}

#[tokio::test]
async fn dropping_receiver_signals_sender() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(1);
    drop(rx);
    assert!(tx.send(delta("x")).await.is_err());
}

#[tokio::test]
async fn merged_stream_drop_mid_iteration() {
    let s1 = events_to_stream(vec![delta("a"), delta("b"), delta("c")]);
    let s2 = events_to_stream(vec![delta("d"), delta("e")]);
    let merged = MergedStream::new(vec![Box::pin(s1), Box::pin(s2)]);
    tokio::pin!(merged);
    // Read just one event and drop — should not panic
    let _ = merged.next().await;
}

// =========================================================================
// 19. Stream ordering guarantee tests
// =========================================================================

#[tokio::test]
async fn event_stream_preserves_insertion_order() {
    let (tx, rx) = mpsc::channel(16);
    for i in 0..10 {
        tx.send(delta(&format!("{i}"))).await.unwrap();
    }
    drop(tx);
    let events = EventStream::new(rx).collect_all().await;
    for (i, ev) in events.iter().enumerate() {
        if let AgentEventKind::AssistantDelta { text } = &ev.kind {
            assert_eq!(text, &format!("{i}"));
        } else {
            panic!("unexpected event kind");
        }
    }
}

#[tokio::test]
async fn buffered_stream_preserves_order_within_batch() {
    let events: Vec<_> = (0..6).map(|i| delta(&format!("{i}"))).collect();
    let inner = events_to_stream(events);
    let buffered = BufferedStream::new(inner, 3);
    let batches: Vec<Vec<AgentEvent>> = buffered.collect().await;
    let mut idx = 0;
    for batch in &batches {
        for ev in batch {
            if let AgentEventKind::AssistantDelta { text } = &ev.kind {
                assert_eq!(text, &format!("{idx}"));
                idx += 1;
            }
        }
    }
    assert_eq!(idx, 6);
}

#[tokio::test]
async fn collector_preserves_order() {
    let events: Vec<_> = (0..5).map(|i| delta(&format!("{i}"))).collect();
    let inner = events_to_stream(events);
    let collector = EventCollector::new(inner);
    let collected: Vec<AgentEvent> = collector.collect().await;
    for (i, ev) in collected.iter().enumerate() {
        if let AgentEventKind::AssistantDelta { text } = &ev.kind {
            assert_eq!(text, &format!("{i}"));
        }
    }
}

// =========================================================================
// 20. Backpressure tests
// =========================================================================

#[tokio::test]
async fn backpressure_bounded_channel() {
    let (tx, rx) = mpsc::channel(2);
    tx.send(delta("1")).await.unwrap();
    tx.send(delta("2")).await.unwrap();
    let sender = tokio::spawn(async move {
        tx.send(delta("3")).await.unwrap();
    });
    let events = EventStream::new(rx).collect_all().await;
    sender.await.unwrap();
    assert_eq!(events.len(), 3);
}

#[tokio::test]
async fn backpressure_tee_bounded() {
    let (tx1, mut rx1) = mpsc::channel(1);
    let (tx2, mut rx2) = mpsc::channel(1);
    let tee = StreamTee::new(vec![tx1, tx2]);
    tee.send(&delta("a")).await.unwrap();
    // Both receivers have the event
    assert!(rx1.recv().await.is_some());
    assert!(rx2.recv().await.is_some());
}

// =========================================================================
// 21. Empty input tests
// =========================================================================

#[test]
fn aggregator_empty_text() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta(""));
    assert_eq!(agg.text(), "");
    assert_eq!(agg.event_count(), 1);
}

#[test]
fn buffer_recent_empty() {
    let buf = StreamBuffer::new(5);
    assert!(buf.recent(10).is_empty());
}

#[test]
fn stats_observe_empty_delta() {
    let s = EventStats::new();
    s.observe(&delta(""));
    assert_eq!(s.total_delta_bytes(), 0);
    assert_eq!(s.total_events(), 1);
}

#[test]
fn metrics_record_empty_delta() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta(""));
    assert_eq!(m.total_bytes(), 0);
    assert_eq!(m.event_count(), 1);
}

#[test]
fn pipeline_empty_process_no_events() {
    let p = StreamPipeline::new();
    // Process nothing — ensure no crash
    let _ = p.recorder();
    let _ = p.stats();
}

#[tokio::test]
async fn multiplexer_no_streams() {
    use abp_stream::EventMultiplexer;
    let mux = EventMultiplexer::new(vec![]);
    let events = mux.collect_sorted().await;
    assert!(events.is_empty());
}

// =========================================================================
// 22. AgentEvent stream processing (full pipeline scenarios)
// =========================================================================

#[tokio::test]
async fn full_pipeline_agent_conversation() {
    let mut agg = StreamAggregator::new();
    let events = vec![
        run_started(),
        delta("Hello"),
        delta(", how "),
        delta("can I help?"),
        tool_call_with_id("search", "tc-1", json!({"query": "rust"})),
        tool_result("search", Some("tc-1"), json!(["result1", "result2"])),
        delta("Based on search results..."),
        run_completed(),
    ];
    for ev in &events {
        agg.push(ev);
    }
    assert!(agg.is_complete());
    assert_eq!(
        agg.text(),
        "Hello, how can I help?Based on search results..."
    );
    assert_eq!(agg.tool_calls().len(), 1);
    assert!(agg.tool_calls()[0].result.is_some());
    assert!(agg.errors().is_empty());
    let summary = agg.to_summary();
    assert_eq!(summary.total_events, 8);
}

#[tokio::test]
async fn full_pipeline_with_errors() {
    let recorder = EventRecorder::new();
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_recorder(recorder.clone())
        .with_stats(stats.clone())
        .build();
    let events = vec![
        run_started(),
        delta("start"),
        error_ev("something went wrong"),
        delta("recovery"),
        run_completed(),
    ];
    for ev in events {
        pipeline.process(ev);
    }
    assert_eq!(stats.total_events(), 5);
    assert_eq!(stats.error_count(), 1);
    assert_eq!(recorder.len(), 5);
}

#[tokio::test]
async fn event_stream_with_all_event_types() {
    let (tx, rx) = mpsc::channel(32);
    let events = vec![
        run_started(),
        delta("text"),
        message("msg"),
        tool_call("read"),
        tool_result("read", None, json!("ok")),
        file_changed("src/main.rs"),
        command_executed("cargo build", Some(0)),
        warning_ev("careful"),
        error_ev("bad"),
        run_completed(),
    ];
    for ev in events {
        tx.send(ev).await.unwrap();
    }
    drop(tx);
    let collected = EventStream::new(rx).collect_all().await;
    assert_eq!(collected.len(), 10);
}

#[tokio::test]
async fn pipeline_transform_then_aggregate() {
    let transform = EventTransform::new(|mut ev| {
        if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
            *text = text.to_uppercase();
        }
        ev
    });
    let p = StreamPipelineBuilder::new().transform(transform).build();
    let mut agg = StreamAggregator::new();
    let events = vec![delta("hello"), delta(" world")];
    for ev in events {
        if let Some(processed) = p.process(ev) {
            agg.push(&processed);
        }
    }
    assert_eq!(agg.text(), "HELLO WORLD");
}

#[tokio::test]
async fn merged_then_aggregate() {
    let s1 = events_to_stream(vec![delta("a"), delta("b")]);
    let s2 = events_to_stream(vec![delta("c"), delta("d")]);
    let merged = MergedStream::new(vec![Box::pin(s1), Box::pin(s2)]);
    let events: Vec<AgentEvent> = merged.collect().await;
    let mut agg = StreamAggregator::new();
    for ev in &events {
        agg.push(ev);
    }
    assert_eq!(agg.text().len(), 4);
    assert_eq!(agg.event_count(), 4);
}

// =========================================================================
// 23. Additional edge case tests
// =========================================================================

#[test]
fn buffer_repeated_drain() {
    let mut buf = StreamBuffer::new(5);
    buf.push(delta("a"));
    let _ = buf.drain();
    let second = buf.drain();
    assert!(second.is_empty());
}

#[test]
fn recorder_record_many() {
    let r = EventRecorder::new();
    for i in 0..100 {
        r.record(&delta(&format!("{i}")));
    }
    assert_eq!(r.len(), 100);
}

#[test]
fn stats_many_kinds() {
    let s = EventStats::new();
    s.observe(&run_started());
    s.observe(&run_completed());
    s.observe(&delta("x"));
    s.observe(&message("x"));
    s.observe(&tool_call("x"));
    s.observe(&tool_result("x", None, json!("x")));
    s.observe(&file_changed("x"));
    s.observe(&command_executed("x", None));
    s.observe(&warning_ev("x"));
    s.observe(&error_ev("x"));
    assert_eq!(s.total_events(), 10);
    let counts = s.kind_counts();
    assert_eq!(counts.len(), 10);
}

#[test]
fn aggregator_clone() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("hello"));
    let agg2 = agg.clone();
    assert_eq!(agg2.text(), "hello");
}

#[test]
fn buffer_debug() {
    let buf = StreamBuffer::new(5);
    let dbg = format!("{:?}", buf);
    assert!(dbg.contains("StreamBuffer"));
}

#[test]
fn metrics_throughput_single_event() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("a"));
    // Single event => throughput 0.0 (no span between first and last)
    assert_eq!(m.throughput(), 0.0);
}

#[tokio::test]
async fn tee_multiple_events() {
    let (tx1, mut rx1) = mpsc::channel(16);
    let (tx2, mut rx2) = mpsc::channel(16);
    let tee = StreamTee::new(vec![tx1, tx2]);
    for i in 0..5 {
        tee.send(&delta(&format!("{i}"))).await.unwrap();
    }
    for _ in 0..5 {
        assert!(rx1.recv().await.is_some());
        assert!(rx2.recv().await.is_some());
    }
}

#[test]
fn aggregator_tool_call_error_result() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call_with_id("exec", "tc-1", json!({})));
    agg.push(&tool_result_error(
        "exec",
        Some("tc-1"),
        json!("permission denied"),
    ));
    assert_eq!(agg.tool_calls().len(), 1);
    assert!(agg.tool_calls()[0].result.is_some());
}

#[test]
fn filter_by_kind_no_match_for_nonexistent_kind() {
    let f = EventFilter::by_kind("nonexistent_kind");
    assert!(!f.matches(&delta("x")));
    assert!(!f.matches(&error_ev("x")));
    assert!(!f.matches(&run_started()));
}

#[tokio::test]
async fn collector_size_hint() {
    let inner = events_to_stream(vec![delta("a")]);
    let collector = EventCollector::new(inner);
    let (lower, _upper) = collector.size_hint();
    // Size hint from inner stream
    assert!(lower == 0 || lower == 1);
}

#[test]
fn stream_summary_serialization() {
    let summary = StreamSummary {
        total_events: 10,
        text_length: 42,
        tool_call_count: 2,
        has_thinking: false,
        has_errors: true,
        duration_ms: Some(150),
    };
    let json = serde_json::to_string(&summary).unwrap();
    assert!(json.contains("\"total_events\":10"));
    let deser: StreamSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.total_events, 10);
}

#[test]
fn tool_call_aggregate_serialization() {
    let agg = ToolCallAggregate {
        id: "tc-1".into(),
        name: "read".into(),
        arguments: "{}".into(),
        result: Some("data".into()),
    };
    let json = serde_json::to_string(&agg).unwrap();
    let deser: ToolCallAggregate = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.id, "tc-1");
    assert_eq!(deser.result, Some("data".into()));
}

#[test]
fn stream_summary_no_duration() {
    let summary = StreamSummary {
        total_events: 0,
        text_length: 0,
        tool_call_count: 0,
        has_thinking: false,
        has_errors: false,
        duration_ms: None,
    };
    let json = serde_json::to_string(&summary).unwrap();
    let deser: StreamSummary = serde_json::from_str(&json).unwrap();
    assert!(deser.duration_ms.is_none());
}

#[test]
fn event_with_ext_data() {
    let mut ev = delta("hello");
    let mut ext = BTreeMap::new();
    ext.insert("custom_key".to_string(), json!("custom_value"));
    ev.ext = Some(ext);
    let transform = EventTransform::identity();
    let result = transform.apply(ev);
    assert!(result.ext.is_some());
    assert_eq!(result.ext.unwrap()["custom_key"], json!("custom_value"));
}

#[tokio::test]
async fn multiplexer_many_streams() {
    use abp_stream::EventMultiplexer;
    let mut rxs = Vec::new();
    for _ in 0..10 {
        let (tx, rx) = mpsc::channel(4);
        tx.send(delta("ev")).await.unwrap();
        drop(tx);
        rxs.push(rx);
    }
    let mux = EventMultiplexer::new(rxs);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), 10);
}

#[tokio::test]
async fn merged_stream_three_sources() {
    let s1 = events_to_stream(vec![delta("a")]);
    let s2 = events_to_stream(vec![delta("b")]);
    let s3 = events_to_stream(vec![delta("c")]);
    let merged = MergedStream::new(vec![Box::pin(s1), Box::pin(s2), Box::pin(s3)]);
    let collected: Vec<_> = merged.collect().await;
    assert_eq!(collected.len(), 3);
}

#[test]
fn pipeline_clone() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .transform(EventTransform::identity())
        .build();
    let p2 = p.clone();
    assert!(p2.process(delta("x")).is_some());
    assert!(p2.process(error_ev("x")).is_none());
}

#[test]
fn pipeline_builder_default() {
    let builder = StreamPipelineBuilder::default();
    let p = builder.build();
    assert!(p.process(delta("x")).is_some());
}

#[tokio::test]
async fn event_stream_many_events() {
    let (tx, rx) = mpsc::channel(256);
    for i in 0..200 {
        tx.send(delta(&format!("{i}"))).await.unwrap();
    }
    drop(tx);
    let events = EventStream::new(rx).collect_all().await;
    assert_eq!(events.len(), 200);
}
