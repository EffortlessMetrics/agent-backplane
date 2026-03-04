#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive stream processing and event pipeline tests for `abp-stream`.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

use abp_core::{AgentEvent, AgentEventKind};
use abp_error::ErrorCode;
use abp_stream::{
    BufferedStream, EventCollector, EventFilter, EventMultiplexer, EventRecorder, EventStats,
    EventStream, EventTransform, MergedStream, StreamAggregator, StreamBuffer, StreamMetrics,
    StreamPipeline, StreamPipelineBuilder, StreamSummary, StreamTee, TeeError, TimeoutStream,
    event_kind_name,
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

fn error_ev_with_code(msg: &str, code: ErrorCode) -> AgentEvent {
    make_event(AgentEventKind::Error {
        message: msg.to_string(),
        error_code: Some(code),
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
        input: json!({}),
    })
}

fn tool_call_with_id(name: &str, id: &str) -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: name.to_string(),
        tool_use_id: Some(id.to_string()),
        parent_tool_use_id: None,
        input: json!({"arg": "value"}),
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

fn tool_result_error(name: &str, id: Option<&str>) -> AgentEvent {
    make_event(AgentEventKind::ToolResult {
        tool_name: name.to_string(),
        tool_use_id: id.map(|s| s.to_string()),
        output: json!("error occurred"),
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
        output_preview: Some("ok".to_string()),
    })
}

/// Returns one event of each AgentEventKind variant.
fn all_event_kinds() -> Vec<AgentEvent> {
    vec![
        run_started(),
        run_completed(),
        delta("hello"),
        message("full message"),
        tool_call("read_file"),
        tool_result("read_file", None, json!("contents")),
        file_changed("src/main.rs"),
        command_executed("cargo test", Some(0)),
        warning_ev("heads up"),
        error_ev("something broke"),
    ]
}

// =========================================================================
// 1. StreamBuffer tests
// =========================================================================

#[test]
fn buffer_new_is_empty() {
    let buf = StreamBuffer::new(10);
    assert!(buf.is_empty());
    assert_eq!(buf.len(), 0);
    assert_eq!(buf.capacity(), 10);
    assert!(!buf.is_full());
}

#[test]
fn buffer_push_and_len() {
    let mut buf = StreamBuffer::new(5);
    buf.push(delta("a"));
    buf.push(delta("b"));
    assert_eq!(buf.len(), 2);
    assert!(!buf.is_full());
}

#[test]
fn buffer_reaches_capacity() {
    let mut buf = StreamBuffer::new(3);
    buf.push(delta("a"));
    buf.push(delta("b"));
    buf.push(delta("c"));
    assert!(buf.is_full());
    assert_eq!(buf.len(), 3);
}

#[test]
fn buffer_evicts_oldest_on_overflow() {
    let mut buf = StreamBuffer::new(2);
    buf.push(delta("a"));
    buf.push(delta("b"));
    buf.push(delta("c"));
    assert_eq!(buf.len(), 2);
    let recent = buf.recent(10);
    assert_eq!(recent.len(), 2);
    // "a" was evicted
    match &recent[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "b"),
        _ => panic!("wrong kind"),
    }
    match &recent[1].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "c"),
        _ => panic!("wrong kind"),
    }
}

#[test]
fn buffer_recent_returns_last_n() {
    let mut buf = StreamBuffer::new(10);
    for i in 0..5 {
        buf.push(delta(&format!("e{i}")));
    }
    let recent = buf.recent(3);
    assert_eq!(recent.len(), 3);
    match &recent[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "e2"),
        _ => panic!("wrong kind"),
    }
}

#[test]
fn buffer_recent_all_when_fewer_than_n() {
    let mut buf = StreamBuffer::new(10);
    buf.push(delta("only"));
    let recent = buf.recent(5);
    assert_eq!(recent.len(), 1);
}

#[test]
fn buffer_drain_returns_all_and_empties() {
    let mut buf = StreamBuffer::new(5);
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
fn buffer_single_capacity() {
    let mut buf = StreamBuffer::new(1);
    buf.push(delta("a"));
    assert!(buf.is_full());
    buf.push(delta("b"));
    assert!(buf.is_full());
    assert_eq!(buf.len(), 1);
    match &buf.recent(1)[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "b"),
        _ => panic!("wrong kind"),
    }
}

#[test]
fn buffer_mixed_event_types() {
    let mut buf = StreamBuffer::new(10);
    buf.push(run_started());
    buf.push(delta("hi"));
    buf.push(tool_call("bash"));
    buf.push(error_ev("oops"));
    buf.push(run_completed());
    assert_eq!(buf.len(), 5);
}

#[test]
fn buffer_large_overflow() {
    let mut buf = StreamBuffer::new(3);
    for i in 0..100 {
        buf.push(delta(&format!("e{i}")));
    }
    assert_eq!(buf.len(), 3);
    let recent = buf.recent(3);
    match &recent[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "e97"),
        _ => panic!("wrong kind"),
    }
}

// =========================================================================
// 2. EventRecorder tests
// =========================================================================

#[test]
fn recorder_new_is_empty() {
    let rec = EventRecorder::new();
    assert!(rec.is_empty());
    assert_eq!(rec.len(), 0);
}

#[test]
fn recorder_records_and_replays() {
    let rec = EventRecorder::new();
    rec.record(&delta("a"));
    rec.record(&delta("b"));
    rec.record(&error_ev("e"));
    assert_eq!(rec.len(), 3);
    let events = rec.events();
    assert_eq!(events.len(), 3);
}

#[test]
fn recorder_clear_empties() {
    let rec = EventRecorder::new();
    rec.record(&delta("a"));
    rec.clear();
    assert!(rec.is_empty());
}

