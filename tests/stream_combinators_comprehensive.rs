#![allow(clippy::all)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unreachable_code)]

//! Comprehensive tests for abp-stream crate streaming abstractions and combinators.

use std::collections::BTreeMap;
use std::pin::Pin;
use std::time::Duration;

use abp_core::{AgentEvent, AgentEventKind};
use chrono::{DateTime, Utc};
use tokio::sync::mpsc;
use tokio_stream::{Stream, StreamExt};

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
        input: serde_json::json!({"key": "value"}),
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

fn command_executed(cmd: &str) -> AgentEvent {
    make_event(AgentEventKind::CommandExecuted {
        command: cmd.to_string(),
        exit_code: Some(0),
        output_preview: None,
    })
}

// ===========================================================================
// StreamBuffer tests (17 tests)
// ===========================================================================

#[test]
fn buffer_new_is_empty() {
    let buf = StreamBuffer::new(10);
    assert!(buf.is_empty());
    assert_eq!(buf.len(), 0);
}

#[test]
fn buffer_accumulates_events() {
    let mut buf = StreamBuffer::new(10);
    buf.push(delta("a"));
    buf.push(delta("b"));
    buf.push(delta("c"));
    assert_eq!(buf.len(), 3);
    assert!(!buf.is_empty());
}

#[test]
fn buffer_drains_in_order() {
    let mut buf = StreamBuffer::new(10);
    buf.push(delta("first"));
    buf.push(delta("second"));
    buf.push(delta("third"));
    let drained = buf.drain();
    assert_eq!(drained.len(), 3);
    assert!(matches!(&drained[0].kind, AgentEventKind::AssistantDelta { text } if text == "first"));
    assert!(
        matches!(&drained[1].kind, AgentEventKind::AssistantDelta { text } if text == "second")
    );
    assert!(matches!(&drained[2].kind, AgentEventKind::AssistantDelta { text } if text == "third"));
}

#[test]
fn buffer_capacity_returns_max() {
    let buf = StreamBuffer::new(42);
    assert_eq!(buf.capacity(), 42);
}

#[test]
fn buffer_capacity_limits_evict_oldest() {
    let mut buf = StreamBuffer::new(3);
    buf.push(delta("a"));
    buf.push(delta("b"));
    buf.push(delta("c"));
    assert!(buf.is_full());
    buf.push(delta("d"));
    assert_eq!(buf.len(), 3);
    let events = buf.drain();
    assert!(matches!(&events[0].kind, AgentEventKind::AssistantDelta { text } if text == "b"));
    assert!(matches!(&events[2].kind, AgentEventKind::AssistantDelta { text } if text == "d"));
}

#[test]
fn buffer_empty_drain_returns_empty_vec() {
    let mut buf = StreamBuffer::new(5);
    let drained = buf.drain();
    assert!(drained.is_empty());
}

#[test]
fn buffer_with_single_event() {
    let mut buf = StreamBuffer::new(5);
    buf.push(error_ev("oops"));
    assert_eq!(buf.len(), 1);
    assert!(!buf.is_full());
}

#[test]
fn buffer_with_many_events_beyond_capacity() {
    let mut buf = StreamBuffer::new(5);
    for i in 0..100 {
        buf.push(delta(&format!("event-{i}")));
    }
    assert_eq!(buf.len(), 5);
    assert!(buf.is_full());
    let events = buf.drain();
    assert!(
        matches!(&events[0].kind, AgentEventKind::AssistantDelta { text } if text == "event-95")
    );
    assert!(
        matches!(&events[4].kind, AgentEventKind::AssistantDelta { text } if text == "event-99")
    );
}

#[test]
fn buffer_recent_returns_last_n() {
    let mut buf = StreamBuffer::new(10);
    for i in 0..7 {
        buf.push(delta(&format!("{i}")));
    }
    let recent = buf.recent(3);
    assert_eq!(recent.len(), 3);
    assert!(matches!(&recent[0].kind, AgentEventKind::AssistantDelta { text } if text == "4"));
}

#[test]
fn buffer_recent_more_than_stored() {
    let mut buf = StreamBuffer::new(10);
    buf.push(delta("only"));
    let recent = buf.recent(100);
    assert_eq!(recent.len(), 1);
}

#[test]
fn buffer_drain_empties_buffer() {
    let mut buf = StreamBuffer::new(10);
    buf.push(delta("a"));
    buf.push(delta("b"));
    let _ = buf.drain();
    assert!(buf.is_empty());
    assert_eq!(buf.len(), 0);
}

#[test]
fn buffer_is_full_exact_capacity() {
    let mut buf = StreamBuffer::new(2);
    assert!(!buf.is_full());
    buf.push(delta("a"));
    assert!(!buf.is_full());
    buf.push(delta("b"));
    assert!(buf.is_full());
}

#[test]
#[should_panic(expected = "StreamBuffer capacity must be > 0")]
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
    let events = buf.drain();
    assert!(matches!(&events[0].kind, AgentEventKind::AssistantDelta { text } if text == "b"));
}

#[test]
fn buffer_mixed_event_types() {
    let mut buf = StreamBuffer::new(10);
    buf.push(run_started());
    buf.push(delta("hello"));
    buf.push(tool_call("read"));
    buf.push(error_ev("fail"));
    buf.push(run_completed());
    assert_eq!(buf.len(), 5);
}

