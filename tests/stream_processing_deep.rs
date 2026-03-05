#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Comprehensive tests for the stream processing pipeline in `abp-stream`:
//! EventFilter, EventRecorder, EventStats, EventStream, EventMultiplexer,
//! StreamPipeline, StreamPipelineBuilder — covering composition, concurrency,
//! edge cases, replay, serde, and large-stream scenarios.

use abp_core::{AgentEvent, AgentEventKind};
use abp_stream::{
    EventFilter, EventMultiplexer, EventRecorder, EventStats, EventStream, EventTransform,
    StreamPipeline, StreamPipelineBuilder, event_kind_name,
};
use chrono::{Duration, Utc};
use std::collections::BTreeMap;
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn mk(kind: AgentEventKind) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: None,
    }
}

fn mk_ts(kind: AgentEventKind, ts: chrono::DateTime<Utc>) -> AgentEvent {
    AgentEvent {
        ts,
        kind,
        ext: None,
    }
}

fn mk_ext(kind: AgentEventKind, ext: BTreeMap<String, serde_json::Value>) -> AgentEvent {
    AgentEvent {
        ts: Utc::now(),
        kind,
        ext: Some(ext),
    }
}

fn delta(t: &str) -> AgentEvent {
    mk(AgentEventKind::AssistantDelta {
        text: t.to_string(),
    })
}

fn msg(t: &str) -> AgentEvent {
    mk(AgentEventKind::AssistantMessage {
        text: t.to_string(),
    })
}

fn err(m: &str) -> AgentEvent {
    mk(AgentEventKind::Error {
        message: m.to_string(),
        error_code: None,
    })
}

fn warn(m: &str) -> AgentEvent {
    mk(AgentEventKind::Warning {
        message: m.to_string(),
    })
}

fn tc(name: &str) -> AgentEvent {
    mk(AgentEventKind::ToolCall {
        tool_name: name.to_string(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::json!({}),
    })
}

fn tr(name: &str) -> AgentEvent {
    mk(AgentEventKind::ToolResult {
        tool_name: name.to_string(),
        tool_use_id: None,
        output: serde_json::json!("ok"),
        is_error: false,
    })
}

fn fc(path: &str) -> AgentEvent {
    mk(AgentEventKind::FileChanged {
        path: path.to_string(),
        summary: "modified".to_string(),
    })
}

fn cmd(c: &str, code: Option<i32>) -> AgentEvent {
    mk(AgentEventKind::CommandExecuted {
        command: c.to_string(),
        exit_code: code,
        output_preview: None,
    })
}

fn started() -> AgentEvent {
    mk(AgentEventKind::RunStarted {
        message: "go".to_string(),
    })
}

fn completed() -> AgentEvent {
    mk(AgentEventKind::RunCompleted {
        message: "done".to_string(),
    })
}

/// Every event variant exactly once.
fn every_kind() -> Vec<AgentEvent> {
    vec![
        started(),
        completed(),
        delta("d"),
        msg("m"),
        tc("t"),
        tr("t"),
        fc("f"),
        cmd("c", Some(0)),
        warn("w"),
        err("e"),
    ]
}

// ===========================================================================
// 1. Filter — composition & negation
// ===========================================================================

#[test]
fn filter_by_kind_warning() {
    let f = EventFilter::by_kind("warning");
    assert!(f.matches(&warn("w")));
    assert!(!f.matches(&err("e")));
    assert!(!f.matches(&delta("d")));
}

#[test]
fn filter_by_kind_command_executed() {
    let f = EventFilter::by_kind("command_executed");
    assert!(f.matches(&cmd("ls", None)));
    assert!(!f.matches(&fc("f")));
}

#[test]
fn filter_by_kind_tool_result() {
    let f = EventFilter::by_kind("tool_result");
    assert!(f.matches(&tr("read")));
    assert!(!f.matches(&tc("read")));
}

#[test]
fn filter_by_kind_assistant_message() {
    let f = EventFilter::by_kind("assistant_message");
    assert!(f.matches(&msg("hi")));
    assert!(!f.matches(&delta("hi")));
}

#[test]
fn filter_errors_only_on_every_kind() {
    let f = EventFilter::errors_only();
    let events = every_kind();
    let matched: Vec<_> = events.iter().filter(|e| f.matches(e)).collect();
    assert_eq!(matched.len(), 1);
    assert!(matches!(matched[0].kind, AgentEventKind::Error { .. }));
}

#[test]
fn filter_exclude_errors_on_every_kind() {
    let f = EventFilter::exclude_errors();
    let events = every_kind();
    let matched: Vec<_> = events.iter().filter(|e| f.matches(e)).collect();
    assert_eq!(matched.len(), 9);
}

#[test]
fn chained_filters_narrow_progressively() {
    let f1 = EventFilter::exclude_errors();
    let f2 = EventFilter::by_kind("assistant_delta");
    // Only deltas that are also not errors pass both.
    let events = every_kind();
    let passed: Vec<_> = events
        .iter()
        .filter(|e| f1.matches(e) && f2.matches(e))
        .collect();
    assert_eq!(passed.len(), 1);
}

#[test]
fn filter_custom_on_exit_code() {
    let f = EventFilter::new(|ev| {
        matches!(
            &ev.kind,
            AgentEventKind::CommandExecuted {
                exit_code: Some(0),
                ..
            }
        )
    });
    assert!(f.matches(&cmd("ok", Some(0))));
    assert!(!f.matches(&cmd("fail", Some(1))));
    assert!(!f.matches(&cmd("none", None)));
}

#[test]
fn filter_custom_on_file_extension() {
    let f = EventFilter::new(
        |ev| matches!(&ev.kind, AgentEventKind::FileChanged { path, .. } if path.ends_with(".rs")),
    );
    assert!(f.matches(&fc("src/main.rs")));
    assert!(!f.matches(&fc("README.md")));
}

#[test]
fn filter_clone_is_independent() {
    let f = EventFilter::by_kind("error");
    let f2 = f.clone();
    assert!(f.matches(&err("x")));
    assert!(f2.matches(&err("x")));
    assert!(!f2.matches(&delta("x")));
}

// ===========================================================================
// 2. Transform — edge cases
// ===========================================================================

#[test]
fn transform_strips_ext() {
    let t = EventTransform::new(|mut ev| {
        ev.ext = None;
        ev
    });
    let mut ext = BTreeMap::new();
    ext.insert("k".to_string(), serde_json::json!(1));
    let ev = mk_ext(
        AgentEventKind::AssistantDelta {
            text: "hi".to_string(),
        },
        ext,
    );
    let result = t.apply(ev);
    assert!(result.ext.is_none());
}

#[test]
fn transform_appends_to_existing_ext() {
    let t = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("added".to_string(), serde_json::json!(true));
        ev
    });
    let mut ext = BTreeMap::new();
    ext.insert("original".to_string(), serde_json::json!("yes"));
    let ev = mk_ext(
        AgentEventKind::AssistantDelta {
            text: "x".to_string(),
        },
        ext,
    );
    let result = t.apply(ev);
    let ext = result.ext.unwrap();
    assert!(ext.contains_key("original"));
    assert!(ext.contains_key("added"));
}