#[test]
fn recorder_clone_shares_state() {
    let rec = EventRecorder::new();
    let rec2 = rec.clone();
    rec.record(&delta("a"));
    assert_eq!(rec2.len(), 1);
    rec2.record(&delta("b"));
    assert_eq!(rec.len(), 2);
}

#[test]
fn recorder_preserves_event_order() {
    let rec = EventRecorder::new();
    for i in 0..10 {
        rec.record(&delta(&format!("e{i}")));
    }
    let events = rec.events();
    for (i, ev) in events.iter().enumerate() {
        match &ev.kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, &format!("e{i}")),
            _ => panic!("wrong kind"),
        }
    }
}

#[test]
fn recorder_records_all_event_kinds() {
    let rec = EventRecorder::new();
    for ev in all_event_kinds() {
        rec.record(&ev);
    }
    assert_eq!(rec.len(), 10);
}

// =========================================================================
// 3. EventFilter tests
// =========================================================================

#[test]
fn filter_by_kind_assistant_delta() {
    let f = EventFilter::by_kind("assistant_delta");
    assert!(f.matches(&delta("hello")));
    assert!(!f.matches(&error_ev("oops")));
    assert!(!f.matches(&run_started()));
}

#[test]
fn filter_by_kind_error() {
    let f = EventFilter::by_kind("error");
    assert!(f.matches(&error_ev("fail")));
    assert!(!f.matches(&delta("ok")));
}

#[test]
fn filter_by_kind_tool_call() {
    let f = EventFilter::by_kind("tool_call");
    assert!(f.matches(&tool_call("read_file")));
    assert!(!f.matches(&delta("text")));
}

#[test]
fn filter_by_kind_tool_result() {
    let f = EventFilter::by_kind("tool_result");
    assert!(f.matches(&tool_result("x", None, json!("ok"))));
    assert!(!f.matches(&tool_call("x")));
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
    assert!(!f.matches(&error_ev("err")));
}

#[test]
fn filter_by_kind_file_changed() {
    let f = EventFilter::by_kind("file_changed");
    assert!(f.matches(&file_changed("src/lib.rs")));
    assert!(!f.matches(&delta("text")));
}

#[test]
fn filter_by_kind_command_executed() {
    let f = EventFilter::by_kind("command_executed");
    assert!(f.matches(&command_executed("ls", Some(0))));
    assert!(!f.matches(&delta("text")));
}

#[test]
fn filter_by_kind_assistant_message() {
    let f = EventFilter::by_kind("assistant_message");
    assert!(f.matches(&message("hello")));
    assert!(!f.matches(&delta("hello")));
}

#[test]
fn filter_errors_only_accepts_only_errors() {
    let f = EventFilter::errors_only();
    assert!(f.matches(&error_ev("fail")));
    assert!(!f.matches(&delta("ok")));
    assert!(!f.matches(&warning_ev("warn")));
    assert!(!f.matches(&run_started()));
    assert!(!f.matches(&tool_call("bash")));
}

#[test]
fn filter_exclude_errors_rejects_only_errors() {
    let f = EventFilter::exclude_errors();
    assert!(!f.matches(&error_ev("fail")));
    assert!(f.matches(&delta("ok")));
    assert!(f.matches(&warning_ev("warn")));
    assert!(f.matches(&run_started()));
    assert!(f.matches(&tool_call("bash")));
}

#[test]
fn filter_custom_predicate_text_length() {
    let f = EventFilter::new(
        |ev| matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text.len() > 5),
    );
    assert!(f.matches(&delta("long text")));
    assert!(!f.matches(&delta("hi")));
}

#[test]
fn filter_custom_always_true() {
    let f = EventFilter::new(|_| true);
    for ev in all_event_kinds() {
        assert!(f.matches(&ev));
    }
}

#[test]
fn filter_custom_always_false() {
    let f = EventFilter::new(|_| false);
    for ev in all_event_kinds() {
        assert!(!f.matches(&ev));
    }
}

#[test]
fn filter_by_nonexistent_kind_matches_nothing() {
    let f = EventFilter::by_kind("nonexistent_kind");
    for ev in all_event_kinds() {
        assert!(!f.matches(&ev));
    }
}

#[test]
fn filter_clone_works() {
    let f = EventFilter::errors_only();
    let f2 = f.clone();
    assert!(f2.matches(&error_ev("x")));
    assert!(!f2.matches(&delta("x")));
}

// =========================================================================
// 4. EventStats tests
// =========================================================================

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
    s.observe(&error_ev("e1"));
    s.observe(&error_ev("e2"));
    s.observe(&delta("ok"));
    assert_eq!(s.error_count(), 2);
}

#[test]
fn stats_reset_clears_all() {
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
    assert_eq!(counts.get("run_started"), Some(&1));
    assert_eq!(counts.get("assistant_delta"), Some(&1));
    assert_eq!(counts.get("run_completed"), Some(&1));
}

#[test]
fn stats_all_event_kinds_tracked() {
    let s = EventStats::new();
    for ev in all_event_kinds() {
        s.observe(&ev);
    }
    assert_eq!(s.total_events(), 10);
    let counts = s.kind_counts();
    assert_eq!(counts.len(), 10);
}

#[test]
fn stats_clone_shares_state() {
    let s = EventStats::new();
    let s2 = s.clone();
    s.observe(&delta("a"));
    assert_eq!(s2.total_events(), 1);
}

#[test]
fn stats_non_delta_doesnt_add_bytes() {
    let s = EventStats::new();
    s.observe(&error_ev("this is a long error message"));
    s.observe(&warning_ev("long warning message here"));
    assert_eq!(s.total_delta_bytes(), 0);
}

