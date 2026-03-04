// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deep tests for stream processing in abp-stream.

use std::collections::BTreeMap;
use std::sync::Arc;

use abp_core::{AgentEvent, AgentEventKind};
use abp_stream::{
    EventFilter, EventRecorder, EventStats, EventStream, EventTransform, StreamAggregator,
    StreamBuffer, StreamMetrics, StreamPipeline, StreamPipelineBuilder, StreamSummary, StreamTee,
    TeeError, ToolCallAggregate,
};
use chrono::Utc;
use tokio::sync::mpsc;

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

fn message(text: &str) -> AgentEvent {
    make_event(AgentEventKind::AssistantMessage {
        text: text.to_string(),
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

fn tool_call(name: &str, id: &str, input: serde_json::Value) -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: name.to_string(),
        tool_use_id: Some(id.to_string()),
        parent_tool_use_id: None,
        input,
    })
}

fn tool_call_no_id(name: &str, input: serde_json::Value) -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: name.to_string(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input,
    })
}

fn tool_result(name: &str, id: &str, output: serde_json::Value) -> AgentEvent {
    make_event(AgentEventKind::ToolResult {
        tool_name: name.to_string(),
        tool_use_id: Some(id.to_string()),
        output,
        is_error: false,
    })
}

fn tool_result_no_id(name: &str, output: serde_json::Value) -> AgentEvent {
    make_event(AgentEventKind::ToolResult {
        tool_name: name.to_string(),
        tool_use_id: None,
        output,
        is_error: false,
    })
}

fn error_event(msg: &str) -> AgentEvent {
    make_event(AgentEventKind::Error {
        message: msg.to_string(),
        error_code: None,
    })
}

fn warning_event(msg: &str) -> AgentEvent {
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

fn command_executed(cmd: &str, exit_code: i32) -> AgentEvent {
    make_event(AgentEventKind::CommandExecuted {
        command: cmd.to_string(),
        exit_code: Some(exit_code),
        output_preview: None,
    })
}

// ===========================================================================
// 1. StreamAggregator — aggregate text deltas into complete messages
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
fn aggregator_default_equals_new() {
    let agg = StreamAggregator::default();
    assert_eq!(agg.text(), "");
    assert_eq!(agg.event_count(), 0);
}

#[test]
fn aggregator_single_delta() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("hello"));
    assert_eq!(agg.text(), "hello");
    assert_eq!(agg.event_count(), 1);
}

#[test]
fn aggregator_multiple_deltas_concatenate() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("Hello"));
    agg.push(&delta(", "));
    agg.push(&delta("world!"));
    assert_eq!(agg.text(), "Hello, world!");
    assert_eq!(agg.event_count(), 3);
}

#[test]
fn aggregator_assistant_message_appends() {
    let mut agg = StreamAggregator::new();
    agg.push(&message("Complete message."));
    assert_eq!(agg.text(), "Complete message.");
}

#[test]
fn aggregator_mixed_deltas_and_messages() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("part1"));
    agg.push(&message(" part2"));
    assert_eq!(agg.text(), "part1 part2");
}

#[test]
fn aggregator_run_completed_sets_complete() {
    let mut agg = StreamAggregator::new();
    assert!(!agg.is_complete());
    agg.push(&run_completed());
    assert!(agg.is_complete());
}

#[test]
fn aggregator_run_started_does_not_set_complete() {
    let mut agg = StreamAggregator::new();
    agg.push(&run_started());
    assert!(!agg.is_complete());
    assert_eq!(agg.event_count(), 1);
}

#[test]
fn aggregator_errors_collected() {
    let mut agg = StreamAggregator::new();
    agg.push(&error_event("err1"));
    agg.push(&error_event("err2"));
    assert_eq!(agg.errors().len(), 2);
}

#[test]
fn aggregator_non_error_events_not_in_errors() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("ok"));
    agg.push(&warning_event("warn"));
    agg.push(&run_started());
    assert!(agg.errors().is_empty());
}

// ===========================================================================
// 2. ToolCallAggregate — aggregate tool call fragments
// ===========================================================================