#[test]
fn buffer_drain_then_push_again() {
    let mut buf = StreamBuffer::new(5);
    buf.push(delta("first"));
    let _ = buf.drain();
    buf.push(delta("second"));
    assert_eq!(buf.len(), 1);
    let events = buf.drain();
    assert!(matches!(&events[0].kind, AgentEventKind::AssistantDelta { text } if text == "second"));
}

#[test]
fn buffer_recent_zero() {
    let mut buf = StreamBuffer::new(5);
    buf.push(delta("a"));
    let recent = buf.recent(0);
    assert!(recent.is_empty());
}

// ===========================================================================
// EventFilter tests (18 tests)
// ===========================================================================

#[test]
fn filter_by_kind_assistant_delta() {
    let filter = EventFilter::by_kind("assistant_delta");
    assert!(filter.matches(&delta("hello")));
    assert!(!filter.matches(&error_ev("err")));
}

#[test]
fn filter_by_kind_tool_call() {
    let filter = EventFilter::by_kind("tool_call");
    assert!(filter.matches(&tool_call("read_file")));
    assert!(!filter.matches(&delta("text")));
}

#[test]
fn filter_by_kind_error() {
    let filter = EventFilter::by_kind("error");
    assert!(filter.matches(&error_ev("bad")));
    assert!(!filter.matches(&warning_ev("warn")));
}

#[test]
fn filter_by_kind_run_started() {
    let filter = EventFilter::by_kind("run_started");
    assert!(filter.matches(&run_started()));
    assert!(!filter.matches(&run_completed()));
}

#[test]
fn filter_by_kind_run_completed() {
    let filter = EventFilter::by_kind("run_completed");
    assert!(filter.matches(&run_completed()));
    assert!(!filter.matches(&run_started()));
}

#[test]
fn filter_by_kind_warning() {
    let filter = EventFilter::by_kind("warning");
    assert!(filter.matches(&warning_ev("caution")));
    assert!(!filter.matches(&error_ev("bad")));
}

#[test]
fn filter_by_kind_file_changed() {
    let filter = EventFilter::by_kind("file_changed");
    assert!(filter.matches(&file_changed("src/main.rs")));
    assert!(!filter.matches(&delta("text")));
}

#[test]
fn filter_by_kind_command_executed() {
    let filter = EventFilter::by_kind("command_executed");
    assert!(filter.matches(&command_executed("ls")));
    assert!(!filter.matches(&delta("text")));
}

#[test]
fn filter_passes_matching_events() {
    let filter = EventFilter::by_kind("assistant_delta");
    let events = vec![delta("a"), error_ev("e"), delta("b"), warning_ev("w")];
    let matched: Vec<_> = events.iter().filter(|e| filter.matches(e)).collect();
    assert_eq!(matched.len(), 2);
}

#[test]
fn filter_blocks_non_matching_events() {
    let filter = EventFilter::by_kind("tool_call");
    assert!(!filter.matches(&delta("text")));
    assert!(!filter.matches(&error_ev("err")));
    assert!(!filter.matches(&run_started()));
    assert!(!filter.matches(&warning_ev("w")));
}

#[test]
fn filter_errors_only() {
    let filter = EventFilter::errors_only();
    assert!(filter.matches(&error_ev("bad")));
    assert!(!filter.matches(&delta("ok")));
    assert!(!filter.matches(&warning_ev("warn")));
    assert!(!filter.matches(&run_started()));
}

#[test]
fn filter_exclude_errors() {
    let filter = EventFilter::exclude_errors();
    assert!(!filter.matches(&error_ev("bad")));
    assert!(filter.matches(&delta("ok")));
    assert!(filter.matches(&warning_ev("warn")));
    assert!(filter.matches(&run_started()));
}

#[test]
fn filter_chained_and_behavior() {
    let f1 = EventFilter::exclude_errors();
    let f2 = EventFilter::by_kind("assistant_delta");
    // Both filters must pass (AND semantics)
    let events = vec![delta("a"), error_ev("e"), warning_ev("w"), delta("b")];
    let matched: Vec<_> = events
        .iter()
        .filter(|e| f1.matches(e) && f2.matches(e))
        .collect();
    assert_eq!(matched.len(), 2);
}

#[test]
fn filter_with_closure_text_length() {
    let filter = EventFilter::new(
        |ev| matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text.len() > 5),
    );
    assert!(filter.matches(&delta("long text")));
    assert!(!filter.matches(&delta("hi")));
    assert!(!filter.matches(&error_ev("err")));
}

#[test]
fn filter_with_closure_ext_present() {
    let filter = EventFilter::new(|ev| ev.ext.is_some());
    assert!(!filter.matches(&delta("no ext")));
    let mut ev = delta("with ext");
    ev.ext = Some(BTreeMap::new());
    assert!(filter.matches(&ev));
}

#[test]
fn filter_nonexistent_kind_matches_nothing() {
    let filter = EventFilter::by_kind("nonexistent_kind");
    assert!(!filter.matches(&delta("text")));
    assert!(!filter.matches(&error_ev("err")));
    assert!(!filter.matches(&run_started()));
}

#[test]
fn filter_by_kind_assistant_message() {
    let filter = EventFilter::by_kind("assistant_message");
    assert!(filter.matches(&message("hello")));
    assert!(!filter.matches(&delta("hello")));
}

#[test]
fn filter_by_kind_tool_result() {
    let filter = EventFilter::by_kind("tool_result");
    assert!(filter.matches(&tool_result("read", "id1", "output")));
    assert!(!filter.matches(&tool_call("read")));
}

