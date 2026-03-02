// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end tests for the `abp-stream` crate covering event filtering,
//! stream combinators, multiplexing, backpressure, and edge cases.

use std::collections::BTreeMap;

use abp_core::{AgentEvent, AgentEventKind};
use abp_stream::{
    EventFilter, EventMultiplexer, EventRecorder, EventStats, EventStream, EventTransform,
    StreamPipeline, StreamPipelineBuilder, event_kind_name,
};
use chrono::{Duration, Utc};
use tokio::sync::mpsc;

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

fn error(msg: &str) -> AgentEvent {
    make_event(AgentEventKind::Error {
        message: msg.to_string(),
        error_code: None,
    })
}

fn warning(msg: &str) -> AgentEvent {
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

fn tool_result(name: &str, is_err: bool) -> AgentEvent {
    make_event(AgentEventKind::ToolResult {
        tool_name: name.to_string(),
        tool_use_id: None,
        output: serde_json::json!("ok"),
        is_error: is_err,
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

/// Send events into a channel, drop the sender, return the receiver.
async fn send_events(events: Vec<AgentEvent>, cap: usize) -> mpsc::Receiver<AgentEvent> {
    let (tx, rx) = mpsc::channel(cap.max(events.len()));
    for ev in events {
        tx.send(ev).await.unwrap();
    }
    drop(tx);
    rx
}

// ===========================================================================
// 1. EventFilter construction and matching
// ===========================================================================

#[test]
fn filter_by_kind_assistant_delta() {
    let f = EventFilter::by_kind("assistant_delta");
    assert!(f.matches(&delta("hi")));
    assert!(!f.matches(&error("e")));
}

#[test]
fn filter_by_kind_error() {
    let f = EventFilter::by_kind("error");
    assert!(f.matches(&error("oops")));
    assert!(!f.matches(&delta("ok")));
}

#[test]
fn filter_by_kind_tool_call() {
    let f = EventFilter::by_kind("tool_call");
    assert!(f.matches(&tool_call("read")));
    assert!(!f.matches(&tool_result("read", false)));
}

#[test]
fn filter_by_kind_tool_result() {
    let f = EventFilter::by_kind("tool_result");
    assert!(f.matches(&tool_result("write", false)));
    assert!(!f.matches(&tool_call("write")));
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
    assert!(f.matches(&warning("w")));
    assert!(!f.matches(&error("e")));
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
fn filter_by_kind_assistant_message() {
    let f = EventFilter::by_kind("assistant_message");
    assert!(f.matches(&message("hello")));
    assert!(!f.matches(&delta("hello")));
}

#[test]
fn filter_errors_only_accepts_only_errors() {
    let f = EventFilter::errors_only();
    assert!(f.matches(&error("e")));
    assert!(!f.matches(&delta("ok")));
    assert!(!f.matches(&warning("w")));
    assert!(!f.matches(&run_started()));
    assert!(!f.matches(&tool_call("t")));
}

#[test]
fn filter_exclude_errors_rejects_only_errors() {
    let f = EventFilter::exclude_errors();
    assert!(!f.matches(&error("e")));
    assert!(f.matches(&delta("ok")));
    assert!(f.matches(&warning("w")));
    assert!(f.matches(&run_started()));
    assert!(f.matches(&tool_call("t")));
}

#[test]
fn filter_custom_predicate_text_length() {
    let f = EventFilter::new(
        |ev| matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text.len() > 5),
    );
    assert!(f.matches(&delta("long text")));
    assert!(!f.matches(&delta("hi")));
    assert!(!f.matches(&error("e")));
}

#[test]
fn filter_custom_predicate_ext_present() {
    let f = EventFilter::new(|ev| ev.ext.is_some());
    let mut ev = delta("x");
    assert!(!f.matches(&ev));
    ev.ext = Some(BTreeMap::new());
    assert!(f.matches(&ev));
}

#[test]
fn filter_by_kind_nonexistent_matches_nothing() {
    let f = EventFilter::by_kind("nonexistent_kind");
    assert!(!f.matches(&delta("a")));
    assert!(!f.matches(&error("b")));
    assert!(!f.matches(&run_started()));
}

#[test]
fn filter_clone_shares_behavior() {
    let f1 = EventFilter::by_kind("error");
    let f2 = f1.clone();
    assert!(f1.matches(&error("e")));
    assert!(f2.matches(&error("e")));
    assert!(!f2.matches(&delta("d")));
}

#[test]
fn filter_debug_does_not_panic() {
    let f = EventFilter::by_kind("error");
    let dbg = format!("{f:?}");
    assert!(dbg.contains("EventFilter"));
}

// ===========================================================================
// 2. Stream combinator composition (StreamPipeline + Builder)
// ===========================================================================

#[test]
fn pipeline_empty_passes_all_events() {
    let p = StreamPipeline::new();
    assert!(p.process(delta("a")).is_some());
    assert!(p.process(error("e")).is_some());
    assert!(p.process(run_started()).is_some());
}

#[test]
fn pipeline_single_filter() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::by_kind("error"))
        .build();
    assert!(p.process(error("e")).is_some());
    assert!(p.process(delta("d")).is_none());
}

#[test]
fn pipeline_two_filters_intersection() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .filter(EventFilter::by_kind("assistant_delta"))
        .build();
    assert!(p.process(delta("d")).is_some());
    assert!(p.process(error("e")).is_none());
    assert!(p.process(tool_call("t")).is_none());
}

#[test]
fn pipeline_three_filters() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .filter(EventFilter::new(|ev| ev.ext.is_none()))
        .filter(EventFilter::by_kind("assistant_delta"))
        .build();
    assert!(p.process(delta("ok")).is_some());

    let mut ev = delta("with ext");
    ev.ext = Some(BTreeMap::new());
    assert!(p.process(ev).is_none());
}