#[test]
fn aggregator_tool_call_with_id() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call(
        "read_file",
        "tc1",
        serde_json::json!({"path": "a.txt"}),
    ));
    let calls = agg.tool_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, "tc1");
    assert_eq!(calls[0].name, "read_file");
    assert!(calls[0].result.is_none());
}

#[test]
fn aggregator_tool_call_without_id_generates_key() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call_no_id("search", serde_json::json!({"q": "test"})));
    let calls = agg.tool_calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].id.starts_with("search_"));
}

#[test]
fn aggregator_tool_result_matches_call() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call("read_file", "tc1", serde_json::json!({})));
    agg.push(&tool_result(
        "read_file",
        "tc1",
        serde_json::json!("contents"),
    ));
    let calls = agg.tool_calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].result.is_some());
    assert!(calls[0].result.as_ref().unwrap().contains("contents"));
}

#[test]
fn aggregator_tool_result_no_matching_call_ignored() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_result(
        "read_file",
        "unknown_id",
        serde_json::json!("data"),
    ));
    assert!(agg.tool_calls().is_empty());
}

#[test]
fn aggregator_tool_result_without_id_no_match() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call("x", "tc1", serde_json::json!({})));
    agg.push(&tool_result_no_id("x", serde_json::json!("res")));
    // No tool_use_id on result => no match
    assert!(agg.tool_calls()[0].result.is_none());
}

#[test]
fn aggregator_multiple_tool_calls() {
    let mut agg = StreamAggregator::new();
    agg.push(&tool_call("read", "tc1", serde_json::json!({})));
    agg.push(&tool_call("write", "tc2", serde_json::json!({})));
    agg.push(&tool_call("exec", "tc3", serde_json::json!({})));
    assert_eq!(agg.tool_calls().len(), 3);
    assert_eq!(agg.tool_calls()[0].name, "read");
    assert_eq!(agg.tool_calls()[1].name, "write");
    assert_eq!(agg.tool_calls()[2].name, "exec");
}

#[test]
fn tool_call_aggregate_serde_roundtrip() {
    let tc = ToolCallAggregate {
        id: "tc1".to_string(),
        name: "read_file".to_string(),
        arguments: r#"{"path":"a.txt"}"#.to_string(),
        result: Some("file contents".to_string()),
    };
    let json = serde_json::to_string(&tc).unwrap();
    let deserialized: ToolCallAggregate = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.id, "tc1");
    assert_eq!(deserialized.name, "read_file");
    assert_eq!(deserialized.result.as_deref(), Some("file contents"));
}

// ===========================================================================
// 3. StreamSummary — generate summary statistics from event stream
// ===========================================================================

#[test]
fn summary_empty_aggregator() {
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
fn summary_counts_all_events() {
    let mut agg = StreamAggregator::new();
    agg.push(&run_started());
    agg.push(&delta("Hello"));
    agg.push(&delta(" world"));
    agg.push(&tool_call("t", "tc1", serde_json::json!({})));
    agg.push(&error_event("oops"));
    agg.push(&run_completed());
    let summary = agg.to_summary();
    assert_eq!(summary.total_events, 6);
    assert_eq!(summary.text_length, 11); // "Hello world"
    assert_eq!(summary.tool_call_count, 1);
    assert!(summary.has_errors);
}

#[test]
fn summary_duration_set_after_multiple_events() {
    let mut agg = StreamAggregator::new();
    agg.push(&delta("a"));
    // Duration may be 0ms for fast tests but should be Some
    agg.push(&delta("b"));
    let summary = agg.to_summary();
    assert!(summary.duration_ms.is_some());
}

#[test]
fn summary_serde_roundtrip() {
    let summary = StreamSummary {
        total_events: 42,
        text_length: 1000,
        tool_call_count: 3,
        has_thinking: false,
        has_errors: true,
        duration_ms: Some(500),
    };
    let json = serde_json::to_string(&summary).unwrap();
    let deserialized: StreamSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.total_events, 42);
    assert_eq!(deserialized.text_length, 1000);
    assert_eq!(deserialized.tool_call_count, 3);
    assert!(deserialized.has_errors);
    assert_eq!(deserialized.duration_ms, Some(500));
}