// ===========================================================================
// EventTransform tests (17 tests)
// ===========================================================================

#[test]
fn transform_identity_preserves_event() {
    let t = EventTransform::identity();
    let ev = delta("hello");
    let ts = ev.ts;
    let result = t.apply(ev);
    assert_eq!(result.ts, ts);
    assert!(matches!(&result.kind, AgentEventKind::AssistantDelta { text } if text == "hello"));
}

#[test]
fn transform_modifies_event_in_flight() {
    let t = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("tag".to_string(), serde_json::json!("transformed"));
        ev
    });
    let result = t.apply(delta("hello"));
    assert!(result.ext.is_some());
    assert_eq!(
        result.ext.unwrap().get("tag").unwrap(),
        &serde_json::json!("transformed")
    );
}

#[test]
fn transform_preserves_event_ordering() {
    let t = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("processed".to_string(), serde_json::json!(true));
        ev
    });
    let events: Vec<_> = (0..5).map(|i| delta(&format!("msg-{i}"))).collect();
    let transformed: Vec<_> = events.into_iter().map(|ev| t.apply(ev)).collect();
    for (i, ev) in transformed.iter().enumerate() {
        assert!(
            matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text == &format!("msg-{i}"))
        );
    }
}

#[test]
fn transform_identity_returns_same_kind() {
    let t = EventTransform::identity();
    let ev = error_ev("oops");
    let result = t.apply(ev);
    assert!(matches!(result.kind, AgentEventKind::Error { .. }));
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
    let ev = t2.apply(t1.apply(delta("x")));
    assert_eq!(ev.ext.unwrap().get("step").unwrap(), &serde_json::json!(2));
}

#[test]
fn transform_adds_metadata() {
    let t = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("source".to_string(), serde_json::json!("test"));
        ext.insert("version".to_string(), serde_json::json!(1));
        ev
    });
    let result = t.apply(delta("hello"));
    let ext = result.ext.unwrap();
    assert_eq!(ext.len(), 2);
    assert_eq!(ext.get("source").unwrap(), &serde_json::json!("test"));
    assert_eq!(ext.get("version").unwrap(), &serde_json::json!(1));
}

#[test]
fn transform_on_error_event() {
    let t = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("handled".to_string(), serde_json::json!(true));
        ev
    });
    let result = t.apply(error_ev("fail"));
    assert!(matches!(result.kind, AgentEventKind::Error { .. }));
    assert_eq!(
        result.ext.unwrap().get("handled").unwrap(),
        &serde_json::json!(true)
    );
}

#[test]
fn transform_on_tool_call() {
    let t = EventTransform::identity();
    let result = t.apply(tool_call("read_file"));
    assert!(matches!(result.kind, AgentEventKind::ToolCall { .. }));
}

