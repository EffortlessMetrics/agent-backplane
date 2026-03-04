#![allow(clippy::all)]
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
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Enhanced tests for stream metrics, buffer, and tee modules.

use abp_core::{AgentEvent, AgentEventKind};
use chrono::Utc;
use tokio::sync::mpsc;

use abp_stream::buffer::StreamBuffer;
use abp_stream::metrics::StreamMetrics;
use abp_stream::tee::StreamTee;

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

fn delta_event(text: &str) -> AgentEvent {
    make_event(AgentEventKind::AssistantDelta {
        text: text.to_string(),
    })
}

fn error_event(msg: &str) -> AgentEvent {
    make_event(AgentEventKind::Error {
        message: msg.to_string(),
        error_code: None,
    })
}

fn run_started_event() -> AgentEvent {
    make_event(AgentEventKind::RunStarted {
        message: "started".to_string(),
    })
}

fn run_completed_event() -> AgentEvent {
    make_event(AgentEventKind::RunCompleted {
        message: "done".to_string(),
    })
}

fn tool_call_event(name: &str) -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: name.to_string(),
        tool_use_id: None,
        parent_tool_use_id: None,
        input: serde_json::json!({}),
    })
}

fn warning_event(msg: &str) -> AgentEvent {
    make_event(AgentEventKind::Warning {
        message: msg.to_string(),
    })
}

fn file_changed_event(path: &str) -> AgentEvent {
    make_event(AgentEventKind::FileChanged {
        path: path.to_string(),
        summary: "modified".to_string(),
    })
}

// ===========================================================================
// StreamMetrics tests
// ===========================================================================

#[test]
fn metrics_new_is_empty() {
    let m = StreamMetrics::new();
    assert_eq!(m.event_count(), 0);
    assert_eq!(m.total_bytes(), 0);
    assert!(m.event_type_counts().is_empty());
}

#[test]
fn metrics_default_is_empty() {
    let m = StreamMetrics::default();
    assert_eq!(m.event_count(), 0);
}

#[test]
fn metrics_record_increments_count() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta_event("hi"));
    assert_eq!(m.event_count(), 1);
    m.record_event(&delta_event("there"));
    assert_eq!(m.event_count(), 2);
}

#[test]
fn metrics_tracks_total_bytes_from_deltas() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta_event("hello")); // 5 bytes
    m.record_event(&delta_event("world!")); // 6 bytes
    assert_eq!(m.total_bytes(), 11);
}

#[test]
fn metrics_non_delta_events_add_zero_bytes() {
    let mut m = StreamMetrics::new();
    m.record_event(&error_event("oops"));
    m.record_event(&run_started_event());
    assert_eq!(m.total_bytes(), 0);
}

#[test]
fn metrics_tracks_event_type_counts() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta_event("a"));
    m.record_event(&delta_event("b"));
    m.record_event(&error_event("e"));
    m.record_event(&tool_call_event("read"));
    m.record_event(&warning_event("w"));

    let counts = m.event_type_counts();
    assert_eq!(counts.get("assistant_delta"), Some(&2));
    assert_eq!(counts.get("error"), Some(&1));
    assert_eq!(counts.get("tool_call"), Some(&1));
    assert_eq!(counts.get("warning"), Some(&1));
    assert_eq!(counts.get("run_started"), None);
}

#[test]
fn metrics_event_type_counts_uses_btreemap() {
    let mut m = StreamMetrics::new();
    m.record_event(&warning_event("w"));
    m.record_event(&error_event("e"));
    m.record_event(&delta_event("d"));

    // BTreeMap keys are sorted
    let keys: Vec<_> = m.event_type_counts().keys().cloned().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

#[test]
fn metrics_elapsed_zero_before_events() {
    let m = StreamMetrics::new();
    assert_eq!(m.elapsed(), std::time::Duration::ZERO);
}

#[test]
fn metrics_elapsed_nonzero_after_events() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta_event("a"));
    std::thread::sleep(std::time::Duration::from_millis(10));
    // elapsed should be at least 10ms
    assert!(m.elapsed() >= std::time::Duration::from_millis(5));
}