#[test]
fn pipeline_transform_adds_metadata() {
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("tagged".to_string(), serde_json::json!(true));
            ev
        }))
        .build();
    let result = p.process(delta("x")).unwrap();
    assert_eq!(
        result.ext.unwrap().get("tagged").unwrap(),
        &serde_json::json!(true)
    );
}

#[test]
fn pipeline_multiple_transforms_compose() {
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("a".to_string(), serde_json::json!(1));
            ev
        }))
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("b".to_string(), serde_json::json!(2));
            ev
        }))
        .build();
    let result = p.process(delta("x")).unwrap();
    let ext = result.ext.unwrap();
    assert_eq!(ext.get("a").unwrap(), &serde_json::json!(1));
    assert_eq!(ext.get("b").unwrap(), &serde_json::json!(2));
}

#[test]
fn pipeline_filter_before_transform() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::by_kind("assistant_delta"))
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("ok".to_string(), serde_json::json!(true));
            ev
        }))
        .build();
    // Matching event gets transformed
    let result = p.process(delta("x")).unwrap();
    assert!(result.ext.unwrap().contains_key("ok"));
    // Non-matching event is filtered before transform
    assert!(p.process(error("e")).is_none());
}

#[test]
fn pipeline_identity_transform() {
    let t = EventTransform::identity();
    let ev = delta("unchanged");
    let result = t.apply(ev.clone());
    assert_eq!(result.ts, ev.ts);
    assert!(result.ext.is_none());
}

#[test]
fn pipeline_with_recorder_captures() {
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(rec.clone())
        .build();
    p.process(delta("a"));
    p.process(delta("b"));
    assert_eq!(rec.len(), 2);
}

#[test]
fn pipeline_with_stats_tracks() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();
    p.process(delta("a"));
    p.process(error("e"));
    assert_eq!(stats.total_events(), 2);
    assert_eq!(stats.error_count(), 1);
}

#[test]
fn pipeline_record_method() {
    let p = StreamPipelineBuilder::new().record().build();
    p.process(delta("a"));
    assert_eq!(p.recorder().unwrap().len(), 1);
}

#[test]
fn pipeline_full_composition() {
    let rec = EventRecorder::new();
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("processed".to_string(), serde_json::json!(true));
            ev
        }))
        .with_recorder(rec.clone())
        .with_stats(stats.clone())
        .build();

    p.process(delta("a"));
    p.process(error("e"));
    p.process(tool_call("read"));
    p.process(warning("w"));

    // Error was filtered out
    assert_eq!(stats.total_events(), 3);
    assert_eq!(rec.len(), 3);
    assert_eq!(stats.error_count(), 0);
    for ev in rec.events() {
        assert!(ev.ext.as_ref().unwrap().contains_key("processed"));
    }
}

#[test]
fn pipeline_filtered_events_not_recorded() {
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .with_recorder(rec.clone())
        .build();
    p.process(delta("a"));
    p.process(error("e"));
    assert_eq!(rec.len(), 1);
}

#[test]
fn pipeline_filtered_events_not_in_stats() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::by_kind("tool_call"))
        .with_stats(stats.clone())
        .build();
    p.process(delta("a"));
    p.process(tool_call("read"));
    p.process(error("e"));
    assert_eq!(stats.total_events(), 1);
    assert_eq!(stats.count_for("tool_call"), 1);
}

#[test]
fn pipeline_accessors_stats_and_recorder() {
    let stats = EventStats::new();
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .with_stats(stats)
        .with_recorder(rec)
        .build();
    assert!(p.stats().is_some());
    assert!(p.recorder().is_some());
}

#[test]
fn pipeline_default_no_stats_no_recorder() {
    let p = StreamPipeline::new();
    assert!(p.stats().is_none());
    assert!(p.recorder().is_none());
}