#[test]
fn transform_on_run_started() {
    let t = EventTransform::identity();
    let result = t.apply(run_started());
    assert!(matches!(result.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn transform_on_run_completed() {
    let t = EventTransform::identity();
    let result = t.apply(run_completed());
    assert!(matches!(result.kind, AgentEventKind::RunCompleted { .. }));
}

#[test]
fn transform_on_warning() {
    let t = EventTransform::identity();
    let result = t.apply(warning_ev("w"));
    assert!(matches!(result.kind, AgentEventKind::Warning { .. }));
}

#[test]
fn transform_on_file_changed() {
    let t = EventTransform::identity();
    let result = t.apply(file_changed("test.rs"));
    assert!(matches!(result.kind, AgentEventKind::FileChanged { .. }));
}

#[test]
fn transform_preserves_timestamp() {
    let ts = Utc::now();
    let ev = make_event_with_ts(
        AgentEventKind::AssistantDelta {
            text: "x".to_string(),
        },
        ts,
    );
    let t = EventTransform::new(|mut ev| {
        ev.ext = Some(BTreeMap::new());
        ev
    });
    let result = t.apply(ev);
    assert_eq!(result.ts, ts);
}

#[test]
fn transform_accumulates_metadata_across_chain() {
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
    let result = t2.apply(t1.apply(delta("x")));
    let ext = result.ext.unwrap();
    assert_eq!(ext.get("a").unwrap(), &serde_json::json!(1));
    assert_eq!(ext.get("b").unwrap(), &serde_json::json!(2));
}

#[test]
fn transform_clone_works() {
    let t = EventTransform::new(|mut ev| {
        ev.ext = Some(BTreeMap::new());
        ev
    });
    let t2 = t.clone();
    let r1 = t.apply(delta("a"));
    let r2 = t2.apply(delta("b"));
    assert!(r1.ext.is_some());
    assert!(r2.ext.is_some());
}

#[test]
fn transform_debug_impl() {
    let t = EventTransform::identity();
    let debug = format!("{:?}", t);
    assert!(debug.contains("EventTransform"));
}

#[test]
fn transform_on_command_executed() {
    let t = EventTransform::identity();
    let result = t.apply(command_executed("cargo test"));
    assert!(matches!(
        result.kind,
        AgentEventKind::CommandExecuted { .. }
    ));
}

// ===========================================================================
// StreamTee tests (12 tests)
// ===========================================================================

#[tokio::test]
async fn tee_splits_to_two_consumers() {
    let (tx1, mut rx1) = mpsc::channel(16);
    let (tx2, mut rx2) = mpsc::channel(16);
    let tee = StreamTee::new(vec![tx1, tx2]);
    let ev = delta("hello");
    tee.send(&ev).await.unwrap();
    let e1 = rx1.recv().await.unwrap();
    let e2 = rx2.recv().await.unwrap();
    assert!(matches!(&e1.kind, AgentEventKind::AssistantDelta { text } if text == "hello"));
    assert!(matches!(&e2.kind, AgentEventKind::AssistantDelta { text } if text == "hello"));
}

#[tokio::test]
async fn tee_both_consumers_receive_all_events() {
    let (tx1, mut rx1) = mpsc::channel(16);
    let (tx2, mut rx2) = mpsc::channel(16);
    let tee = StreamTee::new(vec![tx1, tx2]);
    for i in 0..5 {
        tee.send(&delta(&format!("msg-{i}"))).await.unwrap();
    }
    for i in 0..5 {
        let e1 = rx1.recv().await.unwrap();
        let e2 = rx2.recv().await.unwrap();
        assert!(
            matches!(&e1.kind, AgentEventKind::AssistantDelta { text } if text == &format!("msg-{i}"))
        );
        assert!(
            matches!(&e2.kind, AgentEventKind::AssistantDelta { text } if text == &format!("msg-{i}"))
        );
    }
}

#[tokio::test]
async fn tee_preserves_ordering() {
    let (tx1, mut rx1) = mpsc::channel(16);
    let tee = StreamTee::new(vec![tx1]);
    tee.send(&delta("first")).await.unwrap();
    tee.send(&delta("second")).await.unwrap();
    tee.send(&delta("third")).await.unwrap();
    let e1 = rx1.recv().await.unwrap();
    let e2 = rx1.recv().await.unwrap();
    let e3 = rx1.recv().await.unwrap();
    assert!(matches!(&e1.kind, AgentEventKind::AssistantDelta { text } if text == "first"));
    assert!(matches!(&e2.kind, AgentEventKind::AssistantDelta { text } if text == "second"));
    assert!(matches!(&e3.kind, AgentEventKind::AssistantDelta { text } if text == "third"));
}

#[tokio::test]
async fn tee_no_senders_ok() {
    let tee = StreamTee::new(vec![]);
    let result = tee.send(&delta("hello")).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn tee_all_receivers_closed_error() {
    let (tx1, rx1) = mpsc::channel::<AgentEvent>(16);
    let (tx2, rx2) = mpsc::channel::<AgentEvent>(16);
    drop(rx1);
    drop(rx2);
    let tee = StreamTee::new(vec![tx1, tx2]);
    let result = tee.send(&delta("hello")).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn tee_partial_receiver_close_still_sends() {
    let (tx1, rx1) = mpsc::channel::<AgentEvent>(16);
    let (tx2, mut rx2) = mpsc::channel(16);
    drop(rx1);
    let tee = StreamTee::new(vec![tx1, tx2]);
    let result = tee.send(&delta("hello")).await;
    assert!(result.is_ok());
    let ev = rx2.recv().await.unwrap();
    assert!(matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text == "hello"));
}

#[tokio::test]
async fn tee_active_count_all_open() {
    let (tx1, _rx1) = mpsc::channel::<AgentEvent>(16);
    let (tx2, _rx2) = mpsc::channel::<AgentEvent>(16);
    let tee = StreamTee::new(vec![tx1, tx2]);
    assert_eq!(tee.active_count(), 2);
}

#[tokio::test]
async fn tee_active_count_after_drop() {
    let (tx1, rx1) = mpsc::channel::<AgentEvent>(16);
    let (tx2, _rx2) = mpsc::channel::<AgentEvent>(16);
    let tee = StreamTee::new(vec![tx1, tx2]);
    drop(rx1);
    assert_eq!(tee.active_count(), 1);
}

#[tokio::test]
async fn tee_three_consumers() {
    let (tx1, mut rx1) = mpsc::channel(16);
    let (tx2, mut rx2) = mpsc::channel(16);
    let (tx3, mut rx3) = mpsc::channel(16);
    let tee = StreamTee::new(vec![tx1, tx2, tx3]);
    tee.send(&delta("broadcast")).await.unwrap();
    assert!(rx1.recv().await.is_some());
    assert!(rx2.recv().await.is_some());
    assert!(rx3.recv().await.is_some());
}

#[tokio::test]
async fn tee_sends_mixed_event_types() {
    let (tx1, mut rx1) = mpsc::channel(16);
    let tee = StreamTee::new(vec![tx1]);
    tee.send(&run_started()).await.unwrap();
    tee.send(&delta("text")).await.unwrap();
    tee.send(&tool_call("read")).await.unwrap();
    tee.send(&run_completed()).await.unwrap();
    assert!(matches!(
        rx1.recv().await.unwrap().kind,
        AgentEventKind::RunStarted { .. }
    ));
    assert!(matches!(
        rx1.recv().await.unwrap().kind,
        AgentEventKind::AssistantDelta { .. }
    ));
    assert!(matches!(
        rx1.recv().await.unwrap().kind,
        AgentEventKind::ToolCall { .. }
    ));
    assert!(matches!(
        rx1.recv().await.unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn tee_error_display() {
    let err = TeeError::AllReceiversClosed;
    assert_eq!(format!("{err}"), "all receivers closed");
}

#[tokio::test]
async fn tee_active_count_empty() {
    let tee = StreamTee::new(vec![]);
    assert_eq!(tee.active_count(), 0);
}

// ===========================================================================
// MergedStream tests (15 tests)
// ===========================================================================

#[tokio::test]
async fn merged_empty_streams_yields_none() {
    let mut merged = MergedStream::new(vec![]);
    assert!(merged.next().await.is_none());
}

#[tokio::test]
async fn merged_single_stream() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("only")).await.unwrap();
    drop(tx);
    let stream = EventStream::new(rx);
    let mut merged = MergedStream::new(vec![Box::pin(stream)]);
    let ev = merged.next().await.unwrap();
    assert!(matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text == "only"));
    assert!(merged.next().await.is_none());
}

#[tokio::test]
async fn merged_two_streams_includes_all_events() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    tx1.send(delta("a")).await.unwrap();
    tx2.send(delta("b")).await.unwrap();
    drop(tx1);
    drop(tx2);
    let s1 = EventStream::new(rx1);
    let s2 = EventStream::new(rx2);
    let merged = MergedStream::new(vec![Box::pin(s1), Box::pin(s2)]);
    let events: Vec<_> = merged.collect().await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn merged_preserves_all_events_from_multiple_sources() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    let (tx3, rx3) = mpsc::channel(16);
    for i in 0..3 {
        tx1.send(delta(&format!("s1-{i}"))).await.unwrap();
    }
    for i in 0..2 {
        tx2.send(delta(&format!("s2-{i}"))).await.unwrap();
    }
    tx3.send(delta("s3-0")).await.unwrap();
    drop(tx1);
    drop(tx2);
    drop(tx3);
    let s1 = EventStream::new(rx1);
    let s2 = EventStream::new(rx2);
    let s3 = EventStream::new(rx3);
    let merged = MergedStream::new(vec![Box::pin(s1), Box::pin(s2), Box::pin(s3)]);
    let events: Vec<_> = merged.collect().await;
    assert_eq!(events.len(), 6);
}

#[tokio::test]
async fn merged_with_one_empty_stream() {
    let (tx1, rx1) = mpsc::channel(16);
    let (_tx2, rx2) = mpsc::channel::<AgentEvent>(16);
    tx1.send(delta("from-s1")).await.unwrap();
    drop(tx1);
    drop(_tx2);
    let s1 = EventStream::new(rx1);
    let s2 = EventStream::new(rx2);
    let merged = MergedStream::new(vec![Box::pin(s1), Box::pin(s2)]);
    let events: Vec<_> = merged.collect().await;
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn merged_all_empty_streams() {
    let (_tx1, rx1) = mpsc::channel::<AgentEvent>(16);
    let (_tx2, rx2) = mpsc::channel::<AgentEvent>(16);
    drop(_tx1);
    drop(_tx2);
    let s1 = EventStream::new(rx1);
    let s2 = EventStream::new(rx2);
    let merged = MergedStream::new(vec![Box::pin(s1), Box::pin(s2)]);
    let events: Vec<_> = merged.collect().await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn merged_remaining_count() {
    let (_tx1, rx1) = mpsc::channel::<AgentEvent>(16);
    let (_tx2, rx2) = mpsc::channel::<AgentEvent>(16);
    let s1 = EventStream::new(rx1);
    let s2 = EventStream::new(rx2);
    let merged = MergedStream::new(vec![Box::pin(s1), Box::pin(s2)]);
    assert_eq!(merged.remaining(), 2);
}

#[tokio::test]
async fn merged_remaining_empty() {
    let merged = MergedStream::new(vec![]);
    assert_eq!(merged.remaining(), 0);
}

#[tokio::test]
async fn merged_mixed_event_types() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    tx1.send(run_started()).await.unwrap();
    tx1.send(delta("hello")).await.unwrap();
    tx2.send(tool_call("read")).await.unwrap();
    tx2.send(run_completed()).await.unwrap();
    drop(tx1);
    drop(tx2);
    let s1 = EventStream::new(rx1);
    let s2 = EventStream::new(rx2);
    let merged = MergedStream::new(vec![Box::pin(s1), Box::pin(s2)]);
    let events: Vec<_> = merged.collect().await;
    assert_eq!(events.len(), 4);
}

#[tokio::test]
async fn merged_single_event_per_stream() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    let (tx3, rx3) = mpsc::channel(16);
    tx1.send(delta("a")).await.unwrap();
    tx2.send(delta("b")).await.unwrap();
    tx3.send(delta("c")).await.unwrap();
    drop(tx1);
    drop(tx2);
    drop(tx3);
    let merged = MergedStream::new(vec![
        Box::pin(EventStream::new(rx1)),
        Box::pin(EventStream::new(rx2)),
        Box::pin(EventStream::new(rx3)),
    ]);
    let events: Vec<_> = merged.collect().await;
    assert_eq!(events.len(), 3);
}

#[tokio::test]
async fn merged_large_number_of_events() {
    let (tx, rx) = mpsc::channel(256);
    for i in 0..100 {
        tx.send(delta(&format!("{i}"))).await.unwrap();
    }
    drop(tx);
    let merged = MergedStream::new(vec![Box::pin(EventStream::new(rx))]);
    let events: Vec<_> = merged.collect().await;
    assert_eq!(events.len(), 100);
}

#[tokio::test]
async fn merged_asymmetric_streams() {
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    for i in 0..10 {
        tx1.send(delta(&format!("long-{i}"))).await.unwrap();
    }
    tx2.send(delta("short")).await.unwrap();
    drop(tx1);
    drop(tx2);
    let merged = MergedStream::new(vec![
        Box::pin(EventStream::new(rx1)),
        Box::pin(EventStream::new(rx2)),
    ]);
    let events: Vec<_> = merged.collect().await;
    assert_eq!(events.len(), 11);
}

#[tokio::test]
async fn merged_five_streams() {
    let mut streams: Vec<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> = Vec::new();
    for s in 0..5 {
        let (tx, rx) = mpsc::channel(16);
        tx.send(delta(&format!("stream-{s}"))).await.unwrap();
        drop(tx);
        streams.push(Box::pin(EventStream::new(rx)));
    }
    let merged = MergedStream::new(streams);
    let events: Vec<_> = merged.collect().await;
    assert_eq!(events.len(), 5);
}

#[tokio::test]
async fn merged_events_from_closed_then_open_streams() {
    let (_tx1, rx1) = mpsc::channel::<AgentEvent>(16);
    let (tx2, rx2) = mpsc::channel(16);
    drop(_tx1);
    tx2.send(delta("alive")).await.unwrap();
    drop(tx2);
    let merged = MergedStream::new(vec![
        Box::pin(EventStream::new(rx1)),
        Box::pin(EventStream::new(rx2)),
    ]);
    let events: Vec<_> = merged.collect().await;
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn merged_many_streams_all_with_errors() {
    let mut streams: Vec<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> = Vec::new();
    for i in 0..4 {
        let (tx, rx) = mpsc::channel(16);
        tx.send(error_ev(&format!("err-{i}"))).await.unwrap();
        drop(tx);
        streams.push(Box::pin(EventStream::new(rx)));
    }
    let merged = MergedStream::new(streams);
    let events: Vec<_> = merged.collect().await;
    assert_eq!(events.len(), 4);
    for ev in &events {
        assert!(matches!(ev.kind, AgentEventKind::Error { .. }));
    }
}

// ===========================================================================
// StreamMetrics tests (16 tests)
// ===========================================================================

#[test]
fn metrics_new_is_zero() {
    let m = StreamMetrics::new();
    assert_eq!(m.event_count(), 0);
    assert_eq!(m.total_bytes(), 0);
}

#[test]
fn metrics_tracks_event_count() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("a"));
    m.record_event(&delta("b"));
    m.record_event(&error_ev("e"));
    assert_eq!(m.event_count(), 3);
}

#[test]
fn metrics_tracks_event_kinds() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("a"));
    m.record_event(&delta("b"));
    m.record_event(&error_ev("e"));
    m.record_event(&tool_call("read"));
    let counts = m.event_type_counts();
    assert_eq!(counts.get("assistant_delta"), Some(&2));
    assert_eq!(counts.get("error"), Some(&1));
    assert_eq!(counts.get("tool_call"), Some(&1));
}