#[test]
fn metrics_throughput_zero_with_no_events() {
    let m = StreamMetrics::new();
    assert_eq!(m.throughput(), 0.0);
}

#[test]
fn metrics_throughput_zero_with_single_event() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta_event("a"));
    // Single event: first == last, so elapsed is 0 → throughput 0.0
    assert_eq!(m.throughput(), 0.0);
}

#[test]
fn metrics_throughput_positive_with_time_gap() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta_event("a"));
    std::thread::sleep(std::time::Duration::from_millis(50));
    m.record_event(&delta_event("b"));
    assert!(m.throughput() > 0.0);
}

#[test]
fn metrics_summary_contains_all_fields() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta_event("hello"));
    m.record_event(&error_event("fail"));

    let s = m.summary();
    assert_eq!(s.event_count, 2);
    assert_eq!(s.total_bytes, 5);
    assert_eq!(s.event_type_counts.len(), 2);
    assert!(s.event_type_counts.contains_key("assistant_delta"));
    assert!(s.event_type_counts.contains_key("error"));
}

#[test]
fn metrics_summary_display_format() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta_event("x"));
    let s = m.summary();
    let display = format!("{s}");
    assert!(display.contains("Events: 1"));
    assert!(display.contains("Bytes: 1"));
    assert!(display.contains("assistant_delta: 1"));
}

#[test]
fn metrics_all_event_kinds_tracked() {
    let mut m = StreamMetrics::new();
    m.record_event(&run_started_event());
    m.record_event(&run_completed_event());
    m.record_event(&delta_event("d"));
    m.record_event(&make_event(AgentEventKind::AssistantMessage {
        text: "msg".into(),
    }));
    m.record_event(&tool_call_event("t"));
    m.record_event(&make_event(AgentEventKind::ToolResult {
        tool_name: "t".into(),
        tool_use_id: None,
        output: serde_json::json!(null),
        is_error: false,
    }));
    m.record_event(&file_changed_event("f.rs"));
    m.record_event(&make_event(AgentEventKind::CommandExecuted {
        command: "ls".into(),
        exit_code: Some(0),
        output_preview: None,
    }));
    m.record_event(&warning_event("w"));
    m.record_event(&error_event("e"));

    assert_eq!(m.event_count(), 10);
    assert_eq!(m.event_type_counts().len(), 10);
}

// ===========================================================================
// StreamBuffer tests
// ===========================================================================

#[test]
fn buffer_new_is_empty() {
    let b = StreamBuffer::new(5);
    assert!(b.is_empty());
    assert_eq!(b.len(), 0);
    assert!(!b.is_full());
    assert_eq!(b.capacity(), 5);
}

#[test]
#[should_panic(expected = "capacity must be > 0")]
fn buffer_zero_capacity_panics() {
    let _ = StreamBuffer::new(0);
}

#[test]
fn buffer_push_increments_len() {
    let mut b = StreamBuffer::new(5);
    b.push(delta_event("a"));
    assert_eq!(b.len(), 1);
    b.push(delta_event("b"));
    assert_eq!(b.len(), 2);
}

#[test]
fn buffer_is_full_at_capacity() {
    let mut b = StreamBuffer::new(2);
    b.push(delta_event("a"));
    assert!(!b.is_full());
    b.push(delta_event("b"));
    assert!(b.is_full());
}

#[test]
fn buffer_evicts_oldest_when_full() {
    let mut b = StreamBuffer::new(2);
    b.push(delta_event("first"));
    b.push(delta_event("second"));
    b.push(delta_event("third"));

    assert_eq!(b.len(), 2);
    let recent = b.recent(10);
    assert_eq!(recent.len(), 2);
    // "first" should have been evicted
    assert!(matches!(
        &recent[0].kind,
        AgentEventKind::AssistantDelta { text } if text == "second"
    ));
    assert!(matches!(
        &recent[1].kind,
        AgentEventKind::AssistantDelta { text } if text == "third"
    ));
}

#[test]
fn buffer_recent_returns_last_n() {
    let mut b = StreamBuffer::new(10);
    for i in 0..5 {
        b.push(delta_event(&format!("ev{i}")));
    }
    let recent = b.recent(3);
    assert_eq!(recent.len(), 3);
    assert!(matches!(
        &recent[0].kind,
        AgentEventKind::AssistantDelta { text } if text == "ev2"
    ));
}