#[test]
fn transform_on_tool_result_output() {
    let t = EventTransform::new(|mut ev| {
        if let AgentEventKind::ToolResult { ref mut output, .. } = ev.kind {
            *output = serde_json::json!({"redacted": true});
        }
        ev
    });
    let result = t.apply(tr("read_file"));
    match &result.kind {
        AgentEventKind::ToolResult { output, .. } => {
            assert_eq!(output["redacted"], true);
        }
        _ => panic!("expected ToolResult"),
    }
}

#[test]
fn transform_identity_on_all_kinds() {
    let t = EventTransform::identity();
    for ev in every_kind() {
        let name_before = event_kind_name(&ev.kind);
        let result = t.apply(ev);
        assert_eq!(event_kind_name(&result.kind), name_before);
    }
}

#[test]
fn three_transforms_accumulate_ext_keys() {
    let t1 = EventTransform::new(|mut ev| {
        ev.ext
            .get_or_insert_with(BTreeMap::new)
            .insert("a".into(), serde_json::json!(1));
        ev
    });
    let t2 = EventTransform::new(|mut ev| {
        ev.ext
            .get_or_insert_with(BTreeMap::new)
            .insert("b".into(), serde_json::json!(2));
        ev
    });
    let t3 = EventTransform::new(|mut ev| {
        ev.ext
            .get_or_insert_with(BTreeMap::new)
            .insert("c".into(), serde_json::json!(3));
        ev
    });
    let ev = t3.apply(t2.apply(t1.apply(delta("x"))));
    let ext = ev.ext.unwrap();
    assert_eq!(ext.len(), 3);
    assert_eq!(ext["a"], 1);
    assert_eq!(ext["b"], 2);
    assert_eq!(ext["c"], 3);
}

// ===========================================================================
// 3. Recorder — replay & multi-access
// ===========================================================================

#[test]
fn recorder_replay_preserves_order() {
    let r = EventRecorder::new();
    for i in 0..5 {
        r.record(&delta(&format!("t{i}")));
    }
    let events = r.events();
    for (i, ev) in events.iter().enumerate() {
        match &ev.kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, &format!("t{i}")),
            _ => panic!("unexpected kind"),
        }
    }
}

#[test]
fn recorder_replay_after_clear_is_empty() {
    let r = EventRecorder::new();
    r.record(&delta("a"));
    r.record(&delta("b"));
    r.clear();
    assert!(r.events().is_empty());
}

#[test]
fn recorder_record_after_clear_works() {
    let r = EventRecorder::new();
    r.record(&delta("before"));
    r.clear();
    r.record(&delta("after"));
    assert_eq!(r.len(), 1);
    match &r.events()[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "after"),
        _ => panic!("unexpected"),
    }
}

#[test]
fn recorder_preserves_event_ext() {
    let r = EventRecorder::new();
    let mut ext = BTreeMap::new();
    ext.insert("k".to_string(), serde_json::json!("v"));
    let ev = mk_ext(
        AgentEventKind::Warning {
            message: "w".into(),
        },
        ext,
    );
    r.record(&ev);
    let recorded = &r.events()[0];
    assert_eq!(recorded.ext.as_ref().unwrap()["k"], serde_json::json!("v"));
}

#[test]
fn recorder_multiple_snapshots_are_independent() {
    let r = EventRecorder::new();
    r.record(&delta("a"));
    let snap1 = r.events();
    r.record(&delta("b"));
    let snap2 = r.events();
    assert_eq!(snap1.len(), 1);
    assert_eq!(snap2.len(), 2);
}

#[test]
fn recorder_record_all_event_kinds() {
    let r = EventRecorder::new();
    for ev in every_kind() {
        r.record(&ev);
    }
    assert_eq!(r.len(), 10);
    // Verify kind names preserved
    let names: Vec<_> = r
        .events()
        .iter()
        .map(|e| event_kind_name(&e.kind))
        .collect();
    assert!(names.contains(&"run_started".to_string()));
    assert!(names.contains(&"error".to_string()));
}

// ===========================================================================
// 4. Stats — incremental, reset, edge cases
// ===========================================================================

#[test]
fn stats_observe_then_reset_then_observe() {
    let s = EventStats::new();
    s.observe(&delta("abc"));
    assert_eq!(s.total_events(), 1);
    s.reset();
    s.observe(&err("e"));
    assert_eq!(s.total_events(), 1);
    assert_eq!(s.error_count(), 1);
    assert_eq!(s.total_delta_bytes(), 0);
}

#[test]
fn stats_delta_bytes_empty_string() {
    let s = EventStats::new();
    s.observe(&delta(""));
    assert_eq!(s.total_delta_bytes(), 0);
    assert_eq!(s.total_events(), 1);
}

#[test]
fn stats_no_delta_bytes_for_assistant_message() {
    let s = EventStats::new();
    s.observe(&msg("long assistant message text"));
    assert_eq!(s.total_delta_bytes(), 0);
}

#[test]
fn stats_kind_counts_returns_clone() {
    let s = EventStats::new();
    s.observe(&delta("a"));
    let counts = s.kind_counts();
    s.observe(&delta("b"));
    // Original snapshot unaffected
    assert_eq!(counts["assistant_delta"], 1);
    assert_eq!(s.count_for("assistant_delta"), 2);
}

#[test]
fn stats_count_for_nonexistent_kind_is_zero() {
    let s = EventStats::new();
    s.observe(&delta("a"));
    assert_eq!(s.count_for("totally_made_up"), 0);
}