// ===========================================================================
// 4. StreamTee — broadcast events to multiple receivers
// ===========================================================================

#[tokio::test]
async fn tee_broadcasts_to_all_receivers() {
    let (tx1, mut rx1) = mpsc::channel(16);
    let (tx2, mut rx2) = mpsc::channel(16);
    let tee = StreamTee::new(vec![tx1, tx2]);

    tee.send(&delta("hello")).await.unwrap();

    let ev1 = rx1.recv().await.unwrap();
    let ev2 = rx2.recv().await.unwrap();
    assert!(matches!(ev1.kind, AgentEventKind::AssistantDelta { ref text } if text == "hello"));
    assert!(matches!(ev2.kind, AgentEventKind::AssistantDelta { ref text } if text == "hello"));
}

#[tokio::test]
async fn tee_three_receivers() {
    let (tx1, mut rx1) = mpsc::channel(16);
    let (tx2, mut rx2) = mpsc::channel(16);
    let (tx3, mut rx3) = mpsc::channel(16);
    let tee = StreamTee::new(vec![tx1, tx2, tx3]);

    tee.send(&delta("abc")).await.unwrap();

    assert!(rx1.recv().await.is_some());
    assert!(rx2.recv().await.is_some());
    assert!(rx3.recv().await.is_some());
}

#[tokio::test]
async fn tee_empty_senders_ok() {
    let tee = StreamTee::new(vec![]);
    let result = tee.send(&delta("x")).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn tee_active_count() {
    let (tx1, rx1) = mpsc::channel::<AgentEvent>(16);
    let (tx2, _rx2) = mpsc::channel::<AgentEvent>(16);
    let tee = StreamTee::new(vec![tx1, tx2]);
    assert_eq!(tee.active_count(), 2);
    drop(rx1);
    // After dropping rx1, one sender becomes closed
    assert_eq!(tee.active_count(), 1);
}

// ===========================================================================
// 5. TeeError handling — AllReceiversClosed, individual receiver failures
// ===========================================================================

#[tokio::test]
async fn tee_all_receivers_closed_error() {
    let (tx1, rx1) = mpsc::channel::<AgentEvent>(16);
    let (tx2, rx2) = mpsc::channel::<AgentEvent>(16);
    let tee = StreamTee::new(vec![tx1, tx2]);
    drop(rx1);
    drop(rx2);

    let result = tee.send(&delta("fail")).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), TeeError::AllReceiversClosed));
}

#[tokio::test]
async fn tee_partial_receiver_closed_still_ok() {
    let (tx1, rx1) = mpsc::channel::<AgentEvent>(16);
    let (tx2, mut rx2) = mpsc::channel::<AgentEvent>(16);
    let tee = StreamTee::new(vec![tx1, tx2]);
    drop(rx1);

    // One receiver is still open, so send succeeds
    let result = tee.send(&delta("partial")).await;
    assert!(result.is_ok());
    let ev = rx2.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { ref text } if text == "partial"));
}

#[tokio::test]
async fn tee_error_display() {
    let err = TeeError::AllReceiversClosed;
    let msg = format!("{err}");
    assert!(msg.contains("closed"));
}

#[tokio::test]
async fn tee_error_is_std_error() {
    let err = TeeError::AllReceiversClosed;
    let _: &dyn std::error::Error = &err;
}

// ===========================================================================
// 6. Event filtering — filter events by kind
// ===========================================================================

#[test]
fn filter_by_kind_assistant_delta() {
    let filter = EventFilter::by_kind("assistant_delta");
    assert!(filter.matches(&delta("text")));
    assert!(!filter.matches(&error_event("err")));
    assert!(!filter.matches(&run_started()));
}

#[test]
fn filter_by_kind_error() {
    let filter = EventFilter::by_kind("error");
    assert!(filter.matches(&error_event("bad")));
    assert!(!filter.matches(&delta("ok")));
}