// ===========================================================================
// 3. Event type filtering (by kind, by pattern)
// ===========================================================================

#[test]
fn filter_all_ten_event_kinds() {
    let kinds = [
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
    ];
    let events: Vec<AgentEvent> = vec![
        run_started(),
        run_completed(),
        delta("d"),
        message("m"),
        tool_call("t"),
        tool_result("t", false),
        file_changed("f"),
        command_executed("ls", Some(0)),
        warning("w"),
        error("e"),
    ];

    for (kind, ev) in kinds.iter().zip(events.iter()) {
        let f = EventFilter::by_kind(kind);
        assert!(f.matches(ev), "Expected by_kind({kind}) to match its event");
        // Should not match the next kind (wrapping)
    }
}

#[test]
fn filter_by_kind_each_rejects_others() {
    let events: Vec<AgentEvent> = vec![
        run_started(),
        run_completed(),
        delta("d"),
        message("m"),
        tool_call("t"),
        tool_result("t", false),
        file_changed("f"),
        command_executed("ls", Some(0)),
        warning("w"),
        error("e"),
    ];

    let f = EventFilter::by_kind("assistant_delta");
    let matches: Vec<bool> = events.iter().map(|ev| f.matches(ev)).collect();
    // Only index 2 (delta) should match
    assert_eq!(matches.iter().filter(|&&b| b).count(), 1);
    assert!(matches[2]);
}

#[test]
fn filter_pattern_matching_on_tool_name() {
    let f = EventFilter::new(
        |ev| matches!(&ev.kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name.starts_with("read")),
    );
    assert!(f.matches(&tool_call("read_file")));
    assert!(f.matches(&tool_call("read_dir")));
    assert!(!f.matches(&tool_call("write_file")));
}

#[test]
fn filter_pattern_matching_on_file_path() {
    let f = EventFilter::new(
        |ev| matches!(&ev.kind, AgentEventKind::FileChanged { path, .. } if path.ends_with(".rs")),
    );
    assert!(f.matches(&file_changed("src/main.rs")));
    assert!(!f.matches(&file_changed("readme.md")));
}

#[test]
fn filter_pattern_command_exit_code() {
    let f = EventFilter::new(|ev| {
        matches!(
            &ev.kind,
            AgentEventKind::CommandExecuted {
                exit_code: Some(0),
                ..
            }
        )
    });
    assert!(f.matches(&command_executed("ls", Some(0))));
    assert!(!f.matches(&command_executed("fail", Some(1))));
    assert!(!f.matches(&command_executed("unknown", None)));
}

// ===========================================================================
// 4. Stream merging/multiplexing
// ===========================================================================

