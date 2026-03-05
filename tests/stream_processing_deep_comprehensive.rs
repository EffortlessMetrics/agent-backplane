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
#![allow(clippy::useless_vec, clippy::needless_borrows_for_generic_args)]
//! Deep comprehensive tests for the `abp-stream` event stream processing
//! pipeline: EventFilter, EventRecorder, EventStats, EventStream,
//! EventMultiplexer, EventTransform, StreamPipeline, and
//! StreamPipelineBuilder — covering construction, filtering, recording, stats,
//! pipeline chaining, ordering, sequence validation, timestamp monotonicity,
//! back-pressure, empty/single/large streams, mixed types, error handling, and
//! concurrent processing.

use abp_core::{AgentEvent, AgentEventKind};
use abp_stream::{
    event_kind_name, EventFilter, EventMultiplexer, EventRecorder, EventStats, EventStream,
    EventTransform, StreamPipelineBuilder,
};
use chrono::{Duration, Utc};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
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
    mk(AgentEventKind::AssistantDelta { text: t.into() })
}

fn msg(t: &str) -> AgentEvent {
    mk(AgentEventKind::AssistantMessage { text: t.into() })
}

fn err(m: &str) -> AgentEvent {
    mk(AgentEventKind::Error {
        message: m.into(),
        error_code: None,
    })
}

fn warn(m: &str) -> AgentEvent {
    mk(AgentEventKind::Warning { message: m.into() })
}

fn tc(name: &str) -> AgentEvent {
    mk(AgentEventKind::ToolCall {
        tool_name: name.into(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::json!({}),
    })
}

fn tr(name: &str) -> AgentEvent {
    mk(AgentEventKind::ToolResult {
        tool_name: name.into(),
        tool_use_id: None,
        output: serde_json::json!("ok"),
        is_error: false,
    })
}

fn fc(path: &str) -> AgentEvent {
    mk(AgentEventKind::FileChanged {
        path: path.into(),
        summary: "modified".into(),
    })
}

fn cmd(c: &str, code: Option<i32>) -> AgentEvent {
    mk(AgentEventKind::CommandExecuted {
        command: c.into(),
        exit_code: code,
        output_preview: None,
    })
}

fn started() -> AgentEvent {
    mk(AgentEventKind::RunStarted {
        message: "go".into(),
    })
}

fn completed() -> AgentEvent {
    mk(AgentEventKind::RunCompleted {
        message: "done".into(),
    })
}

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

fn delta_ts(t: &str, ts: chrono::DateTime<Utc>) -> AgentEvent {
    mk_ts(AgentEventKind::AssistantDelta { text: t.into() }, ts)
}

fn err_ts(m: &str, ts: chrono::DateTime<Utc>) -> AgentEvent {
    mk_ts(
        AgentEventKind::Error {
            message: m.into(),
            error_code: None,
        },
        ts,
    )
}

// =========================================================================
// 1. StreamPipeline construction and configuration
// =========================================================================

#[test]
fn pipeline_default_is_passthrough() {
    let p = StreamPipelineBuilder::new().build();
    assert!(p.recorder().is_none());
    assert!(p.stats().is_none());
    for ev in every_kind() {
        assert!(p.process(ev).is_some());
    }
}

#[test]
fn pipeline_new_equals_default() {
    let p = abp_stream::StreamPipeline::new();
    assert!(p.process(delta("x")).is_some());
    assert!(p.recorder().is_none());
    assert!(p.stats().is_none());
}

#[test]
fn pipeline_builder_record_creates_recorder() {
    let p = StreamPipelineBuilder::new().record().build();
    assert!(p.recorder().is_some());
    assert!(p.recorder().unwrap().is_empty());
}

#[test]
fn pipeline_builder_with_external_recorder() {
    let rec = EventRecorder::new();
    rec.record(&delta("pre"));
    let p = StreamPipelineBuilder::new()
        .with_recorder(rec.clone())
        .build();
    p.process(delta("post"));
    assert_eq!(rec.len(), 2);
}

#[test]
fn pipeline_builder_with_stats() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();
    p.process(delta("x"));
    assert_eq!(stats.total_events(), 1);
    assert!(p.stats().is_some());
}

#[test]
fn pipeline_builder_chained_fluent() {
    let rec = EventRecorder::new();
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .transform(EventTransform::identity())
        .with_recorder(rec.clone())
        .with_stats(stats.clone())
        .build();
    p.process(delta("a"));
    p.process(err("b"));
    assert_eq!(rec.len(), 1);
    assert_eq!(stats.total_events(), 1);
}

#[test]
fn pipeline_clone_shares_recorder_and_stats() {
    let rec = EventRecorder::new();
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(rec.clone())
        .with_stats(stats.clone())
        .build();
    let p2 = p.clone();
    p.process(delta("a"));
    p2.process(delta("b"));
    assert_eq!(rec.len(), 2);
    assert_eq!(stats.total_events(), 2);
}

#[test]
fn pipeline_debug_does_not_panic() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .transform(EventTransform::identity())
        .record()
        .with_stats(EventStats::new())
        .build();
    let dbg = format!("{p:?}");
    assert!(dbg.contains("StreamPipeline"));
}

// =========================================================================
// 2. EventFilter with various predicates
// =========================================================================