#[test]
fn stats_error_count_not_affected_by_tool_result_error() {
    let s = EventStats::new();
    let ev = mk(AgentEventKind::ToolResult {
        tool_name: "failing".into(),
        tool_use_id: None,
        output: serde_json::json!({"error": "boom"}),
        is_error: true,
    });
    s.observe(&ev);
    assert_eq!(s.error_count(), 0); // only AgentEventKind::Error counts
    assert_eq!(s.count_for("tool_result"), 1);
}

#[test]
fn stats_clone_shares_inner_state() {
    let s = EventStats::new();
    let s2 = s.clone();
    s.observe(&delta("a"));
    assert_eq!(s2.total_events(), 1);
    s2.observe(&err("e"));
    assert_eq!(s.error_count(), 1);
}

// ===========================================================================
// 5. Pipeline composition — multi-stage
// ===========================================================================

#[test]
fn pipeline_three_filters_conjunctive() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .filter(EventFilter::new(|ev| {
            !matches!(ev.kind, AgentEventKind::Warning { .. })
        }))
        .filter(EventFilter::new(|ev| {
            !matches!(ev.kind, AgentEventKind::RunStarted { .. })
        }))
        .build();

    assert!(p.process(delta("ok")).is_some());
    assert!(p.process(err("e")).is_none());
    assert!(p.process(warn("w")).is_none());
    assert!(p.process(started()).is_none());
    assert!(p.process(tc("t")).is_some());
}

#[test]
fn pipeline_filter_transform_record_stats_all_wired() {
    let rec = EventRecorder::new();
    let st = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::by_kind("assistant_delta"))
        .transform(EventTransform::new(|mut ev| {
            if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
                *text = text.to_uppercase();
            }
            ev
        }))
        .with_recorder(rec.clone())
        .with_stats(st.clone())
        .build();

    p.process(delta("hello"));
    p.process(err("nope"));
    p.process(tc("tool"));
    p.process(delta("world"));

    assert_eq!(rec.len(), 2);
    assert_eq!(st.total_events(), 2);
    match &rec.events()[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "HELLO"),
        _ => panic!("expected delta"),
    }
    match &rec.events()[1].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "WORLD"),
        _ => panic!("expected delta"),
    }
}

#[test]
fn two_pipelines_share_one_recorder() {
    let rec = EventRecorder::new();
    let p1 = StreamPipelineBuilder::new()
        .filter(EventFilter::by_kind("assistant_delta"))
        .with_recorder(rec.clone())
        .build();
    let p2 = StreamPipelineBuilder::new()
        .filter(EventFilter::by_kind("error"))
        .with_recorder(rec.clone())
        .build();

    p1.process(delta("d"));
    p2.process(err("e"));
    p1.process(tc("t")); // filtered
    p2.process(delta("d2")); // filtered

    assert_eq!(rec.len(), 2);
}

#[test]
fn two_pipelines_share_one_stats() {
    let st = EventStats::new();
    let p1 = StreamPipelineBuilder::new().with_stats(st.clone()).build();
    let p2 = StreamPipelineBuilder::new().with_stats(st.clone()).build();

    p1.process(delta("a"));
    p2.process(delta("b"));
    assert_eq!(st.total_events(), 2);
}

#[test]
fn pipeline_process_returns_none_leaves_recorder_unchanged() {
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .with_recorder(rec.clone())
        .build();

    assert!(p.process(delta("x")).is_none());
    assert_eq!(rec.len(), 0);
}

// ===========================================================================
// 6. Empty stream handling
// ===========================================================================

#[tokio::test]
async fn empty_stream_collect_all() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(8);
    drop(tx);
    let s = EventStream::new(rx);
    assert!(s.collect_all().await.is_empty());
}

#[tokio::test]
async fn empty_stream_collect_filtered() {
    let (tx, rx) = mpsc::channel::<AgentEvent>(8);
    drop(tx);
    let s = EventStream::new(rx);
    let f = EventFilter::by_kind("assistant_delta");
    assert!(s.collect_filtered(&f).await.is_empty());
}

#[tokio::test]
async fn empty_stream_pipe_produces_nothing() {
    let (tx_in, rx_in) = mpsc::channel::<AgentEvent>(8);
    let (tx_out, mut rx_out) = mpsc::channel::<AgentEvent>(8);
    drop(tx_in);

    let p = StreamPipeline::new();
    let s = EventStream::new(rx_in);
    s.pipe(&p, tx_out).await;

    assert!(rx_out.recv().await.is_none());
}

#[tokio::test]
async fn empty_multiplexer_collect_sorted() {
    let mux = EventMultiplexer::new(vec![]);
    assert!(mux.collect_sorted().await.is_empty());
}

#[tokio::test]
async fn empty_multiplexer_merge() {
    let mux = EventMultiplexer::new(vec![]);
    let mut rx = mux.merge(8);
    assert!(rx.recv().await.is_none());
}

// ===========================================================================
// 7. Large stream handling (1000+ events)
// ===========================================================================

#[test]
fn large_mixed_stream_through_pipeline() {
    let rec = EventRecorder::new();
    let st = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .with_recorder(rec.clone())
        .with_stats(st.clone())
        .build();

    let mut error_count = 0u64;
    for i in 0..1500 {
        if i % 10 == 0 {
            p.process(err(&format!("e{i}")));
            error_count += 1;
        } else if i % 10 == 1 {
            p.process(tc(&format!("tool{i}")));
        } else {
            p.process(delta(&format!("d{i}")));
        }
    }
    let expected_pass = 1500 - error_count;
    assert_eq!(st.total_events(), expected_pass);
    assert_eq!(rec.len() as u64, expected_pass);
}

#[test]
fn large_stream_stats_delta_bytes_accumulate() {
    let s = EventStats::new();
    for i in 0..2000 {
        s.observe(&delta(&format!("{i:04}")));
    }
    // Each "XXXX" is 4 bytes
    assert_eq!(s.total_delta_bytes(), 2000 * 4);
    assert_eq!(s.total_events(), 2000);
}