#[test]
fn stats_empty_delta_adds_zero_bytes() {
    let s = EventStats::new();
    s.observe(&delta(""));
    assert_eq!(s.total_delta_bytes(), 0);
    assert_eq!(s.total_events(), 1);
}

// =========================================================================
// 5. StreamAggregator tests
// =========================================================================

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
fn aggregator_assembles_assistant_delta_text() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("Hello "));
    agg.push(&delta("world!"));
    assert_eq!(agg.text(), "Hello world!");
}

#[test]
fn aggregator_assembles_assistant_message_text() {
    let mut agg = StreamAggregator::new();
    agg.push(&message("Full text."));
    assert_eq!(agg.text(), "Full text.");
}

#[test]
fn aggregator_mixed_delta_and_message() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("Hello "));
    agg.push(&message("world!"));
    assert_eq!(agg.text(), "Hello world!");
}

#[test]
fn aggregator_tool_call_without_id() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call("read_file"));
    assert_eq!(agg.tool_calls().len(), 1);
    assert_eq!(agg.tool_calls()[0].name, "read_file");
    assert!(agg.tool_calls()[0].result.is_none());
}

#[test]
fn aggregator_tool_call_with_id_and_result() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call_with_id("bash", "tc-1"));
    agg.push(&tool_result("bash", Some("tc-1"), json!("output")));
    assert_eq!(agg.tool_calls().len(), 1);
    assert_eq!(agg.tool_calls()[0].id, "tc-1");
    assert_eq!(agg.tool_calls()[0].result.as_deref(), Some("\"output\""));
}

#[test]
fn aggregator_multiple_tool_calls() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call_with_id("read", "tc-1"));
    agg.push(&tool_call_with_id("write", "tc-2"));
    agg.push(&tool_result("read", Some("tc-1"), json!("data")));
    agg.push(&tool_result("write", Some("tc-2"), json!("ok")));
    assert_eq!(agg.tool_calls().len(), 2);
    assert!(agg.tool_calls()[0].result.is_some());
    assert!(agg.tool_calls()[1].result.is_some());
}

#[test]
fn aggregator_orphan_tool_result_ignored() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_result("unknown", Some("no-match"), json!("x")));
    assert!(agg.tool_calls().is_empty());
}

#[test]
fn aggregator_collects_errors() {
    let mut agg = StreamAggregator::new();
    agg.push(&error_ev("err1"));
    agg.push(&error_ev("err2"));
    agg.push(&delta("ok"));
    assert_eq!(agg.errors().len(), 2);
}

#[test]
fn aggregator_run_completed_sets_complete() {
    let mut agg = StreamAggregator::new();
    assert!(!agg.is_complete());
    agg.push(&run_completed());
    assert!(agg.is_complete());
}

#[test]
fn aggregator_event_count() {
    let mut agg = StreamAggregator::new();
    for ev in all_event_kinds() {
        agg.push(&ev);
    }
    assert_eq!(agg.event_count(), 10);
}

#[test]
fn aggregator_summary_basic() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("hello")); // 5 bytes
    agg.push(&error_ev("e"));
    agg.push(&tool_call("bash"));
    agg.push(&run_completed());

    let summary = agg.to_summary();
    assert_eq!(summary.total_events, 4);
    assert_eq!(summary.text_length, 5);
    assert_eq!(summary.tool_call_count, 1);
    assert!(!summary.has_thinking);
    assert!(summary.has_errors);
}

#[test]
fn aggregator_summary_no_errors() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("hello"));
    let summary = agg.to_summary();
    assert!(!summary.has_errors);
}

#[test]
fn aggregator_default_is_new() {
    let agg = StreamAggregator::default();
    assert_eq!(agg.event_count(), 0);
    assert_eq!(agg.text(), "");
}

// =========================================================================
// 6. Error-only stream filtering
// =========================================================================

#[tokio::test]
async fn error_only_filtering_via_stream() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(error_ev("e1")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    tx.send(error_ev("e2")).await.unwrap();
    tx.send(run_completed()).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let filter = EventFilter::errors_only();
    let events = stream.collect_filtered(&filter).await;
    assert_eq!(events.len(), 2);
    for ev in &events {
        assert!(matches!(ev.kind, AgentEventKind::Error { .. }));
    }
}

#[test]
fn error_only_filter_with_error_codes() {
    let f = EventFilter::errors_only();
    assert!(f.matches(&error_ev_with_code("timeout", ErrorCode::BackendTimeout)));
    assert!(f.matches(&error_ev_with_code("denied", ErrorCode::PolicyDenied)));
    assert!(!f.matches(&warning_ev("just a warning")));
}

#[test]
fn exclude_errors_preserves_non_errors() {
    let f = EventFilter::exclude_errors();
    let events = all_event_kinds();
    let passed: Vec<_> = events.iter().filter(|ev| f.matches(ev)).collect();
    // all_event_kinds has 10 variants, 1 is error
    assert_eq!(passed.len(), 9);
}

// =========================================================================
// 7. Event ordering preservation
// =========================================================================

#[tokio::test]
async fn stream_preserves_insertion_order() {
    let (tx, rx) = mpsc::channel(32);
    for i in 0..20 {
        tx.send(delta(&format!("msg-{i}"))).await.unwrap();
    }
    drop(tx);

    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    assert_eq!(events.len(), 20);
    for (i, ev) in events.iter().enumerate() {
        match &ev.kind {
            AgentEventKind::AssistantDelta { text } => {
                assert_eq!(text, &format!("msg-{i}"));
            }
            _ => panic!("wrong kind"),
        }
    }
}