#[test]
fn filter_by_kind_all_variants() {
    let names = [
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
    let events = every_kind();
    for (name, ev) in names.iter().zip(events.iter()) {
        let f = EventFilter::by_kind(name);
        assert!(f.matches(ev), "by_kind({name}) should match its own event");
    }
}

#[test]
fn filter_by_kind_rejects_other_variants() {
    let f = EventFilter::by_kind("run_started");
    assert!(!f.matches(&delta("x")));
    assert!(!f.matches(&err("x")));
    assert!(!f.matches(&tc("x")));
}

#[test]
fn filter_errors_only_rejects_warnings() {
    let f = EventFilter::errors_only();
    assert!(!f.matches(&warn("w")));
}

#[test]
fn filter_exclude_errors_accepts_all_non_error() {
    let f = EventFilter::exclude_errors();
    for ev in every_kind() {
        if matches!(ev.kind, AgentEventKind::Error { .. }) {
            assert!(!f.matches(&ev));
        } else {
            assert!(f.matches(&ev));
        }
    }
}

#[test]
fn filter_custom_text_length() {
    let f = EventFilter::new(
        |ev| matches!(&ev.kind, AgentEventKind::AssistantDelta { text } if text.len() >= 5),
    );
    assert!(f.matches(&delta("hello")));
    assert!(!f.matches(&delta("hi")));
}

#[test]
fn filter_custom_tool_name_prefix() {
    let f = EventFilter::new(
        |ev| matches!(&ev.kind, AgentEventKind::ToolCall { tool_name, .. } if tool_name.starts_with("file_")),
    );
    assert!(f.matches(&tc("file_read")));
    assert!(!f.matches(&tc("bash")));
}

#[test]
fn filter_custom_has_ext() {
    let f = EventFilter::new(|ev| ev.ext.is_some());
    let mut ext = BTreeMap::new();
    ext.insert("k".into(), serde_json::json!(1));
    assert!(f.matches(&mk_ext(
        AgentEventKind::AssistantDelta { text: "x".into() },
        ext
    )));
    assert!(!f.matches(&delta("x")));
}

#[test]
fn filter_clone_produces_independent_filter() {
    let f = EventFilter::by_kind("error");
    let f2 = f.clone();
    assert!(f.matches(&err("e")));
    assert!(f2.matches(&err("e")));
    assert!(!f2.matches(&delta("d")));
}

#[test]
fn filter_debug_does_not_panic() {
    let f = EventFilter::errors_only();
    let dbg = format!("{f:?}");
    assert!(dbg.contains("EventFilter"));
}

#[test]
fn filter_combined_and_logic() {
    let f1 = EventFilter::exclude_errors();
    let f2 = EventFilter::by_kind("tool_call");
    let events = every_kind();
    let passed: Vec<_> = events
        .iter()
        .filter(|e| f1.matches(e) && f2.matches(e))
        .collect();
    assert_eq!(passed.len(), 1);
}

#[test]
fn filter_combined_or_logic() {
    let f1 = EventFilter::by_kind("error");
    let f2 = EventFilter::by_kind("warning");
    let events = every_kind();
    let passed: Vec<_> = events
        .iter()
        .filter(|e| f1.matches(e) || f2.matches(e))
        .collect();
    assert_eq!(passed.len(), 2);
}

#[test]
fn filter_negation_via_custom() {
    let inner = EventFilter::by_kind("error");
    let negated = EventFilter::new(move |ev| !inner.matches(ev));
    assert!(!negated.matches(&err("e")));
    assert!(negated.matches(&delta("d")));
}

// =========================================================================
// 3. EventRecorder capturing events
// =========================================================================

#[test]
fn recorder_new_is_empty() {
    let r = EventRecorder::new();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
    assert!(r.events().is_empty());
}

#[test]
fn recorder_records_single_event() {
    let r = EventRecorder::new();
    r.record(&delta("a"));
    assert_eq!(r.len(), 1);
    assert!(!r.is_empty());
}

#[test]
fn recorder_preserves_order() {
    let r = EventRecorder::new();
    r.record(&delta("first"));
    r.record(&delta("second"));
    r.record(&delta("third"));
    let evs = r.events();
    assert!(matches!(&evs[0].kind, AgentEventKind::AssistantDelta { text } if text == "first"));
    assert!(matches!(&evs[1].kind, AgentEventKind::AssistantDelta { text } if text == "second"));
    assert!(matches!(&evs[2].kind, AgentEventKind::AssistantDelta { text } if text == "third"));
}

#[test]
fn recorder_records_all_event_types() {
    let r = EventRecorder::new();
    for ev in every_kind() {
        r.record(&ev);
    }
    assert_eq!(r.len(), 10);
}

#[test]
fn recorder_clear_then_reuse() {
    let r = EventRecorder::new();
    r.record(&delta("a"));
    r.record(&delta("b"));
    r.clear();
    assert!(r.is_empty());
    r.record(&delta("c"));
    assert_eq!(r.len(), 1);
}

#[test]
fn recorder_clone_shares_state() {
    let r = EventRecorder::new();
    let r2 = r.clone();
    r.record(&delta("a"));
    assert_eq!(r2.len(), 1);
    r2.record(&delta("b"));
    assert_eq!(r.len(), 2);
}

#[test]
fn recorder_events_returns_snapshot() {
    let r = EventRecorder::new();
    r.record(&delta("a"));
    let snap = r.events();
    r.record(&delta("b"));
    assert_eq!(snap.len(), 1);
    assert_eq!(r.len(), 2);
}

#[test]
fn recorder_preserves_ext_data() {
    let r = EventRecorder::new();
    let mut ext = BTreeMap::new();
    ext.insert("key".into(), serde_json::json!("value"));
    r.record(&mk_ext(
        AgentEventKind::AssistantDelta { text: "x".into() },
        ext,
    ));
    let evs = r.events();
    assert_eq!(
        evs[0].ext.as_ref().unwrap().get("key").unwrap(),
        &serde_json::json!("value")
    );
}

#[test]
fn recorder_default_is_empty() {
    let r = EventRecorder::default();
    assert!(r.is_empty());
}

// =========================================================================
// 4. EventStats computation
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
fn stats_counts_each_kind() {
    let s = EventStats::new();
    for ev in every_kind() {
        s.observe(&ev);
    }
    assert_eq!(s.total_events(), 10);
    assert_eq!(s.count_for("run_started"), 1);
    assert_eq!(s.count_for("run_completed"), 1);
    assert_eq!(s.count_for("assistant_delta"), 1);
    assert_eq!(s.count_for("assistant_message"), 1);
    assert_eq!(s.count_for("tool_call"), 1);
    assert_eq!(s.count_for("tool_result"), 1);
    assert_eq!(s.count_for("file_changed"), 1);
    assert_eq!(s.count_for("command_executed"), 1);
    assert_eq!(s.count_for("warning"), 1);
    assert_eq!(s.count_for("error"), 1);
}

#[test]
fn stats_delta_bytes_accumulates() {
    let s = EventStats::new();
    s.observe(&delta("hello")); // 5
    s.observe(&delta("world!!")); // 7
    s.observe(&delta("")); // 0
    assert_eq!(s.total_delta_bytes(), 12);
}

#[test]
fn stats_delta_bytes_ignores_non_delta() {
    let s = EventStats::new();
    s.observe(&msg("long message text"));
    s.observe(&err("error text"));
    assert_eq!(s.total_delta_bytes(), 0);
}

#[test]
fn stats_error_count_only_errors() {
    let s = EventStats::new();
    s.observe(&err("e1"));
    s.observe(&err("e2"));
    s.observe(&warn("w1"));
    s.observe(&delta("d"));
    assert_eq!(s.error_count(), 2);
}

#[test]
fn stats_count_for_unknown_kind_returns_zero() {
    let s = EventStats::new();
    s.observe(&delta("x"));
    assert_eq!(s.count_for("nonexistent_kind"), 0);
}

#[test]
fn stats_reset_clears_everything() {
    let s = EventStats::new();
    s.observe(&delta("abc"));
    s.observe(&err("e"));
    s.reset();
    assert_eq!(s.total_events(), 0);
    assert_eq!(s.total_delta_bytes(), 0);
    assert_eq!(s.error_count(), 0);
    assert!(s.kind_counts().is_empty());
}

#[test]
fn stats_reset_then_reuse() {
    let s = EventStats::new();
    s.observe(&delta("a"));
    s.reset();
    s.observe(&err("e"));
    assert_eq!(s.total_events(), 1);
    assert_eq!(s.error_count(), 1);
    assert_eq!(s.total_delta_bytes(), 0);
}

#[test]
fn stats_kind_counts_snapshot() {
    let s = EventStats::new();
    s.observe(&delta("x"));
    s.observe(&delta("y"));
    s.observe(&tc("read"));
    let counts = s.kind_counts();
    assert_eq!(counts.get("assistant_delta"), Some(&2));
    assert_eq!(counts.get("tool_call"), Some(&1));
    assert_eq!(counts.len(), 2);
}

#[test]
fn stats_clone_shares_inner() {
    let s = EventStats::new();
    let s2 = s.clone();
    s.observe(&delta("a"));
    assert_eq!(s2.total_events(), 1);
}

#[test]
fn stats_multiple_error_types() {
    let s = EventStats::new();
    s.observe(&err("timeout"));
    s.observe(&err("auth_failure"));
    s.observe(&err("rate_limited"));
    assert_eq!(s.error_count(), 3);
    assert_eq!(s.count_for("error"), 3);
}

// =========================================================================
// 5. Pipeline chaining (filter → record → stats)
// =========================================================================

#[test]
fn pipeline_filter_then_record() {
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::by_kind("assistant_delta"))
        .with_recorder(rec.clone())
        .build();

    p.process(delta("a"));
    p.process(err("e"));
    p.process(tc("t"));
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
    p.process(err("e"));
    p.process(warn("w"));

    assert_eq!(stats.total_events(), 2);
    assert_eq!(stats.error_count(), 0);
}