#[test]
fn filter_by_kind_tool_call() {
    let filter = EventFilter::by_kind("tool_call");
    assert!(filter.matches(&tool_call("t", "id", serde_json::json!({}))));
    assert!(!filter.matches(&delta("x")));
}

#[test]
fn filter_errors_only_accepts_only_errors() {
    let filter = EventFilter::errors_only();
    assert!(filter.matches(&error_event("e")));
    assert!(!filter.matches(&delta("d")));
    assert!(!filter.matches(&warning_event("w")));
    assert!(!filter.matches(&run_started()));
    assert!(!filter.matches(&file_changed("f.txt")));
}

#[test]
fn filter_exclude_errors_rejects_errors() {
    let filter = EventFilter::exclude_errors();
    assert!(!filter.matches(&error_event("e")));
    assert!(filter.matches(&delta("d")));
    assert!(filter.matches(&warning_event("w")));
}

#[test]
fn filter_custom_predicate_text_length() {
    let filter = EventFilter::new(
        |ev| matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text.len() >= 5),
    );
    assert!(filter.matches(&delta("hello")));
    assert!(!filter.matches(&delta("hi")));
}

#[test]
fn filter_by_kind_warning() {
    let filter = EventFilter::by_kind("warning");
    assert!(filter.matches(&warning_event("w")));
    assert!(!filter.matches(&error_event("e")));
}

#[test]
fn filter_by_kind_file_changed() {
    let filter = EventFilter::by_kind("file_changed");
    assert!(filter.matches(&file_changed("test.rs")));
    assert!(!filter.matches(&delta("x")));
}

#[test]
fn filter_by_kind_command_executed() {
    let filter = EventFilter::by_kind("command_executed");
    assert!(filter.matches(&command_executed("ls", 0)));
    assert!(!filter.matches(&delta("x")));
}

#[tokio::test]
async fn filter_applied_via_event_stream() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(error_event("e")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    tx.send(warning_event("w")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let filter = EventFilter::by_kind("assistant_delta");
    let events = stream.collect_filtered(&filter).await;
    assert_eq!(events.len(), 2);
}

// ===========================================================================
// 7. Event transformation — transform events (redact, timestamp, etc.)
// ===========================================================================

#[test]
fn transform_identity_preserves_kind() {
    let t = EventTransform::identity();
    let ev = delta("hello");
    let result = t.apply(ev.clone());
    assert!(matches!(result.kind, AgentEventKind::AssistantDelta { ref text } if text == "hello"));
}

#[test]
fn transform_add_metadata_to_ext() {
    let t = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("source".to_string(), serde_json::json!("test"));
        ev
    });
    let result = t.apply(delta("x"));
    assert!(result.ext.is_some());
    assert_eq!(
        result.ext.unwrap().get("source").unwrap(),
        &serde_json::json!("test")
    );
}

#[test]
fn transform_redact_error_message() {
    let t = EventTransform::new(|mut ev| {
        if let AgentEventKind::Error {
            ref mut message, ..
        } = ev.kind
        {
            *message = "[REDACTED]".to_string();
        }
        ev
    });
    let result = t.apply(error_event("secret key: abc123"));
    if let AgentEventKind::Error { message, .. } = &result.kind {
        assert_eq!(message, "[REDACTED]");
    } else {
        panic!("expected Error event");
    }
}

#[test]
fn transform_add_timestamp_ext() {
    let t = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert(
            "processed_at".to_string(),
            serde_json::json!(Utc::now().to_rfc3339()),
        );
        ev
    });
    let result = t.apply(delta("x"));
    assert!(result.ext.as_ref().unwrap().contains_key("processed_at"));
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
        ext.insert("step2".to_string(), serde_json::json!(2));
        ev
    });
    let result = t2.apply(t1.apply(delta("x")));
    let ext = result.ext.unwrap();
    assert_eq!(ext.get("step").unwrap(), &serde_json::json!(1));
    assert_eq!(ext.get("step2").unwrap(), &serde_json::json!(2));
}

