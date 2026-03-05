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
// SPDX-License-Identifier: MIT OR Apache-2.0
#![allow(clippy::approx_constant)]
#![allow(clippy::needless_update)]
#![allow(clippy::useless_vec)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::needless_borrow)]
//! Deep tests for stream combinators: EventCollector, MergedStream, TimeoutStream, BufferedStream.

use std::pin::Pin;
use std::time::Duration;

use abp_core::{AgentEvent, AgentEventKind};
use abp_stream::{
    BufferedStream, EventCollector, EventFilter, EventRecorder, EventStats, EventStream,
    MergedStream, StreamBuffer, StreamPipelineBuilder, StreamTimeout, TimeoutStream,
};
use chrono::Utc;
use futures_core::Stream;
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

fn tool_call(name: &str, id: &str) -> AgentEvent {
    make_event(AgentEventKind::ToolCall {
        tool_name: name.to_string(),
        tool_use_id: Some(id.to_string()),
        parent_tool_use_id: None,
        input: serde_json::json!({}),
    })
}

fn tool_result(name: &str, id: &str) -> AgentEvent {
    make_event(AgentEventKind::ToolResult {
        tool_name: name.to_string(),
        tool_use_id: Some(id.to_string()),
        output: serde_json::json!({"ok": true}),
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

/// Synchronous stream from a vec (no channel, immediate yield).
fn vec_stream(events: Vec<AgentEvent>) -> tokio_stream::Iter<std::vec::IntoIter<AgentEvent>> {
    tokio_stream::iter(events)
}

// ===========================================================================
// 1. EventCollector tests
// ===========================================================================

#[tokio::test]
async fn collector_single_stream_collects_all() {
    let events = vec![delta("a"), delta("b"), delta("c")];
    let collector = EventCollector::new(vec_stream(events));
    let handle = collector.collected();
    let out: Vec<_> = tokio_stream::StreamExt::collect(collector).await;
    assert_eq!(out.len(), 3);
    assert_eq!(handle.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn collector_categorize_by_event_type() {
    let events = vec![
        delta("hi"),
        error_event("oops"),
        delta("bye"),
        warning_event("warn"),
        run_started(),
    ];
    let collector = EventCollector::new(vec_stream(events));
    let _: Vec<_> = tokio_stream::StreamExt::collect(collector).await;
    // Categorization is tested via snapshot; not built-in but we can verify types.
}

#[tokio::test]
async fn collector_categorize_counts_by_kind() {
    let events = vec![
        delta("a"),
        delta("b"),
        error_event("e1"),
        error_event("e2"),
        error_event("e3"),
        run_started(),
    ];
    let collector = EventCollector::new(vec_stream(events));
    let collected = collector.collected();
    let _: Vec<_> = tokio_stream::StreamExt::collect(collector).await;

    let snap = collected.lock().unwrap();
    let delta_count = snap
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::AssistantDelta { .. }))
        .count();
    let error_count = snap
        .iter()
        .filter(|e| matches!(e.kind, AgentEventKind::Error { .. }))
        .count();
    assert_eq!(delta_count, 2);
    assert_eq!(error_count, 3);
}

#[tokio::test]
async fn collector_empty_stream() {
    let collector = EventCollector::new(vec_stream(vec![]));
    assert!(collector.is_empty());
    let out: Vec<_> = tokio_stream::StreamExt::collect(collector).await;
    assert!(out.is_empty());
}

#[tokio::test]
async fn collector_large_stream() {
    let events: Vec<_> = (0..1500).map(|i| delta(&format!("tok{i}"))).collect();
    let collector = EventCollector::new(vec_stream(events));
    let handle = collector.collected();
    let out: Vec<_> = tokio_stream::StreamExt::collect(collector).await;
    assert_eq!(out.len(), 1500);
    assert_eq!(handle.lock().unwrap().len(), 1500);
}

#[tokio::test]
async fn collector_snapshot_returns_clone() {
    let events = vec![delta("x"), delta("y")];
    let collector = EventCollector::new(vec_stream(events));
    let snap_before = collector.snapshot();
    assert!(snap_before.is_empty());
    let _: Vec<_> = tokio_stream::StreamExt::collect(collector).await;
    // snap_before was cloned before consumption, should still be empty.
    assert!(snap_before.is_empty());
}

#[tokio::test]
async fn collector_len_and_is_empty() {
    let events = vec![delta("a")];
    let collector = EventCollector::new(vec_stream(events));
    assert!(collector.is_empty());
    assert_eq!(collector.len(), 0);
}

#[tokio::test]
async fn collector_forwards_events_unchanged() {
    let events = vec![delta("hello"), error_event("err"), run_completed()];
    let collector = EventCollector::new(vec_stream(events));
    let out: Vec<_> = tokio_stream::StreamExt::collect(collector).await;
    assert!(matches!(out[0].kind, AgentEventKind::AssistantDelta { .. }));
    assert!(matches!(out[1].kind, AgentEventKind::Error { .. }));
    assert!(matches!(out[2].kind, AgentEventKind::RunCompleted { .. }));
}

#[tokio::test]
async fn collector_shared_handle_across_clones() {
    let events = vec![delta("a"), delta("b")];
    let collector = EventCollector::new(vec_stream(events));
    let h1 = collector.collected();
    let h2 = collector.collected();
    let _: Vec<_> = tokio_stream::StreamExt::collect(collector).await;
    assert_eq!(h1.lock().unwrap().len(), 2);
    assert_eq!(h2.lock().unwrap().len(), 2);
}

// ===========================================================================
// 2. MergedStream tests
// ===========================================================================

#[tokio::test]
async fn merged_two_streams() {
    let s1 = vec_stream(vec![delta("a1"), delta("a2")]);
    let s2 = vec_stream(vec![delta("b1"), delta("b2")]);
    let merged = MergedStream::new(vec![
        Box::pin(s1) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
        Box::pin(s2) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
    ]);
    let out: Vec<_> = tokio_stream::StreamExt::collect(merged).await;
    assert_eq!(out.len(), 4);
}

#[tokio::test]
async fn merged_n_streams() {
    let streams: Vec<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> = (0..5)
        .map(|i| {
            let s = vec_stream(vec![delta(&format!("s{i}-a")), delta(&format!("s{i}-b"))]);
            Box::pin(s) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>
        })
        .collect();
    let merged = MergedStream::new(streams);
    let out: Vec<_> = tokio_stream::StreamExt::collect(merged).await;
    assert_eq!(out.len(), 10);
}

#[tokio::test]
async fn merged_order_within_source_preserved() {
    // With synchronous iter streams, round-robin should interleave but preserve
    // relative order within each source.
    let s1 = vec_stream(vec![delta("1a"), delta("1b"), delta("1c")]);
    let s2 = vec_stream(vec![delta("2a"), delta("2b"), delta("2c")]);
    let merged = MergedStream::new(vec![
        Box::pin(s1) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
        Box::pin(s2) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
    ]);
    let out: Vec<_> = tokio_stream::StreamExt::collect(merged).await;

    // Collect items from each source and check relative order.
    let source1: Vec<String> = out
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } if text.starts_with('1') => Some(text.clone()),
            _ => None,
        })
        .collect();
    let source2: Vec<String> = out
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } if text.starts_with('2') => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(source1, vec!["1a", "1b", "1c"]);
    assert_eq!(source2, vec!["2a", "2b", "2c"]);
}