#[test]
fn pipeline_filter_transform_record_stats() {
    let rec = EventRecorder::new();
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("tagged".into(), serde_json::json!(true));
            ev
        }))
        .with_recorder(rec.clone())
        .with_stats(stats.clone())
        .build();

    for ev in every_kind() {
        p.process(ev);
    }

    // 9 non-error events pass
    assert_eq!(stats.total_events(), 9);
    assert_eq!(rec.len(), 9);
    for ev in rec.events() {
        assert_eq!(
            ev.ext.as_ref().unwrap().get("tagged").unwrap(),
            &serde_json::json!(true)
        );
    }
}

#[test]
fn pipeline_multiple_filters_all_must_pass() {
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .filter(EventFilter::by_kind("tool_call"))
        .build();

    assert!(p.process(tc("read")).is_some());
    assert!(p.process(err("e")).is_none());
    assert!(p.process(delta("d")).is_none());
}

#[test]
fn pipeline_multiple_transforms_applied_in_order() {
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("step".into(), serde_json::json!(1));
            ev
        }))
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("step".into(), serde_json::json!(2));
            ev
        }))
        .build();

    let result = p.process(delta("x")).unwrap();
    // Second transform overwrites
    assert_eq!(
        result.ext.as_ref().unwrap().get("step").unwrap(),
        &serde_json::json!(2)
    );
}

#[test]
fn pipeline_stats_see_transformed_events() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("processed".into(), serde_json::json!(true));
            ev
        }))
        .with_stats(stats.clone())
        .build();

    p.process(delta("x"));
    assert_eq!(stats.total_events(), 1);
    assert_eq!(stats.total_delta_bytes(), 1); // "x" = 1 byte
}

// =========================================================================
// 6. Event ordering preservation
// =========================================================================

#[tokio::test]
async fn stream_preserves_insertion_order() {
    let (tx, rx) = mpsc::channel(32);
    for i in 0..20 {
        tx.send(delta(&format!("ev_{i}"))).await.unwrap();
    }
    drop(tx);

    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    assert_eq!(events.len(), 20);
    for (i, ev) in events.iter().enumerate() {
        match &ev.kind {
            AgentEventKind::AssistantDelta { text } => {
                assert_eq!(text, &format!("ev_{i}"));
            }
            _ => panic!("unexpected kind"),
        }
    }
}

#[tokio::test]
async fn pipe_preserves_order() {
    let (tx_in, rx_in) = mpsc::channel(32);
    let (tx_out, mut rx_out) = mpsc::channel(32);

    let pipeline = StreamPipelineBuilder::new().build();

    for i in 0..10 {
        tx_in.send(delta(&format!("{i}"))).await.unwrap();
    }
    drop(tx_in);

    let stream = EventStream::new(rx_in);
    stream.pipe(&pipeline, tx_out).await;

    let mut results = Vec::new();
    while let Some(ev) = rx_out.recv().await {
        results.push(ev);
    }
    assert_eq!(results.len(), 10);
    for (i, ev) in results.iter().enumerate() {
        match &ev.kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, &format!("{i}")),
            _ => panic!("unexpected kind"),
        }
    }
}

#[tokio::test]
async fn recorder_via_pipeline_preserves_order() {
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(rec.clone())
        .build();

    let (tx, rx) = mpsc::channel(32);
    for i in 0..15 {
        tx.send(delta(&format!("ord_{i}"))).await.unwrap();
    }
    drop(tx);

    let stream = EventStream::new(rx);
    let (tx_out, _rx_out) = mpsc::channel(32);
    stream.pipe(&p, tx_out).await;

    let evs = rec.events();
    assert_eq!(evs.len(), 15);
    for (i, ev) in evs.iter().enumerate() {
        match &ev.kind {
            AgentEventKind::AssistantDelta { text } => assert_eq!(text, &format!("ord_{i}")),
            _ => panic!("unexpected kind"),
        }
    }
}

// =========================================================================
// 7. Sequence number validation (simulated via ext)
// =========================================================================

#[test]
fn sequence_numbers_preserved_through_pipeline() {
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(rec.clone())
        .build();

    for seq in 0..10u64 {
        let mut ext = BTreeMap::new();
        ext.insert("seq".into(), serde_json::json!(seq));
        let ev = mk_ext(AgentEventKind::AssistantDelta { text: "x".into() }, ext);
        p.process(ev);
    }

    let evs = rec.events();
    for (i, ev) in evs.iter().enumerate() {
        let seq = ev
            .ext
            .as_ref()
            .unwrap()
            .get("seq")
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(seq, i as u64);
    }
}

#[test]
fn sequence_gaps_detectable_after_filtering() {
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .with_recorder(rec.clone())
        .build();

    for seq in 0..10u64 {
        let mut ext = BTreeMap::new();
        ext.insert("seq".into(), serde_json::json!(seq));
        let kind = if seq % 3 == 0 {
            AgentEventKind::Error {
                message: "skip".into(),
                error_code: None,
            }
        } else {
            AgentEventKind::AssistantDelta { text: "x".into() }
        };
        p.process(mk_ext(kind, ext));
    }

    let evs = rec.events();
    // seq 0, 3, 6, 9 filtered out
    assert_eq!(evs.len(), 6);
    let seqs: Vec<u64> = evs
        .iter()
        .map(|ev| {
            ev.ext
                .as_ref()
                .unwrap()
                .get("seq")
                .unwrap()
                .as_u64()
                .unwrap()
        })
        .collect();
    assert_eq!(seqs, vec![1, 2, 4, 5, 7, 8]);
}

#[test]
fn transform_can_inject_sequence_numbers() {
    let counter = Arc::new(AtomicU64::new(0));
    let c = counter.clone();
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::new(move |mut ev| {
            let seq = c.fetch_add(1, Ordering::SeqCst);
            let ext = ev.ext.get_or_insert_with(BTreeMap::new);
            ext.insert("seq".into(), serde_json::json!(seq));
            ev
        }))
        .record()
        .build();

    for _ in 0..5 {
        p.process(delta("x"));
    }

    let evs = p.recorder().unwrap().events();
    for (i, ev) in evs.iter().enumerate() {
        let seq = ev
            .ext
            .as_ref()
            .unwrap()
            .get("seq")
            .unwrap()
            .as_u64()
            .unwrap();
        assert_eq!(seq, i as u64);
    }
}

// =========================================================================
// 8. Timestamp monotonicity checks
// =========================================================================

#[tokio::test]
async fn multiplexer_sorted_output_is_monotonic() {
    let base = Utc::now();
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    for i in (0..10).step_by(2) {
        tx1.send(delta_ts(&format!("a{i}"), base + Duration::milliseconds(i)))
            .await
            .unwrap();
    }
    for i in (1..10).step_by(2) {
        tx2.send(delta_ts(&format!("b{i}"), base + Duration::milliseconds(i)))
            .await
            .unwrap();
    }
    drop(tx1);
    drop(tx2);

    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let events = mux.collect_sorted().await;

    assert_eq!(events.len(), 10);
    for w in events.windows(2) {
        assert!(
            w[0].ts <= w[1].ts,
            "timestamps should be monotonically non-decreasing"
        );
    }
}