#[tokio::test]
async fn large_stream_pipe_async() {
    let (tx_in, rx_in) = mpsc::channel(128);
    let (tx_out, mut rx_out) = mpsc::channel(128);

    let st = EventStats::new();
    let pipeline = StreamPipelineBuilder::new().with_stats(st.clone()).build();

    let sender = tokio::spawn(async move {
        for i in 0..1200 {
            tx_in.send(delta(&format!("ev{i}"))).await.unwrap();
        }
    });

    let stream = EventStream::new(rx_in);
    let piper = tokio::spawn(async move {
        stream.pipe(&pipeline, tx_out).await;
    });

    let consumer = tokio::spawn(async move {
        let mut n = 0u64;
        while rx_out.recv().await.is_some() {
            n += 1;
        }
        n
    });

    sender.await.unwrap();
    piper.await.unwrap();
    assert_eq!(consumer.await.unwrap(), 1200);
    assert_eq!(st.total_events(), 1200);
}

#[test]
fn large_stream_recorder_replay() {
    let r = EventRecorder::new();
    for i in 0..1100 {
        r.record(&delta(&format!("r{i}")));
    }
    assert_eq!(r.len(), 1100);
    let events = r.events();
    assert_eq!(events.len(), 1100);
    // Spot check order
    match &events[500].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "r500"),
        _ => panic!("unexpected"),
    }
}

// ===========================================================================
// 8. Concurrent stream processing
// ===========================================================================