#[test]
fn transform_non_error_passthrough() {
    let t = EventTransform::new(|mut ev| {
        if let AgentEventKind::Error {
            ref mut message, ..
        } = ev.kind
        {
            *message = "[REDACTED]".to_string();
        }
        ev
    });
    // Non-error events pass through untouched
    let result = t.apply(delta("hello"));
    assert!(matches!(result.kind, AgentEventKind::AssistantDelta { ref text } if text == "hello"));
}

// ===========================================================================
// 8. Backpressure — test behavior when receivers are slow
// ===========================================================================

#[tokio::test]
async fn tee_with_bounded_channel_slow_receiver() {
    // Channel capacity 1 means backpressure after first message
    let (tx1, mut rx1) = mpsc::channel(1);
    let (tx2, mut rx2) = mpsc::channel(1);
    let tee = StreamTee::new(vec![tx1, tx2]);

    tee.send(&delta("first")).await.unwrap();

    // Drain both receivers to make room
    let _ = rx1.recv().await;
    let _ = rx2.recv().await;

    // Should succeed now
    tee.send(&delta("second")).await.unwrap();
    assert!(rx1.recv().await.is_some());
    assert!(rx2.recv().await.is_some());
}

#[tokio::test]
async fn stream_buffer_acts_as_ring() {
    let mut buf = StreamBuffer::new(3);
    buf.push(delta("a"));
    buf.push(delta("b"));
    buf.push(delta("c"));
    buf.push(delta("d")); // evicts "a"

    assert_eq!(buf.len(), 3);
    assert!(buf.is_full());
    let recent = buf.recent(3);
    assert_eq!(recent.len(), 3);
    // First should be "b" since "a" was evicted
    assert!(matches!(&recent[0].kind, AgentEventKind::AssistantDelta { text } if text == "b"));
}

#[tokio::test]
async fn buffer_drain_empties() {
    let mut buf = StreamBuffer::new(5);
    buf.push(delta("a"));
    buf.push(delta("b"));
    let drained = buf.drain();
    assert_eq!(drained.len(), 2);
    assert!(buf.is_empty());
}

// ===========================================================================
// 9. Ordering guarantees — events arrive in order
// ===========================================================================

#[tokio::test]
async fn tee_preserves_event_order() {
    let (tx, mut rx) = mpsc::channel(32);
    let tee = StreamTee::new(vec![tx]);

    for i in 0..10 {
        tee.send(&delta(&format!("msg-{i}"))).await.unwrap();
    }

    for i in 0..10 {
        let ev = rx.recv().await.unwrap();
        if let AgentEventKind::AssistantDelta { text } = &ev.kind {
            assert_eq!(text, &format!("msg-{i}"));
        } else {
            panic!("unexpected event kind");
        }
    }
}

#[test]
fn aggregator_preserves_text_order() {
    let mut agg = StreamAggregator::new();
    for i in 0..20 {
        agg.push(&delta(&format!("[{i}]")));
    }
    let text = agg.text().to_string();
    for i in 0..20 {
        assert!(text.contains(&format!("[{i}]")));
    }
    // Verify concatenation order
    assert!(text.starts_with("[0]"));
    assert!(text.ends_with("[19]"));
}

#[test]
fn aggregator_tool_calls_preserve_order() {
    let mut agg = StreamAggregator::new();
    for i in 0..5 {
        agg.push(&tool_call(
            &format!("tool_{i}"),
            &format!("tc{i}"),
            serde_json::json!({}),
        ));
    }
    for (i, tc) in agg.tool_calls().iter().enumerate() {
        assert_eq!(tc.name, format!("tool_{i}"));
    }
}

// ===========================================================================
// 10. Empty streams — handle zero-event streams gracefully
// ===========================================================================

#[test]
fn aggregator_empty_summary() {
    let agg = StreamAggregator::new();
    let summary = agg.to_summary();
    assert_eq!(summary.total_events, 0);
    assert_eq!(summary.text_length, 0);
    assert_eq!(summary.tool_call_count, 0);
    assert!(!summary.has_thinking);
    assert!(!summary.has_errors);
    assert!(summary.duration_ms.is_none());
}