#[tokio::test]
async fn merge_output_is_monotonic() {
    let base = Utc::now();
    let (tx1, rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);

    tx1.send(delta_ts("a", base)).await.unwrap();
    tx1.send(delta_ts("c", base + Duration::milliseconds(20)))
        .await
        .unwrap();
    tx2.send(delta_ts("b", base + Duration::milliseconds(10)))
        .await
        .unwrap();
    tx2.send(delta_ts("d", base + Duration::milliseconds(30)))
        .await
        .unwrap();
    drop(tx1);
    drop(tx2);

    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let mut merged_rx = mux.merge(16);

    let mut prev_ts = None;
    while let Some(ev) = merged_rx.recv().await {
        if let Some(prev) = prev_ts {
            assert!(ev.ts >= prev, "merged output should be monotonic");
        }
        prev_ts = Some(ev.ts);
    }
}

#[test]
fn single_stream_timestamps_are_monotonic() {
    let base = Utc::now();
    let events: Vec<AgentEvent> = (0..100)
        .map(|i| delta_ts(&format!("ev{i}"), base + Duration::milliseconds(i)))
        .collect();

    for w in events.windows(2) {
        assert!(w[0].ts <= w[1].ts);
    }
}

#[test]
fn out_of_order_timestamps_detectable() {
    let base = Utc::now();
    let events = vec![
        delta_ts("a", base + Duration::milliseconds(10)),
        delta_ts("b", base),
        delta_ts("c", base + Duration::milliseconds(20)),
    ];

    let mut has_out_of_order = false;
    for w in events.windows(2) {
        if w[0].ts > w[1].ts {
            has_out_of_order = true;
        }
    }
    assert!(has_out_of_order, "should detect out-of-order timestamps");
}

// =========================================================================
// 9. Back-pressure handling
// =========================================================================

#[tokio::test]
async fn back_pressure_bounded_channel_blocks_sender() {
    let (tx, rx) = mpsc::channel(2);
    tx.send(delta("1")).await.unwrap();
    tx.send(delta("2")).await.unwrap();

    // Channel full; spawn sender for 3rd event
    let handle = tokio::spawn(async move {
        tx.send(delta("3")).await.unwrap();
    });

    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    handle.await.unwrap();
    assert_eq!(events.len(), 3);
}

#[tokio::test]
async fn back_pressure_pipe_through_narrow_channel() {
    let (tx_in, rx_in) = mpsc::channel(1);
    let (tx_out, mut rx_out) = mpsc::channel(1);
    let pipeline = StreamPipelineBuilder::new().build();

    let producer = tokio::spawn(async move {
        for i in 0..10 {
            tx_in.send(delta(&format!("{i}"))).await.unwrap();
        }
    });

    let consumer = tokio::spawn(async move {
        let mut collected = Vec::new();
        while let Some(ev) = rx_out.recv().await {
            collected.push(ev);
        }
        collected
    });

    let stream = EventStream::new(rx_in);
    stream.pipe(&pipeline, tx_out).await;
    producer.await.unwrap();
    let results = consumer.await.unwrap();
    assert_eq!(results.len(), 10);
}

#[tokio::test]
async fn closed_output_channel_stops_pipe() {
    let (tx_in, rx_in) = mpsc::channel(16);
    let (tx_out, rx_out) = mpsc::channel(16);

    for i in 0..5 {
        tx_in.send(delta(&format!("{i}"))).await.unwrap();
    }
    drop(tx_in);
    // Drop receiver before piping
    drop(rx_out);

    let pipeline = StreamPipelineBuilder::new().build();
    let stream = EventStream::new(rx_in);
    // Should not panic — pipe exits when tx_out fails
    stream.pipe(&pipeline, tx_out).await;
}

// =========================================================================
// 10. Empty stream handling
// =========================================================================