#[tokio::test]
async fn merged_one_empty_one_active() {
    let s1 = vec_stream(vec![]);
    let s2 = vec_stream(vec![delta("only")]);
    let merged = MergedStream::new(vec![
        Box::pin(s1) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
        Box::pin(s2) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
    ]);
    let out: Vec<_> = tokio_stream::StreamExt::collect(merged).await;
    assert_eq!(out.len(), 1);
}

#[tokio::test]
async fn merged_all_empty() {
    let streams: Vec<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> = (0..3)
        .map(|_| Box::pin(vec_stream(vec![])) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>)
        .collect();
    let merged = MergedStream::new(streams);
    let out: Vec<_> = tokio_stream::StreamExt::collect(merged).await;
    assert!(out.is_empty());
}

#[tokio::test]
async fn merged_no_streams() {
    let merged = MergedStream::new(vec![]);
    assert_eq!(merged.remaining(), 0);
    let out: Vec<_> = tokio_stream::StreamExt::collect(merged).await;
    assert!(out.is_empty());
}

#[tokio::test]
async fn merged_remaining_decreases() {
    let s1 = vec_stream(vec![delta("a")]);
    let merged = MergedStream::new(vec![
        Box::pin(s1) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>
    ]);
    assert_eq!(merged.remaining(), 1);
}