#[test]
fn buffer_recent_returns_all_when_n_exceeds_len() {
    let mut b = StreamBuffer::new(10);
    b.push(delta_event("a"));
    b.push(delta_event("b"));
    let recent = b.recent(100);
    assert_eq!(recent.len(), 2);
}

#[test]
fn buffer_recent_empty_buffer() {
    let b = StreamBuffer::new(5);
    let recent = b.recent(3);
    assert!(recent.is_empty());
}

#[test]
fn buffer_drain_returns_all_events() {
    let mut b = StreamBuffer::new(10);
    b.push(delta_event("a"));
    b.push(delta_event("b"));
    b.push(delta_event("c"));

    let drained = b.drain();
    assert_eq!(drained.len(), 3);
    assert!(b.is_empty());
}

#[test]
fn buffer_drain_leaves_empty() {
    let mut b = StreamBuffer::new(5);
    b.push(delta_event("x"));
    b.drain();
    assert!(b.is_empty());
    assert_eq!(b.len(), 0);
    assert!(!b.is_full());
}

#[test]
fn buffer_usable_after_drain() {
    let mut b = StreamBuffer::new(3);
    b.push(delta_event("a"));
    b.drain();
    b.push(delta_event("b"));
    assert_eq!(b.len(), 1);
    assert!(matches!(
        &b.recent(1)[0].kind,
        AgentEventKind::AssistantDelta { text } if text == "b"
    ));
}

#[test]
fn buffer_capacity_one() {
    let mut b = StreamBuffer::new(1);
    b.push(delta_event("a"));
    assert!(b.is_full());
    b.push(delta_event("b"));
    assert!(b.is_full());
    assert_eq!(b.len(), 1);
    assert!(matches!(
        &b.recent(1)[0].kind,
        AgentEventKind::AssistantDelta { text } if text == "b"
    ));
}

#[test]
fn buffer_maintains_insertion_order() {
    let mut b = StreamBuffer::new(5);
    for i in 0..5 {
        b.push(delta_event(&format!("{i}")));
    }
    let drained = b.drain();
    for (i, ev) in drained.iter().enumerate() {
        assert!(matches!(
            &ev.kind,
            AgentEventKind::AssistantDelta { text } if text == &format!("{i}")
        ));
    }
}

// ===========================================================================
// StreamTee tests
// ===========================================================================

#[tokio::test]
async fn tee_broadcasts_to_all_receivers() {
    let (tx1, mut rx1) = mpsc::channel(16);
    let (tx2, mut rx2) = mpsc::channel(16);
    let tee = StreamTee::new(vec![tx1, tx2]);

    tee.send(&delta_event("hello")).await.unwrap();

    assert!(rx1.recv().await.is_some());
    assert!(rx2.recv().await.is_some());
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
    let (tx1, _rx1) = mpsc::channel::<AgentEvent>(16);
    let (tx2, rx2) = mpsc::channel::<AgentEvent>(16);
    let tee = StreamTee::new(vec![tx1, tx2]);
    drop(rx2);
    assert_eq!(tee.active_count(), 1);
}

#[tokio::test]
async fn tee_handles_closed_receiver_gracefully() {
    let (tx1, mut rx1) = mpsc::channel(16);
    let (tx2, rx2) = mpsc::channel(16);
    let tee = StreamTee::new(vec![tx1, tx2]);

    drop(rx2); // close one receiver

    // Should still succeed because tx1 is open
    tee.send(&delta_event("ok")).await.unwrap();
    assert!(rx1.recv().await.is_some());
}