#[tokio::test]
async fn empty_stream_collect_all() {
    let (_tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(_tx);
    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn empty_stream_collect_filtered() {
    let (_tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(_tx);
    let stream = EventStream::new(rx);
    let events = stream.collect_filtered(&EventFilter::errors_only()).await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn empty_stream_pipe() {
    let (_tx, rx) = mpsc::channel::<AgentEvent>(16);
    drop(_tx);
    let (tx_out, mut rx_out) = mpsc::channel(16);
    let pipeline = StreamPipelineBuilder::new().record().build();
    let stream = EventStream::new(rx);
    stream.pipe(&pipeline, tx_out).await;
    assert!(rx_out.recv().await.is_none());
    assert!(pipeline.recorder().unwrap().is_empty());
}

#[tokio::test]
async fn empty_multiplexer() {
    let mux = EventMultiplexer::new(vec![]);
    let events = mux.collect_sorted().await;
    assert!(events.is_empty());
}

#[test]
fn empty_pipeline_process_returns_some() {
    let p = StreamPipelineBuilder::new().build();
    assert!(p.process(delta("x")).is_some());
}

#[test]
fn empty_stats_all_zeros() {
    let s = EventStats::new();
    assert_eq!(s.total_events(), 0);
    assert_eq!(s.total_delta_bytes(), 0);
    assert_eq!(s.error_count(), 0);
    assert!(s.kind_counts().is_empty());
}

// =========================================================================
// 11. Single event streams
// =========================================================================

#[tokio::test]
async fn single_event_collect_all() {
    let (tx, rx) = mpsc::channel(1);
    tx.send(delta("only")).await.unwrap();
    drop(tx);
    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn single_event_filtered_pass() {
    let (tx, rx) = mpsc::channel(1);
    tx.send(delta("x")).await.unwrap();
    drop(tx);
    let stream = EventStream::new(rx);
    let events = stream
        .collect_filtered(&EventFilter::by_kind("assistant_delta"))
        .await;
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn single_event_filtered_reject() {
    let (tx, rx) = mpsc::channel(1);
    tx.send(delta("x")).await.unwrap();
    drop(tx);
    let stream = EventStream::new(rx);
    let events = stream.collect_filtered(&EventFilter::errors_only()).await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn single_event_pipe() {
    let (tx_in, rx_in) = mpsc::channel(1);
    let (tx_out, mut rx_out) = mpsc::channel(1);
    tx_in.send(err("e")).await.unwrap();
    drop(tx_in);

    let pipeline = StreamPipelineBuilder::new().build();
    let stream = EventStream::new(rx_in);
    stream.pipe(&pipeline, tx_out).await;

    let ev = rx_out.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::Error { .. }));
    assert!(rx_out.recv().await.is_none());
}

#[test]
fn single_event_pipeline_records_and_stats() {
    let rec = EventRecorder::new();
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(rec.clone())
        .with_stats(stats.clone())
        .build();

    p.process(delta("only"));
    assert_eq!(rec.len(), 1);
    assert_eq!(stats.total_events(), 1);
}

// =========================================================================
// 12. Large stream processing (1000+ events)
// =========================================================================

#[tokio::test]
async fn large_stream_1000_events_collect() {
    let (tx, rx) = mpsc::channel(128);
    let n = 1000;

    let producer = tokio::spawn(async move {
        for i in 0..n {
            tx.send(delta(&format!("ev_{i}"))).await.unwrap();
        }
    });

    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    producer.await.unwrap();
    assert_eq!(events.len(), n);
}

#[tokio::test]
async fn large_stream_with_pipeline() {
    let n = 2000usize;
    let rec = EventRecorder::new();
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .with_recorder(rec.clone())
        .with_stats(stats.clone())
        .build();

    let (tx_in, rx_in) = mpsc::channel(128);
    let (tx_out, mut rx_out) = mpsc::channel(128);

    let producer = tokio::spawn(async move {
        for i in 0..n {
            let ev = if i % 10 == 0 {
                err("e")
            } else {
                delta(&format!("{i}"))
            };
            tx_in.send(ev).await.unwrap();
        }
    });

    let consumer = tokio::spawn(async move {
        let mut count = 0usize;
        while (rx_out.recv().await).is_some() {
            count += 1;
        }
        count
    });

    let stream = EventStream::new(rx_in);
    stream.pipe(&p, tx_out).await;
    producer.await.unwrap();
    let received = consumer.await.unwrap();

    let error_count = n / 10;
    let expected = n - error_count;
    assert_eq!(rec.len(), expected);
    assert_eq!(stats.total_events() as usize, expected);
    assert_eq!(received, expected);
}

#[test]
fn large_pipeline_processing_sync() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();

    for i in 0..5000 {
        let ev = match i % 5 {
            0 => delta("d"),
            1 => err("e"),
            2 => tc("t"),
            3 => warn("w"),
            _ => fc("f"),
        };
        p.process(ev);
    }
    assert_eq!(stats.total_events(), 5000);
    assert_eq!(stats.count_for("assistant_delta"), 1000);
    assert_eq!(stats.error_count(), 1000);
}

#[tokio::test]
async fn large_multiplexer_1000_events_sorted() {
    let base = Utc::now();
    let (tx1, rx1) = mpsc::channel(1024);
    let (tx2, rx2) = mpsc::channel(1024);

    // Send all events before starting the multiplexer to avoid deadlock
    for i in (0..1000).step_by(2) {
        tx1.send(delta_ts(&format!("a{i}"), base + Duration::milliseconds(i)))
            .await
            .unwrap();
    }
    drop(tx1);

    for i in (1..1000).step_by(2) {
        tx2.send(delta_ts(&format!("b{i}"), base + Duration::milliseconds(i)))
            .await
            .unwrap();
    }
    drop(tx2);

    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), 1000);

    for w in events.windows(2) {
        assert!(w[0].ts <= w[1].ts);
    }
}

// =========================================================================
// 13. Mixed event type streams
// =========================================================================

#[tokio::test]
async fn mixed_types_all_collected() {
    let (tx, rx) = mpsc::channel(32);
    for ev in every_kind() {
        tx.send(ev).await.unwrap();
    }
    drop(tx);

    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    assert_eq!(events.len(), 10);
}

#[tokio::test]
async fn mixed_types_filter_only_deltas() {
    let (tx, rx) = mpsc::channel(32);
    for _ in 0..3 {
        for ev in every_kind() {
            tx.send(ev).await.unwrap();
        }
    }
    drop(tx);

    let stream = EventStream::new(rx);
    let events = stream
        .collect_filtered(&EventFilter::by_kind("assistant_delta"))
        .await;
    assert_eq!(events.len(), 3);
}

#[test]
fn mixed_types_stats_distribution() {
    let stats = EventStats::new();
    let events = vec![
        delta("a"),
        delta("b"),
        delta("c"),
        err("e1"),
        err("e2"),
        tc("read"),
        tr("read"),
        warn("w"),
        fc("f"),
        cmd("ls", Some(0)),
        started(),
        completed(),
    ];
    for ev in &events {
        stats.observe(ev);
    }
    assert_eq!(stats.total_events(), 12);
    assert_eq!(stats.count_for("assistant_delta"), 3);
    assert_eq!(stats.error_count(), 2);
    assert_eq!(stats.count_for("tool_call"), 1);
    assert_eq!(stats.count_for("tool_result"), 1);
    assert_eq!(stats.count_for("warning"), 1);
    assert_eq!(stats.count_for("file_changed"), 1);
    assert_eq!(stats.count_for("command_executed"), 1);
    assert_eq!(stats.count_for("run_started"), 1);
    assert_eq!(stats.count_for("run_completed"), 1);
}

#[test]
fn mixed_types_pipeline_filters_subset() {
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::new(|ev| {
            matches!(
                ev.kind,
                AgentEventKind::ToolCall { .. } | AgentEventKind::ToolResult { .. }
            )
        }))
        .with_recorder(rec.clone())
        .build();

    for ev in every_kind() {
        p.process(ev);
    }
    assert_eq!(rec.len(), 2); // tool_call + tool_result
}

#[test]
fn mixed_types_lifecycle_events_only() {
    let f = EventFilter::new(|ev| {
        matches!(
            ev.kind,
            AgentEventKind::RunStarted { .. } | AgentEventKind::RunCompleted { .. }
        )
    });
    let events = every_kind();
    let passed: Vec<_> = events.iter().filter(|e| f.matches(e)).collect();
    assert_eq!(passed.len(), 2);
}

// =========================================================================
// 14. Error event filtering and counting
// =========================================================================

#[test]
fn error_filter_only_errors() {
    let f = EventFilter::errors_only();
    assert!(f.matches(&err("e")));
    assert!(!f.matches(&delta("d")));
    assert!(!f.matches(&warn("w")));
    assert!(!f.matches(&tc("t")));
}

#[test]
fn error_stats_accumulated_correctly() {
    let stats = EventStats::new();
    for i in 0..50 {
        if i % 5 == 0 {
            stats.observe(&err(&format!("e{i}")));
        } else {
            stats.observe(&delta(&format!("d{i}")));
        }
    }
    assert_eq!(stats.error_count(), 10);
    assert_eq!(stats.count_for("error"), 10);
    assert_eq!(stats.count_for("assistant_delta"), 40);
    assert_eq!(stats.total_events(), 50);
}

#[test]
fn error_pipeline_count_before_filtering() {
    let stats_all = EventStats::new();
    let stats_filtered = EventStats::new();

    // Pipeline without filter — sees everything
    let p_all = StreamPipelineBuilder::new()
        .with_stats(stats_all.clone())
        .build();
    // Pipeline with error filter — sees only errors
    let p_err = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .with_stats(stats_filtered.clone())
        .build();

    let events = vec![delta("d"), err("e1"), delta("d2"), err("e2"), warn("w")];
    for ev in &events {
        p_all.process(ev.clone());
        p_err.process(ev.clone());
    }

    assert_eq!(stats_all.total_events(), 5);
    assert_eq!(stats_all.error_count(), 2);
    assert_eq!(stats_filtered.total_events(), 2);
    assert_eq!(stats_filtered.error_count(), 2);
}

#[test]
fn error_events_with_error_code_still_match() {
    let ev = mk(AgentEventKind::Error {
        message: "with code".into(),
        error_code: Some(abp_error::ErrorCode::BackendCrashed),
    });
    let f = EventFilter::errors_only();
    assert!(f.matches(&ev));
}

#[tokio::test]
async fn error_events_collected_via_stream_filter() {
    let (tx, rx) = mpsc::channel(32);
    tx.send(delta("a")).await.unwrap();
    tx.send(err("e1")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    tx.send(err("e2")).await.unwrap();
    tx.send(err("e3")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let errors = stream.collect_filtered(&EventFilter::errors_only()).await;
    assert_eq!(errors.len(), 3);
}

#[test]
fn exclude_errors_in_pipeline_with_recorder() {
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .with_recorder(rec.clone())
        .build();

    p.process(err("e1"));
    p.process(delta("d1"));
    p.process(err("e2"));
    p.process(warn("w1"));
    p.process(err("e3"));

    assert_eq!(rec.len(), 2); // delta + warning
    for ev in rec.events() {
        assert!(!matches!(ev.kind, AgentEventKind::Error { .. }));
    }
}

// =========================================================================
// 15. Concurrent stream processing
// =========================================================================

#[tokio::test]
async fn concurrent_producers_single_consumer() {
    let (tx, rx) = mpsc::channel(128);

    let mut handles = Vec::new();
    for t in 0..5 {
        let tx = tx.clone();
        handles.push(tokio::spawn(async move {
            for i in 0..20 {
                tx.send(delta(&format!("t{t}_e{i}"))).await.unwrap();
            }
        }));
    }
    drop(tx);

    for h in handles {
        h.await.unwrap();
    }

    let stream = EventStream::new(rx);
    let events = stream.collect_all().await;
    assert_eq!(events.len(), 100);
}

#[tokio::test]
async fn concurrent_pipeline_with_shared_stats() {
    let stats = EventStats::new();
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .with_recorder(rec.clone())
        .build();

    let mut handles = Vec::new();
    for _ in 0..10 {
        let p = p.clone();
        handles.push(tokio::spawn(async move {
            for _ in 0..100 {
                p.process(delta("x"));
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    assert_eq!(stats.total_events(), 1000);
    assert_eq!(rec.len(), 1000);
}

#[tokio::test]
async fn concurrent_multiplexer_three_streams() {
    let base = Utc::now();
    let mut rxs = Vec::new();

    for stream_idx in 0..3 {
        let (tx, rx) = mpsc::channel(64);
        rxs.push(rx);
        tokio::spawn(async move {
            for i in 0..30 {
                let offset = (stream_idx * 30 + i) as i64;
                tx.send(delta_ts(
                    &format!("s{stream_idx}_{i}"),
                    base + Duration::milliseconds(offset),
                ))
                .await
                .unwrap();
            }
        });
    }

    // Allow producers to complete
    tokio::task::yield_now().await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mux = EventMultiplexer::new(rxs);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), 90);

    for w in events.windows(2) {
        assert!(w[0].ts <= w[1].ts);
    }
}

#[tokio::test]
async fn concurrent_pipe_with_filter() {
    let (tx_in, rx_in) = mpsc::channel(128);
    let (tx_out, mut rx_out) = mpsc::channel(128);
    let rec = EventRecorder::new();
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .with_recorder(rec.clone())
        .build();

    let producer = tokio::spawn(async move {
        for i in 0..200 {
            let ev = if i % 4 == 0 { err("e") } else { delta("d") };
            tx_in.send(ev).await.unwrap();
        }
    });

    let consumer = tokio::spawn(async move {
        let mut count = 0u64;
        while (rx_out.recv().await).is_some() {
            count += 1;
        }
        count
    });

    let stream = EventStream::new(rx_in);
    stream.pipe(&pipeline, tx_out).await;
    producer.await.unwrap();
    let count = consumer.await.unwrap();

    assert_eq!(count, 150);
    assert_eq!(rec.len(), 150);
}

// =========================================================================
// 16. EventTransform additional tests
// =========================================================================

#[test]
fn transform_identity_all_kinds() {
    let t = EventTransform::identity();
    for ev in every_kind() {
        let ts_before = ev.ts;
        let result = t.apply(ev);
        assert_eq!(result.ts, ts_before);
    }
}

#[test]
fn transform_add_timestamp_tag() {
    let t = EventTransform::new(|mut ev| {
        let ext = ev.ext.get_or_insert_with(BTreeMap::new);
        ext.insert("tagged_at".into(), serde_json::json!(ev.ts.to_rfc3339()));
        ev
    });
    let result = t.apply(delta("x"));
    assert!(result.ext.as_ref().unwrap().contains_key("tagged_at"));
}

#[test]
fn transform_debug_output() {
    let t = EventTransform::identity();
    let dbg = format!("{t:?}");
    assert!(dbg.contains("EventTransform"));
}

#[test]
fn transform_clone_independence() {
    let t = EventTransform::identity();
    let t2 = t.clone();
    let ev1 = t.apply(delta("a"));
    let ev2 = t2.apply(delta("b"));
    assert!(matches!(&ev1.kind, AgentEventKind::AssistantDelta { text } if text == "a"));
    assert!(matches!(&ev2.kind, AgentEventKind::AssistantDelta { text } if text == "b"));
}

// =========================================================================
// 17. event_kind_name exhaustive
// =========================================================================

#[test]
fn event_kind_name_all_10_variants() {
    let pairs: Vec<(AgentEventKind, &str)> = vec![
        (
            AgentEventKind::RunStarted {
                message: String::new(),
            },
            "run_started",
        ),
        (
            AgentEventKind::RunCompleted {
                message: String::new(),
            },
            "run_completed",
        ),
        (
            AgentEventKind::AssistantDelta {
                text: String::new(),
            },
            "assistant_delta",
        ),
        (
            AgentEventKind::AssistantMessage {
                text: String::new(),
            },
            "assistant_message",
        ),
        (
            AgentEventKind::ToolCall {
                tool_name: String::new(),
                tool_use_id: None,
                parent_tool_use_id: None,
                input: serde_json::json!(null),
            },
            "tool_call",
        ),
        (
            AgentEventKind::ToolResult {
                tool_name: String::new(),
                tool_use_id: None,
                output: serde_json::json!(null),
                is_error: false,
            },
            "tool_result",
        ),
        (
            AgentEventKind::FileChanged {
                path: String::new(),
                summary: String::new(),
            },
            "file_changed",
        ),
        (
            AgentEventKind::CommandExecuted {
                command: String::new(),
                exit_code: None,
                output_preview: None,
            },
            "command_executed",
        ),
        (
            AgentEventKind::Warning {
                message: String::new(),
            },
            "warning",
        ),
        (
            AgentEventKind::Error {
                message: String::new(),
                error_code: None,
            },
            "error",
        ),
    ];
    for (kind, expected) in pairs {
        assert_eq!(event_kind_name(&kind), expected);
    }
}

// =========================================================================
// 18. EventStream — recv and into_inner
// =========================================================================

#[tokio::test]
async fn stream_recv_one_at_a_time() {
    let (tx, rx) = mpsc::channel(16);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);

    let mut stream = EventStream::new(rx);
    let ev1 = stream.recv().await.unwrap();
    assert!(matches!(&ev1.kind, AgentEventKind::AssistantDelta { text } if text == "a"));
    let ev2 = stream.recv().await.unwrap();
    assert!(matches!(&ev2.kind, AgentEventKind::AssistantDelta { text } if text == "b"));
    assert!(stream.recv().await.is_none());
}

#[tokio::test]
async fn stream_into_inner_returns_receiver() {
    let (tx, rx) = mpsc::channel(4);
    tx.send(delta("x")).await.unwrap();
    drop(tx);

    let stream = EventStream::new(rx);
    let mut inner = stream.into_inner();
    let ev = inner.recv().await.unwrap();
    assert!(matches!(ev.kind, AgentEventKind::AssistantDelta { .. }));
}

// =========================================================================
// 19. EventMultiplexer edge cases
// =========================================================================

#[tokio::test]
async fn multiplexer_single_stream() {
    let (tx, rx) = mpsc::channel(8);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);

    let mux = EventMultiplexer::new(vec![rx]);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn multiplexer_one_empty_one_full() {
    let (_tx1, rx1) = mpsc::channel::<AgentEvent>(8);
    drop(_tx1);

    let (tx2, rx2) = mpsc::channel(8);
    tx2.send(delta("a")).await.unwrap();
    tx2.send(delta("b")).await.unwrap();
    drop(tx2);

    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn multiplexer_merge_returns_all_events() {
    let base = Utc::now();
    let (tx1, rx1) = mpsc::channel(8);
    let (tx2, rx2) = mpsc::channel(8);

    tx1.send(delta_ts("1", base)).await.unwrap();
    tx2.send(delta_ts("2", base + Duration::milliseconds(1)))
        .await
        .unwrap();
    tx1.send(delta_ts("3", base + Duration::milliseconds(2)))
        .await
        .unwrap();
    drop(tx1);
    drop(tx2);

    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let mut rx = mux.merge(8);

    let mut count = 0;
    while (rx.recv().await).is_some() {
        count += 1;
    }
    assert_eq!(count, 3);
}

// =========================================================================
// 20. Pipeline with transform that changes event kind semantics
// =========================================================================

#[test]
fn transform_replaces_delta_text() {
    let t = EventTransform::new(|mut ev| {
        if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
            *text = text.to_uppercase();
        }
        ev
    });
    let result = t.apply(delta("hello"));
    match &result.kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "HELLO"),
        _ => panic!("expected AssistantDelta"),
    }
}

#[test]
fn pipeline_transform_then_stats_sees_modified() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::new(|mut ev| {
            if let AgentEventKind::AssistantDelta { ref mut text } = ev.kind {
                *text = "expanded content here".into();
            }
            ev
        }))
        .with_stats(stats.clone())
        .build();

    p.process(delta("x"));
    // Stats should see the expanded text length
    assert_eq!(stats.total_delta_bytes(), 21); // "expanded content here" = 21
}

// =========================================================================
// 21. Pipeline filters reject-all and accept-all
// =========================================================================

#[test]
fn pipeline_reject_all_filter() {
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::new(|_| false))
        .with_recorder(rec.clone())
        .build();

    for ev in every_kind() {
        assert!(p.process(ev).is_none());
    }
    assert!(rec.is_empty());
}