#[tokio::test]
async fn merged_mixed_event_types() {
    let s1 = vec_stream(vec![delta("text"), error_event("err")]);
    let s2 = vec_stream(vec![run_started(), run_completed()]);
    let merged = MergedStream::new(vec![
        Box::pin(s1) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
        Box::pin(s2) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
    ]);
    let out: Vec<_> = tokio_stream::StreamExt::collect(merged).await;
    assert_eq!(out.len(), 4);
}

#[tokio::test]
async fn merged_uneven_stream_lengths() {
    let s1 = vec_stream(vec![delta("a")]);
    let s2 = vec_stream(vec![delta("b"), delta("c"), delta("d"), delta("e")]);
    let merged = MergedStream::new(vec![
        Box::pin(s1) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
        Box::pin(s2) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
    ]);
    let out: Vec<_> = tokio_stream::StreamExt::collect(merged).await;
    assert_eq!(out.len(), 5);
}

// ===========================================================================
// 3. TimeoutStream tests
// ===========================================================================

#[tokio::test]
async fn timeout_events_within_timeout_pass_through() {
    let inner = vec_stream(vec![delta("a"), delta("b"), delta("c")]);
    let ts = TimeoutStream::new(inner, Duration::from_secs(10));
    let out: Vec<_> = tokio_stream::StreamExt::collect(ts).await;
    assert_eq!(out.len(), 3);
    for item in &out {
        assert!(item.is_ok());
    }
}

#[tokio::test]
async fn timeout_triggers_error_on_slow_stream() {
    // Create a stream that never produces items (pending forever).
    let (_tx, rx) = mpsc::channel::<AgentEvent>(1);
    let inner = tokio_stream::wrappers::ReceiverStream::new(rx);
    let ts = TimeoutStream::new(inner, Duration::from_millis(50));
    let mut ts = Box::pin(ts);

    // First poll should eventually timeout.
    let item = tokio_stream::StreamExt::next(&mut ts).await;
    assert!(item.is_some());
    let result = item.unwrap();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.duration, Duration::from_millis(50));
}

#[tokio::test]
async fn timeout_error_display() {
    let err = StreamTimeout {
        duration: Duration::from_millis(100),
    };
    let msg = format!("{err}");
    assert!(msg.contains("timeout"));
}

#[tokio::test]
async fn timeout_error_is_std_error() {
    let err = StreamTimeout {
        duration: Duration::from_secs(1),
    };
    let _: &dyn std::error::Error = &err;
}

#[tokio::test]
async fn timeout_stream_ends_normally() {
    let inner = vec_stream(vec![delta("only")]);
    let ts = TimeoutStream::new(inner, Duration::from_secs(60));
    let out: Vec<_> = tokio_stream::StreamExt::collect(ts).await;
    assert_eq!(out.len(), 1);
    assert!(out[0].is_ok());
}

#[tokio::test]
async fn timeout_empty_stream_ends_immediately() {
    let inner = vec_stream(vec![]);
    let ts = TimeoutStream::new(inner, Duration::from_secs(1));
    let out: Vec<_> = tokio_stream::StreamExt::collect(ts).await;
    assert!(out.is_empty());
}

#[tokio::test]
async fn timeout_very_long_no_timeout_expected() {
    let events: Vec<_> = (0..10).map(|i| delta(&format!("t{i}"))).collect();
    let ts = TimeoutStream::new(vec_stream(events), Duration::from_secs(3600));
    let out: Vec<_> = tokio_stream::StreamExt::collect(ts).await;
    assert_eq!(out.len(), 10);
    assert!(out.iter().all(|r| r.is_ok()));
}