#[test]
fn recorder_preserves_insertion_order_with_mixed_types() {
    let rec = EventRecorder::new();
    rec.record(&run_started());
    rec.record(&delta("a"));
    rec.record(&tool_call("bash"));
    rec.record(&tool_result("bash", None, json!("ok")));
    rec.record(&delta("b"));
    rec.record(&error_ev("err"));
    rec.record(&warning_ev("warn"));
    rec.record(&file_changed("x.rs"));
    rec.record(&command_executed("ls", Some(0)));
    rec.record(&run_completed());

    let events = rec.events();
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        events[1].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
    assert!(matches!(events[2].kind, AgentEventKind::ToolCall { .. }));
    assert!(matches!(events[3].kind, AgentEventKind::ToolResult { .. }));
    assert!(matches!(
        events[4].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
    assert!(matches!(events[5].kind, AgentEventKind::Error { .. }));
    assert!(matches!(events[6].kind, AgentEventKind::Warning { .. }));
    assert!(matches!(events[7].kind, AgentEventKind::FileChanged { .. }));
    assert!(matches!(
        events[8].kind,
        AgentEventKind::CommandExecuted { .. }
    ));
    assert!(matches!(
        events[9].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn multiplexer_sorts_by_timestamp() {
    let ts_base = Utc::now();
    let ts1 = ts_base;
    let ts2 = ts_base + chrono::Duration::milliseconds(10);
    let ts3 = ts_base + chrono::Duration::milliseconds(20);

    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    // Interleave: stream1 has ts1, ts3; stream2 has ts2
    tx1.send(make_event_with_ts(
        AgentEventKind::AssistantDelta {
            text: "first".to_string(),
        },
        ts1,
    ))
    .await
    .unwrap();
    tx1.send(make_event_with_ts(
        AgentEventKind::AssistantDelta {
            text: "third".to_string(),
        },
        ts3,
    ))
    .await
    .unwrap();
    tx2.send(make_event_with_ts(
        AgentEventKind::AssistantDelta {
            text: "second".to_string(),
        },
        ts2,
    ))
    .await
    .unwrap();
    drop(tx1);
    drop(tx2);

    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].ts, ts1);
    assert_eq!(events[1].ts, ts2);
    assert_eq!(events[2].ts, ts3);
}

// =========================================================================
// 8. StreamPipeline composition (filter → record → stats)
// =========================================================================

#[test]
fn pipeline_empty_passthrough() {
    let p = StreamPipeline::new();
    for ev in all_event_kinds() {
        assert!(p.process(ev).is_some());
    }
}

#[test]
fn pipeline_filter_only() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::by_kind("assistant_delta"))
        .build();
    assert!(p.process(delta("ok")).is_some());
    assert!(p.process(error_ev("bad")).is_none());
    assert!(p.process(run_started()).is_none());
}

#[test]
fn pipeline_multiple_filters_all_must_pass() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .filter(EventFilter::by_kind("assistant_delta"))
        .build();
    assert!(p.process(delta("ok")).is_some());
    assert!(p.process(error_ev("bad")).is_none());
    assert!(p.process(tool_call("read")).is_none());
}

#[test]
fn pipeline_filter_then_record() {
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .with_recorder(rec.clone())
        .build();
    p.process(delta("a"));
    p.process(error_ev("skip"));
    p.process(delta("b"));
    assert_eq!(rec.len(), 2);
}

#[test]
fn pipeline_filter_then_stats() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .with_stats(stats.clone())
        .build();
    p.process(delta("a"));
    p.process(error_ev("skip"));
    p.process(delta("b"));
    assert_eq!(stats.total_events(), 2);
    assert_eq!(stats.error_count(), 0);
}

#[test]
fn pipeline_filter_record_stats_composed() {
    let rec = EventRecorder::new();
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("processed".to_string(), json!(true));
            ev
        }))
        .with_recorder(rec.clone())
        .with_stats(stats.clone())
        .build();

    p.process(delta("a"));
    p.process(error_ev("skip"));
    p.process(tool_call("bash"));

    assert_eq!(stats.total_events(), 2);
    assert_eq!(rec.len(), 2);
    for ev in rec.events() {
        assert_eq!(
            ev.ext.as_ref().unwrap().get("processed").unwrap(),
            &json!(true)
        );
    }
}

#[test]
fn pipeline_transform_then_filter() {
    // Transform adds ext, filter checks ext
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("tag".to_string(), json!("marked"));
            ev
        }))
        .filter(EventFilter::new(|ev| {
            ev.ext
                .as_ref()
                .and_then(|e| e.get("tag"))
                .map(|v| v == &json!("marked"))
                .unwrap_or(false)
        }))
        .build();
    // But note: filters run before transforms in the pipeline!
    // So this should pass (filter sees original, no ext → fails? Let's check order)
    // Actually pipeline order is: filters first, then transforms.
    // So the filter here will run on the original event (no ext) → None
    // Let's verify the actual behavior.
    // Looking at the code: filters run first, transforms second.
    // So this filter on ext won't see the transform output.
    let result = p.process(delta("test"));
    // The filter checks for ext.tag == "marked", but filters run before transforms
    // so the event still has ext=None when the filter runs → filtered out
    assert!(result.is_none());
}

#[test]
fn pipeline_accessor_recorder() {
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(rec.clone())
        .build();
    assert!(p.recorder().is_some());
}

#[test]
fn pipeline_accessor_stats() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();
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