#[test]
fn pipeline_accept_all_filter() {
    let rec = EventRecorder::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::new(|_| true))
        .with_recorder(rec.clone())
        .build();

    for ev in every_kind() {
        assert!(p.process(ev).is_some());
    }
    assert_eq!(rec.len(), 10);
}

// =========================================================================
// 22. Recorder + Stats interaction
// =========================================================================

#[test]
fn recorder_and_stats_count_match() {
    let rec = EventRecorder::new();
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(rec.clone())
        .with_stats(stats.clone())
        .build();

    for _ in 0..100 {
        p.process(delta("x"));
    }
    assert_eq!(rec.len() as u64, stats.total_events());
}

#[test]
fn recorder_and_stats_after_filter_match() {
    let rec = EventRecorder::new();
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .with_recorder(rec.clone())
        .with_stats(stats.clone())
        .build();

    for i in 0..50 {
        let ev = if i % 3 == 0 { err("e") } else { delta("d") };
        p.process(ev);
    }
    assert_eq!(rec.len() as u64, stats.total_events());
    assert_eq!(stats.error_count(), 0); // errors filtered before stats
}

// =========================================================================
// 23. Timestamp-based filtering
// =========================================================================

#[test]
fn filter_by_timestamp_range() {
    let base = Utc::now();
    let cutoff = base + Duration::milliseconds(50);
    let f = EventFilter::new(move |ev| ev.ts >= cutoff);

    let early = delta_ts("early", base + Duration::milliseconds(10));
    let late = delta_ts("late", base + Duration::milliseconds(100));
    assert!(!f.matches(&early));
    assert!(f.matches(&late));
}

