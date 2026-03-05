#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

//! Exhaustive event streaming tests for the AgentEvent pipeline.
//!
//! Covers: variant creation, serde round-trips, mpsc streaming, filtering,
//! transformation, aggregation, timeline reconstruction, concurrency,
//! backpressure, metadata propagation, large streams, and deduplication.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use abp_core::aggregate::{AggregationSummary, EventAggregator, RunAnalytics};
use abp_core::ext::{AgentEventExt, ReceiptExt};
use abp_core::filter::EventFilter;
use abp_core::stream::EventStream;
use abp_core::{AgentEvent, AgentEventKind};
use abp_error::ErrorCode;

// ───────────────────────────── helpers ─────────────────────────────

fn ts(offset_ms: i64) -> DateTime<Utc> {
    let base = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .to_utc();
    base + TimeDelta::milliseconds(offset_ms)
}

fn ev(offset_ms: i64, kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: ts(offset_ms),
        kind,
        ext: None,
    }
}

fn ev_now(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn ev_ext(offset_ms: i64, kind: AgentEventKind, ext: BTreeMap<String, Value>) -> AgentEvent {
    AgentEvent {
        ts: ts(offset_ms),
        kind,
        ext: Some(ext),
    }
}

fn run_started(ms: i64) -> AgentEvent {
    ev(
        ms,
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
    )
}
fn run_completed(ms: i64) -> AgentEvent {
    ev(
        ms,
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
    )
}
fn delta(ms: i64, text: &str) -> AgentEvent {
    ev(ms, AgentEventKind::AssistantDelta { text: text.into() })
}
fn assistant_msg(ms: i64, text: &str) -> AgentEvent {
    ev(ms, AgentEventKind::AssistantMessage { text: text.into() })
}
fn tool_call(ms: i64, name: &str, id: Option<&str>, input: Value) -> AgentEvent {
    ev(
        ms,
        AgentEventKind::ToolCall {
            tool_name: name.into(),
            tool_use_id: id.map(Into::into),
            parent_tool_use_id: None,
            input,
        },
    )
}
fn tool_result(ms: i64, name: &str, id: Option<&str>, output: Value, is_error: bool) -> AgentEvent {
    ev(
        ms,
        AgentEventKind::ToolResult {
            tool_name: name.into(),
            tool_use_id: id.map(Into::into),
            output,
            is_error,
        },
    )
}
fn file_changed(ms: i64, path: &str, summary: &str) -> AgentEvent {
    ev(
        ms,
        AgentEventKind::FileChanged {
            path: path.into(),
            summary: summary.into(),
        },
    )
}
fn cmd_exec(ms: i64, cmd: &str, exit_code: Option<i32>) -> AgentEvent {
    ev(
        ms,
        AgentEventKind::CommandExecuted {
            command: cmd.into(),
            exit_code,
            output_preview: None,
        },
    )
}
fn warning(ms: i64, msg: &str) -> AgentEvent {
    ev(
        ms,
        AgentEventKind::Warning {
            message: msg.into(),
        },
    )
}
fn error_ev(ms: i64, msg: &str) -> AgentEvent {
    ev(
        ms,
        AgentEventKind::Error {
            message: msg.into(),
            error_code: None,
        },
    )
}
fn error_with_code(ms: i64, msg: &str, code: ErrorCode) -> AgentEvent {
    ev(
        ms,
        AgentEventKind::Error {
            message: msg.into(),
            error_code: Some(code),
        },
    )
}

/// Build a "standard" run trace: start, deltas, message, tool call+result, done.
fn standard_trace() -> Vec<AgentEvent> {
    vec![
        run_started(0),
        delta(10, "Hel"),
        delta(20, "lo"),
        assistant_msg(30, "Hello"),
        tool_call(40, "read_file", Some("t1"), json!({"path": "foo.rs"})),
        tool_result(50, "read_file", Some("t1"), json!("contents"), false),
        file_changed(60, "foo.rs", "edited"),
        cmd_exec(70, "cargo build", Some(0)),
        warning(80, "unused variable"),
        run_completed(100),
    ]
}

// ═══════════════════════════════════════════════════════════════════
// 1. AgentEventKind variant creation
// ═══════════════════════════════════════════════════════════════════

#[test]
fn create_run_started() {
    let e = run_started(0);
    assert!(matches!(e.kind, AgentEventKind::RunStarted { .. }));
}

#[test]
fn create_run_completed() {
    let e = run_completed(0);
    assert!(matches!(e.kind, AgentEventKind::RunCompleted { .. }));
}

#[test]
fn create_assistant_delta() {
    let e = delta(0, "hi");
    assert!(matches!(e.kind, AgentEventKind::AssistantDelta { .. }));
}

#[test]
fn create_assistant_message() {
    let e = assistant_msg(0, "hello");
    assert!(matches!(e.kind, AgentEventKind::AssistantMessage { .. }));
}

#[test]
fn create_tool_call_with_id() {
    let e = tool_call(0, "edit", Some("tc1"), json!({"file": "a.rs"}));
    if let AgentEventKind::ToolCall { tool_use_id, .. } = &e.kind {
        assert_eq!(tool_use_id.as_deref(), Some("tc1"));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn create_tool_call_without_id() {
    let e = tool_call(0, "edit", None, json!(null));
    if let AgentEventKind::ToolCall { tool_use_id, .. } = &e.kind {
        assert!(tool_use_id.is_none());
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn create_tool_call_with_parent() {
    let e = ev(
        0,
        AgentEventKind::ToolCall {
            tool_name: "inner".into(),
            tool_use_id: Some("c2".into()),
            parent_tool_use_id: Some("c1".into()),
            input: json!({}),
        },
    );
    if let AgentEventKind::ToolCall {
        parent_tool_use_id, ..
    } = &e.kind
    {
        assert_eq!(parent_tool_use_id.as_deref(), Some("c1"));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn create_tool_result_success() {
    let e = tool_result(0, "grep", Some("t1"), json!(["match"]), false);
    if let AgentEventKind::ToolResult { is_error, .. } = &e.kind {
        assert!(!is_error);
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn create_tool_result_error() {
    let e = tool_result(0, "grep", None, json!("not found"), true);
    if let AgentEventKind::ToolResult { is_error, .. } = &e.kind {
        assert!(is_error);
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn create_file_changed() {
    let e = file_changed(0, "src/main.rs", "added fn main");
    if let AgentEventKind::FileChanged { path, summary } = &e.kind {
        assert_eq!(path, "src/main.rs");
        assert_eq!(summary, "added fn main");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn create_command_executed() {
    let e = cmd_exec(0, "ls -la", Some(0));
    if let AgentEventKind::CommandExecuted {
        command, exit_code, ..
    } = &e.kind
    {
        assert_eq!(command, "ls -la");
        assert_eq!(*exit_code, Some(0));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn create_command_executed_no_exit_code() {
    let e = cmd_exec(0, "sleep 100", None);
    if let AgentEventKind::CommandExecuted { exit_code, .. } = &e.kind {
        assert!(exit_code.is_none());
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn create_command_with_output_preview() {
    let e = ev(
        0,
        AgentEventKind::CommandExecuted {
            command: "cat foo".into(),
            exit_code: Some(0),
            output_preview: Some("file contents…".into()),
        },
    );
    if let AgentEventKind::CommandExecuted { output_preview, .. } = &e.kind {
        assert_eq!(output_preview.as_deref(), Some("file contents…"));
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn create_warning() {
    let e = warning(0, "careful");
    assert!(matches!(e.kind, AgentEventKind::Warning { .. }));
}

#[test]
fn create_error_without_code() {
    let e = error_ev(0, "boom");
    if let AgentEventKind::Error { error_code, .. } = &e.kind {
        assert!(error_code.is_none());
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn create_error_with_code() {
    let e = error_with_code(0, "timeout", ErrorCode::BackendTimeout);
    if let AgentEventKind::Error { error_code, .. } = &e.kind {
        assert_eq!(*error_code, Some(ErrorCode::BackendTimeout));
    } else {
        panic!("wrong variant");
    }
}

// ═══════════════════════════════════════════════════════════════════
// 2. Serde round-trips
// ═══════════════════════════════════════════════════════════════════

fn serde_roundtrip(event: &AgentEvent) -> AgentEvent {
    let json = serde_json::to_string(event).expect("serialize");
    serde_json::from_str(&json).expect("deserialize")
}

#[test]
fn serde_run_started() {
    let e = run_started(0);
    let rt = serde_roundtrip(&e);
    assert!(matches!(rt.kind, AgentEventKind::RunStarted { .. }));
    assert_eq!(e.ts, rt.ts);
}

#[test]
fn serde_run_completed() {
    let e = run_completed(100);
    let rt = serde_roundtrip(&e);
    assert!(matches!(rt.kind, AgentEventKind::RunCompleted { .. }));
}

#[test]
fn serde_assistant_delta() {
    let e = delta(5, "chunk");
    let rt = serde_roundtrip(&e);
    if let AgentEventKind::AssistantDelta { text } = &rt.kind {
        assert_eq!(text, "chunk");
    } else {
        panic!("wrong variant after roundtrip");
    }
}

#[test]
fn serde_assistant_message() {
    let e = assistant_msg(10, "full message");
    let rt = serde_roundtrip(&e);
    if let AgentEventKind::AssistantMessage { text } = &rt.kind {
        assert_eq!(text, "full message");
    } else {
        panic!("wrong variant after roundtrip");
    }
}

#[test]
fn serde_tool_call() {
    let e = tool_call(20, "edit", Some("tc1"), json!({"file": "a.rs", "line": 42}));
    let rt = serde_roundtrip(&e);
    if let AgentEventKind::ToolCall {
        tool_name,
        tool_use_id,
        input,
        ..
    } = &rt.kind
    {
        assert_eq!(tool_name, "edit");
        assert_eq!(tool_use_id.as_deref(), Some("tc1"));
        assert_eq!(input["file"], "a.rs");
        assert_eq!(input["line"], 42);
    } else {
        panic!("wrong variant after roundtrip");
    }
}

#[test]
fn serde_tool_result() {
    let e = tool_result(30, "edit", Some("tc1"), json!({"ok": true}), false);
    let rt = serde_roundtrip(&e);
    if let AgentEventKind::ToolResult {
        output, is_error, ..
    } = &rt.kind
    {
        assert_eq!(output["ok"], true);
        assert!(!is_error);
    } else {
        panic!("wrong variant after roundtrip");
    }
}

#[test]
fn serde_tool_result_is_error() {
    let e = tool_result(30, "bash", None, json!("permission denied"), true);
    let rt = serde_roundtrip(&e);
    if let AgentEventKind::ToolResult { is_error, .. } = &rt.kind {
        assert!(is_error);
    } else {
        panic!("wrong variant after roundtrip");
    }
}

#[test]
fn serde_file_changed() {
    let e = file_changed(40, "lib.rs", "refactored");
    let rt = serde_roundtrip(&e);
    if let AgentEventKind::FileChanged { path, summary } = &rt.kind {
        assert_eq!(path, "lib.rs");
        assert_eq!(summary, "refactored");
    } else {
        panic!("wrong variant after roundtrip");
    }
}

#[test]
fn serde_command_executed() {
    let e = cmd_exec(50, "cargo test", Some(0));
    let rt = serde_roundtrip(&e);
    if let AgentEventKind::CommandExecuted {
        command, exit_code, ..
    } = &rt.kind
    {
        assert_eq!(command, "cargo test");
        assert_eq!(*exit_code, Some(0));
    } else {
        panic!("wrong variant after roundtrip");
    }
}

#[test]
fn serde_warning() {
    let e = warning(60, "heads up");
    let rt = serde_roundtrip(&e);
    if let AgentEventKind::Warning { message } = &rt.kind {
        assert_eq!(message, "heads up");
    } else {
        panic!("wrong variant after roundtrip");
    }
}

#[test]
fn serde_error_without_code() {
    let e = error_ev(70, "oops");
    let rt = serde_roundtrip(&e);
    if let AgentEventKind::Error {
        message,
        error_code,
    } = &rt.kind
    {
        assert_eq!(message, "oops");
        assert!(error_code.is_none());
    } else {
        panic!("wrong variant after roundtrip");
    }
}

#[test]
fn serde_error_with_code() {
    let e = error_with_code(70, "fail", ErrorCode::BackendCrashed);
    let rt = serde_roundtrip(&e);
    if let AgentEventKind::Error { error_code, .. } = &rt.kind {
        assert_eq!(*error_code, Some(ErrorCode::BackendCrashed));
    } else {
        panic!("wrong variant after roundtrip");
    }
}

#[test]
fn serde_event_type_tag_present() {
    let e = run_started(0);
    let v: Value = serde_json::to_value(&e).unwrap();
    assert_eq!(v["type"], "run_started");
}

#[test]
fn serde_all_variant_type_tags() {
    let cases: Vec<(&str, AgentEvent)> = vec![
        ("run_started", run_started(0)),
        ("run_completed", run_completed(0)),
        ("assistant_delta", delta(0, "x")),
        ("assistant_message", assistant_msg(0, "x")),
        ("tool_call", tool_call(0, "t", None, json!(null))),
        ("tool_result", tool_result(0, "t", None, json!(null), false)),
        ("file_changed", file_changed(0, "p", "s")),
        ("command_executed", cmd_exec(0, "c", None)),
        ("warning", warning(0, "w")),
        ("error", error_ev(0, "e")),
    ];
    for (expected_tag, event) in cases {
        let v: Value = serde_json::to_value(&event).unwrap();
        assert_eq!(v["type"], expected_tag, "tag mismatch for {expected_tag}");
    }
}

#[test]
fn serde_ext_field_none_omitted() {
    let e = run_started(0);
    let json = serde_json::to_string(&e).unwrap();
    assert!(!json.contains("ext"), "ext should be omitted when None");
}

#[test]
fn serde_ext_field_round_trip() {
    let mut ext = BTreeMap::new();
    ext.insert("raw_message".into(), json!({"vendor": "test"}));
    let e = ev_ext(
        0,
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext,
    );
    let rt = serde_roundtrip(&e);
    assert!(rt.ext.is_some());
    let ext = rt.ext.unwrap();
    assert_eq!(ext["raw_message"]["vendor"], "test");
}

#[test]
fn serde_deserialize_from_raw_json() {
    let raw = r#"{"ts":"2025-01-01T00:00:00Z","type":"assistant_message","text":"hi"}"#;
    let e: AgentEvent = serde_json::from_str(raw).unwrap();
    assert!(matches!(e.kind, AgentEventKind::AssistantMessage { .. }));
}

#[test]
fn serde_empty_string_fields() {
    let e = ev(0, AgentEventKind::RunStarted { message: "".into() });
    let rt = serde_roundtrip(&e);
    if let AgentEventKind::RunStarted { message } = &rt.kind {
        assert_eq!(message, "");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn serde_unicode_text() {
    let e = assistant_msg(0, "こんにちは 🌍 émojis");
    let rt = serde_roundtrip(&e);
    if let AgentEventKind::AssistantMessage { text } = &rt.kind {
        assert_eq!(text, "こんにちは 🌍 émojis");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn serde_tool_call_complex_input() {
    let input = json!({
        "nested": {"arr": [1, 2, 3], "obj": {"key": true}},
        "null_val": null,
        "num": 3.14
    });
    let e = tool_call(0, "complex", Some("cx"), input.clone());
    let rt = serde_roundtrip(&e);
    if let AgentEventKind::ToolCall {
        input: rt_input, ..
    } = &rt.kind
    {
        assert_eq!(*rt_input, input);
    } else {
        panic!("wrong variant");
    }
}

// ═══════════════════════════════════════════════════════════════════
// 3. Event ordering and sequencing
// ═══════════════════════════════════════════════════════════════════

#[test]
fn events_maintain_insertion_order() {
    let events = standard_trace();
    let stream = EventStream::new(events.clone());
    let collected: Vec<_> = stream.iter().collect();
    for (i, e) in collected.iter().enumerate() {
        assert_eq!(e.ts, events[i].ts);
    }
}

#[test]
fn events_sorted_by_timestamp() {
    let events = standard_trace();
    for window in events.windows(2) {
        assert!(window[0].ts <= window[1].ts);
    }
}

#[test]
fn stream_first_is_run_started() {
    let stream = EventStream::new(standard_trace());
    let first = stream.first_of_kind("run_started");
    assert!(first.is_some());
}

#[test]
fn stream_last_is_run_completed() {
    let stream = EventStream::new(standard_trace());
    let last = stream.last_of_kind("run_completed");
    assert!(last.is_some());
}

#[test]
fn stream_by_kind_filters_correctly() {
    let stream = EventStream::new(standard_trace());
    let deltas = stream.by_kind("assistant_delta");
    assert_eq!(deltas.len(), 2);
}

#[test]
fn stream_duration_between_first_and_last() {
    let stream = EventStream::new(standard_trace());
    let dur = stream.duration().unwrap();
    assert_eq!(dur.as_millis(), 100);
}

#[test]
fn stream_duration_empty() {
    let stream = EventStream::new(vec![]);
    assert!(stream.duration().is_none());
}

#[test]
fn stream_duration_single_event() {
    let stream = EventStream::new(vec![run_started(0)]);
    assert!(stream.duration().is_none());
}

#[test]
fn stream_merge_preserves_order() {
    let a = EventStream::new(vec![run_started(0), delta(20, "a")]);
    let b = EventStream::new(vec![delta(10, "b"), run_completed(30)]);
    let merged = a.merge(&b);
    assert_eq!(merged.len(), 4);
    let timestamps: Vec<_> = merged.iter().map(|e| e.ts).collect();
    for window in timestamps.windows(2) {
        assert!(window[0] <= window[1]);
    }
}

#[test]
fn stream_merge_self_priority_on_equal_timestamps() {
    let a = EventStream::new(vec![delta(10, "from-a")]);
    let b = EventStream::new(vec![delta(10, "from-b")]);
    let merged = a.merge(&b);
    assert_eq!(merged.len(), 2);
    // Self (a) should come first when timestamps are equal
    if let AgentEventKind::AssistantDelta { text } = &merged.iter().next().unwrap().kind {
        assert_eq!(text, "from-a");
    }
}

// ═══════════════════════════════════════════════════════════════════
// 4. mpsc channel streaming
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn mpsc_send_receive_single_event() {
    let (tx, mut rx) = mpsc::channel(16);
    let e = run_started(0);
    tx.send(e.clone()).await.unwrap();
    drop(tx);
    let received = rx.recv().await.unwrap();
    assert_eq!(received.ts, e.ts);
}

#[tokio::test]
async fn mpsc_stream_full_trace() {
    let (tx, mut rx) = mpsc::channel(64);
    let trace = standard_trace();
    let expected_len = trace.len();
    for e in trace {
        tx.send(e).await.unwrap();
    }
    drop(tx);

    let mut received = Vec::new();
    while let Some(e) = rx.recv().await {
        received.push(e);
    }
    assert_eq!(received.len(), expected_len);
}

#[tokio::test]
async fn mpsc_channel_closed_on_sender_drop() {
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(16);
    tx.send(run_started(0)).await.unwrap();
    drop(tx);
    let _ = rx.recv().await; // consume the one event
    let next = rx.recv().await;
    assert!(next.is_none());
}

#[tokio::test]
async fn mpsc_producer_consumer_pipeline() {
    let (tx, mut rx) = mpsc::channel(32);
    let (out_tx, mut out_rx) = mpsc::channel(32);

    // Producer
    let producer = tokio::spawn(async move {
        for i in 0..10 {
            let e = delta(i * 10, &format!("token{i}"));
            tx.send(e).await.unwrap();
        }
    });

    // Consumer: filter only even-indexed deltas
    let consumer = tokio::spawn(async move {
        let mut idx = 0usize;
        while let Some(e) = rx.recv().await {
            if idx % 2 == 0 {
                out_tx.send(e).await.unwrap();
            }
            idx += 1;
        }
    });

    producer.await.unwrap();
    consumer.await.unwrap();

    let mut count = 0;
    while let Some(_) = out_rx.try_recv().ok() {
        count += 1;
    }
    assert_eq!(count, 5);
}

#[tokio::test]
async fn mpsc_preserves_event_order() {
    let (tx, mut rx) = mpsc::channel(128);
    let count = 100;
    for i in 0..count {
        tx.send(delta(i, &format!("{i}"))).await.unwrap();
    }
    drop(tx);

    let mut prev_ts = None;
    while let Some(e) = rx.recv().await {
        if let Some(p) = prev_ts {
            assert!(e.ts >= p);
        }
        prev_ts = Some(e.ts);
    }
}

// ═══════════════════════════════════════════════════════════════════
// 5. Event filtering and transformation
// ═══════════════════════════════════════════════════════════════════

#[test]
fn filter_include_single_kind() {
    let f = EventFilter::include_kinds(&["warning"]);
    assert!(f.matches(&warning(0, "w")));
    assert!(!f.matches(&error_ev(0, "e")));
}

#[test]
fn filter_include_multiple_kinds() {
    let f = EventFilter::include_kinds(&["warning", "error"]);
    assert!(f.matches(&warning(0, "w")));
    assert!(f.matches(&error_ev(0, "e")));
    assert!(!f.matches(&run_started(0)));
}

#[test]
fn filter_exclude_kind() {
    let f = EventFilter::exclude_kinds(&["assistant_delta"]);
    assert!(!f.matches(&delta(0, "x")));
    assert!(f.matches(&assistant_msg(0, "x")));
}

#[test]
fn filter_include_empty_passes_nothing() {
    let f = EventFilter::include_kinds(&[]);
    assert!(!f.matches(&run_started(0)));
    assert!(!f.matches(&error_ev(0, "e")));
}

#[test]
fn filter_exclude_empty_passes_everything() {
    let f = EventFilter::exclude_kinds(&[]);
    assert!(f.matches(&run_started(0)));
    assert!(f.matches(&error_ev(0, "e")));
}

#[test]
fn filter_case_insensitive() {
    // EventFilter uses exact lowercase comparison; verify lowercase works
    let f = EventFilter::include_kinds(&["run_started"]);
    assert!(f.matches(&run_started(0)));
}

#[test]
fn stream_filter_with_event_filter() {
    let stream = EventStream::new(standard_trace());
    let f = EventFilter::include_kinds(&["tool_call", "tool_result"]);
    let filtered = stream.filter(&f);
    assert_eq!(filtered.len(), 2);
}

#[test]
fn stream_filter_pred_custom() {
    let stream = EventStream::new(standard_trace());
    let only_text = stream.filter_pred(|e| {
        matches!(
            e.kind,
            AgentEventKind::AssistantDelta { .. } | AgentEventKind::AssistantMessage { .. }
        )
    });
    assert_eq!(only_text.len(), 3); // 2 deltas + 1 message
}

#[test]
fn stream_take_until_run_completed() {
    let stream = EventStream::new(standard_trace());
    let before_done = stream.take_until(|e| matches!(e.kind, AgentEventKind::RunCompleted { .. }));
    // Should exclude RunCompleted itself
    assert!(before_done.last_of_kind("run_completed").is_none());
    assert!(before_done.len() < standard_trace().len());
}

#[test]
fn stream_map_events_transform_text() {
    let stream = EventStream::new(vec![delta(0, "hello"), delta(10, "world")]);
    let upper = stream.map_events(|mut e| {
        if let AgentEventKind::AssistantDelta { ref mut text } = e.kind {
            *text = text.to_uppercase();
        }
        e
    });
    let texts: Vec<_> = upper
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["HELLO", "WORLD"]);
}

#[test]
fn stream_throttle_limits_rate() {
    let events: Vec<_> = (0..100).map(|i| delta(i, &format!("{i}"))).collect();
    let stream = EventStream::new(events);
    let throttled = stream.throttle(Duration::from_millis(10));
    // With 1ms per event and 10ms throttle, roughly 10 events should pass
    assert!(throttled.len() <= 11);
    assert!(throttled.len() >= 1);
}

// ═══════════════════════════════════════════════════════════════════
// 6. Event aggregation
// ═══════════════════════════════════════════════════════════════════

#[test]
fn aggregator_counts_events() {
    let mut agg = EventAggregator::new();
    for e in &standard_trace() {
        agg.add(e);
    }
    assert_eq!(agg.event_count(), 10);
}

#[test]
fn aggregator_count_by_kind() {
    let mut agg = EventAggregator::new();
    for e in &standard_trace() {
        agg.add(e);
    }
    let counts = agg.count_by_kind();
    assert_eq!(counts.get("assistant_delta"), Some(&2));
    assert_eq!(counts.get("tool_call"), Some(&1));
    assert_eq!(counts.get("run_started"), Some(&1));
}

#[test]
fn aggregator_tool_calls_list() {
    let mut agg = EventAggregator::new();
    for e in &standard_trace() {
        agg.add(e);
    }
    let tools = agg.tool_calls();
    assert_eq!(tools, vec!["read_file"]);
}

#[test]
fn aggregator_unique_tool_count() {
    let mut agg = EventAggregator::new();
    agg.add(&tool_call(0, "edit", None, json!(null)));
    agg.add(&tool_call(10, "grep", None, json!(null)));
    agg.add(&tool_call(20, "edit", None, json!(null)));
    assert_eq!(agg.unique_tool_count(), 2);
}

#[test]
fn aggregator_has_errors() {
    let mut agg = EventAggregator::new();
    agg.add(&run_started(0));
    assert!(!agg.has_errors());
    agg.add(&error_ev(10, "oops"));
    assert!(agg.has_errors());
}

#[test]
fn aggregator_error_messages() {
    let mut agg = EventAggregator::new();
    agg.add(&error_ev(0, "err1"));
    agg.add(&error_ev(10, "err2"));
    let msgs = agg.error_messages();
    assert_eq!(msgs, vec!["err1", "err2"]);
}

#[test]
fn aggregator_text_length() {
    let mut agg = EventAggregator::new();
    agg.add(&delta(0, "abc"));
    agg.add(&assistant_msg(10, "defgh"));
    assert_eq!(agg.text_length(), 8); // 3 + 5
}

#[test]
fn aggregator_duration_ms() {
    let mut agg = EventAggregator::new();
    agg.add(&run_started(0));
    agg.add(&run_completed(500));
    assert_eq!(agg.duration_ms(), Some(500));
}

#[test]
fn aggregator_duration_ms_single_event() {
    let mut agg = EventAggregator::new();
    agg.add(&run_started(0));
    assert!(agg.duration_ms().is_none());
}

#[test]
fn aggregator_summary_fields() {
    let mut agg = EventAggregator::new();
    for e in &standard_trace() {
        agg.add(e);
    }
    let summary = agg.summary();
    assert_eq!(summary.total_events, 10);
    assert_eq!(summary.tool_calls, 1);
    assert_eq!(summary.unique_tools, 1);
    assert_eq!(summary.errors, 0);
    assert!(summary.duration_ms.is_some());
}

#[test]
fn aggregation_summary_serializable() {
    let mut agg = EventAggregator::new();
    for e in &standard_trace() {
        agg.add(e);
    }
    let summary = agg.summary();
    let json = serde_json::to_string(&summary).unwrap();
    let rt: AggregationSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(rt, summary);
}

#[test]
fn run_analytics_is_successful() {
    let events = standard_trace();
    let analytics = RunAnalytics::from_events(&events);
    assert!(analytics.is_successful());
}

#[test]
fn run_analytics_not_successful_with_errors() {
    let mut events = standard_trace();
    events.push(error_ev(200, "boom"));
    let analytics = RunAnalytics::from_events(&events);
    assert!(!analytics.is_successful());
}

#[test]
fn run_analytics_tool_usage_ratio() {
    let events = vec![
        run_started(0),
        tool_call(10, "edit", None, json!(null)),
        tool_call(20, "grep", None, json!(null)),
        run_completed(30),
    ];
    let analytics = RunAnalytics::from_events(&events);
    let ratio = analytics.tool_usage_ratio();
    assert!((ratio - 0.5).abs() < 0.01); // 2 tool calls / 4 events
}

#[test]
fn run_analytics_average_text_per_event() {
    let events = vec![
        delta(0, "abc"),   // 3 chars
        delta(10, "de"),   // 2 chars
        run_completed(20), // 0 chars
    ];
    let analytics = RunAnalytics::from_events(&events);
    let avg = analytics.average_text_per_event();
    assert!((avg - 5.0 / 3.0).abs() < 0.01);
}

#[test]
fn run_analytics_empty_events() {
    let analytics = RunAnalytics::from_events(&[]);
    assert_eq!(analytics.tool_usage_ratio(), 0.0);
    assert_eq!(analytics.average_text_per_event(), 0.0);
    assert!(analytics.is_successful());
}

#[test]
fn stream_count_by_kind() {
    let stream = EventStream::new(standard_trace());
    let counts = stream.count_by_kind();
    assert!(counts.contains_key("run_started"));
    assert!(counts.contains_key("run_completed"));
    assert!(counts.contains_key("assistant_delta"));
    assert!(counts.contains_key("tool_call"));
}

// ═══════════════════════════════════════════════════════════════════
// 7. Event timeline reconstruction
// ═══════════════════════════════════════════════════════════════════

#[test]
fn timeline_reconstruct_text_from_deltas() {
    let events = vec![
        delta(0, "Hello"),
        delta(10, ", "),
        delta(20, "world"),
        delta(30, "!"),
    ];
    let full_text: String = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(full_text, "Hello, world!");
}

#[test]
fn timeline_tool_call_result_pairing() {
    let events = vec![
        tool_call(0, "edit", Some("t1"), json!({"file": "a"})),
        tool_result(10, "edit", Some("t1"), json!("ok"), false),
        tool_call(20, "grep", Some("t2"), json!({"q": "foo"})),
        tool_result(30, "grep", Some("t2"), json!(["match"]), false),
    ];

    // Pair calls and results by tool_use_id
    let mut pairs: HashMap<String, (Option<&AgentEvent>, Option<&AgentEvent>)> = HashMap::new();
    for e in &events {
        match &e.kind {
            AgentEventKind::ToolCall {
                tool_use_id: Some(id),
                ..
            } => {
                pairs.entry(id.clone()).or_default().0 = Some(e);
            }
            AgentEventKind::ToolResult {
                tool_use_id: Some(id),
                ..
            } => {
                pairs.entry(id.clone()).or_default().1 = Some(e);
            }
            _ => {}
        }
    }
    assert_eq!(pairs.len(), 2);
    for (_, (call, result)) in &pairs {
        assert!(call.is_some());
        assert!(result.is_some());
    }
}

#[test]
fn timeline_file_changes_ordered() {
    let events = vec![
        file_changed(10, "a.rs", "created"),
        file_changed(20, "b.rs", "modified"),
        file_changed(30, "a.rs", "updated"),
    ];
    let paths: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::FileChanged { path, .. } => Some(path.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(paths, vec!["a.rs", "b.rs", "a.rs"]);
}

#[test]
fn timeline_commands_with_exit_codes() {
    let events = vec![
        cmd_exec(0, "cargo check", Some(0)),
        cmd_exec(10, "cargo test", Some(1)),
        cmd_exec(20, "cargo build", Some(0)),
    ];
    let failures: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::CommandExecuted {
                command,
                exit_code: Some(code),
                ..
            } if *code != 0 => Some(command.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(failures, vec!["cargo test"]);
}

#[test]
fn timeline_warnings_before_error() {
    let events = vec![warning(0, "w1"), warning(10, "w2"), error_ev(20, "fatal")];
    let stream = EventStream::new(events);
    let before_error = stream.take_until(|e| matches!(e.kind, AgentEventKind::Error { .. }));
    assert_eq!(before_error.len(), 2);
}

#[test]
fn timeline_first_and_last_timestamps() {
    let mut agg = EventAggregator::new();
    let events = standard_trace();
    for e in &events {
        agg.add(e);
    }
    let first = agg.first_timestamp().unwrap();
    let last = agg.last_timestamp().unwrap();
    assert!(first < last);
}

// ═══════════════════════════════════════════════════════════════════
// 8. Concurrent event production/consumption
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn concurrent_multiple_producers() {
    let (tx, mut rx) = mpsc::channel(256);
    let n_producers = 5;
    let events_per_producer = 20;

    let mut handles = Vec::new();
    for p in 0..n_producers {
        let tx = tx.clone();
        handles.push(tokio::spawn(async move {
            for i in 0..events_per_producer {
                let e = delta((p * events_per_producer + i) as i64, &format!("p{p}-{i}"));
                tx.send(e).await.unwrap();
            }
        }));
    }
    drop(tx);

    for h in handles {
        h.await.unwrap();
    }

    let mut count = 0;
    while let Some(_) = rx.recv().await {
        count += 1;
    }
    assert_eq!(count, n_producers * events_per_producer);
}

#[tokio::test]
async fn concurrent_producer_consumer_no_loss() {
    let (tx, mut rx) = mpsc::channel(64);
    let total = 1000usize;
    let counter = Arc::new(AtomicUsize::new(0));

    let producer = {
        let tx = tx.clone();
        tokio::spawn(async move {
            for i in 0..total {
                tx.send(delta(i as i64, "t")).await.unwrap();
            }
        })
    };
    drop(tx);

    let consumer_counter = counter.clone();
    let consumer = tokio::spawn(async move {
        while let Some(_) = rx.recv().await {
            consumer_counter.fetch_add(1, Ordering::SeqCst);
        }
    });

    producer.await.unwrap();
    consumer.await.unwrap();
    assert_eq!(counter.load(Ordering::SeqCst), total);
}

#[tokio::test]
async fn concurrent_fan_out_with_broadcast() {
    use tokio::sync::broadcast;

    let (tx, _) = broadcast::channel::<AgentEvent>(128);
    let mut sub1 = tx.subscribe();
    let mut sub2 = tx.subscribe();

    let events = vec![run_started(0), delta(10, "hi"), run_completed(20)];
    for e in &events {
        tx.send(e.clone()).unwrap();
    }
    drop(tx);

    let mut count1 = 0;
    while sub1.recv().await.is_ok() {
        count1 += 1;
    }
    let mut count2 = 0;
    while sub2.recv().await.is_ok() {
        count2 += 1;
    }
    assert_eq!(count1, 3);
    assert_eq!(count2, 3);
}

#[tokio::test]
async fn concurrent_select_first_error() {
    let (tx, mut rx) = mpsc::channel(32);
    let events = vec![
        delta(0, "ok"),
        delta(10, "fine"),
        error_ev(20, "boom"),
        delta(30, "after error"),
    ];
    for e in events {
        tx.send(e).await.unwrap();
    }
    drop(tx);

    let mut first_error = None;
    while let Some(e) = rx.recv().await {
        if matches!(e.kind, AgentEventKind::Error { .. }) {
            first_error = Some(e);
            break;
        }
    }
    assert!(first_error.is_some());
}

// ═══════════════════════════════════════════════════════════════════
// 9. Backpressure handling
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn backpressure_bounded_channel_full() {
    let (tx, _rx) = mpsc::channel(2);
    tx.send(run_started(0)).await.unwrap();
    tx.send(delta(10, "a")).await.unwrap();
    // Channel is now full (capacity 2); try_send should fail
    let result = tx.try_send(delta(20, "b"));
    assert!(result.is_err());
}

#[tokio::test]
async fn backpressure_slow_consumer() {
    let (tx, mut rx) = mpsc::channel(4);

    let producer = tokio::spawn(async move {
        for i in 0..20 {
            tx.send(delta(i, &format!("{i}"))).await.unwrap();
        }
    });

    // Slow consumer: small delay per event
    let consumer = tokio::spawn(async move {
        let mut count = 0;
        while let Some(_) = rx.recv().await {
            count += 1;
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        count
    });

    producer.await.unwrap();
    let count = consumer.await.unwrap();
    assert_eq!(count, 20); // all events delivered despite backpressure
}

#[tokio::test]
async fn backpressure_broadcast_lag_detection() {
    use tokio::sync::broadcast;

    let (tx, mut rx) = broadcast::channel::<AgentEvent>(4);

    // Send more events than buffer capacity
    for i in 0..10 {
        let _ = tx.send(delta(i, "x"));
    }

    // Subscriber should get a Lagged error
    let result = rx.recv().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn backpressure_try_send_recovers() {
    let (tx, mut rx) = mpsc::channel(2);
    tx.send(run_started(0)).await.unwrap();
    tx.send(delta(10, "a")).await.unwrap();

    // Full; try_send fails
    assert!(tx.try_send(delta(20, "b")).is_err());

    // Consume one event to free space
    rx.recv().await.unwrap();

    // Now try_send should succeed
    assert!(tx.try_send(delta(20, "b")).is_ok());
}

// ═══════════════════════════════════════════════════════════════════
// 10. Event metadata propagation (ext field)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn metadata_none_by_default() {
    let e = run_started(0);
    assert!(e.ext.is_none());
}

#[test]
fn metadata_propagates_through_clone() {
    let mut ext = BTreeMap::new();
    ext.insert("source".into(), json!("claude"));
    let e = ev_ext(
        0,
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext,
    );
    let cloned = e.clone();
    assert_eq!(cloned.ext, e.ext);
}

#[test]
fn metadata_survives_serde() {
    let mut ext = BTreeMap::new();
    ext.insert("model".into(), json!("gpt-4"));
    ext.insert("tokens".into(), json!(42));
    let e = ev_ext(
        0,
        AgentEventKind::AssistantMessage { text: "hi".into() },
        ext.clone(),
    );
    let rt = serde_roundtrip(&e);
    assert_eq!(rt.ext.unwrap(), ext);
}

#[test]
fn metadata_raw_message_passthrough() {
    let mut ext = BTreeMap::new();
    ext.insert(
        "raw_message".into(),
        json!({"role": "assistant", "content": "hello"}),
    );
    let e = ev_ext(
        0,
        AgentEventKind::AssistantMessage {
            text: "hello".into(),
        },
        ext,
    );
    let raw = &e.ext.as_ref().unwrap()["raw_message"];
    assert_eq!(raw["role"], "assistant");
}

#[test]
fn metadata_multiple_keys() {
    let mut ext = BTreeMap::new();
    ext.insert("vendor".into(), json!("anthropic"));
    ext.insert("request_id".into(), json!("req-123"));
    ext.insert("latency_ms".into(), json!(150));
    let e = ev_ext(
        0,
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext,
    );
    let ext = e.ext.as_ref().unwrap();
    assert_eq!(ext.len(), 3);
    assert_eq!(ext["vendor"], "anthropic");
}

#[tokio::test]
async fn metadata_preserved_through_mpsc() {
    let (tx, mut rx) = mpsc::channel(16);
    let mut ext = BTreeMap::new();
    ext.insert("trace_id".into(), json!("abc-123"));
    let e = ev_ext(
        0,
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext,
    );
    tx.send(e).await.unwrap();
    drop(tx);

    let received = rx.recv().await.unwrap();
    assert_eq!(received.ext.as_ref().unwrap()["trace_id"], json!("abc-123"));
}

#[test]
fn metadata_in_stream_map() {
    let mut ext = BTreeMap::new();
    ext.insert("tag".into(), json!("original"));
    let events = vec![ev_ext(
        0,
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext,
    )];
    let stream = EventStream::new(events);
    let mapped = stream.map_events(|mut e| {
        if let Some(ref mut ext) = e.ext {
            ext.insert("tag".into(), json!("modified"));
        }
        e
    });
    let result = mapped.iter().next().unwrap();
    assert_eq!(result.ext.as_ref().unwrap()["tag"], "modified");
}

// ═══════════════════════════════════════════════════════════════════
// 11. Large event streams
// ═══════════════════════════════════════════════════════════════════

#[test]
fn large_stream_10k_events() {
    let events: Vec<_> = (0..10_000)
        .map(|i| delta(i, &format!("token-{i}")))
        .collect();
    let stream = EventStream::new(events);
    assert_eq!(stream.len(), 10_000);
}

#[test]
fn large_stream_count_by_kind() {
    let mut events: Vec<AgentEvent> = Vec::new();
    events.push(run_started(0));
    for i in 1..=5000 {
        events.push(delta(i, "t"));
    }
    for i in 5001..=7000 {
        events.push(tool_call(i, "edit", None, json!(null)));
    }
    events.push(run_completed(10000));

    let stream = EventStream::new(events);
    let counts = stream.count_by_kind();
    assert_eq!(counts["assistant_delta"], 5000);
    assert_eq!(counts["tool_call"], 2000);
    assert_eq!(counts["run_started"], 1);
    assert_eq!(counts["run_completed"], 1);
}

#[tokio::test]
async fn large_stream_mpsc_throughput() {
    let (tx, mut rx) = mpsc::channel(1024);
    let n = 50_000;

    let producer = tokio::spawn(async move {
        for i in 0..n {
            tx.send(delta(i, "x")).await.unwrap();
        }
    });

    let consumer = tokio::spawn(async move {
        let mut count = 0u64;
        while let Some(_) = rx.recv().await {
            count += 1;
        }
        count
    });

    producer.await.unwrap();
    let count = consumer.await.unwrap();
    assert_eq!(count, n as u64);
}

#[test]
fn large_stream_aggregation() {
    let mut agg = EventAggregator::new();
    for i in 0..10_000 {
        agg.add(&delta(i, "a"));
    }
    assert_eq!(agg.event_count(), 10_000);
    assert_eq!(agg.text_length(), 10_000);
}

#[test]
fn large_stream_filter_performance() {
    let events: Vec<_> = (0..10_000)
        .map(|i| {
            if i % 100 == 0 {
                error_ev(i, "err")
            } else {
                delta(i, "t")
            }
        })
        .collect();
    let stream = EventStream::new(events);
    let errors = stream.by_kind("error");
    assert_eq!(errors.len(), 100);
}

#[test]
fn large_stream_merge() {
    let a: Vec<_> = (0..500).step_by(2).map(|i| delta(i, "a")).collect();
    let b: Vec<_> = (1..500).step_by(2).map(|i| delta(i, "b")).collect();
    let sa = EventStream::new(a);
    let sb = EventStream::new(b);
    let merged = sa.merge(&sb);
    assert_eq!(merged.len(), 500);
    // Check sorted
    let tss: Vec<_> = merged.iter().map(|e| e.ts).collect();
    for w in tss.windows(2) {
        assert!(w[0] <= w[1]);
    }
}

// ═══════════════════════════════════════════════════════════════════
// 12. Event deduplication
// ═══════════════════════════════════════════════════════════════════

fn dedup_by_content(events: Vec<AgentEvent>) -> Vec<AgentEvent> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for e in events {
        let key = serde_json::to_string(&e.kind).unwrap();
        if seen.insert(key) {
            result.push(e);
        }
    }
    result
}

fn dedup_by_tool_use_id(events: Vec<AgentEvent>) -> Vec<AgentEvent> {
    let mut seen_ids = BTreeSet::new();
    let mut result = Vec::new();
    for e in &events {
        match &e.kind {
            AgentEventKind::ToolCall {
                tool_use_id: Some(id),
                ..
            } => {
                if seen_ids.insert(format!("call:{id}")) {
                    result.push(e.clone());
                }
            }
            AgentEventKind::ToolResult {
                tool_use_id: Some(id),
                ..
            } => {
                if seen_ids.insert(format!("result:{id}")) {
                    result.push(e.clone());
                }
            }
            _ => result.push(e.clone()),
        }
    }
    result
}

#[test]
fn dedup_removes_duplicate_deltas() {
    let events = vec![delta(0, "hello"), delta(10, "hello"), delta(20, "world")];
    let deduped = dedup_by_content(events);
    assert_eq!(deduped.len(), 2);
}

#[test]
fn dedup_keeps_unique_events() {
    let events = standard_trace();
    let deduped = dedup_by_content(events.clone());
    assert_eq!(deduped.len(), events.len());
}

#[test]
fn dedup_by_tool_id_removes_duplicate_calls() {
    let events = vec![
        tool_call(0, "edit", Some("t1"), json!({"v": 1})),
        tool_call(10, "edit", Some("t1"), json!({"v": 2})), // dup id
        tool_result(20, "edit", Some("t1"), json!("ok"), false),
        tool_result(30, "edit", Some("t1"), json!("ok2"), false), // dup id
    ];
    let deduped = dedup_by_tool_use_id(events);
    // Should keep first call and first result for t1
    assert_eq!(deduped.len(), 2);
}

#[test]
fn dedup_does_not_affect_events_without_ids() {
    let events = vec![
        run_started(0),
        delta(10, "a"),
        delta(20, "a"), // same content, different timestamp - dedup_by_content
        run_completed(30),
    ];
    let deduped = dedup_by_tool_use_id(events);
    assert_eq!(deduped.len(), 4); // no tool_use_id to dedup on
}

#[test]
fn dedup_consecutive_same_kind() {
    let events = vec![
        warning(0, "same"),
        warning(10, "same"),
        warning(20, "same"),
        error_ev(30, "different"),
    ];
    let deduped = dedup_by_content(events);
    assert_eq!(deduped.len(), 2); // "same" warning + error
}

#[test]
fn dedup_preserves_first_occurrence_timestamp() {
    let events = vec![delta(100, "dup"), delta(200, "dup")];
    let deduped = dedup_by_content(events);
    assert_eq!(deduped.len(), 1);
    assert_eq!(deduped[0].ts, ts(100));
}

// ═══════════════════════════════════════════════════════════════════
// 13. Extension trait tests (AgentEventExt)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn ext_is_tool_call_true() {
    let e = tool_call(0, "edit", None, json!(null));
    assert!(e.is_tool_call());
}

#[test]
fn ext_is_tool_call_false() {
    let e = run_started(0);
    assert!(!e.is_tool_call());
}

#[test]
fn ext_is_terminal_on_run_completed() {
    let e = run_completed(0);
    assert!(e.is_terminal());
}

#[test]
fn ext_is_terminal_false_on_other() {
    let e = delta(0, "x");
    assert!(!e.is_terminal());
}

#[test]
fn ext_text_content_delta() {
    let e = delta(0, "hello");
    assert_eq!(e.text_content(), Some("hello"));
}

#[test]
fn ext_text_content_message() {
    let e = assistant_msg(0, "world");
    assert_eq!(e.text_content(), Some("world"));
}

#[test]
fn ext_text_content_none_for_non_text() {
    let e = tool_call(0, "x", None, json!(null));
    assert!(e.text_content().is_none());
}

#[test]
fn ext_text_content_none_for_error() {
    let e = error_ev(0, "oops");
    assert!(e.text_content().is_none());
}

// ═══════════════════════════════════════════════════════════════════
// 14. EventStream combinator edge cases
// ═══════════════════════════════════════════════════════════════════

#[test]
fn stream_empty_operations() {
    let stream = EventStream::new(vec![]);
    assert!(stream.is_empty());
    assert_eq!(stream.len(), 0);
    assert!(stream.first_of_kind("run_started").is_none());
    assert!(stream.last_of_kind("run_completed").is_none());
    assert!(stream.duration().is_none());
    assert!(stream.count_by_kind().is_empty());
}

#[test]
fn stream_into_vec() {
    let events = standard_trace();
    let len = events.len();
    let stream = EventStream::new(events);
    let vec = stream.into_vec();
    assert_eq!(vec.len(), len);
}

#[test]
fn stream_into_iter() {
    let stream = EventStream::new(standard_trace());
    let count = stream.into_iter().count();
    assert_eq!(count, 10);
}

#[test]
fn stream_ref_iter() {
    let stream = EventStream::new(standard_trace());
    let count = (&stream).into_iter().count();
    assert_eq!(count, 10);
}

#[test]
fn stream_take_until_not_found() {
    let stream = EventStream::new(vec![delta(0, "a"), delta(10, "b")]);
    let result = stream.take_until(|e| matches!(e.kind, AgentEventKind::Error { .. }));
    assert_eq!(result.len(), 2); // predicate never matched, keeps all
}

#[test]
fn stream_take_until_first_event() {
    let stream = EventStream::new(vec![error_ev(0, "e"), delta(10, "a")]);
    let result = stream.take_until(|e| matches!(e.kind, AgentEventKind::Error { .. }));
    assert_eq!(result.len(), 0);
}

#[test]
fn stream_filter_then_count() {
    let stream = EventStream::new(standard_trace());
    let tools = stream.by_kind("tool_call");
    let results = stream.by_kind("tool_result");
    assert_eq!(tools.len(), results.len()); // matched pairs in standard trace
}

#[test]
fn stream_chained_filters() {
    let stream = EventStream::new(standard_trace());
    let f1 = EventFilter::exclude_kinds(&["run_started", "run_completed"]);
    let f2 = EventFilter::exclude_kinds(&["warning"]);
    let filtered = stream.filter(&f1);
    let double_filtered = filtered.filter(&f2);
    // Removed: run_started, run_completed, warning = 3 events gone from 10
    assert_eq!(double_filtered.len(), 7);
}

#[test]
fn stream_merge_empty_with_nonempty() {
    let empty = EventStream::new(vec![]);
    let nonempty = EventStream::new(vec![run_started(0)]);
    let merged = empty.merge(&nonempty);
    assert_eq!(merged.len(), 1);
}

#[test]
fn stream_merge_two_empty() {
    let a = EventStream::new(vec![]);
    let b = EventStream::new(vec![]);
    assert!(a.merge(&b).is_empty());
}

#[test]
fn stream_throttle_empty() {
    let stream = EventStream::new(vec![]);
    let throttled = stream.throttle(Duration::from_secs(1));
    assert!(throttled.is_empty());
}

#[test]
fn stream_throttle_single_event() {
    let stream = EventStream::new(vec![delta(0, "x")]);
    let throttled = stream.throttle(Duration::from_secs(1));
    assert_eq!(throttled.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════
// 15. Misc edge-case and robustness tests
// ═══════════════════════════════════════════════════════════════════

#[test]
fn event_clone_equality() {
    let e = tool_call(42, "grep", Some("x"), json!({"q": "pattern"}));
    let cloned = e.clone();
    assert_eq!(e.ts, cloned.ts);
    let e_json = serde_json::to_value(&e).unwrap();
    let c_json = serde_json::to_value(&cloned).unwrap();
    assert_eq!(e_json, c_json);
}

#[test]
fn event_debug_format() {
    let e = run_started(0);
    let debug = format!("{:?}", e);
    assert!(debug.contains("RunStarted"));
}

#[test]
fn error_code_serde_roundtrip() {
    let e = error_with_code(0, "timeout", ErrorCode::BackendTimeout);
    let json = serde_json::to_string(&e).unwrap();
    assert!(json.contains("backend_timeout"));
    let rt: AgentEvent = serde_json::from_str(&json).unwrap();
    if let AgentEventKind::Error { error_code, .. } = &rt.kind {
        assert_eq!(*error_code, Some(ErrorCode::BackendTimeout));
    }
}

#[test]
fn multiple_error_codes_distinct() {
    let codes = vec![
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::BackendNotFound,
        ErrorCode::ExecutionToolFailed,
        ErrorCode::BackendTimeout,
    ];
    for code in codes {
        let e = error_with_code(0, "test", code);
        let rt = serde_roundtrip(&e);
        if let AgentEventKind::Error { error_code, .. } = &rt.kind {
            assert_eq!(*error_code, Some(code));
        }
    }
}

#[test]
fn tool_call_empty_input() {
    let e = tool_call(0, "noop", None, json!({}));
    let rt = serde_roundtrip(&e);
    if let AgentEventKind::ToolCall { input, .. } = &rt.kind {
        assert!(input.is_object());
        assert!(input.as_object().unwrap().is_empty());
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn tool_call_null_input() {
    let e = tool_call(0, "noop", None, json!(null));
    let rt = serde_roundtrip(&e);
    if let AgentEventKind::ToolCall { input, .. } = &rt.kind {
        assert!(input.is_null());
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn tool_result_array_output() {
    let e = tool_result(0, "search", None, json!(["a", "b", "c"]), false);
    let rt = serde_roundtrip(&e);
    if let AgentEventKind::ToolResult { output, .. } = &rt.kind {
        assert_eq!(output.as_array().unwrap().len(), 3);
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn aggregator_default_trait() {
    let agg = EventAggregator::default();
    assert_eq!(agg.event_count(), 0);
}

#[test]
fn aggregator_no_tool_calls() {
    let mut agg = EventAggregator::new();
    agg.add(&run_started(0));
    agg.add(&run_completed(10));
    assert!(agg.tool_calls().is_empty());
    assert_eq!(agg.unique_tool_count(), 0);
}

#[test]
fn aggregator_no_errors() {
    let mut agg = EventAggregator::new();
    agg.add(&run_started(0));
    assert!(!agg.has_errors());
    assert!(agg.error_messages().is_empty());
}

#[test]
fn aggregator_text_length_zero_for_non_text() {
    let mut agg = EventAggregator::new();
    agg.add(&tool_call(0, "x", None, json!(null)));
    agg.add(&warning(10, "w"));
    assert_eq!(agg.text_length(), 0);
}

#[tokio::test]
async fn mpsc_forward_with_transform() {
    let (tx, mut rx) = mpsc::channel(32);
    let (out_tx, mut out_rx) = mpsc::channel(32);

    let events = vec![delta(0, "hello"), delta(10, "world")];
    for e in events {
        tx.send(e).await.unwrap();
    }
    drop(tx);

    // Forward with uppercase transform
    tokio::spawn(async move {
        while let Some(mut e) = rx.recv().await {
            if let AgentEventKind::AssistantDelta { ref mut text } = e.kind {
                *text = text.to_uppercase();
            }
            out_tx.send(e).await.unwrap();
        }
    });

    let e1 = out_rx.recv().await.unwrap();
    let e2 = out_rx.recv().await.unwrap();
    assert_eq!(e1.text_content(), Some("HELLO"));
    assert_eq!(e2.text_content(), Some("WORLD"));
}

#[test]
fn stream_by_kind_all_variants() {
    let events = vec![
        run_started(0),
        run_completed(10),
        delta(20, "d"),
        assistant_msg(30, "m"),
        tool_call(40, "t", None, json!(null)),
        tool_result(50, "t", None, json!(null), false),
        file_changed(60, "p", "s"),
        cmd_exec(70, "c", None),
        warning(80, "w"),
        error_ev(90, "e"),
    ];
    let stream = EventStream::new(events);
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
    for kind in &kinds {
        let filtered = stream.by_kind(kind);
        assert_eq!(filtered.len(), 1, "expected 1 event for kind {kind}");
    }
}

#[test]
fn stream_by_kind_nonexistent() {
    let stream = EventStream::new(standard_trace());
    let result = stream.by_kind("nonexistent_kind");
    assert!(result.is_empty());
}