#[tokio::test]
async fn concurrent_pipeline_clones_share_state() {
    let rec = EventRecorder::new();
    let st = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(rec.clone())
        .with_stats(st.clone())
        .build();

    let mut handles = Vec::new();
    for tid in 0..8 {
        let pc = p.clone();
        handles.push(tokio::spawn(async move {
            for i in 0..125 {
                pc.process(delta(&format!("t{tid}-{i}")));
            }
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(rec.len(), 1000);
    assert_eq!(st.total_events(), 1000);
}

#[tokio::test]
async fn concurrent_recorders_shared_across_tasks() {
    let r = EventRecorder::new();
    let mut handles = Vec::new();
    for tid in 0..4 {
        let rc = r.clone();
        handles.push(tokio::spawn(async move {
            for i in 0..250 {
                rc.record(&delta(&format!("{tid}-{i}")));
            }
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(r.len(), 1000);
}

#[tokio::test]
async fn concurrent_stats_error_tracking() {
    let s = EventStats::new();
    let mut handles = Vec::new();
    for _ in 0..5 {
        let sc = s.clone();
        handles.push(tokio::spawn(async move {
            for _ in 0..100 {
                sc.observe(&err("e"));
            }
            for _ in 0..100 {
                sc.observe(&delta("d"));
            }
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(s.total_events(), 1000);
    assert_eq!(s.error_count(), 500);
    assert_eq!(s.count_for("error"), 500);
    assert_eq!(s.count_for("assistant_delta"), 500);
}

// ===========================================================================
// 9. Stream error handling (channel closure)
// ===========================================================================

#[tokio::test]
async fn stream_recv_none_after_sender_drop() {
    let (tx, rx) = mpsc::channel(4);
    tx.send(delta("first")).await.unwrap();
    drop(tx);
    let mut s = EventStream::new(rx);
    assert!(s.recv().await.is_some());
    assert!(s.recv().await.is_none());
}

#[tokio::test]
async fn pipe_stops_when_input_closes() {
    let (tx_in, rx_in) = mpsc::channel(4);
    let (tx_out, mut rx_out) = mpsc::channel(4);

    tx_in.send(delta("one")).await.unwrap();
    drop(tx_in);

    let p = StreamPipeline::new();
    EventStream::new(rx_in).pipe(&p, tx_out).await;

    let ev = rx_out.recv().await;
    assert!(ev.is_some());
    assert!(rx_out.recv().await.is_none());
}

#[tokio::test]
async fn pipe_stops_when_output_receiver_drops() {
    let (tx_in, rx_in) = mpsc::channel(4);
    let (tx_out, rx_out) = mpsc::channel(4);

    // Drop receiver — pipe should exit gracefully
    drop(rx_out);

    let p = StreamPipeline::new();
    let sender = tokio::spawn(async move {
        for i in 0..10 {
            let _ = tx_in.send(delta(&format!("ev{i}"))).await;
        }
        drop(tx_in);
    });

    // pipe should exit without panic
    EventStream::new(rx_in).pipe(&p, tx_out).await;
    sender.await.unwrap();
}

// ===========================================================================
// 10. Event type filtering — selective pass-through
// ===========================================================================

#[test]
fn filter_only_tool_events() {
    let f = EventFilter::new(|ev| {
        matches!(
            ev.kind,
            AgentEventKind::ToolCall { .. } | AgentEventKind::ToolResult { .. }
        )
    });
    for ev in every_kind() {
        let is_tool = matches!(
            ev.kind,
            AgentEventKind::ToolCall { .. } | AgentEventKind::ToolResult { .. }
        );
        assert_eq!(f.matches(&ev), is_tool);
    }
}

#[test]
fn filter_only_lifecycle_events() {
    let f = EventFilter::new(|ev| {
        matches!(
            ev.kind,
            AgentEventKind::RunStarted { .. } | AgentEventKind::RunCompleted { .. }
        )
    });
    assert!(f.matches(&started()));
    assert!(f.matches(&completed()));
    assert!(!f.matches(&delta("d")));
    assert!(!f.matches(&err("e")));
}

#[test]
fn pipeline_filters_to_file_changes_only() {
    let st = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::by_kind("file_changed"))
        .with_stats(st.clone())
        .build();

    for ev in every_kind() {
        p.process(ev);
    }
    assert_eq!(st.total_events(), 1);
    assert_eq!(st.count_for("file_changed"), 1);
}

// ===========================================================================
// 11. Stream pause/resume (via recorder clear & re-record)
// ===========================================================================

#[test]
fn recorder_simulates_pause_resume() {
    let r = EventRecorder::new();

    // Phase 1: record
    r.record(&delta("a"));
    r.record(&delta("b"));
    assert_eq!(r.len(), 2);

    // "Pause" — take snapshot and clear
    let phase1 = r.events();
    r.clear();
    assert_eq!(r.len(), 0);

    // Phase 2: resume
    r.record(&delta("c"));
    r.record(&delta("d"));
    let phase2 = r.events();

    assert_eq!(phase1.len(), 2);
    assert_eq!(phase2.len(), 2);
}

#[test]
fn stats_reset_simulates_window_boundary() {
    let s = EventStats::new();

    // Window 1
    for _ in 0..50 {
        s.observe(&delta("x"));
    }
    let w1_total = s.total_events();
    let w1_bytes = s.total_delta_bytes();

    s.reset();

    // Window 2
    for _ in 0..30 {
        s.observe(&err("e"));
    }
    assert_eq!(w1_total, 50);
    assert_eq!(w1_bytes, 50);
    assert_eq!(s.total_events(), 30);
    assert_eq!(s.error_count(), 30);
    assert_eq!(s.total_delta_bytes(), 0);
}

// ===========================================================================
// 12. Stream replay
// ===========================================================================

#[tokio::test]
async fn replay_recorded_events_through_new_pipeline() {
    let r = EventRecorder::new();
    let original_pipeline = StreamPipelineBuilder::new()
        .with_recorder(r.clone())
        .build();

    // Record some events
    original_pipeline.process(delta("one"));
    original_pipeline.process(tc("read"));
    original_pipeline.process(err("oops"));

    // Replay through a filtering pipeline
    let st = EventStats::new();
    let replay_pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .with_stats(st.clone())
        .build();

    for ev in r.events() {
        replay_pipeline.process(ev);
    }

    assert_eq!(st.total_events(), 2);
    assert_eq!(st.error_count(), 0);
}

#[tokio::test]
async fn replay_via_channel_and_stream() {
    let r = EventRecorder::new();
    r.record(&delta("a"));
    r.record(&delta("b"));
    r.record(&delta("c"));

    let (tx, rx) = mpsc::channel(8);
    for ev in r.events() {
        tx.send(ev).await.unwrap();
    }
    drop(tx);

    let stream = EventStream::new(rx);
    let collected = stream.collect_all().await;
    assert_eq!(collected.len(), 3);
}

// ===========================================================================
// 13. Serde roundtrip of events through pipeline
// ===========================================================================

#[test]
fn serde_roundtrip_every_event_kind() {
    for ev in every_kind() {
        let json = serde_json::to_string(&ev).unwrap();
        let de: AgentEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event_kind_name(&de.kind), event_kind_name(&ev.kind));
    }
}

#[test]
fn serde_roundtrip_event_with_ext() {
    let mut ext = BTreeMap::new();
    ext.insert("num".into(), serde_json::json!(42));
    ext.insert("arr".into(), serde_json::json!([1, 2, 3]));
    let ev = mk_ext(
        AgentEventKind::AssistantDelta {
            text: "hello".into(),
        },
        ext,
    );
    let json = serde_json::to_string(&ev).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(de.ext.as_ref().unwrap()["num"], 42);
    assert_eq!(
        de.ext.as_ref().unwrap()["arr"],
        serde_json::json!([1, 2, 3])
    );
}

#[test]
fn serde_roundtrip_tool_call_complex_input() {
    let ev = mk(AgentEventKind::ToolCall {
        tool_name: "edit_file".into(),
        tool_use_id: Some("tc-42".into()),
        parent_tool_use_id: Some("tc-0".into()),
        input: serde_json::json!({
            "path": "src/main.rs",
            "content": "fn main() {}",
            "lines": [1, 2, 3]
        }),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    match &de.kind {
        AgentEventKind::ToolCall {
            tool_name,
            tool_use_id,
            parent_tool_use_id,
            input,
        } => {
            assert_eq!(tool_name, "edit_file");
            assert_eq!(tool_use_id.as_deref(), Some("tc-42"));
            assert_eq!(parent_tool_use_id.as_deref(), Some("tc-0"));
            assert_eq!(input["path"], "src/main.rs");
        }
        _ => panic!("expected ToolCall"),
    }
}

#[test]
fn serde_roundtrip_command_executed_all_fields() {
    let ev = mk(AgentEventKind::CommandExecuted {
        command: "cargo test".into(),
        exit_code: Some(0),
        output_preview: Some("ok".into()),
    });
    let json = serde_json::to_string(&ev).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    match &de.kind {
        AgentEventKind::CommandExecuted {
            command,
            exit_code,
            output_preview,
        } => {
            assert_eq!(command, "cargo test");
            assert_eq!(*exit_code, Some(0));
            assert_eq!(output_preview.as_deref(), Some("ok"));
        }
        _ => panic!("expected CommandExecuted"),
    }
}

#[test]
fn serde_roundtrip_after_pipeline_transform() {
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            ev.ext
                .get_or_insert_with(BTreeMap::new)
                .insert("pipeline_version".into(), serde_json::json!("1.0"));
            ev
        }))
        .build();

    let result = p.process(delta("test")).unwrap();
    let json = serde_json::to_string(&result).unwrap();
    let de: AgentEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(
        de.ext.as_ref().unwrap()["pipeline_version"],
        serde_json::json!("1.0")
    );
}

// ===========================================================================
// 14. Multiplexer — advanced scenarios
// ===========================================================================

#[tokio::test]
async fn multiplexer_single_stream() {
    let (tx, rx) = mpsc::channel(8);
    tx.send(delta("only")).await.unwrap();
    drop(tx);

    let mux = EventMultiplexer::new(vec![rx]);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn multiplexer_five_streams_sorted() {
    let base = Utc::now();
    let mut receivers = Vec::new();

    for i in 0..5u32 {
        let (tx, rx) = mpsc::channel(8);
        // Reverse order timestamps to verify sorting
        let ts = base + Duration::milliseconds(50 - i as i64 * 10);
        tx.send(mk_ts(
            AgentEventKind::AssistantDelta {
                text: format!("s{i}"),
            },
            ts,
        ))
        .await
        .unwrap();
        drop(tx);
        receivers.push(rx);
    }

    let mux = EventMultiplexer::new(receivers);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), 5);
    for w in events.windows(2) {
        assert!(w[0].ts <= w[1].ts, "events must be sorted by timestamp");
    }
}

#[tokio::test]
async fn multiplexer_merge_preserves_all_events() {
    let base = Utc::now();
    let (tx1, rx1) = mpsc::channel(8);
    let (tx2, rx2) = mpsc::channel(8);

    tx1.send(mk_ts(
        AgentEventKind::RunStarted {
            message: "a".into(),
        },
        base,
    ))
    .await
    .unwrap();
    tx1.send(mk_ts(
        AgentEventKind::AssistantDelta { text: "d".into() },
        base + Duration::milliseconds(20),
    ))
    .await
    .unwrap();

    tx2.send(mk_ts(
        AgentEventKind::ToolCall {
            tool_name: "t".into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        },
        base + Duration::milliseconds(10),
    ))
    .await
    .unwrap();

    drop(tx1);
    drop(tx2);

    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let mut merged = mux.merge(16);

    let mut events = Vec::new();
    while let Some(ev) = merged.recv().await {
        events.push(ev);
    }
    assert_eq!(events.len(), 3);
    // Verify all event kinds are present (merge order is nondeterministic)
    assert!(events
        .iter()
        .any(|e| matches!(e.kind, AgentEventKind::RunStarted { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e.kind, AgentEventKind::ToolCall { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e.kind, AgentEventKind::AssistantDelta { .. })));
}

// ===========================================================================
// 15. EventStream — collect_filtered, into_inner
// ===========================================================================

#[tokio::test]
async fn stream_collect_filtered_complex_filter() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("short")).await.unwrap();
    tx.send(delta("a longer sentence")).await.unwrap();
    tx.send(err("bad")).await.unwrap();
    tx.send(delta("hi")).await.unwrap();
    drop(tx);

    let f = EventFilter::new(
        |ev| matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text.len() > 4),
    );
    let stream = EventStream::new(rx);
    let events = stream.collect_filtered(&f).await;
    assert_eq!(events.len(), 2); // "short" (5) and "a longer sentence" (18)
}

#[tokio::test]
async fn stream_into_inner_returns_usable_receiver() {
    let (tx, rx) = mpsc::channel(4);
    tx.send(delta("hi")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let mut inner = stream.into_inner();
    let ev = inner.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { .. }));
    assert!(inner.recv().await.is_none());
}

#[tokio::test]
async fn stream_recv_sequential_events() {
    let (tx, rx) = mpsc::channel(8);
    tx.send(started()).await.unwrap();
    tx.send(delta("hello")).await.unwrap();
    tx.send(completed()).await.unwrap();
    drop(tx);

    let mut s = EventStream::new(rx);
    assert!(matches!(
        s.recv().await.unwrap().kind,
        AgentEventKind::RunStarted { .. }
    ));
    assert!(matches!(
        s.recv().await.unwrap().kind,
        AgentEventKind::AssistantDelta { .. }
    ));
    assert!(matches!(
        s.recv().await.unwrap().kind,
        AgentEventKind::RunCompleted { .. }
    ));
    assert!(s.recv().await.is_none());
}

// ===========================================================================
// 16. Back-pressure
// ===========================================================================

#[tokio::test]
async fn back_pressure_small_channel() {
    let (tx, rx) = mpsc::channel(1);

    tx.send(delta("1")).await.unwrap();
    // Channel full; spawn a task to send more once drained
    let sender = tokio::spawn(async move {
        tx.send(delta("2")).await.unwrap();
        tx.send(delta("3")).await.unwrap();
    });

    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    sender.await.unwrap();
    assert_eq!(events.len(), 3);
}

// ===========================================================================
// 17. event_kind_name exhaustive
// ===========================================================================

#[test]
fn event_kind_name_all_names_unique() {
    let events = every_kind();
    let names: Vec<_> = events.iter().map(|e| event_kind_name(&e.kind)).collect();
    let unique: std::collections::HashSet<_> = names.iter().collect();
    assert_eq!(names.len(), unique.len(), "all kind names must be unique");
}

#[test]
fn event_kind_name_are_snake_case() {
    for ev in every_kind() {
        let name = event_kind_name(&ev.kind);
        assert!(
            name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "kind name {name} is not snake_case"
        );
    }
}

// ===========================================================================
// 18. Pipeline cloning
// ===========================================================================

#[test]
fn pipeline_clone_processes_independently() {
    let rec = EventRecorder::new();
    let st = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(rec.clone())
        .with_stats(st.clone())
        .build();

    let p2 = p.clone();
    p.process(delta("from-p"));
    p2.process(delta("from-p2"));

    // Both share the same Arc-based recorder/stats
    assert_eq!(rec.len(), 2);
    assert_eq!(st.total_events(), 2);
}

#[test]
fn pipeline_default_and_new_are_equivalent() {
    let p1 = StreamPipeline::new();
    let p2 = StreamPipeline::default();
    // Both pass-through, no recorder, no stats
    assert!(p1.recorder().is_none());
    assert!(p2.recorder().is_none());
    assert!(p1.stats().is_none());
    assert!(p2.stats().is_none());
    assert!(p1.process(delta("x")).is_some());
    assert!(p2.process(delta("x")).is_some());
}

// ===========================================================================
// 19. Debug impls
// ===========================================================================

#[test]
fn all_types_implement_debug() {
    let f = EventFilter::by_kind("error");
    let t = EventTransform::identity();
    let r = EventRecorder::new();
    let s = EventStats::new();
    let p = StreamPipeline::new();
    let b = StreamPipelineBuilder::new();

    assert!(!format!("{f:?}").is_empty());
    assert!(!format!("{t:?}").is_empty());
    assert!(!format!("{r:?}").is_empty());
    assert!(!format!("{s:?}").is_empty());
    assert!(!format!("{p:?}").is_empty());
    assert!(!format!("{b:?}").is_empty());
}

// ===========================================================================
// 20. Miscellaneous edge cases
// ===========================================================================

#[test]
fn filter_by_nonexistent_kind_rejects_all() {
    let f = EventFilter::by_kind("nonexistent_kind_name");
    for ev in every_kind() {
        assert!(!f.matches(&ev));
    }
}

#[test]
fn pipeline_with_no_filter_no_transform_records_all() {
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(rec.clone())
        .build();
    for ev in every_kind() {
        p.process(ev);
    }
    assert_eq!(rec.len(), 10);
}

#[test]
fn stats_observe_same_event_many_times() {
    let s = EventStats::new();
    let ev = delta("repeat");
    for _ in 0..100 {
        s.observe(&ev);
    }
    assert_eq!(s.total_events(), 100);
    assert_eq!(s.count_for("assistant_delta"), 100);
    assert_eq!(s.total_delta_bytes(), 600); // "repeat" = 6 bytes × 100
}

#[test]
fn transform_on_warning_event() {
    let t = EventTransform::new(|mut ev| {
        if let AgentEventKind::Warning { ref mut message } = ev.kind {
            *message = format!("[WARN] {message}");
        }
        ev
    });
    let result = t.apply(warn("disk space low"));
    match &result.kind {
        AgentEventKind::Warning { message } => assert_eq!(message, "[WARN] disk space low"),
        _ => panic!("expected Warning"),
    }
}

#[test]
fn pipeline_filter_and_stats_only() {
    let st = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::by_kind("tool_call"))
        .with_stats(st.clone())
        .build();

    p.process(tc("read_file"));
    p.process(tr("read_file"));
    p.process(delta("d"));
    p.process(tc("write_file"));

    assert_eq!(st.total_events(), 2);
    assert_eq!(st.count_for("tool_call"), 2);
}

#[test]
fn recorder_len_matches_events_len() {
    let r = EventRecorder::new();
    for i in 0..50 {
        r.record(&delta(&format!("ev{i}")));
        assert_eq!(r.len(), r.events().len());
    }
}

#[tokio::test]
async fn multiplexer_with_different_event_types() {
    let base = Utc::now();
    let (tx1, rx1) = mpsc::channel(4);
    let (tx2, rx2) = mpsc::channel(4);
    let (tx3, rx3) = mpsc::channel(4);

    tx1.send(mk_ts(
        AgentEventKind::RunStarted {
            message: "go".into(),
        },
        base,
    ))
    .await
    .unwrap();
    tx2.send(mk_ts(
        AgentEventKind::Error {
            message: "err".into(),
            error_code: None,
        },
        base + Duration::milliseconds(5),
    ))
    .await
    .unwrap();
    tx3.send(mk_ts(
        AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        base + Duration::milliseconds(10),
    ))
    .await
    .unwrap();

    drop(tx1);
    drop(tx2);
    drop(tx3);

    let mux = EventMultiplexer::new(vec![rx1, rx2, rx3]);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), 3);
    assert!(matches!(events[0].kind, AgentEventKind::RunStarted { .. }));
    assert!(matches!(events[1].kind, AgentEventKind::Error { .. }));
    assert!(matches!(
        events[2].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[test]
fn pipeline_transform_then_filter_via_two_pipelines() {
    // Transform first, then filter the output in a second pipeline
    let p1 = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
                *text = format!("prefix:{text}");
            }
            ev
        }))
        .build();

    let rec = EventRecorder::new();
    let p2 = StreamPipelineBuilder::new()
        .filter(EventFilter::new(|ev| {
            matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text.starts_with("prefix:"))
        }))
        .with_recorder(rec.clone())
        .build();

    // Feed through p1 then p2
    if let Some(ev) = p1.process(delta("hello")) {
        p2.process(ev);
    }
    if let Some(ev) = p1.process(err("nope")) {
        p2.process(ev);
    }
    if let Some(ev) = p1.process(delta("world")) {
        p2.process(ev);
    }

    assert_eq!(rec.len(), 2);
    match &rec.events()[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "prefix:hello"),
        _ => panic!("expected delta"),
    }
}