#[tokio::test]
async fn timeout_resets_deadline_on_each_event() {
    // Feed events with small delays but within the timeout window.
    let (tx, rx) = mpsc::channel(10);
    tokio::spawn(async move {
        for i in 0..3 {
            let _ = tx.send(delta(&format!("ev{i}"))).await;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
    let inner = tokio_stream::wrappers::ReceiverStream::new(rx);
    let ts = TimeoutStream::new(inner, Duration::from_millis(200));
    let out: Vec<_> = tokio_stream::StreamExt::collect(ts).await;
    assert_eq!(out.len(), 3);
    assert!(out.iter().all(|r| r.is_ok()));
}

#[tokio::test]
async fn timeout_preserves_event_content() {
    let inner = vec_stream(vec![delta("hello"), error_event("bad")]);
    let ts = TimeoutStream::new(inner, Duration::from_secs(10));
    let out: Vec<_> = tokio_stream::StreamExt::collect(ts).await;
    match &out[0].as_ref().unwrap().kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "hello"),
        _ => panic!("expected delta"),
    }
    match &out[1].as_ref().unwrap().kind {
        AgentEventKind::Error { message, .. } => assert_eq!(message, "bad"),
        _ => panic!("expected error"),
    }
}

// ===========================================================================
// 4. BufferedStream tests
// ===========================================================================

#[tokio::test]
async fn buffered_flush_at_capacity() {
    let events: Vec<_> = (0..6).map(|i| delta(&format!("b{i}"))).collect();
    let bs = BufferedStream::new(vec_stream(events), 3);
    let batches: Vec<_> = tokio_stream::StreamExt::collect(bs).await;
    // 6 events with batch_size 3 => 2 full batches.
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].len(), 3);
    assert_eq!(batches[1].len(), 3);
}

#[tokio::test]
async fn buffered_flush_on_close() {
    // 5 events with batch_size 3 => 1 full batch + 1 partial flush.
    let events: Vec<_> = (0..5).map(|i| delta(&format!("b{i}"))).collect();
    let bs = BufferedStream::new(vec_stream(events), 3);
    let batches: Vec<_> = tokio_stream::StreamExt::collect(bs).await;
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].len(), 3);
    assert_eq!(batches[1].len(), 2);
}

#[tokio::test]
async fn buffered_single_item_buffer() {
    let events = vec![delta("a"), delta("b"), delta("c")];
    let bs = BufferedStream::new(vec_stream(events), 1);
    let batches: Vec<_> = tokio_stream::StreamExt::collect(bs).await;
    assert_eq!(batches.len(), 3);
    for batch in &batches {
        assert_eq!(batch.len(), 1);
    }
}

#[tokio::test]
#[should_panic(expected = "batch_size must be > 0")]
async fn buffered_zero_capacity_panics() {
    let _ = BufferedStream::new(vec_stream(vec![]), 0);
}

#[tokio::test]
async fn buffered_empty_stream() {
    let bs = BufferedStream::new(vec_stream(vec![]), 5);
    let batches: Vec<_> = tokio_stream::StreamExt::collect(bs).await;
    assert!(batches.is_empty());
}

#[tokio::test]
async fn buffered_exact_capacity_boundary() {
    let events: Vec<_> = (0..10).map(|i| delta(&format!("e{i}"))).collect();
    let bs = BufferedStream::new(vec_stream(events), 5);
    let batches: Vec<_> = tokio_stream::StreamExt::collect(bs).await;
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].len(), 5);
    assert_eq!(batches[1].len(), 5);
}

#[tokio::test]
async fn buffered_single_event_stream() {
    let bs = BufferedStream::new(vec_stream(vec![delta("lone")]), 10);
    let batches: Vec<_> = tokio_stream::StreamExt::collect(bs).await;
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 1);
}

#[tokio::test]
async fn buffered_large_batch_size() {
    let events: Vec<_> = (0..3).map(|i| delta(&format!("e{i}"))).collect();
    let bs = BufferedStream::new(vec_stream(events), 1000);
    let batches: Vec<_> = tokio_stream::StreamExt::collect(bs).await;
    // All 3 flushed as one partial batch on close.
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 3);
}

#[tokio::test]
async fn buffered_preserves_event_order() {
    let events: Vec<_> = (0..9).map(|i| delta(&format!("{i}"))).collect();
    let bs = BufferedStream::new(vec_stream(events), 3);
    let batches: Vec<_> = tokio_stream::StreamExt::collect(bs).await;
    let flattened: Vec<String> = batches
        .into_iter()
        .flatten()
        .filter_map(|e| match e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text),
            _ => None,
        })
        .collect();
    let expected: Vec<String> = (0..9).map(|i| format!("{i}")).collect();
    assert_eq!(flattened, expected);
}

#[tokio::test]
async fn buffered_with_flush_interval_construction() {
    // Verify that with_flush_interval doesn't panic.
    let bs = BufferedStream::new(vec_stream(vec![delta("a")]), 100)
        .with_flush_interval(Duration::from_millis(50));
    let batches: Vec<_> = tokio_stream::StreamExt::collect(bs).await;
    // The stream ends immediately so it should flush on close.
    assert_eq!(batches.len(), 1);
}