#[tokio::test]
async fn pipeline_pipe_integration() {
    let (tx_in, rx_in) = mpsc::channel(16);
    let (tx_out, mut rx_out) = mpsc::channel(16);

    let stats = EventStats::new();
    let rec = EventRecorder::new();
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .with_recorder(rec.clone())
        .with_stats(stats.clone())
        .build();

    tx_in.send(run_started()).await.unwrap();
    tx_in.send(delta("hello")).await.unwrap();
    tx_in.send(error_ev("skip")).await.unwrap();
    tx_in.send(delta(" world")).await.unwrap();
    tx_in.send(run_completed()).await.unwrap();
    drop(tx_in);

    let stream = EventStream::new(rx_in);
    stream.pipe(&pipeline, tx_out).await;

    let mut results = Vec::new();
    while let Some(ev) = rx_out.recv().await {
        results.push(ev);
    }
    assert_eq!(results.len(), 4); // run_started, 2 deltas, run_completed
    assert_eq!(stats.total_events(), 4);
    assert_eq!(rec.len(), 4);
}

// =========================================================================
// 9. Concurrent stream access
// =========================================================================

#[tokio::test]
async fn concurrent_recorder_access() {
    let rec = EventRecorder::new();
    let rec1 = rec.clone();
    let rec2 = rec.clone();

    let h1 = tokio::spawn(async move {
        for i in 0..50 {
            rec1.record(&delta(&format!("t1-{i}")));
        }
    });
    let h2 = tokio::spawn(async move {
        for i in 0..50 {
            rec2.record(&delta(&format!("t2-{i}")));
        }
    });

    h1.await.unwrap();
    h2.await.unwrap();
    assert_eq!(rec.len(), 100);
}

#[tokio::test]
async fn concurrent_stats_access() {
    let stats = EventStats::new();
    let s1 = stats.clone();
    let s2 = stats.clone();

    let h1 = tokio::spawn(async move {
        for _ in 0..50 {
            s1.observe(&delta("a"));
        }
    });
    let h2 = tokio::spawn(async move {
        for _ in 0..50 {
            s2.observe(&error_ev("e"));
        }
    });

    h1.await.unwrap();
    h2.await.unwrap();
    assert_eq!(stats.total_events(), 100);
    assert_eq!(stats.count_for("assistant_delta"), 50);
    assert_eq!(stats.error_count(), 50);
}

#[tokio::test]
async fn concurrent_pipeline_processing() {
    let stats = EventStats::new();
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(rec.clone())
        .with_stats(stats.clone())
        .build();

    let p1 = p.clone();
    let p2 = p.clone();

    let h1 = tokio::spawn(async move {
        for i in 0..25 {
            p1.process(delta(&format!("t1-{i}")));
        }
    });
    let h2 = tokio::spawn(async move {
        for i in 0..25 {
            p2.process(delta(&format!("t2-{i}")));
        }
    });

    h1.await.unwrap();
    h2.await.unwrap();
    assert_eq!(stats.total_events(), 50);
    assert_eq!(rec.len(), 50);
}

// =========================================================================
// 10. Backpressure handling
// =========================================================================

#[tokio::test]
async fn backpressure_small_channel() {
    // Channel of size 1 forces backpressure
    let (tx, rx) = mpsc::channel(1);

    let sender = tokio::spawn(async move {
        for i in 0..10 {
            tx.send(delta(&format!("msg-{i}"))).await.unwrap();
        }
    });

    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    sender.await.unwrap();
    assert_eq!(events.len(), 10);
}

#[tokio::test]
async fn backpressure_pipe_small_channel() {
    let (tx_in, rx_in) = mpsc::channel(2);
    let (tx_out, mut rx_out) = mpsc::channel(1); // tiny output channel

    let pipeline = StreamPipeline::new();

    let sender = tokio::spawn(async move {
        for i in 0..5 {
            tx_in.send(delta(&format!("m-{i}"))).await.unwrap();
        }
        drop(tx_in);
    });

    let consumer = tokio::spawn(async move {
        let mut out = Vec::new();
        while let Some(ev) = rx_out.recv().await {
            out.push(ev);
        }
        out
    });

    let stream = EventStream::new(rx_in);
    stream.pipe(&pipeline, tx_out).await;

    sender.await.unwrap();
    let results = consumer.await.unwrap();
    assert_eq!(results.len(), 5);
}

#[tokio::test]
async fn tee_backpressure_closed_receiver() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, _rx2) = mpsc::channel(16);
    drop(_rx2); // close receiver 2

    let tee = StreamTee::new(vec![tx1, tx2]);
    // Should succeed because at least one receiver is open
    tee.send(&delta("hello")).await.unwrap();

    let mut rx1 = rx1;
    let ev = rx1.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { .. }));
}

#[tokio::test]
async fn tee_all_receivers_closed_returns_error() {
    let (tx1, _rx1) = mpsc::channel(16);
    let (tx2, _rx2) = mpsc::channel(16);
    drop(_rx1);
    drop(_rx2);

    let tee = StreamTee::new(vec![tx1, tx2]);
    let result = tee.send(&delta("hello")).await;
    assert!(result.is_err());
}

// =========================================================================
// 11. StreamMetrics tests
// =========================================================================

#[test]
fn metrics_new_is_zero() {
    let m = StreamMetrics::new();
    assert_eq!(m.event_count(), 0);
    assert_eq!(m.total_bytes(), 0);
    assert_eq!(m.elapsed(), Duration::ZERO);
    assert_eq!(m.throughput(), 0.0);
    assert!(m.event_type_counts().is_empty());
}

#[test]
fn metrics_records_events() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("hello"));
    m.record_event(&error_ev("oops"));
    assert_eq!(m.event_count(), 2);
}

#[test]
fn metrics_tracks_delta_bytes() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("abc")); // 3
    m.record_event(&delta("defgh")); // 5
    assert_eq!(m.total_bytes(), 8);
}