#[tokio::test]
async fn stream_collect_filtered_accept_all() {
    let (tx, rx) = mpsc::channel(16);
    for ev in every_kind() {
        tx.send(ev).await.unwrap();
    }
    drop(tx);

    let f = EventFilter::new(|_| true);
    let events = EventStream::new(rx).collect_filtered(&f).await;
    assert_eq!(events.len(), 10);
}

#[tokio::test]
async fn stream_collect_filtered_reject_all() {
    let (tx, rx) = mpsc::channel(16);
    for ev in every_kind() {
        tx.send(ev).await.unwrap();
    }
    drop(tx);

    let f = EventFilter::new(|_| false);
    let events = EventStream::new(rx).collect_filtered(&f).await;
    assert!(events.is_empty());
}

#[test]
fn stats_multiple_resets() {
    let s = EventStats::new();
    for _ in 0..3 {
        s.observe(&delta("x"));
        s.observe(&err("e"));
        assert!(s.total_events() > 0);
        s.reset();
        assert_eq!(s.total_events(), 0);
        assert_eq!(s.error_count(), 0);
        assert_eq!(s.total_delta_bytes(), 0);
    }
}

#[test]
fn recorder_clear_then_is_empty() {
    let r = EventRecorder::new();
    assert!(r.is_empty());
    r.record(&delta("a"));
    assert!(!r.is_empty());
    r.clear();
    assert!(r.is_empty());
}