#[tokio::test]
async fn tee_error_when_all_receivers_closed() {
    let (tx1, rx1) = mpsc::channel::<AgentEvent>(16);
    let (tx2, rx2) = mpsc::channel::<AgentEvent>(16);
    let tee = StreamTee::new(vec![tx1, tx2]);

    drop(rx1);
    drop(rx2);

    let result = tee.send(&delta_event("fail")).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn tee_empty_senders_is_ok() {
    let tee = StreamTee::new(vec![]);
    assert_eq!(tee.active_count(), 0);
    tee.send(&delta_event("noop")).await.unwrap();
}

#[tokio::test]
async fn tee_sends_correct_event_data() {
    let (tx, mut rx) = mpsc::channel(16);
    let tee = StreamTee::new(vec![tx]);

    let ev = delta_event("specific_text");
    tee.send(&ev).await.unwrap();

    let received = rx.recv().await.unwrap();
    assert!(matches!(
        &received.kind,
        AgentEventKind::AssistantDelta { text } if text == "specific_text"
    ));
}

#[tokio::test]
async fn tee_multiple_sends() {
    let (tx, mut rx) = mpsc::channel(16);
    let tee = StreamTee::new(vec![tx]);

    tee.send(&delta_event("a")).await.unwrap();
    tee.send(&delta_event("b")).await.unwrap();
    tee.send(&delta_event("c")).await.unwrap();

    let mut received = Vec::new();
    for _ in 0..3 {
        received.push(rx.recv().await.unwrap());
    }
    assert_eq!(received.len(), 3);
}

// ===========================================================================
// Combined / integration tests
// ===========================================================================

#[test]
fn metrics_and_buffer_combined() {
    let mut metrics = StreamMetrics::new();
    let mut buffer = StreamBuffer::new(3);

    let events = vec![
        run_started_event(),
        delta_event("hello"),
        tool_call_event("read_file"),
        delta_event("world"),
        run_completed_event(),
    ];

    for ev in &events {
        metrics.record_event(ev);
        buffer.push(ev.clone());
    }

    assert_eq!(metrics.event_count(), 5);
    assert_eq!(metrics.total_bytes(), 10); // "hello" + "world"
    assert_eq!(buffer.len(), 3); // only last 3 retained
    assert!(matches!(
        &buffer.recent(1)[0].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

#[tokio::test]
async fn tee_with_metrics_and_buffer() {
    let (tx1, mut rx1) = mpsc::channel(16);
    let (tx2, mut rx2) = mpsc::channel(16);
    let tee = StreamTee::new(vec![tx1, tx2]);

    let events = vec![
        run_started_event(),
        delta_event("data"),
        run_completed_event(),
    ];

    for ev in &events {
        tee.send(ev).await.unwrap();
    }

    // Consumer 1: collect into metrics
    let mut metrics = StreamMetrics::new();
    for _ in 0..3 {
        let ev = rx1.recv().await.unwrap();
        metrics.record_event(&ev);
    }
    assert_eq!(metrics.event_count(), 3);
    assert_eq!(metrics.total_bytes(), 4); // "data"

    // Consumer 2: collect into buffer
    let mut buffer = StreamBuffer::new(2);
    for _ in 0..3 {
        let ev = rx2.recv().await.unwrap();
        buffer.push(ev);
    }
    assert_eq!(buffer.len(), 2); // capacity 2, last 2 kept
}

#[test]
fn buffer_drain_correctness_with_eviction() {
    let mut b = StreamBuffer::new(3);
    // Push 5 events into capacity-3 buffer
    for i in 0..5 {
        b.push(delta_event(&format!("ev{i}")));
    }
    let drained = b.drain();
    assert_eq!(drained.len(), 3);
    // Should contain ev2, ev3, ev4
    assert!(matches!(
        &drained[0].kind,
        AgentEventKind::AssistantDelta { text } if text == "ev2"
    ));
    assert!(matches!(
        &drained[1].kind,
        AgentEventKind::AssistantDelta { text } if text == "ev3"
    ));
    assert!(matches!(
        &drained[2].kind,
        AgentEventKind::AssistantDelta { text } if text == "ev4"
    ));
}

#[test]
fn metrics_mixed_event_types_byte_tracking() {
    let mut m = StreamMetrics::new();
    m.record_event(&delta_event("abc")); // 3 bytes
    m.record_event(&error_event("not counted"));
    m.record_event(&delta_event("de")); // 2 bytes
    m.record_event(&tool_call_event("also not counted"));
    assert_eq!(m.total_bytes(), 5);
    assert_eq!(m.event_count(), 4);
}