#[test]
fn metrics_tracks_delta_bytes() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("hello")); // 5 bytes
    m.record_event(&delta("world!")); // 6 bytes
    assert_eq!(m.total_bytes(), 11);
}

#[test]
fn metrics_non_delta_zero_bytes() {
    let mut m = StreamMetrics::new();
    m.record_event(&error_ev("oops"));
    m.record_event(&run_started());
    assert_eq!(m.total_bytes(), 0);
}

#[test]
fn metrics_summary_event_count() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("a"));
    m.record_event(&delta("b"));
    let summary = m.summary();
    assert_eq!(summary.event_count, 2);
}

#[test]
fn metrics_summary_total_bytes() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("abc")); // 3 bytes
    let summary = m.summary();
    assert_eq!(summary.total_bytes, 3);
}

#[test]
fn metrics_summary_event_type_counts() {
    let mut m = StreamMetrics::new();
    m.record_event(&run_started());
    m.record_event(&delta("x"));
    m.record_event(&run_completed());
    let summary = m.summary();
    assert_eq!(summary.event_type_counts.get("run_started"), Some(&1));
    assert_eq!(summary.event_type_counts.get("assistant_delta"), Some(&1));
    assert_eq!(summary.event_type_counts.get("run_completed"), Some(&1));
}