#[test]
fn pipeline_builder_default_same_as_new() {
    let b1 = StreamPipelineBuilder::new();
    let b2 = StreamPipelineBuilder::default();
    let p1 = b1.build();
    let p2 = b2.build();
    assert!(p1.recorder().is_none());
    assert!(p2.recorder().is_none());
}

// ===========================================================================
// 21. Additional back-pressure & concurrency tests
// ===========================================================================

#[tokio::test]
async fn back_pressure_channel_size_one_no_loss() {
    let (tx, rx) = mpsc::channel(1);
    let sender = tokio::spawn(async move {
        for i in 0..20 {
            tx.send(delta(&format!("bp-{i}"))).await.unwrap();
        }
    });
    let events = EventStream::new(rx).collect_all().await;
    sender.await.unwrap();
    assert_eq!(events.len(), 20);
}

#[tokio::test]
async fn back_pressure_multiple_producers_no_loss() {
    let (tx, rx) = mpsc::channel(4);
    let tx2 = tx.clone();
    let h1 = tokio::spawn(async move {
        for i in 0..25 {
            tx.send(delta(&format!("p1-{i}"))).await.unwrap();
        }
    });
    let h2 = tokio::spawn(async move {
        for i in 0..25 {
            tx2.send(delta(&format!("p2-{i}"))).await.unwrap();
        }
    });
    let events = EventStream::new(rx).collect_all().await;
    h1.await.unwrap();
    h2.await.unwrap();
    assert_eq!(events.len(), 50);
}

#[tokio::test]
async fn concurrent_recorder_from_multiple_tasks() {
    let rec = EventRecorder::new();
    let r1 = rec.clone();
    let r2 = rec.clone();
    let h1 = tokio::spawn(async move {
        for i in 0..40 {
            r1.record(&delta(&format!("a{i}")));
        }
    });
    let h2 = tokio::spawn(async move {
        for i in 0..40 {
            r2.record(&delta(&format!("b{i}")));
        }
    });
    h1.await.unwrap();
    h2.await.unwrap();
    assert_eq!(rec.len(), 80);
}