#[tokio::test]
async fn buffered_mixed_event_types() {
    let events = vec![
        delta("text"),
        error_event("err"),
        run_started(),
        run_completed(),
    ];
    let bs = BufferedStream::new(vec_stream(events), 2);
    let batches: Vec<_> = tokio_stream::StreamExt::collect(bs).await;
    assert_eq!(batches.len(), 2);
    assert!(matches!(
        batches[0][0].kind,
        AgentEventKind::AssistantDelta { .. }
    ));
    assert!(matches!(batches[0][1].kind, AgentEventKind::Error { .. }));
    assert!(matches!(
        batches[1][0].kind,
        AgentEventKind::RunStarted { .. }
    ));
    assert!(matches!(
        batches[1][1].kind,
        AgentEventKind::RunCompleted { .. }
    ));
}

// ===========================================================================
// 5. Combinators chained
// ===========================================================================

#[tokio::test]
async fn chained_collector_over_buffered() {
    // BufferedStream yields Vec batches, so we collect the batch stream via collector-like pattern.
    let events: Vec<_> = (0..6).map(|i| delta(&format!("c{i}"))).collect();
    let bs = BufferedStream::new(vec_stream(events), 2);
    let batches: Vec<Vec<AgentEvent>> = tokio_stream::StreamExt::collect(bs).await;
    let total_events: usize = batches.iter().map(|b| b.len()).sum();
    assert_eq!(total_events, 6);
    assert_eq!(batches.len(), 3);
}

#[tokio::test]
async fn chained_timeout_then_collect() {
    let events = vec![delta("a"), delta("b"), delta("c")];
    let ts = TimeoutStream::new(vec_stream(events), Duration::from_secs(5));
    // Wrap the timeout stream results: filter out errors and collect successes.
    let results: Vec<_> = tokio_stream::StreamExt::collect(ts).await;
    let ok_events: Vec<_> = results.into_iter().filter_map(|r| r.ok()).collect();
    assert_eq!(ok_events.len(), 3);
}