#[test]
fn metrics_default_creates_new() {
    let m = StreamMetrics::default();
    assert_eq!(m.event_count(), 0);
}

#[test]
fn metrics_throughput_no_events() {
    let m = StreamMetrics::new();
    assert_eq!(m.throughput(), 0.0);
}

#[test]
fn metrics_elapsed_no_events() {
    let m = StreamMetrics::new();
    assert_eq!(m.elapsed(), Duration::ZERO);
}

#[test]
fn metrics_elapsed_after_events() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("a"));
    // elapsed should be >= 0 (non-negative)
    assert!(m.elapsed() >= Duration::ZERO);
}

#[test]
fn metrics_summary_display() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("hello"));
    let summary = m.summary();
    let display = format!("{summary}");
    assert!(display.contains("Events: 1"));
    assert!(display.contains("Bytes: 5"));
}

#[test]
fn metrics_event_type_counts_btreemap_ordering() {
    let mut m = StreamMetrics::new();
    m.record_event(&warning_ev("w"));
    m.record_event(&delta("d"));
    m.record_event(&error_ev("e"));
    let counts = m.event_type_counts();
    let keys: Vec<_> = counts.keys().collect();
    // BTreeMap should be sorted
    let mut sorted_keys = keys.clone();
    sorted_keys.sort();
    assert_eq!(keys, sorted_keys);
}

#[test]
fn metrics_many_events() {
    let mut m = StreamMetrics::new();
    for i in 0..1000 {
        m.record_event(&delta(&format!("{i}")));
    }
    assert_eq!(m.event_count(), 1000);
}