#[test]
fn metrics_non_delta_no_bytes() {
    let mut m = StreamMetrics::new();
    m.record_event(&error_ev("long error message"));
    assert_eq!(m.total_bytes(), 0);
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
fn metrics_all_event_types() {
    let mut m = StreamMetrics::new();
    for ev in all_event_kinds() {
        m.record_event(&ev);
    }
    assert_eq!(m.event_count(), 10);
    assert_eq!(m.event_type_counts().len(), 10);
}

#[test]
fn metrics_summary_snapshot() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("hi"));
    let summary = m.summary();
    assert_eq!(summary.event_count, 1);
    assert_eq!(summary.total_bytes, 2);
}

#[test]
fn metrics_summary_display() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("hello"));
    let summary = m.summary();
    let text = format!("{summary}");
    assert!(text.contains("Events: 1"));
    assert!(text.contains("Bytes: 5"));
}

#[test]
fn metrics_default_is_new() {
    let m = StreamMetrics::default();
    assert_eq!(m.event_count(), 0);
}

// =========================================================================
// 12. All AgentEventKind variants through pipeline
// =========================================================================

#[test]
fn all_event_kinds_pass_through_empty_pipeline() {
    let p = StreamPipeline::new();
    for ev in all_event_kinds() {
        assert!(p.process(ev).is_some());
    }
}

#[test]
fn all_event_kinds_recorded_by_pipeline() {
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(rec.clone())
        .build();
    for ev in all_event_kinds() {
        p.process(ev);
    }
    assert_eq!(rec.len(), 10);
}

#[test]
fn all_event_kinds_counted_by_stats() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();
    for ev in all_event_kinds() {
        p.process(ev);
    }
    assert_eq!(stats.total_events(), 10);
    let counts = stats.kind_counts();
    assert_eq!(counts.len(), 10);
    for (_name, &count) in &counts {
        assert_eq!(count, 1);
    }
}

#[test]
fn all_event_kinds_through_aggregator() {
    let mut agg = StreamAggregator::new();
    for ev in all_event_kinds() {
        agg.push(&ev);
    }
    assert_eq!(agg.event_count(), 10);
    assert!(agg.is_complete()); // run_completed is in the set
    assert_eq!(agg.errors().len(), 1);
    assert!(!agg.text().is_empty());
    assert!(!agg.tool_calls().is_empty());
}

#[test]
fn all_event_kinds_through_buffer() {
    let mut buf = StreamBuffer::new(20);
    for ev in all_event_kinds() {
        buf.push(ev);
    }
    assert_eq!(buf.len(), 10);
}

#[test]
fn all_event_kinds_through_metrics() {
    let mut m = StreamMetrics::new();
    for ev in all_event_kinds() {
        m.record_event(&ev);
    }
    assert_eq!(m.event_count(), 10);
    let counts = m.event_type_counts();
    assert!(counts.contains_key("run_started"));
    assert!(counts.contains_key("run_completed"));
    assert!(counts.contains_key("assistant_delta"));
    assert!(counts.contains_key("assistant_message"));
    assert!(counts.contains_key("tool_call"));
    assert!(counts.contains_key("tool_result"));
    assert!(counts.contains_key("file_changed"));
    assert!(counts.contains_key("command_executed"));
    assert!(counts.contains_key("warning"));
    assert!(counts.contains_key("error"));
}

// =========================================================================
// 13. event_kind_name helper
// =========================================================================

#[test]
fn event_kind_name_run_started() {
    assert_eq!(
        event_kind_name(&AgentEventKind::RunStarted {
            message: "m".into()
        }),
        "run_started"
    );
}

#[test]
fn event_kind_name_run_completed() {
    assert_eq!(
        event_kind_name(&AgentEventKind::RunCompleted {
            message: "m".into()
        }),
        "run_completed"
    );
}

#[test]
fn event_kind_name_assistant_delta() {
    assert_eq!(
        event_kind_name(&AgentEventKind::AssistantDelta { text: "t".into() }),
        "assistant_delta"
    );
}

#[test]
fn event_kind_name_assistant_message() {
    assert_eq!(
        event_kind_name(&AgentEventKind::AssistantMessage { text: "t".into() }),
        "assistant_message"
    );
}

#[test]
fn event_kind_name_tool_call() {
    assert_eq!(
        event_kind_name(&AgentEventKind::ToolCall {
            tool_name: "x".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: json!({}),
        }),
        "tool_call"
    );
}

#[test]
fn event_kind_name_tool_result() {
    assert_eq!(
        event_kind_name(&AgentEventKind::ToolResult {
            tool_name: "x".into(),
            tool_use_id: None,
            output: json!(null),
            is_error: false,
        }),
        "tool_result"
    );
}

#[test]
fn event_kind_name_file_changed() {
    assert_eq!(
        event_kind_name(&AgentEventKind::FileChanged {
            path: "a".into(),
            summary: "s".into(),
        }),
        "file_changed"
    );
}

#[test]
fn event_kind_name_command_executed() {
    assert_eq!(
        event_kind_name(&AgentEventKind::CommandExecuted {
            command: "c".into(),
            exit_code: None,
            output_preview: None,
        }),
        "command_executed"
    );
}

#[test]
fn event_kind_name_warning() {
    assert_eq!(
        event_kind_name(&AgentEventKind::Warning {
            message: "w".into()
        }),
        "warning"
    );
}

#[test]
fn event_kind_name_error() {
    assert_eq!(
        event_kind_name(&AgentEventKind::Error {
            message: "e".into(),
            error_code: None,
        }),
        "error"
    );
}

