// SPDX-License-Identifier: MIT OR Apache-2.0
//! Integration tests for `EventStream` combinator utilities.

use std::collections::BTreeMap;
use std::time::Duration;

use abp_core::filter::EventFilter;
use abp_core::stream::EventStream;
use abp_core::{AgentEvent, AgentEventKind};
use chrono::{TimeZone, Utc};

// ── helpers ──────────────────────────────────────────────────────────

fn ts(secs: i64) -> chrono::DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
}

fn run_started(secs: i64) -> AgentEvent {
    AgentEvent {
        ts: ts(secs),
        kind: AgentEventKind::RunStarted {
            message: "go".into(),
        },
        ext: None,
    }
}

fn run_completed(secs: i64) -> AgentEvent {
    AgentEvent {
        ts: ts(secs),
        kind: AgentEventKind::RunCompleted {
            message: "done".into(),
        },
        ext: None,
    }
}

fn tool_call(secs: i64, name: &str) -> AgentEvent {
    AgentEvent {
        ts: ts(secs),
        kind: AgentEventKind::ToolCall {
            tool_name: name.into(),
            tool_use_id: None,
            parent_tool_use_id: None,
            input: serde_json::json!({}),
        },
        ext: None,
    }
}

fn assistant_msg(secs: i64, text: &str) -> AgentEvent {
    AgentEvent {
        ts: ts(secs),
        kind: AgentEventKind::AssistantMessage { text: text.into() },
        ext: None,
    }
}

fn warning(secs: i64, msg: &str) -> AgentEvent {
    AgentEvent {
        ts: ts(secs),
        kind: AgentEventKind::Warning {
            message: msg.into(),
        },
        ext: None,
    }
}

fn error_event(secs: i64, msg: &str) -> AgentEvent {
    AgentEvent {
        ts: ts(secs),
        kind: AgentEventKind::Error {
            message: msg.into(),
            error_code: None,
        },
        ext: None,
    }
}

// ── filter_pred tests ────────────────────────────────────────────────

#[test]
fn filter_pred_only_tool_calls() {
    let stream = EventStream::new(vec![
        run_started(0),
        tool_call(1, "read_file"),
        assistant_msg(2, "hi"),
        tool_call(3, "write_file"),
        run_completed(4),
    ]);
    let filtered = stream.filter_pred(|e| matches!(e.kind, AgentEventKind::ToolCall { .. }));
    assert_eq!(filtered.len(), 2);
    for e in filtered.iter() {
        assert!(matches!(e.kind, AgentEventKind::ToolCall { .. }));
    }
}

#[test]
fn filter_pred_empty_stream() {
    let stream = EventStream::new(vec![]);
    let filtered = stream.filter_pred(|_| true);
    assert!(filtered.is_empty());
}

#[test]
fn filter_pred_none_match() {
    let stream = EventStream::new(vec![run_started(0), run_completed(1)]);
    let filtered = stream.filter_pred(|e| matches!(e.kind, AgentEventKind::Error { .. }));
    assert!(filtered.is_empty());
}

#[test]
fn filter_pred_all_match() {
    let events = vec![run_started(0), run_completed(1)];
    let stream = EventStream::new(events.clone());
    let filtered = stream.filter_pred(|_| true);
    assert_eq!(filtered.len(), 2);
}

#[test]
fn filter_pred_single_event() {
    let stream = EventStream::new(vec![warning(0, "careful")]);
    let filtered = stream.filter_pred(|e| matches!(e.kind, AgentEventKind::Warning { .. }));
    assert_eq!(filtered.len(), 1);
}

// ── existing EventFilter integration ─────────────────────────────────

#[test]
fn filter_by_event_filter_include() {
    let stream = EventStream::new(vec![
        run_started(0),
        warning(1, "w"),
        error_event(2, "e"),
        run_completed(3),
    ]);
    let f = EventFilter::include_kinds(&["warning", "error"]);
    let filtered = stream.filter(&f);
    assert_eq!(filtered.len(), 2);
}

#[test]
fn filter_by_event_filter_exclude() {
    let stream = EventStream::new(vec![run_started(0), warning(1, "w"), run_completed(2)]);
    let f = EventFilter::exclude_kinds(&["warning"]);
    let filtered = stream.filter(&f);
    assert_eq!(filtered.len(), 2);
}

// ── map_events tests ─────────────────────────────────────────────────

#[test]
fn map_events_add_metadata() {
    let stream = EventStream::new(vec![run_started(0), run_completed(1)]);
    let mapped = stream.map_events(|mut e| {
        let mut ext = BTreeMap::new();
        ext.insert("mapped".to_string(), serde_json::Value::Bool(true));
        e.ext = Some(ext);
        e
    });
    assert_eq!(mapped.len(), 2);
    for e in mapped.iter() {
        let ext = e.ext.as_ref().expect("ext should be set");
        assert_eq!(ext.get("mapped"), Some(&serde_json::Value::Bool(true)));
    }
}