// =========================================================================
// 24. ext preservation through pipeline
// =========================================================================

#[test]
fn ext_preserved_through_identity_pipeline() {
    let mut ext = BTreeMap::new();
    ext.insert("key".into(), serde_json::json!(42));
    let ev = mk_ext(AgentEventKind::AssistantDelta { text: "x".into() }, ext);

    let p = StreamPipelineBuilder::new()
        .transform(EventTransform::identity())
        .build();
    let result = p.process(ev).unwrap();
    assert_eq!(
        result.ext.unwrap().get("key").unwrap(),
        &serde_json::json!(42)
    );
}

#[test]
fn ext_none_preserved_when_no_transform() {
    let p = StreamPipelineBuilder::new().build();
    let result = p.process(delta("x")).unwrap();
    assert!(result.ext.is_none());
}

// =========================================================================
// 25. Stats throughput simulation
// =========================================================================

#[test]
fn stats_throughput_large_deltas() {
    let stats = EventStats::new();
    let text = "a".repeat(1024);
    for _ in 0..100 {
        stats.observe(&delta(&text));
    }
    assert_eq!(stats.total_delta_bytes(), 100 * 1024);
    assert_eq!(stats.total_events(), 100);
}

#[test]
fn stats_empty_delta_zero_bytes() {
    let stats = EventStats::new();
    stats.observe(&delta(""));
    assert_eq!(stats.total_delta_bytes(), 0);
    assert_eq!(stats.total_events(), 1);
}

// =========================================================================
// 26. Pipeline idempotent operations
// =========================================================================

#[test]
fn pipeline_process_same_event_twice() {
    let rec = EventRecorder::new();
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_recorder(rec.clone())
        .with_stats(stats.clone())
        .build();

    let ev = delta("same");
    p.process(ev.clone());
    p.process(ev);
    assert_eq!(rec.len(), 2);
    assert_eq!(stats.total_events(), 2);
}

// =========================================================================
// 27. ToolResult with is_error flag
// =========================================================================

#[test]
fn tool_result_error_flag_not_counted_as_error_event() {
    let stats = EventStats::new();
    let ev = mk(AgentEventKind::ToolResult {
        tool_name: "bash".into(),
        tool_use_id: None,
        output: serde_json::json!("failed"),
        is_error: true,
    });
    stats.observe(&ev);
    // ToolResult with is_error=true is NOT an AgentEventKind::Error
    assert_eq!(stats.error_count(), 0);
    assert_eq!(stats.count_for("tool_result"), 1);
}

// =========================================================================
// 28. CommandExecuted variants
// =========================================================================

#[test]
fn command_executed_with_output_preview() {
    let ev = mk(AgentEventKind::CommandExecuted {
        command: "echo hello".into(),
        exit_code: Some(0),
        output_preview: Some("hello\n".into()),
    });
    let stats = EventStats::new();
    stats.observe(&ev);
    assert_eq!(stats.count_for("command_executed"), 1);
}

#[test]
fn command_executed_no_exit_code() {
    let ev = mk(AgentEventKind::CommandExecuted {
        command: "long-running".into(),
        exit_code: None,
        output_preview: None,
    });
    assert_eq!(event_kind_name(&ev.kind), "command_executed");
}