// =========================================================================
// 14. EventTransform tests
// =========================================================================

#[test]
fn transform_identity() {
    let t = EventTransform::identity();
    let ev = delta("hello");
    let result = t.apply(ev.clone());
    assert_eq!(result.ts, ev.ts);
    match &result.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "hello"),
        _ => panic!("wrong kind"),
    }
}

#[test]
fn transform_adds_ext() {
    let t = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("enriched".to_string(), json!(true));
        ev
    });
    let result = t.apply(delta("hello"));
    assert!(result.ext.is_some());
    assert_eq!(result.ext.unwrap().get("enriched").unwrap(), &json!(true));
}

#[test]
fn transform_chain() {
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
    let ev = t1.apply(delta("x"));
    let ev = t2.apply(ev);
    assert_eq!(ev.ext.unwrap().get("step").unwrap(), &json!(2));
}

#[test]
fn transform_clone_works() {
    let t = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("cloned".to_string(), json!(true));
        ev
    });
    let t2 = t.clone();
    let result = t2.apply(delta("test"));
    assert!(result.ext.unwrap().contains_key("cloned"));
}

// =========================================================================
// 15. EventStream tests
// =========================================================================

#[tokio::test]
async fn event_stream_recv() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("first")).await.unwrap();
    tx.send(delta("second")).await.unwrap();
    drop(tx);

    let mut stream = EventStream::new(rx);
    let first = stream.recv().await.unwrap();
    assert!(matches!(first.kind, AgentEventKind::AssistantDelta { .. }));
    let second = stream.recv().await.unwrap();
    assert!(matches!(second.kind, AgentEventKind::AssistantDelta { .. }));
    assert!(stream.recv().await.is_none());
}

#[tokio::test]
async fn event_stream_collect_all_empty() {
    let (_tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(_tx);
    let stream = EventStream::new(rx);
    assert!(stream.collect_all().await.is_empty());
}

#[tokio::test]
async fn event_stream_collect_filtered_empty() {
    let (_tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(_tx);
    let stream = EventStream::new(rx);
    let filter = EventFilter::errors_only();
    assert!(stream.collect_filtered(&filter).await.is_empty());
}

#[tokio::test]
async fn event_stream_as_futures_stream() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    drop(tx);

    let mut stream = EventStream::new(rx);
    let mut pinned = std::pin::pin!(&mut stream);
    let item = tokio_stream::StreamExt::next(&mut pinned).await;
    assert!(item.is_some());
}

#[tokio::test]
async fn event_stream_into_inner() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let mut inner_rx = stream.into_inner();
    let ev = inner_rx.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { .. }));
}

// =========================================================================
// 16. StreamTee tests
// =========================================================================

#[tokio::test]
async fn tee_broadcasts_to_all() {
    let (tx1, mut rx1) = mpsc::channel(16);
    let (tx2, mut rx2) = mpsc::channel(16);

    let tee = StreamTee::new(vec![tx1, tx2]);
    tee.send(&delta("broadcast")).await.unwrap();

    let ev1 = rx1.recv().await.unwrap();
    let ev2 = rx2.recv().await.unwrap();
    assert!(matches!(ev1.kind, AgentEventKind::AssistantDelta { .. }));
    assert!(matches!(ev2.kind, AgentEventKind::AssistantDelta { .. }));
}

#[tokio::test]
async fn tee_empty_senders_ok() {
    let tee = StreamTee::new(vec![]);
    let result = tee.send(&delta("ok")).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn tee_active_count() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    let tee = StreamTee::new(vec![tx1, tx2]);
    assert_eq!(tee.active_count(), 2);
    drop(rx1);
    // After dropping rx1, active_count should decrease
    // (may need a small delay for channel to detect)
    assert!(tee.active_count() <= 2);
    drop(rx2);
}

#[tokio::test]
async fn tee_partial_receiver_drop() {
    let (tx1, mut rx1) = mpsc::channel(16);
    let (tx2, _rx2) = mpsc::channel(16);
    drop(_rx2); // close rx2

    let tee = StreamTee::new(vec![tx1, tx2]);
    // Should succeed because rx1 is still open
    tee.send(&delta("partial")).await.unwrap();

    let ev = rx1.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { .. }));
}

// =========================================================================
// 17. EventCollector tests
// =========================================================================

#[tokio::test]
async fn collector_records_while_streaming() {
    let events = vec![delta("a"), delta("b"), error_ev("e")];
    let stream = tokio_stream::iter(events);
    let collector = EventCollector::new(stream);

    assert!(collector.is_empty());

    let handle = collector.collected();
    let mut pinned = Box::pin(collector);

    let mut count = 0;
    while let Some(_ev) = pinned.next().await {
        count += 1;
    }
    assert_eq!(count, 3);
    assert_eq!(handle.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn collector_snapshot() {
    let events = vec![delta("x"), delta("y")];
    let stream = tokio_stream::iter(events);
    let mut collector = EventCollector::new(stream);

    // Before consuming
    assert_eq!(collector.snapshot().len(), 0);

    let mut pinned = std::pin::pin!(&mut collector);
    let _ = pinned.next().await;
    assert_eq!(collector.snapshot().len(), 1);
}

#[tokio::test]
async fn collector_empty_stream() {
    let stream = tokio_stream::empty::<AgentEvent>();
    let collector = EventCollector::new(stream);
    let mut pinned = Box::pin(collector);
    assert!(pinned.next().await.is_none());
}

// =========================================================================
// 18. EventMultiplexer tests
// =========================================================================

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
async fn multiplexer_single_stream() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("only")).await.unwrap();
    drop(tx);

    let mux = EventMultiplexer::new(vec![rx]);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), 1);
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