#[test]
fn map_events_adjust_timestamp() {
    let stream = EventStream::new(vec![assistant_msg(10, "hello")]);
    let mapped = stream.map_events(|mut e| {
        e.ts = ts(99);
        e
    });
    assert_eq!(mapped.iter().next().unwrap().ts, ts(99));
}

#[test]
fn map_events_empty_stream() {
    let stream = EventStream::new(vec![]);
    let mapped = stream.map_events(|e| e);
    assert!(mapped.is_empty());
}

#[test]
fn map_events_preserves_order() {
    let stream = EventStream::new(vec![
        assistant_msg(0, "a"),
        assistant_msg(1, "b"),
        assistant_msg(2, "c"),
    ]);
    let mapped = stream.map_events(|e| e);
    let texts: Vec<&str> = mapped
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantMessage { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["a", "b", "c"]);
}

// ── take_until tests ─────────────────────────────────────────────────

#[test]
fn take_until_run_completed() {
    let stream = EventStream::new(vec![
        run_started(0),
        tool_call(1, "search"),
        assistant_msg(2, "result"),
        run_completed(3),
        warning(4, "post-run"),
    ]);
    let taken = stream.take_until(|e| matches!(e.kind, AgentEventKind::RunCompleted { .. }));
    assert_eq!(taken.len(), 3);
    // RunCompleted itself should NOT be in the result
    assert!(
        taken
            .iter()
            .all(|e| !matches!(e.kind, AgentEventKind::RunCompleted { .. }))
    );
}

#[test]
fn take_until_never_triggers() {
    let stream = EventStream::new(vec![run_started(0), assistant_msg(1, "hi")]);
    let taken = stream.take_until(|e| matches!(e.kind, AgentEventKind::Error { .. }));
    assert_eq!(taken.len(), 2);
}

#[test]
fn take_until_first_event() {
    let stream = EventStream::new(vec![run_completed(0), assistant_msg(1, "after")]);
    let taken = stream.take_until(|e| matches!(e.kind, AgentEventKind::RunCompleted { .. }));
    assert!(taken.is_empty());
}

#[test]
fn take_until_empty_stream() {
    let stream = EventStream::new(vec![]);
    let taken = stream.take_until(|_| true);
    assert!(taken.is_empty());
}

#[test]
fn take_until_single_non_matching() {
    let stream = EventStream::new(vec![assistant_msg(0, "only")]);
    let taken = stream.take_until(|e| matches!(e.kind, AgentEventKind::RunCompleted { .. }));
    assert_eq!(taken.len(), 1);
}

// ── throttle tests ───────────────────────────────────────────────────

#[test]
fn throttle_keeps_first_event() {
    let stream = EventStream::new(vec![
        assistant_msg(0, "a"),
        assistant_msg(0, "b"),
        assistant_msg(0, "c"),
    ]);
    let throttled = stream.throttle(Duration::from_secs(5));
    assert_eq!(throttled.len(), 1);
}

#[test]
fn throttle_respects_window() {
    let stream = EventStream::new(vec![
        assistant_msg(0, "a"),
        assistant_msg(3, "b"),
        assistant_msg(6, "c"),
        assistant_msg(9, "d"),
    ]);
    // Window of 5s: keep events at 0, 6 (skip 3 and 9<6+5=11? no 9>=6+5=11? 9<11 so skip)
    // Actually: 0 → keep, 3 → 3-0=3 <5 skip, 6 → 6-0=6 >=5 keep, 9 → 9-6=3 <5 skip
    let throttled = stream.throttle(Duration::from_secs(5));
    assert_eq!(throttled.len(), 2);
}

#[test]
fn throttle_zero_window_keeps_all() {
    let stream = EventStream::new(vec![
        assistant_msg(0, "a"),
        assistant_msg(1, "b"),
        assistant_msg(2, "c"),
    ]);
    let throttled = stream.throttle(Duration::ZERO);
    assert_eq!(throttled.len(), 3);
}

#[test]
fn throttle_empty_stream() {
    let stream = EventStream::new(vec![]);
    let throttled = stream.throttle(Duration::from_secs(1));
    assert!(throttled.is_empty());
}

// ── merge tests ──────────────────────────────────────────────────────