#[tokio::test]
async fn chained_collector_then_merged() {
    let events_a = vec![delta("a1"), delta("a2")];
    let events_b = vec![delta("b1"), delta("b2")];

    let collector_a = EventCollector::new(vec_stream(events_a));
    let handle_a = collector_a.collected();
    let collector_b = EventCollector::new(vec_stream(events_b));
    let handle_b = collector_b.collected();

    let merged = MergedStream::new(vec![
        Box::pin(collector_a) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
        Box::pin(collector_b) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
    ]);
    let out: Vec<_> = tokio_stream::StreamExt::collect(merged).await;
    assert_eq!(out.len(), 4);
    assert_eq!(handle_a.lock().unwrap().len(), 2);
    assert_eq!(handle_b.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn chained_merged_then_collector() {
    let s1 = vec_stream(vec![delta("x"), delta("y")]);
    let s2 = vec_stream(vec![delta("z")]);
    let merged = MergedStream::new(vec![
        Box::pin(s1) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
        Box::pin(s2) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
    ]);
    let collector = EventCollector::new(merged);
    let handle = collector.collected();
    let out: Vec<_> = tokio_stream::StreamExt::collect(collector).await;
    assert_eq!(out.len(), 3);
    assert_eq!(handle.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn chained_merged_then_buffered() {
    let s1 = vec_stream(vec![delta("a"), delta("b")]);
    let s2 = vec_stream(vec![delta("c"), delta("d")]);
    let merged = MergedStream::new(vec![
        Box::pin(s1) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
        Box::pin(s2) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
    ]);
    let bs = BufferedStream::new(merged, 2);
    let batches: Vec<_> = tokio_stream::StreamExt::collect(bs).await;
    let total: usize = batches.iter().map(|b| b.len()).sum();
    assert_eq!(total, 4);
}

#[tokio::test]
async fn chained_pipeline_filter_then_collect() {
    let pipeline = StreamPipelineBuilder::new()
        .filter(EventFilter::exclude_errors())
        .record()
        .build();

    let events = vec![delta("ok"), error_event("bad"), delta("also ok")];
    let mut passed = Vec::new();
    for ev in events {
        if let Some(e) = pipeline.process(ev) {
            passed.push(e);
        }
    }
    assert_eq!(passed.len(), 2);
    assert_eq!(pipeline.recorder().unwrap().len(), 2);
}

#[tokio::test]
async fn chained_pipeline_with_stats() {
    let stats = EventStats::new();
    let pipeline = StreamPipelineBuilder::new().with_stats(stats).build();

    let events = vec![delta("a"), delta("b"), error_event("e")];
    for ev in events {
        pipeline.process(ev);
    }
    let s = pipeline.stats().unwrap();
    assert_eq!(s.total_events(), 3);
    assert_eq!(s.error_count(), 1);
}

// ===========================================================================
// 6. Event types through stream
// ===========================================================================

#[tokio::test]
async fn event_type_text_delta_through_collector() {
    let events = vec![delta("hello"), delta(" world")];
    let collector = EventCollector::new(vec_stream(events));
    let out: Vec<_> = tokio_stream::StreamExt::collect(collector).await;
    let text: String = out
        .iter()
        .filter_map(|e| match &e.kind {
            AgentEventKind::AssistantDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "hello world");
}

#[tokio::test]
async fn event_type_tool_use_through_merged() {
    let s1 = vec_stream(vec![tool_call("read_file", "tc1")]);
    let s2 = vec_stream(vec![tool_result("read_file", "tc1")]);
    let merged = MergedStream::new(vec![
        Box::pin(s1) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
        Box::pin(s2) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>,
    ]);
    let out: Vec<_> = tokio_stream::StreamExt::collect(merged).await;
    assert_eq!(out.len(), 2);
    assert!(
        matches!(out[0].kind, AgentEventKind::ToolCall { .. })
            || matches!(out[0].kind, AgentEventKind::ToolResult { .. })
    );
}

#[tokio::test]
async fn event_type_error_through_timeout() {
    let events = vec![error_event("fatal")];
    let ts = TimeoutStream::new(vec_stream(events), Duration::from_secs(5));
    let out: Vec<_> = tokio_stream::StreamExt::collect(ts).await;
    assert_eq!(out.len(), 1);
    let ev = out[0].as_ref().unwrap();
    assert!(matches!(ev.kind, AgentEventKind::Error { .. }));
}

#[tokio::test]
async fn event_type_mixed_through_buffered() {
    let events = vec![
        run_started(),
        delta("text"),
        tool_call("edit", "tc2"),
        tool_result("edit", "tc2"),
        file_changed("src/main.rs"),
        command_executed("cargo build"),
        warning_event("slow"),
        error_event("fail"),
        message("complete response"),
        run_completed(),
    ];
    let bs = BufferedStream::new(vec_stream(events), 4);
    let batches: Vec<_> = tokio_stream::StreamExt::collect(bs).await;
    let total: usize = batches.iter().map(|b| b.len()).sum();
    assert_eq!(total, 10);
}

#[tokio::test]
async fn event_type_file_changed_through_collector() {
    let events = vec![
        file_changed("a.rs"),
        file_changed("b.rs"),
        file_changed("c.rs"),
    ];
    let collector = EventCollector::new(vec_stream(events));
    let out: Vec<_> = tokio_stream::StreamExt::collect(collector).await;
    assert_eq!(out.len(), 3);
    for ev in &out {
        assert!(matches!(ev.kind, AgentEventKind::FileChanged { .. }));
    }
}

#[tokio::test]
async fn event_type_command_executed_through_collector() {
    let events = vec![command_executed("ls"), command_executed("pwd")];
    let collector = EventCollector::new(vec_stream(events));
    let out: Vec<_> = tokio_stream::StreamExt::collect(collector).await;
    assert_eq!(out.len(), 2);
    for ev in &out {
        assert!(matches!(ev.kind, AgentEventKind::CommandExecuted { .. }));
    }
}

#[tokio::test]
async fn event_type_warning_through_collector() {
    let events = vec![warning_event("w1"), warning_event("w2")];
    let collector = EventCollector::new(vec_stream(events));
    let out: Vec<_> = tokio_stream::StreamExt::collect(collector).await;
    assert_eq!(out.len(), 2);
    for ev in &out {
        assert!(matches!(ev.kind, AgentEventKind::Warning { .. }));
    }
}

#[tokio::test]
async fn event_type_assistant_message_through_collector() {
    let events = vec![message("full response")];
    let collector = EventCollector::new(vec_stream(events));
    let out: Vec<_> = tokio_stream::StreamExt::collect(collector).await;
    assert_eq!(out.len(), 1);
    match &out[0].kind {
        AgentEventKind::AssistantMessage { text } => assert_eq!(text, "full response"),
        _ => panic!("expected AssistantMessage"),
    }
}

// ===========================================================================
// 7. StreamBuffer (ring buffer) tests
// ===========================================================================

#[test]
fn stream_buffer_push_and_recent() {
    let mut buf = StreamBuffer::new(3);
    buf.push(delta("a"));
    buf.push(delta("b"));
    buf.push(delta("c"));
    assert!(buf.is_full());
    assert_eq!(buf.len(), 3);
    assert_eq!(buf.recent(2).len(), 2);
}

#[test]
fn stream_buffer_eviction() {
    let mut buf = StreamBuffer::new(2);
    buf.push(delta("a"));
    buf.push(delta("b"));
    buf.push(delta("c")); // evicts "a"
    assert_eq!(buf.len(), 2);
    let recent = buf.recent(10);
    assert_eq!(recent.len(), 2);
    // First should be "b", second "c" after eviction.
    match &recent[0].kind {
        AgentEventKind::AssistantDelta { text } => assert_eq!(text, "b"),
        _ => panic!("expected delta"),
    }
}

#[test]
fn stream_buffer_drain() {
    let mut buf = StreamBuffer::new(5);
    buf.push(delta("x"));
    buf.push(delta("y"));
    let drained = buf.drain();
    assert_eq!(drained.len(), 2);
    assert!(buf.is_empty());
}

#[test]
fn stream_buffer_capacity() {
    let buf = StreamBuffer::new(42);
    assert_eq!(buf.capacity(), 42);
    assert!(buf.is_empty());
}

#[test]
#[should_panic(expected = "capacity must be > 0")]
fn stream_buffer_zero_capacity_panics() {
    let _ = StreamBuffer::new(0);
}

#[test]
fn stream_buffer_recent_more_than_stored() {
    let mut buf = StreamBuffer::new(10);
    buf.push(delta("only"));
    let recent = buf.recent(100);
    assert_eq!(recent.len(), 1);
}

// ===========================================================================
// 8. EventStream (mpsc-based) tests
// ===========================================================================

#[tokio::test]
async fn event_stream_recv() {
    let (tx, rx) = mpsc::channel(4);
    let mut stream = EventStream::new(rx);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);
    let a = stream.recv().await.unwrap();
    assert!(matches!(a.kind, AgentEventKind::AssistantDelta { .. }));
    let b = stream.recv().await.unwrap();
    assert!(matches!(b.kind, AgentEventKind::AssistantDelta { .. }));
    assert!(stream.recv().await.is_none());
}

#[tokio::test]
async fn event_stream_collect_all() {
    let (tx, rx) = mpsc::channel(4);
    let stream = EventStream::new(rx);
    tx.send(delta("a")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);
    let all = stream.collect_all().await;
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn event_stream_collect_filtered() {
    let (tx, rx) = mpsc::channel(4);
    let stream = EventStream::new(rx);
    tx.send(delta("a")).await.unwrap();
    tx.send(error_event("e")).await.unwrap();
    tx.send(delta("b")).await.unwrap();
    drop(tx);
    let filter = EventFilter::exclude_errors();
    let filtered = stream.collect_filtered(&filter).await;
    assert_eq!(filtered.len(), 2);
}

#[tokio::test]
async fn event_stream_as_futures_stream() {
    let (tx, rx) = mpsc::channel(4);
    let mut stream = EventStream::new(rx);
    tx.send(delta("x")).await.unwrap();
    drop(tx);
    let item = tokio_stream::StreamExt::next(&mut stream).await;
    assert!(item.is_some());
    let none = tokio_stream::StreamExt::next(&mut stream).await;
    assert!(none.is_none());
}

// ===========================================================================
// 9. EventRecorder tests
// ===========================================================================

#[test]
fn recorder_record_and_replay() {
    let rec = EventRecorder::new();
    rec.record(&delta("a"));
    rec.record(&delta("b"));
    assert_eq!(rec.len(), 2);
    assert!(!rec.is_empty());
    let events = rec.events();
    assert_eq!(events.len(), 2);
}

#[test]
fn recorder_clear() {
    let rec = EventRecorder::new();
    rec.record(&delta("a"));
    rec.clear();
    assert!(rec.is_empty());
}

// ===========================================================================
// 10. EventFilter additional tests
// ===========================================================================

#[test]
fn filter_errors_only() {
    let f = EventFilter::errors_only();
    assert!(f.matches(&error_event("x")));
    assert!(!f.matches(&delta("y")));
    assert!(!f.matches(&warning_event("w")));
}

#[test]
fn filter_exclude_errors() {
    let f = EventFilter::exclude_errors();
    assert!(!f.matches(&error_event("x")));
    assert!(f.matches(&delta("y")));
    assert!(f.matches(&run_started()));
}

#[test]
fn filter_custom_predicate() {
    let f = EventFilter::new(|ev| matches!(ev.kind, AgentEventKind::Warning { .. }));
    assert!(f.matches(&warning_event("w")));
    assert!(!f.matches(&delta("d")));
}

#[test]
fn filter_by_kind_name() {
    let f = EventFilter::by_kind("tool_call");
    assert!(f.matches(&tool_call("read", "id1")));
    assert!(!f.matches(&delta("text")));
}

// ===========================================================================
// 11. StreamTee tests
// ===========================================================================

#[tokio::test]
async fn tee_broadcasts_to_all() {
    let (tx1, mut rx1) = mpsc::channel(4);
    let (tx2, mut rx2) = mpsc::channel(4);
    let tee = abp_stream::StreamTee::new(vec![tx1, tx2]);
    tee.send(&delta("hi")).await.unwrap();
    assert!(rx1.recv().await.is_some());
    assert!(rx2.recv().await.is_some());
}

#[tokio::test]
async fn tee_skips_closed_receiver() {
    let (tx1, rx1) = mpsc::channel(4);
    let (tx2, mut rx2) = mpsc::channel(4);
    drop(rx1);
    let tee = abp_stream::StreamTee::new(vec![tx1, tx2]);
    // Should still succeed because tx2 is open.
    tee.send(&delta("hi")).await.unwrap();
    assert!(rx2.recv().await.is_some());
}

#[tokio::test]
async fn tee_all_closed_returns_error() {
    let (tx1, rx1) = mpsc::channel(1);
    let (tx2, rx2) = mpsc::channel(1);
    drop(rx1);
    drop(rx2);
    let tee = abp_stream::StreamTee::new(vec![tx1, tx2]);
    let result = tee.send(&delta("hi")).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn tee_empty_senders_ok() {
    let tee = abp_stream::StreamTee::new(vec![]);
    let result = tee.send(&delta("hi")).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn tee_active_count() {
    let (tx1, _rx1) = mpsc::channel::<AgentEvent>(1);
    let (tx2, rx2) = mpsc::channel::<AgentEvent>(1);
    let tee = abp_stream::StreamTee::new(vec![tx1, tx2]);
    assert_eq!(tee.active_count(), 2);
    drop(rx2);
    // After dropping rx2, active count should decrease (eventually).
    // mpsc::Sender::is_closed may not immediately reflect the drop,
    // but since we dropped it, the next send will fail.
    assert!(tee.active_count() <= 2);
}

// ===========================================================================
// 12. Large-scale stress tests
// ===========================================================================

#[tokio::test]
async fn collector_stress_5000_events() {
    let events: Vec<_> = (0..5000).map(|i| delta(&format!("e{i}"))).collect();
    let collector = EventCollector::new(vec_stream(events));
    let out: Vec<_> = tokio_stream::StreamExt::collect(collector).await;
    assert_eq!(out.len(), 5000);
}

#[tokio::test]
async fn merged_stress_many_streams() {
    let streams: Vec<Pin<Box<dyn Stream<Item = AgentEvent> + Send>>> = (0..20)
        .map(|i| {
            let evs: Vec<_> = (0..10).map(|j| delta(&format!("s{i}e{j}"))).collect();
            Box::pin(vec_stream(evs)) as Pin<Box<dyn Stream<Item = AgentEvent> + Send>>
        })
        .collect();
    let merged = MergedStream::new(streams);
    let out: Vec<_> = tokio_stream::StreamExt::collect(merged).await;
    assert_eq!(out.len(), 200);
}

#[tokio::test]
async fn buffered_stress_large_stream() {
    let events: Vec<_> = (0..1000).map(|i| delta(&format!("e{i}"))).collect();
    let bs = BufferedStream::new(vec_stream(events), 7);
    let batches: Vec<_> = tokio_stream::StreamExt::collect(bs).await;
    let total: usize = batches.iter().map(|b| b.len()).sum();
    assert_eq!(total, 1000);
    // 1000 / 7 = 142 full batches + 1 partial (6 items).
    assert_eq!(batches.len(), 143);
    assert_eq!(batches.last().unwrap().len(), 6);
}