#[test]
fn metrics_mixed_event_types_all_tracked() {
    let mut m = StreamMetrics::new();
    m.record_event(&run_started());
    m.record_event(&delta("text"));
    m.record_event(&tool_call("read"));
    m.record_event(&file_changed("main.rs"));
    m.record_event(&command_executed("cargo build"));
    m.record_event(&warning_ev("warn"));
    m.record_event(&error_ev("err"));
    m.record_event(&run_completed());
    assert_eq!(m.event_count(), 8);
    let counts = m.event_type_counts();
    assert_eq!(counts.len(), 8);
}

// ===========================================================================
// StreamAggregator tests (17 tests)
// ===========================================================================

#[test]
fn aggregator_new_is_empty() {
    let agg = StreamAggregator::new();
    assert_eq!(agg.event_count(), 0);
    assert_eq!(agg.text(), "");
    assert!(agg.tool_calls().is_empty());
    assert!(!agg.is_complete());
}

#[test]
fn aggregator_aggregates_text_from_deltas() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("Hello"));
    agg.push(&delta(" "));
    agg.push(&delta("World"));
    assert_eq!(agg.text(), "Hello World");
}

#[test]
fn aggregator_aggregates_text_from_message() {
    let mut agg = StreamAggregator::new();
    agg.push(&message("Complete message"));
    assert_eq!(agg.text(), "Complete message");
}

#[test]
fn aggregator_tracks_tool_calls() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call_with_id("read_file", "tc-1"));
    assert_eq!(agg.tool_calls().len(), 1);
    assert_eq!(agg.tool_calls()[0].name, "read_file");
    assert_eq!(agg.tool_calls()[0].id, "tc-1");
}

#[test]
fn aggregator_tool_result_matched() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call_with_id("read_file", "tc-1"));
    agg.push(&tool_result("read_file", "tc-1", "file contents"));
    assert_eq!(
        agg.tool_calls()[0].result.as_deref(),
        Some("\"file contents\"")
    );
}

#[test]
fn aggregator_tool_result_unmatched() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call_with_id("read_file", "tc-1"));
    agg.push(&tool_result("read_file", "tc-999", "output"));
    assert!(agg.tool_calls()[0].result.is_none());
}

#[test]
fn aggregator_detects_run_completed() {
    let mut agg = StreamAggregator::new();
    assert!(!agg.is_complete());
    agg.push(&run_completed());
    assert!(agg.is_complete());
}

#[test]
fn aggregator_tracks_errors() {
    let mut agg = StreamAggregator::new();
    agg.push(&error_ev("failure 1"));
    agg.push(&error_ev("failure 2"));
    assert_eq!(agg.errors().len(), 2);
}

#[test]
fn aggregator_thinking_none_by_default() {
    let agg = StreamAggregator::new();
    assert!(agg.thinking().is_none());
}

#[test]
fn aggregator_event_count() {
    let mut agg = StreamAggregator::new();
    agg.push(&run_started());
    agg.push(&delta("hello"));
    agg.push(&run_completed());
    assert_eq!(agg.event_count(), 3);
}

#[test]
fn aggregator_summary_total_events() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("a"));
    agg.push(&delta("b"));
    let summary = agg.to_summary();
    assert_eq!(summary.total_events, 2);
}

#[test]
fn aggregator_summary_text_length() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("hello")); // 5 chars
    let summary = agg.to_summary();
    assert_eq!(summary.text_length, 5);
}

#[test]
fn aggregator_summary_tool_call_count() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call_with_id("read", "tc-1"));
    agg.push(&tool_call_with_id("write", "tc-2"));
    let summary = agg.to_summary();
    assert_eq!(summary.tool_call_count, 2);
}

#[test]
fn aggregator_summary_has_errors() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("ok"));
    assert!(!agg.to_summary().has_errors);
    agg.push(&error_ev("fail"));
    assert!(agg.to_summary().has_errors);
}

#[test]
fn aggregator_summary_has_thinking_false() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("text"));
    assert!(!agg.to_summary().has_thinking);
}

#[test]
fn aggregator_default_impl() {
    let agg = StreamAggregator::default();
    assert_eq!(agg.event_count(), 0);
}

#[test]
fn aggregator_mixed_event_types() {
    let mut agg = StreamAggregator::new();
    agg.push(&run_started());
    agg.push(&delta("Hello "));
    agg.push(&delta("World"));
    agg.push(&tool_call_with_id("read_file", "tc-1"));
    agg.push(&tool_result("read_file", "tc-1", "data"));
    agg.push(&file_changed("main.rs"));
    agg.push(&warning_ev("caution"));
    agg.push(&error_ev("fail"));
    agg.push(&run_completed());
    assert_eq!(agg.event_count(), 9);
    assert_eq!(agg.text(), "Hello World");
    assert_eq!(agg.tool_calls().len(), 1);
    assert_eq!(agg.errors().len(), 1);
    assert!(agg.is_complete());
}

// ===========================================================================
// EventRecorder tests (5 tests)
// ===========================================================================

#[test]
fn recorder_captures_events() {
    let r = EventRecorder::new();
    r.record(&delta("a"));
    r.record(&delta("b"));
    assert_eq!(r.len(), 2);
}

#[test]
fn recorder_clear_resets() {
    let r = EventRecorder::new();
    r.record(&delta("a"));
    r.clear();
    assert!(r.is_empty());
}

#[test]
fn recorder_clone_shares_state() {
    let r = EventRecorder::new();
    let r2 = r.clone();
    r.record(&delta("shared"));
    assert_eq!(r2.len(), 1);
}