#[tokio::test]
async fn tee_empty_stream_no_events() {
    let (tx, mut rx) = mpsc::channel(16);
    let _tee = StreamTee::new(vec![tx]);
    // Don't send anything, just drop the tee
    drop(_tee);
    assert!(rx.recv().await.is_none());
}

#[tokio::test]
async fn event_stream_empty_collect() {
    let (_tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(_tx);
    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    assert!(events.is_empty());
}

#[test]
fn metrics_empty_summary() {
    let m = StreamMetrics::new();
    let s = m.summary();
    assert_eq!(s.event_count, 0);
    assert_eq!(s.total_bytes, 0);
    assert!(s.event_type_counts.is_empty());
}

#[test]
fn buffer_empty_recent() {
    let buf = StreamBuffer::new(5);
    assert!(buf.recent(10).is_empty());
    assert!(buf.is_empty());
    assert!(!buf.is_full());
}

#[test]
fn recorder_empty_events() {
    let r = EventRecorder::new();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
    assert!(r.events().is_empty());
}

// ===========================================================================
// 11. Large streams — handle many events (1000+) without issues
// ===========================================================================

#[test]
fn aggregator_handles_1000_deltas() {
    let mut agg = StreamAggregator::new();
    for i in 0..1000 {
        agg.push(&delta(&format!("chunk{i}")));
    }
    assert_eq!(agg.event_count(), 1000);
    assert!(agg.text().contains("chunk0"));
    assert!(agg.text().contains("chunk999"));
    let summary = agg.to_summary();
    assert_eq!(summary.total_events, 1000);
    assert!(summary.text_length > 0);
}

#[test]
fn aggregator_handles_many_tool_calls() {
    let mut agg = StreamAggregator::new();
    for i in 0..200 {
        agg.push(&tool_call(
            &format!("tool_{i}"),
            &format!("tc_{i}"),
            serde_json::json!({"index": i}),
        ));
    }
    assert_eq!(agg.tool_calls().len(), 200);
    let summary = agg.to_summary();
    assert_eq!(summary.tool_call_count, 200);
}

#[tokio::test]
async fn tee_handles_1000_events() {
    let (tx1, mut rx1) = mpsc::channel(1024);
    let (tx2, mut rx2) = mpsc::channel(1024);
    let tee = StreamTee::new(vec![tx1, tx2]);

    for i in 0..1000 {
        tee.send(&delta(&format!("ev{i}"))).await.unwrap();
    }

    for _ in 0..1000 {
        assert!(rx1.recv().await.is_some());
        assert!(rx2.recv().await.is_some());
    }
}

#[test]
fn metrics_handles_large_stream() {
    let mut m = StreamMetrics::new();
    for i in 0..2000 {
        m.record_event(&delta(&format!("d{i}")));
    }
    assert_eq!(m.event_count(), 2000);
    assert!(m.total_bytes() > 0);
}

#[test]
fn buffer_eviction_under_large_load() {
    let mut buf = StreamBuffer::new(100);
    for i in 0..500 {
        buf.push(delta(&format!("ev{i}")));
    }
    assert_eq!(buf.len(), 100);
    assert!(buf.is_full());
    // Most recent should be ev499
    let last = buf.recent(1);
    assert!(matches!(
        &last[0].kind,
        AgentEventKind::AssistantDelta { text } if text == "ev499"
    ));
}

#[test]
fn stats_handles_large_stream() {
    let stats = EventStats::new();
    for _ in 0..1500 {
        stats.observe(&delta("x"));
    }
    for _ in 0..500 {
        stats.observe(&error_event("e"));
    }
    assert_eq!(stats.total_events(), 2000);
    assert_eq!(stats.count_for("assistant_delta"), 1500);
    assert_eq!(stats.count_for("error"), 500);
    assert_eq!(stats.error_count(), 500);
}

// ===========================================================================
// 12. Serde roundtrip — stream-related types serialize correctly
// ===========================================================================

#[test]
fn tool_call_aggregate_serde() {
    let tc = ToolCallAggregate {
        id: "tc-42".to_string(),
        name: "write_file".to_string(),
        arguments: r#"{"path":"out.txt","content":"hi"}"#.to_string(),
        result: None,
    };
    let json = serde_json::to_string(&tc).unwrap();
    let back: ToolCallAggregate = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "tc-42");
    assert_eq!(back.name, "write_file");
    assert!(back.result.is_none());
}