#[test]
fn merge_interleaved_by_timestamp() {
    let a = EventStream::new(vec![assistant_msg(0, "a1"), assistant_msg(2, "a2")]);
    let b = EventStream::new(vec![tool_call(1, "t1"), tool_call(3, "t2")]);
    let merged = a.merge(&b);
    assert_eq!(merged.len(), 4);
    // Verify chronological order
    let times: Vec<_> = merged.iter().map(|e| e.ts).collect();
    for w in times.windows(2) {
        assert!(w[0] <= w[1], "timestamps should be non-decreasing");
    }
}

#[test]
fn merge_self_first_on_tie() {
    let a = EventStream::new(vec![assistant_msg(0, "from_a")]);
    let b = EventStream::new(vec![tool_call(0, "from_b")]);
    let merged = a.merge(&b);
    assert_eq!(merged.len(), 2);
    // self (a) should come first when timestamps tie
    assert!(matches!(
        merged.iter().next().unwrap().kind,
        AgentEventKind::AssistantMessage { .. }
    ));
}

#[test]
fn merge_one_empty() {
    let a = EventStream::new(vec![run_started(0)]);
    let b = EventStream::new(vec![]);
    assert_eq!(a.merge(&b).len(), 1);
    assert_eq!(b.merge(&a).len(), 1);
}

#[test]
fn merge_both_empty() {
    let a = EventStream::new(vec![]);
    let b = EventStream::new(vec![]);
    assert!(a.merge(&b).is_empty());
}

// ── combinator chaining ──────────────────────────────────────────────

#[test]
fn chain_filter_then_map() {
    let stream = EventStream::new(vec![
        run_started(0),
        tool_call(1, "read"),
        assistant_msg(2, "text"),
        run_completed(3),
    ]);
    let result = stream
        .filter_pred(|e| matches!(e.kind, AgentEventKind::ToolCall { .. }))
        .map_events(|mut e| {
            let mut ext = BTreeMap::new();
            ext.insert("traced".into(), serde_json::Value::Bool(true));
            e.ext = Some(ext);
            e
        });
    assert_eq!(result.len(), 1);
    assert!(result.iter().next().unwrap().ext.is_some());
}

#[test]
fn chain_merge_then_take_until() {
    let a = EventStream::new(vec![run_started(0), tool_call(2, "search")]);
    let b = EventStream::new(vec![
        assistant_msg(1, "hi"),
        run_completed(3),
        warning(4, "late"),
    ]);
    let result = a
        .merge(&b)
        .take_until(|e| matches!(e.kind, AgentEventKind::RunCompleted { .. }));
    // 0: RunStarted, 1: AssistantMessage, 2: ToolCall → stop before RunCompleted at 3
    assert_eq!(result.len(), 3);
}

#[test]
fn chain_take_until_then_throttle() {
    let stream = EventStream::new(vec![
        assistant_msg(0, "a"),
        assistant_msg(1, "b"),
        assistant_msg(10, "c"),
        run_completed(20),
        assistant_msg(30, "d"),
    ]);
    let result = stream
        .take_until(|e| matches!(e.kind, AgentEventKind::RunCompleted { .. }))
        .throttle(Duration::from_secs(5));
    // take_until: [a(0), b(1), c(10)]
    // throttle(5s): a(0) → keep, b(1) → 1<5 skip, c(10) → 10>=5 keep
    assert_eq!(result.len(), 2);
}

// ── by_kind / first_of_kind / last_of_kind ───────────────────────────

#[test]
fn by_kind_filters_correctly() {
    let stream = EventStream::new(vec![
        run_started(0),
        warning(1, "w1"),
        warning(2, "w2"),
        run_completed(3),
    ]);
    let warnings = stream.by_kind("warning");
    assert_eq!(warnings.len(), 2);
}

#[test]
fn first_and_last_of_kind() {
    let stream = EventStream::new(vec![
        assistant_msg(0, "first"),
        tool_call(1, "middle"),
        assistant_msg(2, "last"),
    ]);
    let first = stream.first_of_kind("assistant_message").unwrap();
    let last = stream.last_of_kind("assistant_message").unwrap();
    assert_eq!(first.ts, ts(0));
    assert_eq!(last.ts, ts(2));
}

// ── into_vec ─────────────────────────────────────────────────────────

#[test]
fn into_vec_returns_events() {
    let events = vec![run_started(0), run_completed(1)];
    let stream = EventStream::new(events);
    let v = stream.into_vec();
    assert_eq!(v.len(), 2);
}

// ── count_by_kind ────────────────────────────────────────────────────

#[test]
fn count_by_kind_after_merge() {
    let a = EventStream::new(vec![tool_call(0, "a"), tool_call(1, "b")]);
    let b = EventStream::new(vec![warning(2, "w")]);
    let counts = a.merge(&b).count_by_kind();
    assert_eq!(counts.get("tool_call"), Some(&2));
    assert_eq!(counts.get("warning"), Some(&1));
}