#[tokio::test]
async fn concurrent_stats_from_multiple_tasks() {
    let stats = EventStats::new();
    let s1 = stats.clone();
    let s2 = stats.clone();
    let h1 = tokio::spawn(async move {
        for _ in 0..30 {
            s1.observe(&delta("x"));
        }
    });
    let h2 = tokio::spawn(async move {
        for _ in 0..30 {
            s2.observe(&err("e"));
        }
    });
    h1.await.unwrap();
    h2.await.unwrap();
    assert_eq!(stats.total_events(), 60);
    assert_eq!(stats.error_count(), 30);
    assert_eq!(stats.count_for("assistant_delta"), 30);
}

// ===========================================================================
// 22. Multiplexer additional edge cases
// ===========================================================================

#[tokio::test]
async fn multiplexer_single_stream_sorted() {
    let (tx, rx) = mpsc::channel(8);
    tx.send(delta("solo")).await.unwrap();
    drop(tx);
    let events = EventMultiplexer::new(vec![rx]).collect_sorted().await;
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn multiplexer_five_streams_interleaved() {
    let base = Utc::now();
    let mut rxs = vec![];
    for stream_idx in 0..5i64 {
        let (tx, rx) = mpsc::channel(4);
        for ev_idx in 0..3i64 {
            let offset = stream_idx + ev_idx * 5;
            tx.send(mk_ts(
                AgentEventKind::AssistantDelta {
                    text: format!("s{stream_idx}-e{ev_idx}"),
                },
                base + Duration::milliseconds(offset),
            ))
            .await
            .unwrap();
        }
        drop(tx);
        rxs.push(rx);
    }
    let events = EventMultiplexer::new(rxs).collect_sorted().await;
    assert_eq!(events.len(), 15);
    for i in 1..events.len() {
        assert!(events[i].ts >= events[i - 1].ts);
    }
}

#[tokio::test]
async fn multiplexer_merge_single_stream() {
    let (tx, rx) = mpsc::channel(4);
    tx.send(delta("only")).await.unwrap();
    drop(tx);
    let mut merged = EventMultiplexer::new(vec![rx]).merge(4);
    assert!(merged.recv().await.is_some());
    assert!(merged.recv().await.is_none());
}

#[tokio::test]
async fn multiplexer_same_timestamp_stable() {
    let ts = Utc::now();
    let mut rxs = vec![];
    for _ in 0..5 {
        let (tx, rx) = mpsc::channel(2);
        tx.send(mk_ts(
            AgentEventKind::AssistantDelta {
                text: "same".into(),
            },
            ts,
        ))
        .await
        .unwrap();
        drop(tx);
        rxs.push(rx);
    }
    let events = EventMultiplexer::new(rxs).collect_sorted().await;
    assert_eq!(events.len(), 5);
    for e in &events {
        assert_eq!(e.ts, ts);
    }
}

// ===========================================================================
// 23. Stream pipe & completion edge cases
// ===========================================================================

#[tokio::test]
async fn pipe_stops_when_output_closed() {
    let (tx_in, rx_in) = mpsc::channel(4);
    let (tx_out, rx_out) = mpsc::channel(1);
    drop(rx_out); // close output side

    tx_in.send(delta("a")).await.unwrap();
    tx_in.send(delta("b")).await.unwrap();
    drop(tx_in);

    // Should not panic even though output is closed
    let pipeline = StreamPipeline::new();
    EventStream::new(rx_in).pipe(&pipeline, tx_out).await;
}

#[tokio::test]
async fn pipe_with_transform_and_filter_and_stats() {
    let stats = EventStats::new();
    let recorder = EventRecorder::new();
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("piped".into(), serde_json::json!(true));
            ev
        }))
        .with_stats(stats.clone())
        .with_recorder(recorder.clone())
        .build();

    let (tx_in, rx_in) = mpsc::channel(16);
    let (tx_out, mut rx_out) = mpsc::channel(16);
    tx_in.send(delta("a")).await.unwrap();
    tx_in.send(err("e")).await.unwrap();
    tx_in.send(warn("w")).await.unwrap();
    tx_in.send(tc("read")).await.unwrap();
    drop(tx_in);

    EventStream::new(rx_in).pipe(&pipeline, tx_out).await;

    let mut results = vec![];
    while let Some(ev) = rx_out.recv().await {
        results.push(ev);
    }
    assert_eq!(results.len(), 3); // error filtered
    assert_eq!(stats.total_events(), 3);
    assert_eq!(recorder.len(), 3);
    for r in &results {
        assert!(r.ext.as_ref().unwrap().contains_key("piped"));
    }
}

#[tokio::test]
async fn collect_filtered_rejects_all() {
    let (tx, rx) = mpsc::channel(8);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);
    let filter = EventFilter::errors_only();
    let events = EventStream::new(rx).collect_filtered(&filter).await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn collect_filtered_accepts_all() {
    let (tx, rx) = mpsc::channel(8);
    tx.send(err("e1")).await.unwrap();
    tx.send(err("e2")).await.unwrap();
    drop(tx);
    let filter = EventFilter::errors_only();
    let events = EventStream::new(rx).collect_filtered(&filter).await;
    assert_eq!(events.len(), 2);
}

// ===========================================================================
// 24. Unicode / large payload edge cases
// ===========================================================================

#[test]
fn stats_unicode_delta_bytes() {
    let s = EventStats::new();
    s.observe(&delta("🦀")); // 4 UTF-8 bytes
    assert_eq!(s.total_delta_bytes(), 4);
}

#[test]
fn large_delta_through_pipeline() {
    let big = "x".repeat(100_000);
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::identity())
        .build();
    let result = p.process(delta(&big)).unwrap();
    match &result.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text.len(), 100_000),
        _ => panic!("wrong kind"),
    }
}

#[test]
fn empty_string_events_pass_through_pipeline() {
    let p = StreamPipeline::new();
    assert!(p.process(delta("")).is_some());
    assert!(p.process(msg("")).is_some());
    assert!(p.process(err("")).is_some());
}