#[test]
fn stream_summary_serde_with_none_duration() {
    let summary = StreamSummary {
        total_events: 0,
        text_length: 0,
        tool_call_count: 0,
        has_thinking: false,
        has_errors: false,
        duration_ms: None,
    };
    let json = serde_json::to_string(&summary).unwrap();
    let back: StreamSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(back.total_events, 0);
    assert!(back.duration_ms.is_none());
}

#[test]
fn stream_summary_serde_full() {
    let summary = StreamSummary {
        total_events: 100,
        text_length: 5000,
        tool_call_count: 7,
        has_thinking: true,
        has_errors: true,
        duration_ms: Some(12345),
    };
    let json = serde_json::to_string(&summary).unwrap();
    let back: StreamSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(back.total_events, 100);
    assert_eq!(back.text_length, 5000);
    assert_eq!(back.tool_call_count, 7);
    assert!(back.has_thinking);
    assert!(back.has_errors);
    assert_eq!(back.duration_ms, Some(12345));
}

#[test]
fn tool_call_aggregate_with_result_serde() {
    let tc = ToolCallAggregate {
        id: "tc-1".to_string(),
        name: "bash".to_string(),
        arguments: r#"{"cmd":"echo hi"}"#.to_string(),
        result: Some("hi\n".to_string()),
    };
    let json = serde_json::to_string(&tc).unwrap();
    assert!(json.contains("hi\\n"));
    let back: ToolCallAggregate = serde_json::from_str(&json).unwrap();
    assert_eq!(back.result.as_deref(), Some("hi\n"));
}

// ===========================================================================
// 13. Concurrent access — multiple tasks aggregating simultaneously
// ===========================================================================

#[tokio::test]
async fn concurrent_stats_observation() {
    let stats = EventStats::new();
    let stats_clone = stats.clone();

    let t1 = tokio::spawn({
        let s = stats.clone();
        async move {
            for _ in 0..500 {
                s.observe(&delta("x"));
            }
        }
    });

    let t2 = tokio::spawn(async move {
        for _ in 0..500 {
            stats_clone.observe(&error_event("e"));
        }
    });

    t1.await.unwrap();
    t2.await.unwrap();

    assert_eq!(stats.total_events(), 1000);
    assert_eq!(stats.count_for("assistant_delta"), 500);
    assert_eq!(stats.error_count(), 500);
}

#[tokio::test]
async fn concurrent_recorder_access() {
    let recorder = EventRecorder::new();
    let r1 = recorder.clone();
    let r2 = recorder.clone();

    let t1 = tokio::spawn(async move {
        for i in 0..100 {
            r1.record(&delta(&format!("r1-{i}")));
        }
    });

    let t2 = tokio::spawn(async move {
        for i in 0..100 {
            r2.record(&delta(&format!("r2-{i}")));
        }
    });

    t1.await.unwrap();
    t2.await.unwrap();

    assert_eq!(recorder.len(), 200);
}

#[tokio::test]
async fn concurrent_tee_sends() {
    let (tx1, mut rx1) = mpsc::channel(256);
    let (tx2, mut rx2) = mpsc::channel(256);
    let tee = Arc::new(StreamTee::new(vec![tx1, tx2]));

    let tee1 = Arc::clone(&tee);
    let t1 = tokio::spawn(async move {
        for i in 0..50 {
            tee1.send(&delta(&format!("a-{i}"))).await.unwrap();
        }
    });

    let tee2 = Arc::clone(&tee);
    let t2 = tokio::spawn(async move {
        for i in 0..50 {
            tee2.send(&delta(&format!("b-{i}"))).await.unwrap();
        }
    });

    t1.await.unwrap();
    t2.await.unwrap();

    let mut count1 = 0;
    while rx1.try_recv().is_ok() {
        count1 += 1;
    }
    let mut count2 = 0;
    while rx2.try_recv().is_ok() {
        count2 += 1;
    }
    assert_eq!(count1, 100);
    assert_eq!(count2, 100);
}