// =========================================================================
// 29. Pipeline with multiple concurrent clone usage
// =========================================================================

#[tokio::test]
async fn pipeline_cloned_across_tasks() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();

    let mut handles = Vec::new();
    for _ in 0..5 {
        let p = p.clone();
        handles.push(tokio::spawn(async move {
            for _ in 0..50 {
                p.process(delta("x"));
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
    assert_eq!(stats.total_events(), 250);
}

// =========================================================================
// 30. Filter with complex predicate combining ext and kind
// =========================================================================

#[test]
fn filter_complex_ext_and_kind() {
    let f = EventFilter::new(|ev| {
        matches!(ev.kind, AgentEventKind::AssistantDelta { .. })
            && ev
                .ext
                .as_ref()
                .and_then(|e| e.get("priority"))
                .and_then(|v| v.as_str())
                .is_some_and(|s| s == "high")
    });

    let mut ext = BTreeMap::new();
    ext.insert("priority".into(), serde_json::json!("high"));
    assert!(f.matches(&mk_ext(
        AgentEventKind::AssistantDelta { text: "x".into() },
        ext
    )));

    let mut ext_low = BTreeMap::new();
    ext_low.insert("priority".into(), serde_json::json!("low"));
    assert!(!f.matches(&mk_ext(
        AgentEventKind::AssistantDelta { text: "x".into() },
        ext_low
    )));

    assert!(!f.matches(&delta("no ext")));
}

// =========================================================================
// 31. Warning events through the pipeline
// =========================================================================

#[test]
fn warning_events_not_counted_as_errors() {
    let stats = EventStats::new();
    stats.observe(&warn("w1"));
    stats.observe(&warn("w2"));
    assert_eq!(stats.error_count(), 0);
    assert_eq!(stats.count_for("warning"), 2);
}

#[test]
fn filter_warnings_via_by_kind() {
    let f = EventFilter::by_kind("warning");
    assert!(f.matches(&warn("w")));
    assert!(!f.matches(&err("e")));
}

// =========================================================================
// 32. Stress: rapid recorder clear/record cycles
// =========================================================================

#[test]
fn recorder_rapid_clear_record_cycles() {
    let r = EventRecorder::new();
    for cycle in 0..100 {
        for _ in 0..10 {
            r.record(&delta(&format!("c{cycle}")));
        }
        assert_eq!(r.len(), 10);
        r.clear();
        assert!(r.is_empty());
    }
}

// =========================================================================
// 33. Stats reset mid-stream
// =========================================================================

#[test]
fn stats_reset_mid_processing() {
    let stats = EventStats::new();
    let p = StreamPipelineBuilder::new()
        .with_stats(stats.clone())
        .build();

    for _ in 0..50 {
        p.process(delta("x"));
    }
    assert_eq!(stats.total_events(), 50);

    stats.reset();
    assert_eq!(stats.total_events(), 0);

    for _ in 0..30 {
        p.process(err("e"));
    }
    assert_eq!(stats.total_events(), 30);
    assert_eq!(stats.error_count(), 30);
}

// =========================================================================
// 34. Pipeline process returns correct event
// =========================================================================

#[test]
fn pipeline_process_returns_event_with_correct_kind() {
    let p = StreamPipelineBuilder::new().build();

    let result = p.process(started()).unwrap();
    assert!(matches!(result.kind, AgentEventKind::RunStarted { .. }));

    let result = p.process(completed()).unwrap();
    assert!(matches!(result.kind, AgentEventKind::RunCompleted { .. }));

    let result = p.process(delta("x")).unwrap();
    assert!(matches!(result.kind, AgentEventKind::AssistantDelta { .. }));
}

// =========================================================================
// 35. FileChanged and ToolCall with tool_use_id
// =========================================================================

#[test]
fn tool_call_with_tool_use_id() {
    let ev = mk(AgentEventKind::ToolCall {
        tool_name: "read".into(),
        tool_use_id: Some("tuid_123".into()),
        parent_tool_use_id: Some("parent_456".into()),
        input: serde_json::json!({"path": "src/main.rs"}),
    });
    assert_eq!(event_kind_name(&ev.kind), "tool_call");
    let f = EventFilter::by_kind("tool_call");
    assert!(f.matches(&ev));
}

#[test]
fn file_changed_filter_matches() {
    let f = EventFilter::by_kind("file_changed");
    assert!(f.matches(&fc("src/lib.rs")));
    assert!(!f.matches(&delta("d")));
}

// =========================================================================
// 36. Pipe with filter and verify output channel content
// =========================================================================

#[tokio::test]
async fn pipe_filter_verify_output_content() {
    let (tx_in, rx_in) = mpsc::channel(16);
    let (tx_out, mut rx_out) = mpsc::channel(16);

    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::errors_only())
        .build();

    tx_in.send(delta("d1")).await.unwrap();
    tx_in.send(err("e1")).await.unwrap();
    tx_in.send(delta("d2")).await.unwrap();
    tx_in.send(err("e2")).await.unwrap();
    drop(tx_in);

    let stream = EventStream::new(rx_in);
    stream.pipe(&pipeline, tx_out).await;

    let mut errors = Vec::new();
    while let Some(ev) = rx_out.recv().await {
        assert!(matches!(ev.kind, AgentEventKind::Error { .. }));
        errors.push(ev);
    }
    assert_eq!(errors.len(), 2);
}

// =========================================================================
// 37. Multiplexer with different event types per stream
// =========================================================================

#[tokio::test]
async fn multiplexer_mixed_event_types() {
    let base = Utc::now();
    let (tx1, rx1) = mpsc::channel(8);
    let (tx2, rx2) = mpsc::channel(8);

    // Stream 1: deltas
    tx1.send(delta_ts("d1", base)).await.unwrap();
    tx1.send(delta_ts("d2", base + Duration::milliseconds(20)))
        .await
        .unwrap();
    drop(tx1);

    // Stream 2: errors
    tx2.send(err_ts("e1", base + Duration::milliseconds(10)))
        .await
        .unwrap();
    tx2.send(err_ts("e2", base + Duration::milliseconds(30)))
        .await
        .unwrap();
    drop(tx2);

    let mux = EventMultiplexer::new(vec![rx1, rx2]);
    let events = mux.collect_sorted().await;
    assert_eq!(events.len(), 4);
    assert!(matches!(
        events[0].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
    assert!(matches!(events[1].kind, AgentEventKind::Error { .. }));
    assert!(matches!(
        events[2].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
    assert!(matches!(events[3].kind, AgentEventKind::Error { .. }));
}

// =========================================================================
// 38. AssistantMessage (non-delta) handling
// =========================================================================

#[test]
fn assistant_message_not_counted_as_delta_bytes() {
    let stats = EventStats::new();
    stats.observe(&msg("this is a full message"));
    assert_eq!(stats.total_delta_bytes(), 0);
    assert_eq!(stats.count_for("assistant_message"), 1);
}

#[test]
fn filter_assistant_message_vs_delta() {
    let f_msg = EventFilter::by_kind("assistant_message");
    let f_delta = EventFilter::by_kind("assistant_delta");
    assert!(f_msg.matches(&msg("m")));
    assert!(!f_msg.matches(&delta("d")));
    assert!(f_delta.matches(&delta("d")));
    assert!(!f_delta.matches(&msg("m")));
}