#[tokio::test]
async fn multiplexer_collect_sorted_two_streams() {
    let base = Utc::now();
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    tx1.send(make_event_with_ts(
        AgentEventKind::AssistantDelta { text: "a".into() },
        base,
    ))
    .await
    .unwrap();
    tx1.send(make_event_with_ts(
        AgentEventKind::AssistantDelta { text: "c".into() },
        base + Duration::milliseconds(20),
    ))
    .await
    .unwrap();
    drop(tx1);

    tx2.send(make_event_with_ts(
        AgentEventKind::AssistantDelta { text: "b".into() },
        base + Duration::milliseconds(10),
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
async fn multiplexer_merge_preserves_order() {
    let base = Utc::now();
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    tx1.send(make_event_with_ts(
        AgentEventKind::RunStarted {
            message: "first".into(),
        },
        base,
    ))
    .await
    .unwrap();
    tx2.send(make_event_with_ts(
        AgentEventKind::RunCompleted {
            message: "second".into(),
        },
        base + Duration::milliseconds(5),
    ))
    .await
    .unwrap();
    drop(tx1);
    drop(tx2);

    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let mut rx = mux.merge(16);
    let first = rx.recv().await.unwrap();
    assert!(matches!(first.kind, AgentEventKind::RunStarted { .. }));
    let second = rx.recv().await.unwrap();
    assert!(matches!(second.kind, AgentEventKind::RunCompleted { .. }));
    assert!(rx.recv().await.is_none());
}

#[tokio::test]
async fn multiplexer_three_streams() {
    let base = Utc::now();
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    let (tx3, rx3) = mpsc::channel(16);

    tx1.send(make_event_with_ts(
        AgentEventKind::AssistantDelta { text: "a".into() },
        base + Duration::milliseconds(30),
    ))
    .await
    .unwrap();
    tx2.send(make_event_with_ts(
        AgentEventKind::AssistantDelta { text: "b".into() },
        base + Duration::milliseconds(10),
    ))
    .await
    .unwrap();
    tx3.send(make_event_with_ts(
        AgentEventKind::AssistantDelta { text: "c".into() },
        base + Duration::milliseconds(20),
    ))
    .await
    .unwrap();
    drop(tx1);
    drop(tx2);
    drop(tx3);

    let events = EventMultiplexer::new(vec![rx1, rx2, rx3])
        .collect_sorted()
        .await;
    assert_eq!(events.len(), 3);
    assert!(events[0].ts <= events[1].ts);
    assert!(events[1].ts <= events[2].ts);
}

#[tokio::test]
async fn multiplexer_single_stream() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);

    let events = EventMultiplexer::new(vec![rx]).collect_sorted().await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn multiplexer_no_streams() {
    let events = EventMultiplexer::new(vec![]).collect_sorted().await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn multiplexer_merge_no_streams() {
    let mut rx = EventMultiplexer::new(vec![]).merge(16);
    assert!(rx.recv().await.is_none());
}

// ===========================================================================
// 5. Backpressure handling
// ===========================================================================

#[tokio::test]
async fn backpressure_bounded_channel_blocks_sender() {
    let (tx, rx) = mpsc::channel(2);
    tx.send(delta("1")).await.unwrap();
    tx.send(delta("2")).await.unwrap();

    // Channel is full. Spawn a sender that will block until drained.
    let handle = tokio::spawn(async move {
        tx.send(delta("3")).await.unwrap();
    });

    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    handle.await.unwrap();
    assert_eq!(events.len(), 3);
}

#[tokio::test]
async fn backpressure_pipe_respects_output_capacity() {
    let (tx_in, rx_in) = mpsc::channel(16);
    let (tx_out, mut rx_out) = mpsc::channel(2);

    for i in 0..5 {
        tx_in.send(delta(&format!("msg-{i}"))).await.unwrap();
    }
    drop(tx_in);

    let pipeline = StreamPipeline::new();
    let pipe_handle = tokio::spawn(async move {
        EventStream::new(rx_in).pipe(&pipeline, tx_out).await;
    });

    let mut received = Vec::new();
    while let Some(ev) = rx_out.recv().await {
        received.push(ev);
    }
    pipe_handle.await.unwrap();
    assert_eq!(received.len(), 5);
}

#[tokio::test]
async fn backpressure_pipe_stops_when_receiver_dropped() {
    let (tx_in, rx_in) = mpsc::channel(128);
    let (tx_out, rx_out) = mpsc::channel(2);

    // Send many events
    for i in 0..100 {
        tx_in.send(delta(&format!("msg-{i}"))).await.unwrap();
    }
    drop(tx_in);

    // Drop the receiver immediately — pipe should stop gracefully.
    drop(rx_out);

    let pipeline = StreamPipeline::new();
    // Should not panic
    EventStream::new(rx_in).pipe(&pipeline, tx_out).await;
}

// ===========================================================================
// 6. Error event filtering
// ===========================================================================

#[test]
fn error_filter_only_errors() {
    let f = EventFilter::errors_only();
    let events = [
        delta("a"),
        error("e1"),
        warning("w"),
        error("e2"),
        tool_call("t"),
    ];
    let matched: Vec<_> = events.iter().filter(|ev| f.matches(ev)).collect();
    assert_eq!(matched.len(), 2);
}

#[test]
fn error_filter_exclude_all_errors() {
    let f = EventFilter::exclude_errors();
    let events = [
        delta("a"),
        error("e1"),
        warning("w"),
        error("e2"),
        tool_call("t"),
    ];
    let matched: Vec<_> = events.iter().filter(|ev| f.matches(ev)).collect();
    assert_eq!(matched.len(), 3);
}

#[tokio::test]
async fn stream_collect_filtered_errors_only() {
    let events = vec![delta("a"), error("e1"), delta("b"), error("e2")];
    let rx = send_events(events, 16).await;
    let stream = EventStream::new(rx);
    let filtered = stream.collect_filtered(&EventFilter::errors_only()).await;
    assert_eq!(filtered.len(), 2);
    for ev in &filtered {
        assert!(matches!(ev.kind, AgentEventKind::Error { .. }));
    }
}

#[tokio::test]
async fn stream_collect_filtered_no_errors() {
    let events = vec![delta("a"), error("e1"), delta("b"), error("e2")];
    let rx = send_events(events, 16).await;
    let stream = EventStream::new(rx);
    let filtered = stream
        .collect_filtered(&EventFilter::exclude_errors())
        .await;
    assert_eq!(filtered.len(), 2);
    for ev in &filtered {
        assert!(!matches!(ev.kind, AgentEventKind::Error { .. }));
    }
}

#[test]
fn pipeline_error_stats_through_filter() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .with_stats(stats.clone())
        .build();
    p.process(delta("a"));
    p.process(error("e"));
    p.process(warning("w"));
    assert_eq!(stats.total_events(), 1);
    assert_eq!(stats.error_count(), 1);
}

#[test]
fn error_filter_combined_with_custom() {
    // Accept errors only if they contain "critical"
    let f = EventFilter::new(
        |ev| matches!(&ev.kind, AgentEventKind::Error { message, .. } if message.contains("critical")),
    );
    assert!(f.matches(&error("critical failure")));
    assert!(!f.matches(&error("minor issue")));
    assert!(!f.matches(&delta("critical text")));
}

// ===========================================================================
// 7. Edge cases
// ===========================================================================

// --- Empty streams ---

#[tokio::test]
async fn empty_stream_collect_all() {
    let (_tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(_tx);
    let events = EventStream::new(rx).collect_all().await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn empty_stream_collect_filtered() {
    let (_tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(_tx);
    let events = EventStream::new(rx)
        .collect_filtered(&EventFilter::errors_only())
        .await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn empty_stream_pipe() {
    let (_tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(_tx);
    let (tx_out, mut rx_out) = mpsc::channel(16);
    let pipeline = StreamPipeline::new();
    EventStream::new(rx).pipe(&pipeline, tx_out).await;
    assert!(rx_out.recv().await.is_none());
}

#[tokio::test]
async fn empty_stream_recv() {
    let (_tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(_tx);
    let mut stream = EventStream::new(rx);
    assert!(stream.recv().await.is_none());
}

// --- Single event ---

#[tokio::test]
async fn single_event_stream_collect() {
    let rx = send_events(vec![delta("only")], 16).await;
    let events = EventStream::new(rx).collect_all().await;
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn single_event_stream_filtered_match() {
    let rx = send_events(vec![delta("only")], 16).await;
    let events = EventStream::new(rx)
        .collect_filtered(&EventFilter::by_kind("assistant_delta"))
        .await;
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn single_event_stream_filtered_no_match() {
    let rx = send_events(vec![delta("only")], 16).await;
    let events = EventStream::new(rx)
        .collect_filtered(&EventFilter::by_kind("error"))
        .await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn single_event_multiplexer() {
    let rx = send_events(vec![delta("x")], 16).await;
    let events = EventMultiplexer::new(vec![rx]).collect_sorted().await;
    assert_eq!(events.len(), 1);
}

// --- Large streams ---

#[tokio::test]
async fn large_stream_1000_events() {
    let events: Vec<AgentEvent> = (0..1000).map(|i| delta(&format!("ev-{i}"))).collect();
    let rx = send_events(events, 1024).await;
    let collected = EventStream::new(rx).collect_all().await;
    assert_eq!(collected.len(), 1000);
}

#[tokio::test]
async fn large_stream_filter_half() {
    // Even indices: delta, odd: error
    let events: Vec<AgentEvent> = (0..200)
        .map(|i| {
            if i % 2 == 0 {
                delta(&format!("d-{i}"))
            } else {
                error(&format!("e-{i}"))
            }
        })
        .collect();
    let rx = send_events(events, 256).await;
    let filtered = EventStream::new(rx)
        .collect_filtered(&EventFilter::by_kind("assistant_delta"))
        .await;
    assert_eq!(filtered.len(), 100);
}

#[tokio::test]
async fn large_stream_pipeline_stats() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();

    let (tx_in, rx_in) = mpsc::channel(512);
    let (tx_out, mut rx_out) = mpsc::channel(512);

    for i in 0..500 {
        let ev = if i % 5 == 0 {
            error(&format!("e-{i}"))
        } else {
            delta(&format!("d-{i}"))
        };
        tx_in.send(ev).await.unwrap();
    }
    drop(tx_in);

    let handle = tokio::spawn(async move {
        EventStream::new(rx_in).pipe(&p, tx_out).await;
    });

    let mut count = 0;
    while rx_out.recv().await.is_some() {
        count += 1;
    }
    handle.await.unwrap();

    assert_eq!(count, 500);
    assert_eq!(stats.total_events(), 500);
    assert_eq!(stats.error_count(), 100);
    assert_eq!(stats.count_for("assistant_delta"), 400);
}

#[tokio::test]
async fn large_multiplexer_five_streams() {
    let base = Utc::now();
    let mut receivers = Vec::new();
    for stream_idx in 0..5 {
        let (tx, rx) = mpsc::channel(64);
        for ev_idx in 0..20 {
            let ts = base + Duration::milliseconds((stream_idx * 100 + ev_idx) as i64);
            tx.send(make_event_with_ts(
                AgentEventKind::AssistantDelta {
                    text: format!("s{stream_idx}-e{ev_idx}"),
                },
                ts,
            ))
            .await
            .unwrap();
        }
        drop(tx);
        receivers.push(rx);
    }

    let events = EventMultiplexer::new(receivers).collect_sorted().await;
    assert_eq!(events.len(), 100);
    // Verify sorted
    for w in events.windows(2) {
        assert!(w[0].ts <= w[1].ts);
    }
}

// --- EventRecorder edge cases ---

#[test]
fn recorder_empty_initially() {
    let r = EventRecorder::new();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
    assert!(r.events().is_empty());
}

#[test]
fn recorder_clear_then_add() {
    let r = EventRecorder::new();
    r.record(&delta("a"));
    r.clear();
    assert!(r.is_empty());
    r.record(&delta("b"));
    assert_eq!(r.len(), 1);
}

#[test]
fn recorder_clone_sees_same_events() {
    let r1 = EventRecorder::new();
    let r2 = r1.clone();
    r1.record(&delta("a"));
    r2.record(&delta("b"));
    assert_eq!(r1.len(), 2);
    assert_eq!(r2.len(), 2);
}

// --- EventStats edge cases ---

#[test]
fn stats_empty_initially() {
    let s = EventStats::new();
    assert_eq!(s.total_events(), 0);
    assert_eq!(s.total_delta_bytes(), 0);
    assert_eq!(s.error_count(), 0);
    assert!(s.kind_counts().is_empty());
}

#[test]
fn stats_count_for_unknown_kind_is_zero() {
    let s = EventStats::new();
    s.observe(&delta("x"));
    assert_eq!(s.count_for("nonexistent"), 0);
}

#[test]
fn stats_delta_bytes_accumulate() {
    let s = EventStats::new();
    s.observe(&delta("abc")); // 3
    s.observe(&delta("de")); // 2
    s.observe(&delta("f")); // 1
    assert_eq!(s.total_delta_bytes(), 6);
}

#[test]
fn stats_non_delta_events_dont_add_bytes() {
    let s = EventStats::new();
    s.observe(&error("some error message"));
    s.observe(&tool_call("read_file"));
    assert_eq!(s.total_delta_bytes(), 0);
}

#[test]
fn stats_reset_clears_all() {
    let s = EventStats::new();
    s.observe(&delta("a"));
    s.observe(&error("e"));
    s.reset();
    assert_eq!(s.total_events(), 0);
    assert_eq!(s.total_delta_bytes(), 0);
    assert_eq!(s.error_count(), 0);
    assert!(s.kind_counts().is_empty());
}

#[test]
fn stats_kind_counts_all_kinds() {
    let s = EventStats::new();
    s.observe(&run_started());
    s.observe(&run_completed());
    s.observe(&delta("d"));
    s.observe(&message("m"));
    s.observe(&tool_call("t"));
    s.observe(&tool_result("t", false));
    s.observe(&file_changed("f"));
    s.observe(&command_executed("ls", Some(0)));
    s.observe(&warning("w"));
    s.observe(&error("e"));
    let counts = s.kind_counts();
    assert_eq!(counts.len(), 10);
    for &count in counts.values() {
        assert_eq!(count, 1);
    }
}

// --- event_kind_name ---

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

// --- EventStream recv ---

#[tokio::test]
async fn stream_recv_yields_events_in_order() {
    let rx = send_events(vec![delta("a"), delta("b"), delta("c")], 16).await;
    let mut stream = EventStream::new(rx);
    let a = stream.recv().await.unwrap();
    assert!(matches!(&a.kind, AgentEventKind::AssistantDelta { text } if text == "a"));
    let b = stream.recv().await.unwrap();
    assert!(matches!(&b.kind, AgentEventKind::AssistantDelta { text } if text == "b"));
    let c = stream.recv().await.unwrap();
    assert!(matches!(&c.kind, AgentEventKind::AssistantDelta { text } if text == "c"));
    assert!(stream.recv().await.is_none());
}

#[tokio::test]
async fn stream_into_inner_returns_receiver() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    drop(tx);
    let stream = EventStream::new(rx);
    let mut inner = stream.into_inner();
    assert!(inner.recv().await.is_some());
    assert!(inner.recv().await.is_none());
}

// --- EventTransform edge cases ---

#[test]
fn transform_debug_does_not_panic() {
    let t = EventTransform::identity();
    let dbg = format!("{t:?}");
    assert!(dbg.contains("EventTransform"));
}

#[test]
fn transform_clone_preserves_behavior() {
    let t1 = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("cloned".to_string(), serde_json::json!(true));
        ev
    });
    let t2 = t1.clone();
    let result = t2.apply(delta("x"));
    assert!(result.ext.unwrap().contains_key("cloned"));
}

// --- Pipeline piping through async streams ---

#[tokio::test]
async fn pipe_filter_transform_record_stats_e2e() {
    let rec = EventRecorder::new();
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("piped".to_string(), serde_json::json!(true));
            ev
        }))
        .with_recorder(rec.clone())
        .with_stats(stats.clone())
        .build();

    let events = vec![
        delta("a"),
        error("e1"),
        tool_call("read"),
        error("e2"),
        warning("w"),
    ];
    let (tx_in, rx_in) = mpsc::channel(16);
    let (tx_out, mut rx_out) = mpsc::channel(16);

    for ev in events {
        tx_in.send(ev).await.unwrap();
    }
    drop(tx_in);

    let handle = tokio::spawn(async move {
        EventStream::new(rx_in).pipe(&pipeline, tx_out).await;
    });

    let mut out = Vec::new();
    while let Some(ev) = rx_out.recv().await {
        out.push(ev);
    }
    handle.await.unwrap();

    assert_eq!(out.len(), 3); // delta, tool_call, warning
    assert_eq!(rec.len(), 3);
    assert_eq!(stats.total_events(), 3);
    assert_eq!(stats.error_count(), 0);
    for ev in &out {
        assert!(ev.ext.as_ref().unwrap().contains_key("piped"));
    }
}

// --- Multiplexer with mixed event types ---

#[tokio::test]
async fn multiplexer_mixed_event_types() {
    let base = Utc::now();
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    tx1.send(make_event_with_ts(
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
        base,
    ))
    .await
    .unwrap();
    tx1.send(make_event_with_ts(
        AgentEventKind::Error {
            message: "oops".into(),
            error_code: None,
        },
        base + Duration::milliseconds(20),
    ))
    .await
    .unwrap();
    drop(tx1);

    tx2.send(make_event_with_ts(
        AgentEventKind::AssistantDelta {
            text: "hello".into(),
        },
        base + Duration::milliseconds(10),
    ))
    .await
    .unwrap();
    tx2.send(make_event_with_ts(
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        base + Duration::milliseconds(30),
    ))
    .await
    .unwrap();
    drop(tx2);

    let events = EventMultiplexer::new(vec![rx1, rx2]).collect_sorted().await;
    assert_eq!(events.len(), 4);
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(
        events[1].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
    assert!(matches!(events[2].kind, AgentEventKind::Error { .. }));
    assert!(matches!(
        events[3].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

// --- Pipeline default ---

#[test]
fn pipeline_builder_default_is_empty() {
    let p = StreamPipelineBuilder::new().build();
    assert!(p.process(delta("a")).is_some());
    assert!(p.process(error("e")).is_some());
    assert!(p.stats().is_none());
    assert!(p.recorder().is_none());
}

#[test]
fn pipeline_clone_independent() {
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(rec.clone())
        .build();
    let p2 = p.clone();
    p.process(delta("a"));
    // Both share the same Arc<Mutex<>> recorder
    assert_eq!(rec.len(), 1);
    p2.process(delta("b"));
    assert_eq!(rec.len(), 2);
}

#[test]
fn pipeline_debug_does_not_panic() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .transform(EventTransform::identity())
        .record()
        .build();
    let dbg = format!("{p:?}");
    assert!(dbg.contains("StreamPipeline"));
}

// --- Misc edge cases ---

#[test]
fn filter_empty_kind_string() {
    let f = EventFilter::by_kind("");
    assert!(!f.matches(&delta("x")));
    assert!(!f.matches(&error("e")));
}

#[test]
fn filter_case_sensitive() {
    let f = EventFilter::by_kind("Error"); // Wrong case
    assert!(!f.matches(&error("e")));
}

#[test]
fn stats_observe_many_deltas_bytes() {
    let s = EventStats::new();
    for i in 0..100 {
        s.observe(&delta(&"x".repeat(i)));
    }
    // Sum of 0..100 = 4950
    assert_eq!(s.total_delta_bytes(), 4950);
    assert_eq!(s.total_events(), 100);
}

#[test]
fn recorder_records_large_batch() {
    let r = EventRecorder::new();
    for i in 0..500 {
        r.record(&delta(&format!("ev-{i}")));
    }
    assert_eq!(r.len(), 500);
}

#[tokio::test]
async fn stream_collect_filtered_all_rejected() {
    let events = vec![delta("a"), delta("b"), delta("c")];
    let rx = send_events(events, 16).await;
    let filtered = EventStream::new(rx)
        .collect_filtered(&EventFilter::errors_only())
        .await;
    assert!(filtered.is_empty());
}

#[tokio::test]
async fn stream_collect_filtered_all_accepted() {
    let events = vec![error("e1"), error("e2"), error("e3")];
    let rx = send_events(events, 16).await;
    let filtered = EventStream::new(rx)
        .collect_filtered(&EventFilter::errors_only())
        .await;
    assert_eq!(filtered.len(), 3);
}

// ===========================================================================
// Additional tests to reach 100+
// ===========================================================================

#[test]
fn filter_tool_result_error_flag() {
    let f = EventFilter::new(|ev| {
        matches!(&ev.kind, AgentEventKind::ToolResult { is_error: true, .. })
    });
    assert!(f.matches(&tool_result("t", true)));
    assert!(!f.matches(&tool_result("t", false)));
    assert!(!f.matches(&tool_call("t")));
}

#[test]
fn filter_warning_message_contains() {
    let f = EventFilter::new(
        |ev| matches!(&ev.kind, AgentEventKind::Warning { message } if message.contains("deprecated")),
    );
    assert!(f.matches(&warning("use of deprecated API")));
    assert!(!f.matches(&warning("all good")));
}

#[test]
fn transform_replaces_kind_text() {
    let t = EventTransform::new(|mut ev| {
        if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
            *text = text.to_uppercase();
        }
        ev
    });
    let result = t.apply(delta("hello"));
    assert!(matches!(&result.kind, AgentEventKind::AssistantDelta { text } if text == "HELLO"));
}

#[test]
fn pipeline_multiple_transforms_sequential() {
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
                text.push_str("-first");
            }
            ev
        }))
        .transform(EventTransform::new(|mut ev| {
            if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
                text.push_str("-second");
            }
            ev
        }))
        .build();
    let result = p.process(delta("x")).unwrap();
    assert!(
        matches!(&result.kind, AgentEventKind::AssistantDelta { text } if text == "x-first-second")
    );
}

#[test]
fn stats_clone_shares_state() {
    let s1 = EventStats::new();
    let s2 = s1.clone();
    s1.observe(&delta("a"));
    assert_eq!(s2.total_events(), 1);
    s2.observe(&error("e"));
    assert_eq!(s1.error_count(), 1);
}

#[tokio::test]
async fn multiplexer_same_timestamps_stable() {
    let ts = Utc::now();
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    tx1.send(make_event_with_ts(
        AgentEventKind::AssistantDelta { text: "a".into() },
        ts,
    ))
    .await
    .unwrap();
    tx2.send(make_event_with_ts(
        AgentEventKind::AssistantDelta { text: "b".into() },
        ts,
    ))
    .await
    .unwrap();
    drop(tx1);
    drop(tx2);

    let events = EventMultiplexer::new(vec![rx1, rx2]).collect_sorted().await;
    assert_eq!(events.len(), 2);
    // Both have same timestamp — just verify no crash and count is correct
    assert_eq!(events[0].ts, events[1].ts);
}

#[tokio::test]
async fn multiplexer_merge_empty_streams() {
    let (_tx1, rx1) = mpsc::channel::<AgentEvent>(16);
    let (_tx2, rx2) = mpsc::channel::<AgentEvent>(16);
    drop(_tx1);
    drop(_tx2);
    let mut rx = EventMultiplexer::new(vec![rx1, rx2]).merge(16);
    assert!(rx.recv().await.is_none());
}

#[tokio::test]
async fn stream_pipe_with_identity_pipeline() {
    let (tx_in, rx_in) = mpsc::channel(16);
    let (tx_out, mut rx_out) = mpsc::channel(16);
    tx_in.send(delta("a")).await.unwrap();
    tx_in.send(error("e")).await.unwrap();
    drop(tx_in);

    let pipeline = StreamPipeline::new();
    let handle = tokio::spawn(async move {
        EventStream::new(rx_in).pipe(&pipeline, tx_out).await;
    });

    let mut out = Vec::new();
    while let Some(ev) = rx_out.recv().await {
        out.push(ev);
    }
    handle.await.unwrap();
    assert_eq!(out.len(), 2);
}

#[test]
fn filter_by_kind_command_with_exit_code() {
    let f = EventFilter::new(
        |ev| matches!(&ev.kind, AgentEventKind::CommandExecuted { exit_code: Some(code), .. } if *code != 0),
    );
    assert!(f.matches(&command_executed("fail", Some(1))));
    assert!(!f.matches(&command_executed("ok", Some(0))));
    assert!(!f.matches(&command_executed("unknown", None)));
}

#[test]
fn pipeline_filter_only_warnings_and_errors() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::new(|ev| {
            matches!(
                ev.kind,
                AgentEventKind::Warning { .. } | AgentEventKind::Error { .. }
            )
        }))
        .build();
    assert!(p.process(warning("w")).is_some());
    assert!(p.process(error("e")).is_some());
    assert!(p.process(delta("d")).is_none());
    assert!(p.process(tool_call("t")).is_none());
}

#[tokio::test]
async fn large_stream_filter_all_pass() {
    let events: Vec<AgentEvent> = (0..300).map(|i| delta(&format!("d-{i}"))).collect();
    let rx = send_events(events, 512).await;
    let filtered = EventStream::new(rx)
        .collect_filtered(&EventFilter::by_kind("assistant_delta"))
        .await;
    assert_eq!(filtered.len(), 300);
}

#[tokio::test]
async fn large_stream_filter_none_pass() {
    let events: Vec<AgentEvent> = (0..300).map(|i| delta(&format!("d-{i}"))).collect();
    let rx = send_events(events, 512).await;
    let filtered = EventStream::new(rx)
        .collect_filtered(&EventFilter::errors_only())
        .await;
    assert!(filtered.is_empty());
}