// ===========================================================================
// Additional coverage: StreamPipeline integration
// ===========================================================================

#[test]
fn pipeline_filter_then_transform() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("processed".to_string(), serde_json::json!(true));
            ev
        }))
        .build();

    // Error is filtered out
    assert!(pipeline.process(error_event("bad")).is_none());

    // Delta passes through and gets transformed
    let result = pipeline.process(delta("good")).unwrap();
    assert!(result.ext.is_some());
    assert_eq!(
        result.ext.unwrap().get("processed").unwrap(),
        &serde_json::json!(true)
    );
}

#[test]
fn pipeline_with_stats_and_recorder() {
    let stats = EventStats::new();
    let recorder = EventRecorder::new();
    let pipeline = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .with_recorder(recorder.clone())
        .build();

    pipeline.process(delta("a"));
    pipeline.process(delta("b"));
    pipeline.process(error_event("e"));

    assert_eq!(stats.total_events(), 3);
    assert_eq!(recorder.len(), 3);
}

#[test]
fn pipeline_empty_passthrough() {
    let pipeline = StreamPipeline::new();
    let ev = delta("passthrough");
    let result = pipeline.process(ev);
    assert!(result.is_some());
    assert!(matches!(
        result.unwrap().kind,
        AgentEventKind::AssistantDelta { ref text } if text == "passthrough"
    ));
}

#[test]
fn pipeline_multiple_filters_all_must_pass() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .filter(EventFilter::by_kind("assistant_delta"))
        .build();

    // Delta passes both filters
    assert!(pipeline.process(delta("ok")).is_some());
    // Error fails first filter
    assert!(pipeline.process(error_event("e")).is_none());
    // Warning passes first but fails second
    assert!(pipeline.process(warning_event("w")).is_none());
}

#[tokio::test]
async fn pipeline_pipe_through_stream() {
    let (src_tx, src_rx) = mpsc::channel(16);
    let (dst_tx, dst_rx) = mpsc::channel(16);

    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .build();

    src_tx.send(delta("a")).await.unwrap();
    src_tx.send(error_event("e")).await.unwrap();
    src_tx.send(delta("b")).await.unwrap();
    drop(src_tx);

    let stream = EventStream::new(src_rx);
    stream.pipe(&pipeline, dst_tx).await;

    let out = EventStream::new(dst_rx);
    let events = out.collect_all().await;
    assert_eq!(events.len(), 2);
}

// ===========================================================================
// Additional: event_kind_name coverage
// ===========================================================================

#[test]
fn event_kind_name_all_variants() {
    use abp_stream::event_kind_name;

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
            summary: String::new()
        }),
        "file_changed"
    );
    assert_eq!(
        event_kind_name(&AgentEventKind::CommandExecuted {
            command: String::new(),
            exit_code: None,
            output_preview: None
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
            error_code: None
        }),
        "error"
    );
}

// ===========================================================================
// Additional: MetricsSummary Display
// ===========================================================================

#[test]
fn metrics_summary_display_format() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta("hello"));
    m.record_event(&error_event("e"));
    let summary = m.summary();
    let display = format!("{summary}");
    assert!(display.contains("Events: 2"));
    assert!(display.contains("Bytes: 5"));
}

#[test]
fn buffer_capacity_reports_correct_value() {
    let buf = StreamBuffer::new(42);
    assert_eq!(buf.capacity(), 42);
}

#[test]
fn buffer_recent_returns_fewer_when_underfilled() {
    let mut buf = StreamBuffer::new(10);
    buf.push(delta("only"));
    let recent = buf.recent(5);
    assert_eq!(recent.len(), 1);
}