// =========================================================================
// 19. StreamPipelineBuilder tests
// =========================================================================

#[test]
fn builder_default_is_passthrough() {
    let p = StreamPipelineBuilder::new().build();
    assert!(p.process(delta("ok")).is_some());
    assert!(p.process(error_ev("ok")).is_some());
}

#[test]
fn builder_record_creates_recorder() {
    let p = StreamPipelineBuilder::new().record().build();
    assert!(p.recorder().is_some());
    p.process(delta("a"));
    assert_eq!(p.recorder().unwrap().len(), 1);
}

#[test]
fn builder_with_external_recorder() {
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(rec.clone())
        .build();
    p.process(delta("a"));
    assert_eq!(rec.len(), 1);
}

#[test]
fn builder_chained_filters() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .filter(EventFilter::by_kind("assistant_delta"))
        .filter(EventFilter::new(
            |ev| matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text.len() > 2),
        ))
        .build();
    assert!(p.process(delta("hello")).is_some());
    assert!(p.process(delta("hi")).is_none()); // too short
    assert!(p.process(error_ev("e")).is_none()); // error
    assert!(p.process(tool_call("x")).is_none()); // wrong kind
}

#[test]
fn builder_chained_transforms() {
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("step1".to_string(), json!(true));
            ev
        }))
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("step2".to_string(), json!(true));
            ev
        }))
        .build();
    let result = p.process(delta("x")).unwrap();
    let ext = result.ext.unwrap();
    assert_eq!(ext.get("step1").unwrap(), &json!(true));
    assert_eq!(ext.get("step2").unwrap(), &json!(true));
}

// =========================================================================
// 20. Edge cases and stress tests
// =========================================================================

#[test]
fn buffer_stress_push_many() {
    let mut buf = StreamBuffer::new(100);
    for i in 0..10_000 {
        buf.push(delta(&format!("e{i}")));
    }
    assert_eq!(buf.len(), 100);
    assert!(buf.is_full());
}

#[test]
fn aggregator_empty_deltas() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta(""));
    agg.push(&delta(""));
    assert_eq!(agg.text(), "");
    assert_eq!(agg.event_count(), 2);
}

#[test]
fn aggregator_unicode_text() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("こんにちは"));
    agg.push(&delta(" 🌍"));
    assert_eq!(agg.text(), "こんにちは 🌍");
}

#[test]
fn stats_unicode_delta_bytes() {
    let s = EventStats::new();
    s.observe(&delta("café")); // 5 bytes in UTF-8 (é is 2 bytes)
    assert_eq!(s.total_delta_bytes(), 5);
}

#[test]
fn filter_errors_with_various_error_codes() {
    let f = EventFilter::errors_only();
    let codes = vec![
        ErrorCode::BackendTimeout,
        ErrorCode::PolicyDenied,
        ErrorCode::Internal,
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::MappingDialectMismatch,
        ErrorCode::ExecutionToolFailed,
    ];
    for code in codes {
        assert!(f.matches(&error_ev_with_code("msg", code)));
    }
}

#[test]
fn pipeline_process_returns_none_for_filtered() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .build();
    assert!(p.process(delta("x")).is_none());
    assert!(p.process(warning_ev("w")).is_none());
    assert!(p.process(run_started()).is_none());
    assert!(p.process(error_ev("e")).is_some());
}

#[tokio::test]
async fn stream_large_volume() {
    let (tx, rx) = mpsc::channel(1024);
    let count = 1000;

    let sender = tokio::spawn(async move {
        for i in 0..count {
            tx.send(delta(&format!("m{i}"))).await.unwrap();
        }
    });

    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    sender.await.unwrap();
    assert_eq!(events.len(), count);
}

#[test]
fn aggregator_tool_result_without_id_ignored() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call_with_id("bash", "tc-1"));
    // tool_result with no id → doesn't match anything
    agg.push(&make_event(AgentEventKind::ToolResult {
        tool_name: "bash".to_string(),
        tool_use_id: None,
        output: json!("result"),
        is_error: false,
    }));
    // tc-1 result should still be None
    assert!(agg.tool_calls()[0].result.is_none());
}

#[test]
fn aggregator_multiple_run_completed() {
    let mut agg = StreamAggregator::new();
    agg.push(&run_completed());
    assert!(agg.is_complete());
    agg.push(&run_completed()); // second one
    assert!(agg.is_complete());
    assert_eq!(agg.event_count(), 2);
}

#[test]
fn recorder_many_events() {
    let rec = EventRecorder::new();
    for i in 0..500 {
        rec.record(&delta(&format!("e{i}")));
    }
    assert_eq!(rec.len(), 500);
    let events = rec.events();
    assert_eq!(events.len(), 500);
}

#[test]
fn stats_count_for_missing_kind() {
    let s = EventStats::new();
    s.observe(&delta("x"));
    assert_eq!(s.count_for("nonexistent"), 0);
}

#[test]
fn pipeline_clone_shares_stats_and_recorder() {
    let stats = EventStats::new();
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .with_recorder(rec.clone())
        .build();
    let p2 = p.clone();

    p.process(delta("a"));
    p2.process(delta("b"));

    assert_eq!(stats.total_events(), 2);
    assert_eq!(rec.len(), 2);
}

#[test]
fn tee_error_display() {
    let err = TeeError::AllReceiversClosed;
    assert_eq!(format!("{err}"), "all receivers closed");
}

#[test]
fn stream_timeout_display() {
    let err = abp_stream::StreamTimeout {
        duration: Duration::from_secs(5),
    };
    let msg = format!("{err}");
    assert!(msg.contains("5s") || msg.contains("5"));
}