#[test]
fn recorder_events_returns_snapshot() {
    let r = EventRecorder::new();
    r.record(&delta("a"));
    r.record(&error_ev("e"));
    let events = r.events();
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
    assert!(matches!(&events[1].kind, AgentEventKind::Error { .. }));
}

#[test]
fn recorder_empty_initially() {
    let r = EventRecorder::new();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
}

// ===========================================================================
// EventStats tests (5 tests)
// ===========================================================================

#[test]
fn stats_tracks_by_kind() {
    let s = EventStats::new();
    s.observe(&delta("a"));
    s.observe(&delta("b"));
    s.observe(&error_ev("e"));
    assert_eq!(s.total_events(), 3);
    assert_eq!(s.count_for("assistant_delta"), 2);
    assert_eq!(s.count_for("error"), 1);
}

#[test]
fn stats_delta_bytes() {
    let s = EventStats::new();
    s.observe(&delta("abc")); // 3 bytes
    s.observe(&delta("de")); // 2 bytes
    assert_eq!(s.total_delta_bytes(), 5);
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
fn stats_reset_clears() {
    let s = EventStats::new();
    s.observe(&delta("a"));
    s.observe(&error_ev("e"));
    s.reset();
    assert_eq!(s.total_events(), 0);
    assert_eq!(s.error_count(), 0);
    assert!(s.kind_counts().is_empty());
}

#[test]
fn stats_kind_counts_snapshot() {
    let s = EventStats::new();
    s.observe(&run_started());
    s.observe(&run_completed());
    let counts = s.kind_counts();
    assert_eq!(counts.get("run_started"), Some(&1));
    assert_eq!(counts.get("run_completed"), Some(&1));
}

// ===========================================================================
// StreamPipeline tests (8 tests)
// ===========================================================================

#[test]
fn pipeline_empty_passes_through() {
    let p = StreamPipeline::new();
    let result = p.process(delta("test"));
    assert!(result.is_some());
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
fn pipeline_transform_modifies() {
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("p".to_string(), serde_json::json!(true));
            ev
        }))
        .build();
    let result = p.process(delta("x")).unwrap();
    assert_eq!(
        result.ext.unwrap().get("p").unwrap(),
        &serde_json::json!(true)
    );
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
}

#[test]
fn pipeline_filter_then_record() {
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
fn pipeline_multiple_filters_and_behavior() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .filter(EventFilter::by_kind("assistant_delta"))
        .build();
    assert!(p.process(delta("ok")).is_some());
    assert!(p.process(error_ev("bad")).is_none());
    assert!(p.process(warning_ev("warn")).is_none());
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
    p.process(error_ev("e"));
    p.process(tool_call("write"));
    assert_eq!(stats.total_events(), 2);
    assert_eq!(recorder.len(), 2);
    for ev in recorder.events() {
        assert!(ev.ext.as_ref().unwrap().contains_key("processed"));
    }
}

// ===========================================================================
// EventStream tests (5 tests)
// ===========================================================================

#[tokio::test]
async fn event_stream_collect_all() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);
    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    assert_eq!(events.len(), 2);
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
    let stream = EventStream::new(rx);
    let filter = EventFilter::by_kind("assistant_delta");
    let events = stream.collect_filtered(&filter).await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn event_stream_empty_collect() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(tx);
    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn event_stream_as_futures_stream() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("x")).await.unwrap();
    drop(tx);
    let mut stream = EventStream::new(rx);
    // Use StreamExt from tokio_stream
    let ev = StreamExt::next(&mut stream).await;
    assert!(ev.is_some());
    let ev = StreamExt::next(&mut stream).await;
    assert!(ev.is_none());
}

// ===========================================================================
// EventMultiplexer tests (3 tests)
// ===========================================================================

#[tokio::test]
async fn multiplexer_collect_sorted() {
    let ts_base = Utc::now();
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    tx1.send(make_event_with_ts(
        AgentEventKind::AssistantDelta {
            text: "late".to_string(),
        },
        ts_base + chrono::Duration::milliseconds(10),
    ))
    .await
    .unwrap();
    tx2.send(make_event_with_ts(
        AgentEventKind::AssistantDelta {
            text: "early".to_string(),
        },
        ts_base,
    ))
    .await
    .unwrap();
    drop(tx1);
    drop(tx2);
    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), 2);
    assert!(events[0].ts <= events[1].ts);
}

#[tokio::test]
async fn multiplexer_empty_streams() {
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
}

// ===========================================================================
// EventCollector tests (3 tests)
// ===========================================================================

#[tokio::test]
async fn collector_records_while_forwarding() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);
    let stream = EventStream::new(rx);
    let mut collector = EventCollector::new(stream);
    // Consume all events through the collector stream
    while StreamExt::next(&mut collector).await.is_some() {}
    assert_eq!(collector.len(), 2);
    assert!(!collector.is_empty());
}

#[tokio::test]
async fn collector_snapshot() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("hello")).await.unwrap();
    drop(tx);
    let stream = EventStream::new(rx);
    let mut collector = EventCollector::new(stream);
    let _ = StreamExt::next(&mut collector).await;
    let snap = collector.snapshot();
    assert_eq!(snap.len(), 1);
}

#[tokio::test]
async fn collector_empty_stream() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(tx);
    let stream = EventStream::new(rx);
    let mut collector = EventCollector::new(stream);
    assert!(StreamExt::next(&mut collector).await.is_none());
    assert!(collector.is_empty());
}
